//! Langfuse observability integration for agent traces.
//!
//! This module provides a StreamingPromptHook that sends all agent events
//! to Langfuse for visualization, debugging, and performance monitoring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use rig::agent::{HookAction, StreamingPromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;
use rig::message::Message;
use tokio::sync::Mutex;

#[cfg(feature = "langfuse")]
use langfuse_ergonomic::LangfuseClient;
#[cfg(feature = "langfuse")]
use serde_json::json;

// ── Internal State ──────────────────────────────────────────────────

struct LangfuseState {
    #[cfg(feature = "langfuse")]
    client: LangfuseClient,
    #[cfg(feature = "langfuse")]
    trace_id: String,
    #[cfg(feature = "langfuse")]
    base_url: String,
    session_start: Instant,
    turn: u32,
    /// Map of internal_call_id -> (observation_id, start_time)
    #[cfg(feature = "langfuse")]
    pending_tool_calls: HashMap<String, (String, Instant)>,
    /// Current completion observation ID
    #[cfg(feature = "langfuse")]
    current_completion_id: Option<String>,
    #[cfg(feature = "langfuse")]
    completion_start: Option<Instant>,
    /// Metadata for the session
    #[cfg(feature = "langfuse")]
    metadata: serde_json::Value,
}

// ── Public LangfuseHook ──────────────────────────────────────────────

/// A StreamingPromptHook that sends all events to Langfuse.
///
/// Provides rich observability with:
/// - Trace-level session tracking
/// - Generation-level LLM call monitoring
/// - Span-level tool execution tracking
/// - Performance metrics (latency, token usage)
/// - Error tracking
#[derive(Clone)]
pub struct LangfuseHook {
    state: Arc<Mutex<LangfuseState>>,
}

impl LangfuseHook {
    /// Create a new Langfuse hook for the agent session.
    ///
    /// # Arguments
    /// * `client` - Langfuse client (configured with API keys)
    /// * `session_name` - Human-readable name for this session
    /// * `user_id` - Optional user identifier
    /// * `metadata` - Additional metadata to attach to the trace
    #[cfg(feature = "langfuse")]
    pub async fn new(
        client: LangfuseClient,
        session_name: String,
        user_id: Option<String>,
        metadata: serde_json::Value,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Create the trace in Langfuse
        let trace_builder = client
            .trace()
            .name(session_name.clone())
            .user_id(user_id.as_deref().unwrap_or(""))
            .metadata(metadata.clone())
            .tags(vec!["forge-agent".to_string(), "ai-coding".to_string()]);
        
        let trace = trace_builder.call().await?;
        let trace_id = trace.id.clone();
        let base_url = std::env::var("LANGFUSE_BASE_URL")
            .unwrap_or_else(|_| "https://cloud.langfuse.com".to_string());
        
        tracing::info!("[Langfuse] Trace created: {}", trace_id);
        tracing::info!("[Langfuse] View at: {}/trace/{}", base_url, trace_id);
        
        Ok(Self {
            state: Arc::new(Mutex::new(LangfuseState {
                client,
                trace_id,
                base_url,
                session_start: Instant::now(),
                turn: 0,
                pending_tool_calls: HashMap::new(),
                current_completion_id: None,
                completion_start: None,
                metadata,
            })),
        })
    }
    
    /// Create a no-op hook when Langfuse feature is disabled
    #[cfg(not(feature = "langfuse"))]
    pub async fn new(
        _session_name: String,
        _user_id: Option<String>,
        _metadata: serde_json::Value,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            state: Arc::new(Mutex::new(LangfuseState {
                session_start: Instant::now(),
                turn: 0,
            })),
        })
    }
    
    /// Get the trace ID for this session
    #[cfg(feature = "langfuse")]
    pub async fn trace_id(&self) -> String {
        let state = self.state.lock().await;
        state.trace_id.clone()
    }
    
    /// Get the public Langfuse URL for this trace
    #[cfg(feature = "langfuse")]
    pub async fn trace_url(&self) -> String {
        let state = self.state.lock().await;
        format!("{}/trace/{}", state.base_url, state.trace_id)
    }
    
    /// Mark the session as complete
    #[cfg(feature = "langfuse")]
    pub async fn finish_session(&self, success: bool, error_message: Option<String>) {
        let state = self.state.lock().await;
        let elapsed_s = state.session_start.elapsed().as_secs_f64();
        
        // Create a final event marking session completion
        let _ = state.client
            .event()
            .trace_id(&state.trace_id)
            .name("session_end")
            .level(if success { "INFO" } else { "ERROR" })
            .output(json!({
                "success": success,
                "total_turns": state.turn,
                "duration_s": elapsed_s,
                "error": error_message,
            }))
            .call()
            .await;
        
        tracing::info!("[Langfuse] Session finished: {} turns, {:.2}s", state.turn, elapsed_s);
    }
    
    /// Mark the session as complete (no-op when feature disabled)
    #[cfg(not(feature = "langfuse"))]
    pub async fn finish_session(&self, _success: bool, _error_message: Option<String>) {
        // No-op when feature is disabled
    }
}

