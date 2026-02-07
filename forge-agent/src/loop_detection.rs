//! Loop detection service, ported from gemini-cli's `loopDetectionService.ts`.
//!
//! Detects when the agent is stuck in unproductive loops via two strategies:
//! 1. **Tool call repetition:** SHA-256 hash of (tool_name, args_json). If the same
//!    hash appears N+ times consecutively, a loop is detected.
//! 2. **Content chanting:** Sliding window over streamed text. Hash chunks of text
//!    and detect when the same chunk repeats beyond a threshold.

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

/// Thresholds (from gemini-cli)
const TOOL_CALL_LOOP_THRESHOLD: usize = 4;
const CONTENT_CHUNK_SIZE: usize = 50;
const CONTENT_LOOP_THRESHOLD: usize = 10;
const CONTENT_BUFFER_MAX: usize = 5000;

/// Result of a loop detection check.
#[derive(Debug, Clone)]
pub enum LoopCheckResult {
    /// No loop detected, continue normally.
    Ok,
    /// Tool call loop detected -- same tool+args repeated N times.
    ToolCallLoop {
        tool_name: String,
        consecutive_count: usize,
    },
    /// Content chanting detected -- same text chunk repeated.
    ContentChanting {
        chunk_preview: String,
        repeat_count: usize,
    },
}

impl LoopCheckResult {
    pub fn is_loop(&self) -> bool {
        !matches!(self, LoopCheckResult::Ok)
    }

    pub fn message(&self) -> String {
        match self {
            LoopCheckResult::Ok => String::new(),
            LoopCheckResult::ToolCallLoop { tool_name, consecutive_count } => {
                format!(
                    "Loop detected: tool '{}' called with identical arguments {} times consecutively. \
                     Breaking to prevent infinite loop. Try a different approach.",
                    tool_name, consecutive_count
                )
            }
            LoopCheckResult::ContentChanting { chunk_preview, repeat_count } => {
                format!(
                    "Loop detected: repetitive text output detected ({} repeats of '{}...'). \
                     Breaking to prevent infinite loop.",
                    repeat_count,
                    &chunk_preview[..chunk_preview.len().min(30)]
                )
            }
        }
    }
}

/// Tracks agent behavior and detects loops.
pub struct LoopDetector {
    // ── Tool call tracking ──
    /// History of tool call hashes (most recent last).
    tool_call_hashes: VecDeque<u64>,
    /// Last tool name (for error messages).
    last_tool_name: String,

    // ── Content chanting tracking ──
    /// Buffer of streamed text for chunk analysis.
    content_buffer: String,
    /// Hashes of content chunks, keyed by position.
    chunk_hashes: VecDeque<u64>,
}

impl LoopDetector {
    pub fn new() -> Self {
        Self {
            tool_call_hashes: VecDeque::with_capacity(TOOL_CALL_LOOP_THRESHOLD + 1),
            last_tool_name: String::new(),
            content_buffer: String::with_capacity(CONTENT_BUFFER_MAX),
            chunk_hashes: VecDeque::new(),
        }
    }

    /// Check a tool call for repetition.
    /// Call this each time the agent requests a tool call.
    pub fn check_tool_call(&mut self, tool_name: &str, args_json: &str) -> LoopCheckResult {
        let hash = {
            let mut hasher = DefaultHasher::new();
            tool_name.hash(&mut hasher);
            args_json.hash(&mut hasher);
            hasher.finish()
        };

        self.last_tool_name = tool_name.to_string();
        self.tool_call_hashes.push_back(hash);

        // Keep only the last N entries
        while self.tool_call_hashes.len() > TOOL_CALL_LOOP_THRESHOLD + 1 {
            self.tool_call_hashes.pop_front();
        }

        // Check if the last N entries are all the same
        if self.tool_call_hashes.len() >= TOOL_CALL_LOOP_THRESHOLD {
            let last = *self.tool_call_hashes.back().unwrap();
            let consecutive = self
                .tool_call_hashes
                .iter()
                .rev()
                .take_while(|&&h| h == last)
                .count();

            if consecutive >= TOOL_CALL_LOOP_THRESHOLD {
                return LoopCheckResult::ToolCallLoop {
                    tool_name: tool_name.to_string(),
                    consecutive_count: consecutive,
                };
            }
        }

        LoopCheckResult::Ok
    }

