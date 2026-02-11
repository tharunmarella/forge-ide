//! Tool call loop detection.
//!
//! Detects when the agent is stuck making identical consecutive tool calls
//! and signals the caller to intervene (inject an error, ask the user, etc.).
//!
//! Also detects content repetition — when the model generates the same text
//! block multiple times in a row.

use std::collections::VecDeque;

/// Default limit: after this many identical consecutive tool calls, flag a loop.
const DEFAULT_TOOL_REPEAT_LIMIT: usize = 3;

/// Default limit: after this many near-identical content blocks, flag a loop.
const DEFAULT_CONTENT_REPEAT_LIMIT: usize = 3;

/// Minimum content length to track (short strings are too noisy).
const MIN_CONTENT_LEN: usize = 40;

/// Result of a loop check.
#[derive(Debug, Clone)]
pub struct LoopCheckResult {
    pub is_loop: bool,
    pub reason: Option<String>,
}

impl LoopCheckResult {
    fn ok() -> Self {
        Self { is_loop: false, reason: None }
    }

    fn looped(reason: impl Into<String>) -> Self {
        Self { is_loop: true, reason: Some(reason.into()) }
    }

    /// Convenience: returns true if a loop was detected.
    pub fn is_loop(&self) -> bool {
        self.is_loop
    }

    /// Human-readable message (empty if no loop).
    pub fn message(&self) -> String {
        self.reason.clone().unwrap_or_default()
    }
}

/// Detects agent loops by tracking consecutive identical tool calls
/// and repeated content blocks.
pub struct LoopDetector {
    // ── Tool call tracking ──
    tool_repeat_limit: usize,
    /// Serialized form of the last tool call (name + args).
    last_tool_call: Option<String>,
    /// How many times the current tool call has been seen consecutively.
    consecutive_tool_count: usize,

    // ── Content tracking ──
    content_repeat_limit: usize,
    /// Recent content hashes for repetition detection.
    recent_content: VecDeque<u64>,
}

impl LoopDetector {
    /// Create a new detector with default limits.
    pub fn new() -> Self {
        Self {
            tool_repeat_limit: DEFAULT_TOOL_REPEAT_LIMIT,
            content_repeat_limit: DEFAULT_CONTENT_REPEAT_LIMIT,
            last_tool_call: None,
            consecutive_tool_count: 0,
            recent_content: VecDeque::with_capacity(10),
        }
    }

    /// Create with custom limits.
    pub fn with_limits(tool_repeat_limit: usize, content_repeat_limit: usize) -> Self {
        Self {
            tool_repeat_limit,
            content_repeat_limit,
            ..Self::new()
        }
    }

    /// Check a tool call for looping.
    ///
    /// Call this before executing each tool. If `is_loop()` returns true,
    /// you should inject an error message back to the LLM instead of
    /// executing the tool.
    pub fn check_tool_call(&mut self, tool_name: &str, args_json: &str) -> LoopCheckResult {
        let key = format!("{}::{}", tool_name, args_json);

        if self.last_tool_call.as_deref() == Some(&key) {
            self.consecutive_tool_count += 1;
        } else {
            self.last_tool_call = Some(key);
            self.consecutive_tool_count = 1;
        }

        if self.consecutive_tool_count >= self.tool_repeat_limit {
            // Reset so the agent can recover if the error message helps.
            self.consecutive_tool_count = 0;
            self.last_tool_call = None;

            return LoopCheckResult::looped(format!(
                "Loop detected: tool '{}' called {} times with identical arguments. \
                 Try a different approach or use different parameters.",
                tool_name, self.tool_repeat_limit,
            ));
        }

        LoopCheckResult::ok()
    }

    /// Check streamed content for repetition.
    ///
    /// Call this with each text chunk from the model. If `is_loop()` returns
    /// true, the model is repeating itself and should be stopped.
    pub fn check_content(&mut self, text: &str) -> LoopCheckResult {
        let trimmed = text.trim();
        if trimmed.len() < MIN_CONTENT_LEN {
            return LoopCheckResult::ok();
        }

        let hash = simple_hash(trimmed);
        self.recent_content.push_back(hash);

        // Keep a bounded window.
        if self.recent_content.len() > 20 {
            self.recent_content.pop_front();
        }

        // Count how many of the last N entries are the same hash.
        let tail_count = self
            .recent_content
            .iter()
            .rev()
            .take_while(|&&h| h == hash)
            .count();

        if tail_count >= self.content_repeat_limit {
            self.recent_content.clear();
            return LoopCheckResult::looped(format!(
                "Loop detected: model repeated the same content {} times in a row.",
                tail_count,
            ));
        }

        LoopCheckResult::ok()
    }

    /// Reset all state (e.g. between turns or conversations).
    pub fn reset(&mut self) {
        self.last_tool_call = None;
        self.consecutive_tool_count = 0;
        self.recent_content.clear();
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Fast non-cryptographic hash for content comparison.
fn simple_hash(s: &str) -> u64 {
    // FNV-1a 64-bit
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_loop_on_different_calls() {
        let mut d = LoopDetector::new();
        assert!(!d.check_tool_call("grep", r#"{"pattern":"foo"}"#).is_loop());
        assert!(!d.check_tool_call("grep", r#"{"pattern":"bar"}"#).is_loop());
        assert!(!d.check_tool_call("read_file", r#"{"path":"x.rs"}"#).is_loop());
    }

    #[test]
    fn detects_tool_loop() {
        let mut d = LoopDetector::new();
        let args = r#"{"pattern":"stuck"}"#;
        assert!(!d.check_tool_call("grep", args).is_loop());
        assert!(!d.check_tool_call("grep", args).is_loop());
        assert!(d.check_tool_call("grep", args).is_loop()); // 3rd = limit
    }

    #[test]
    fn resets_after_detection() {
        let mut d = LoopDetector::new();
        let args = r#"{"pattern":"stuck"}"#;
        d.check_tool_call("grep", args);
        d.check_tool_call("grep", args);
        assert!(d.check_tool_call("grep", args).is_loop());
        // After detection, counter resets — next call should be fine.
        assert!(!d.check_tool_call("grep", args).is_loop());
    }

    #[test]
    fn detects_content_loop() {
        let mut d = LoopDetector::new();
        let block = "This is a sufficiently long repeated content block for detection.";
        assert!(!d.check_content(block).is_loop());
        assert!(!d.check_content(block).is_loop());
        assert!(d.check_content(block).is_loop()); // 3rd = limit
    }

    #[test]
    fn ignores_short_content() {
        let mut d = LoopDetector::new();
        assert!(!d.check_content("hi").is_loop());
        assert!(!d.check_content("hi").is_loop());
        assert!(!d.check_content("hi").is_loop());
        assert!(!d.check_content("hi").is_loop()); // Short => never flagged
    }
}