// ── StreamingPromptHook Implementation ──────────────────────────────

impl<M> StreamingPromptHook<M> for LangfuseHook
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
        let prompt_len = prompt_text.len();
        let history_len = history.len();
        
        async move {
            #[cfg(feature = "langfuse")]
            {
                let mut s = state.lock().await;
                s.turn += 1;
                s.completion_start = Some(Instant::now());
                
                // Create a generation observation for this LLM call
                let generation_name = format!("turn-{}", s.turn);
                match s.client
                    .generation()
                    .trace_id(&s.trace_id)
                    .name(generation_name)
                    .input(json!({
                        "prompt": if s.turn == 1 { 
                            // Include full prompt on first turn
                            truncate(&prompt_text, 8000) 
                        } else { 
                            truncate(&prompt_text, 1000) 
                        },
                        "prompt_len": prompt_len,
                        "history_len": history_len,
                    }))
                    .metadata(json!({
                        "turn": s.turn,
                        "elapsed_s": s.session_start.elapsed().as_secs_f64(),
                    }))
                    .call()
                    .await 
                {
                    Ok(id) => {
                        s.current_completion_id = Some(id);
                        tracing::debug!("[Langfuse] Turn {} generation created", s.turn);
                    }
                    Err(e) => {
                        tracing::warn!("[Langfuse] Failed to create generation: {}", e);
                    }
                }
            }
            
            #[cfg(not(feature = "langfuse"))]
            {
                let mut s = state.lock().await;
                s.turn += 1;
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
            #[cfg(feature = "langfuse")]
            {
                let mut s = state.lock().await;
                if let Some(obs_id) = s.current_completion_id.take() {
                    let duration_s = s.completion_start
                        .take()
                        .map(|start| start.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    
                    // Update the generation with completion info
                    // Note: langfuse-ergonomic doesn't support updating observations yet,
                    // so we log an event instead
                    let _ = s.client
                        .event()
                        .trace_id(&s.trace_id)
                        .parent_observation_id(&obs_id)
                        .name("completion_finish")
                        .level("DEBUG")
                        .output(json!({
                            "duration_s": duration_s,
                        }))
                        .call()
                        .await;
                    
                    tracing::debug!("[Langfuse] Turn {} completed in {:.2}s", s.turn, duration_s);
                }
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
            #[cfg(feature = "langfuse")]
            {
                let mut s = state.lock().await;
                
                // Create a span for this tool execution
                let span_name = format!("tool_{}", tool_name);
                let parent_id = s.current_completion_id.clone().unwrap_or_default();
                
                let span_builder = s.client
                    .span()
                    .trace_id(&s.trace_id)
                    .parent_observation_id(&parent_id)
                    .name(span_name)
                    .input(json!({
                        "tool": tool_name,
                        "args": args,
                    }))
                    .metadata(json!({
                        "turn": s.turn,
                        "elapsed_s": s.session_start.elapsed().as_secs_f64(),
                    }));
                
                match span_builder.call().await {
                    Ok(span_id) => {
                        s.pending_tool_calls.insert(
                            internal_call_id.clone(),
                            (span_id, Instant::now())
                        );
                        tracing::debug!("[Langfuse] Tool call {} started", tool_name);
                    }
                    Err(e) => {
                        tracing::warn!("[Langfuse] Failed to create span for tool {}: {}", tool_name, e);
                    }
                }
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
            #[cfg(feature = "langfuse")]
            {
                let mut s = state.lock().await;
                
                if let Some((span_id, start_time)) = s.pending_tool_calls.remove(&internal_call_id) {
                    let duration_s = start_time.elapsed().as_secs_f64();
                    
                    // Log completion event
                    let _ = s.client
                        .event()
                        .trace_id(&s.trace_id)
                        .parent_observation_id(&span_id)
                        .name("tool_result")
                        .level("DEBUG")
                        .output(json!({
                            "tool": tool_name,
                            "result_len": result_len,
                            "result_preview": result_preview,
                            "duration_s": duration_s,
                        }))
                        .call()
                        .await;
                    
                    tracing::debug!("[Langfuse] Tool {} completed in {:.3}s", tool_name, duration_s);
                }
            }
            
            HookAction::cont()
        }
    }

    fn on_text_delta(
        &self,
        _text_delta: &str,
        _aggregated_text: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        // We don't log every text delta to avoid spam in Langfuse
        // The final output will be captured in the generation
        async move {
            HookAction::cont()
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...[truncated, {} total chars]", &s[..max], s.len())
    }
}
