//! AI-powered inline completion (ghost text) provider.
//!
//! This module provides Copilot-style tab-completions from an AI model.
//! It integrates with the existing `InlineCompletionData` system by
//! producing `InlineCompletionItem`s that render as ghost text.
//!
//! Architecture:
//! - On each keystroke in Insert mode, `request_ai_completion` is called
//!   (debounced to avoid excessive requests).
//! - It extracts FIM (Fill-in-the-Middle) context: prefix + suffix around cursor.
//! - Sends `ProxyRequest::AiInlineCompletion` to the proxy.
//! - The proxy calls a fast model (e.g. Gemini Flash) with FIM context.
//! - Response items are converted to `InlineCompletionItem`s and merged
//!   into the existing completion data.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

use crate::inline_completion::{InlineCompletionItem, InlineCompletionStatus};

/// Configuration for AI inline completions.
#[derive(Clone, Debug)]
pub struct AiCompletionConfig {
    /// Whether AI inline completions are enabled.
    pub enabled: bool,
    /// Minimum delay between requests (debounce).
    pub debounce_ms: u64,
    /// Maximum prefix chars to send (context window budget).
    pub max_prefix_chars: usize,
    /// Maximum suffix chars to send.
    pub max_suffix_chars: usize,
}

impl Default for AiCompletionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 300,
            max_prefix_chars: 1500,
            max_suffix_chars: 500,
        }
    }
}

/// Manages AI inline completion state.
#[derive(Clone, Debug)]
pub struct AiCompletionData {
    /// Whether there's an in-flight request.
    pub pending: RwSignal<bool>,
    /// The request ID of the last sent request (for deduplication).
    pub last_request_id: RwSignal<u64>,
    /// Last request timestamp (for debouncing).
    pub last_request_time: RwSignal<Option<Instant>>,
    /// Configuration.
    pub config: AiCompletionConfig,
}

static AI_COMPLETION_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

impl AiCompletionData {
    pub fn new(cx: Scope) -> Self {
        Self {
            pending: cx.create_rw_signal(false),
            last_request_id: cx.create_rw_signal(0),
            last_request_time: cx.create_rw_signal(None),
            config: AiCompletionConfig::default(),
        }
    }

    /// Check if enough time has passed since the last request (debounce).
    pub fn should_request(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        let now = Instant::now();
        let last = self.last_request_time.get_untracked();
        match last {
            Some(t) => now.duration_since(t) >= Duration::from_millis(self.config.debounce_ms),
            None => true,
        }
    }

    /// Generate a new request ID.
    pub fn next_request_id(&self) -> u64 {
        let id = AI_COMPLETION_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.last_request_id.set(id);
        self.last_request_time.set(Some(Instant::now()));
        self.pending.set(true);
        id
    }

    /// Mark the request as completed.
    pub fn request_completed(&self, request_id: u64) {
        // Only clear pending if this is the most recent request
        if self.last_request_id.get_untracked() == request_id {
            self.pending.set(false);
        }
    }

    /// Extract FIM (Fill-in-the-Middle) context from a buffer at a cursor position.
    pub fn extract_fim_context(
        &self,
        text: &str,
        cursor_offset: usize,
    ) -> (String, String) {
        let prefix_start = cursor_offset.saturating_sub(self.config.max_prefix_chars);
        let suffix_end = (cursor_offset + self.config.max_suffix_chars).min(text.len());

        let prefix = &text[prefix_start..cursor_offset];
        let suffix = &text[cursor_offset..suffix_end];

        (prefix.to_string(), suffix.to_string())
    }

    /// Convert an AI completion response item into an InlineCompletionItem.
    pub fn to_inline_item(
        ai_item: &lapce_rpc::core::AiInlineCompletionItem,
    ) -> InlineCompletionItem {
        InlineCompletionItem {
            insert_text: ai_item.insert_text.clone(),
            filter_text: None,
            range: Some(ai_item.start_offset..ai_item.end_offset),
            command: None,
            insert_text_format: Some(lsp_types::InsertTextFormat::PLAIN_TEXT),
        }
    }
}
