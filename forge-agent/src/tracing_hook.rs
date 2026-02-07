//! Detailed agent trace logging via rig's StreamingPromptHook system.
//!
//! Writes a JSONL trace file for each agent session, recording every model call,
//! tool invocation, tool result, and text delta with timing information.
//! This makes it easy to see where the agent spends time and where it gets stuck.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rig::agent::{HookAction, StreamingPromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;
use rig::message::Message;
use serde::Serialize;

// ── Trace entry ──────────────────────────────────────────────────

#[derive(Serialize)]
struct TraceEntry {
    /// ISO 8601 wall-clock timestamp
    timestamp: String,
    /// Seconds elapsed since session start
    elapsed_s: f64,
    /// Duration of this specific step (tool exec, model thinking), if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_s: Option<f64>,
    /// Event type
    event: String,
    /// Agent turn counter (increments on each completion call)
    turn: u32,
    /// Event-specific payload
    data: serde_json::Value,
}

// ── Internal mutable state ───────────────────────────────────────

struct TracingState {
    file: BufWriter<File>,
    session_start: Instant,
    turn: u32,
    /// Start times for pending tool calls, keyed by internal_call_id
    pending_tool_starts: HashMap<String, Instant>,
    /// Start time for the current completion call
    completion_start: Option<Instant>,
    /// Count of tools invoked this session
    tools_called: u32,
}

impl TracingState {
    fn elapsed_s(&self) -> f64 {
        self.session_start.elapsed().as_secs_f64()
    }

    fn write_entry(&mut self, entry: &TraceEntry) {
        if let Ok(line) = serde_json::to_string(entry) {
            let _ = writeln!(self.file, "{}", line);
            let _ = self.file.flush();
        }
    }
}

// ── Public TracingHook ───────────────────────────────────────────

/// A StreamingPromptHook that writes detailed JSONL traces for every agent event.
///
/// Create one per agent prompt session. The trace file is flushed after
/// every event so you can `tail -f` it in real time.
#[derive(Clone)]
pub struct TracingHook {
    state: Arc<Mutex<TracingState>>,
    /// The path to the trace file (exposed for logging)
    pub trace_path: PathBuf,
}

impl TracingHook {
    /// Create a new tracing hook that writes to the given file path.
    /// Parent directories are created if they don't exist.
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        let state = TracingState {
            file: BufWriter::new(file),
            session_start: Instant::now(),
            turn: 0,
            pending_tool_starts: HashMap::new(),
            completion_start: None,
            tools_called: 0,
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            trace_path: path,
        })
    }

    /// Write a custom event to the trace (e.g. "session_end", "error").
    pub fn write_event(&self, event: &str, data: serde_json::Value) {
        if let Ok(mut state) = self.state.lock() {
            let entry = TraceEntry {
                timestamp: chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                elapsed_s: round2(state.elapsed_s()),
                duration_s: None,
                event: event.to_string(),
                turn: state.turn,
                data,
            };
            state.write_entry(&entry);
        }
    }

    /// Write the final session_end summary.
    pub fn write_session_end(&self) {
        if let Ok(mut state) = self.state.lock() {
            let elapsed = state.elapsed_s();
            let entry = TraceEntry {
                timestamp: chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                elapsed_s: round2(elapsed),
                duration_s: None,
                event: "session_end".to_string(),
                turn: state.turn,
                data: serde_json::json!({
                    "total_s": round2(elapsed),
                    "turns": state.turn,
                    "tools_called": state.tools_called,
                }),
            };
            state.write_entry(&entry);
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...[truncated, {} total]", &s[..max], s.len())
    }
}

// ── StreamingPromptHook implementation ───────────────────────────