    /// Check streamed text content for chanting (repetitive patterns).
    /// Call this incrementally as text arrives.
    pub fn check_content(&mut self, text: &str) -> LoopCheckResult {
        // Skip code blocks, tables, and headings (they can legitimately repeat)
        let filtered = strip_structural_content(text);
        if filtered.is_empty() {
            return LoopCheckResult::Ok;
        }

        self.content_buffer.push_str(&filtered);

        // Cap buffer size
        if self.content_buffer.len() > CONTENT_BUFFER_MAX {
            let drain = self.content_buffer.len() - CONTENT_BUFFER_MAX;
            self.content_buffer = self.content_buffer[drain..].to_string();
            // Rebuild chunk hashes for the truncated buffer
            self.rebuild_chunk_hashes();
        }

        // Only check when we have enough content
        if self.content_buffer.len() < CONTENT_CHUNK_SIZE * 3 {
            return LoopCheckResult::Ok;
        }

        // Hash new chunks at the end of the buffer
        let chunks_needed = self.content_buffer.len() / CONTENT_CHUNK_SIZE;
        while self.chunk_hashes.len() < chunks_needed {
            let idx = self.chunk_hashes.len() * CONTENT_CHUNK_SIZE;
            if idx + CONTENT_CHUNK_SIZE > self.content_buffer.len() {
                break;
            }
            let chunk = &self.content_buffer[idx..idx + CONTENT_CHUNK_SIZE];
            let hash = {
                let mut hasher = DefaultHasher::new();
                chunk.hash(&mut hasher);
                hasher.finish()
            };
            self.chunk_hashes.push_back(hash);
        }

        // Check for repeated chunk hashes using a sliding window
        if self.chunk_hashes.len() >= CONTENT_LOOP_THRESHOLD {
            let last_hash = *self.chunk_hashes.back().unwrap();
            let window_size = CONTENT_LOOP_THRESHOLD * 5; // check within 5x threshold distance
            let start = self
                .chunk_hashes
                .len()
                .saturating_sub(window_size);

            let repeat_count = self.chunk_hashes
                .iter()
                .skip(start)
                .filter(|&&h| h == last_hash)
                .count();

            if repeat_count >= CONTENT_LOOP_THRESHOLD {
                // Get the actual chunk text for the preview
                let last_idx = (self.chunk_hashes.len() - 1) * CONTENT_CHUNK_SIZE;
                let preview = if last_idx + CONTENT_CHUNK_SIZE <= self.content_buffer.len() {
                    self.content_buffer[last_idx..last_idx + CONTENT_CHUNK_SIZE].to_string()
                } else {
                    self.content_buffer[last_idx..].to_string()
                };

                return LoopCheckResult::ContentChanting {
                    chunk_preview: preview,
                    repeat_count,
                };
            }
        }

        LoopCheckResult::Ok
    }

    /// Reset detector state (e.g., after a successful user interaction).
    pub fn reset(&mut self) {
        self.tool_call_hashes.clear();
        self.last_tool_name.clear();
        self.content_buffer.clear();
        self.chunk_hashes.clear();
    }

    fn rebuild_chunk_hashes(&mut self) {
        self.chunk_hashes.clear();
        let mut idx = 0;
        while idx + CONTENT_CHUNK_SIZE <= self.content_buffer.len() {
            let chunk = &self.content_buffer[idx..idx + CONTENT_CHUNK_SIZE];
            let hash = {
                let mut hasher = DefaultHasher::new();
                chunk.hash(&mut hasher);
                hasher.finish()
            };
            self.chunk_hashes.push_back(hash);
            idx += CONTENT_CHUNK_SIZE;
        }
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip markdown structural content that can legitimately repeat.
/// Removes code blocks, tables, headings, and list markers.
fn strip_structural_content(text: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Toggle code blocks
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            continue;
        }

        // Skip headings
        if trimmed.starts_with('#') {
            continue;
        }

        // Skip table rows
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            continue;
        }

        // Strip list markers but keep content
        let content = trimmed
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(". ");

        if !content.is_empty() {
            result.push_str(content);
            result.push(' ');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_loop_detection() {
        let mut detector = LoopDetector::new();

        // Different calls should not trigger
        assert!(!detector
            .check_tool_call("read_file", r#"{"path":"a.rs"}"#)
            .is_loop());
        assert!(!detector
            .check_tool_call("read_file", r#"{"path":"b.rs"}"#)
            .is_loop());
        assert!(!detector
            .check_tool_call("grep", r#"{"pattern":"foo"}"#)
            .is_loop());

        // Same call repeated should trigger at threshold
        for i in 0..TOOL_CALL_LOOP_THRESHOLD {
            let result = detector.check_tool_call("read_file", r#"{"path":"stuck.rs"}"#);
            if i < TOOL_CALL_LOOP_THRESHOLD - 1 {
                assert!(!result.is_loop(), "Should not trigger at iteration {}", i);
            } else {
                assert!(result.is_loop(), "Should trigger at iteration {}", i);
            }
        }
    }

    #[test]
    fn test_content_chanting_detection() {
        let mut detector = LoopDetector::new();
        // Use a string that's exactly 49 chars -- strip_structural_content adds 1 space,
        // giving exactly CONTENT_CHUNK_SIZE (50) chars per call, so chunks align perfectly.
        let repeated = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVW";
        assert_eq!(repeated.len(), 49);

        // Feed the same text many times until chanting is detected.
        // With perfect alignment, detection should trigger after CONTENT_LOOP_THRESHOLD + a few buffer calls.
        let mut detected = false;
        for _ in 0..30 {
            let result = detector.check_content(repeated);
            if result.is_loop() {
                detected = true;
                break;
            }
        }

        assert!(detected, "Should detect content chanting after many repetitions");
    }

    #[test]
    fn test_reset_clears_state() {
        let mut detector = LoopDetector::new();

        // Build up some state
        for _ in 0..3 {
            detector.check_tool_call("read_file", r#"{"path":"a.rs"}"#);
        }

        detector.reset();

        // After reset, should not trigger
        assert!(!detector
            .check_tool_call("read_file", r#"{"path":"a.rs"}"#)
            .is_loop());
    }
}