impl<M> StreamingPromptHook<M> for TracingHook
where
    M: CompletionModel,
{
    fn on_completion_call(
        &self,
        prompt: &Message,
        history: &[Message],
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let state = self.state.clone();
        let prompt_text = format!("{:?}", prompt);
        let prompt_text = truncate(&prompt_text, 500);
        let history_len = history.len();
        async move {
            if let Ok(mut s) = state.lock() {
                s.turn += 1;
                s.completion_start = Some(Instant::now());
                let entry = TraceEntry {
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    elapsed_s: round2(s.elapsed_s()),
                    duration_s: None,
                    event: "completion_call".to_string(),
                    turn: s.turn,
                    data: serde_json::json!({
                        "prompt": truncate(&prompt_text, 500),
                        "history_len": history_len,
                    }),
                };
                s.write_entry(&entry);
            }
            HookAction::cont()
        }
    }

    fn on_stream_completion_response_finish(
        &self,
        _prompt: &Message,
        _response: &<M as CompletionModel>::StreamingResponse,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let state = self.state.clone();
        async move {
            if let Ok(mut s) = state.lock() {
                let duration = s
                    .completion_start
                    .take()
                    .map(|t| round2(t.elapsed().as_secs_f64()));
                let entry = TraceEntry {
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    elapsed_s: round2(s.elapsed_s()),
                    duration_s: duration,
                    event: "completion_response_finish".to_string(),
                    turn: s.turn,
                    data: serde_json::json!({}),
                };
                s.write_entry(&entry);
            }
            HookAction::cont()
        }
    }

    fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
    ) -> impl std::future::Future<Output = ToolCallHookAction> + Send {
        let state = self.state.clone();
        let tool_name = tool_name.to_string();
        let internal_call_id = internal_call_id.to_string();
        let args = truncate(args, 2000);
        async move {
            if let Ok(mut s) = state.lock() {
                s.tools_called += 1;
                s.pending_tool_starts
                    .insert(internal_call_id.clone(), Instant::now());
                let entry = TraceEntry {
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    elapsed_s: round2(s.elapsed_s()),
                    duration_s: None,
                    event: "tool_call".to_string(),
                    turn: s.turn,
                    data: serde_json::json!({
                        "tool": tool_name,
                        "args": args,
                    }),
                };
                s.write_entry(&entry);
            }
            ToolCallHookAction::cont()
        }
    }

    fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        internal_call_id: &str,
        _args: &str,
        result: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let state = self.state.clone();
        let tool_name = tool_name.to_string();
        let internal_call_id = internal_call_id.to_string();
        let result_len = result.len();
        let result_preview = truncate(result, 500);
        async move {
            if let Ok(mut s) = state.lock() {
                let duration = s
                    .pending_tool_starts
                    .remove(&internal_call_id)
                    .map(|t| round2(t.elapsed().as_secs_f64()));
                let entry = TraceEntry {
                    timestamp: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    elapsed_s: round2(s.elapsed_s()),
                    duration_s: duration,
                    event: "tool_result".to_string(),
                    turn: s.turn,
                    data: serde_json::json!({
                        "tool": tool_name,
                        "result_len": result_len,
                        "result_preview": result_preview,
                    }),
                };
                s.write_entry(&entry);
            }
            HookAction::cont()
        }
    }

    fn on_text_delta(
        &self,
        text_delta: &str,
        aggregated_text: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let state = self.state.clone();
        let delta = text_delta.to_string();
        let agg_len = aggregated_text.len();
        async move {
            // Only log every ~200 chars of accumulated text to avoid flooding
            if agg_len % 200 < delta.len() || agg_len < 50 {
                if let Ok(mut s) = state.lock() {
                    let entry = TraceEntry {
                        timestamp: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        elapsed_s: round2(s.elapsed_s()),
                        duration_s: None,
                        event: "text_delta".to_string(),
                        turn: s.turn,
                        data: serde_json::json!({
                            "delta_len": delta.len(),
                            "aggregated_len": agg_len,
                            "delta_preview": truncate(&delta, 200),
                        }),
                    };
                    s.write_entry(&entry);
                }
            }
            HookAction::cont()
        }
    }
}
