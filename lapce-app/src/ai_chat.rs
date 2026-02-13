//! AI Chat data model with keyboard focus support.

use std::collections::HashMap;
use std::rc::Rc;

use floem::{
    ext_event::create_ext_action,
    keyboard::Modifiers,
    kurbo::Point,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use serde::{Deserialize, Serialize};

use crate::{
    command::{CommandExecuted, CommandKind, LapceCommand},
    editor::EditorData,
    keypress::KeyPressFocus,
    main_split::Editors,
    window_tab::CommonData,
};
use lapce_core::directory::Directory;
use lapce_core::mode::Mode;
use lapce_core::command::EditCommand;

// ── AI Keys Config (persisted to ai-keys.toml) ──────────────────

const AI_KEYS_FILE: &str = "ai-keys.toml";

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AiKeysConfig {
    #[serde(default)]
    pub keys: HashMap<String, String>,
    #[serde(default)]
    pub defaults: AiDefaults,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiDefaults {
    pub provider: String,
    pub model: String,
}

impl Default for AiDefaults {
    fn default() -> Self {
        Self {
            provider: "gemini".to_string(),
            model: "gemini-2.5-flash".to_string(),
        }
    }
}

impl AiKeysConfig {
    /// Load from disk, or return default if file doesn't exist.
    pub fn load() -> Self {
        let Some(path) = Directory::config_directory().map(|d| d.join(AI_KEYS_FILE)) else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save to disk.
    pub fn save(&self) {
        let Some(path) = Directory::config_directory().map(|d| d.join(AI_KEYS_FILE)) else {
            return;
        };
        if let Ok(content) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, content);
        }
    }

    /// Returns true if at least one provider has a non-empty key.
    pub fn has_any_key(&self) -> bool {
        self.keys.values().any(|k| !k.trim().is_empty())
    }

    /// Get the API key for a given provider, if configured.
    pub fn key_for(&self, provider: &str) -> Option<&str> {
        self.keys
            .get(provider)
            .map(|k| k.as_str())
            .filter(|k| !k.trim().is_empty())
    }

    /// Returns the list of providers that have a configured key.
    pub fn configured_providers(&self) -> Vec<String> {
        self.keys
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// Static list of models per provider.
pub fn models_for_provider(provider: &str) -> Vec<&'static str> {
    match provider {
        "gemini" => vec![
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-2.0-flash",
            "gemini-3-flash-preview",
            "gemini-3-pro-preview",
        ],
        "anthropic" => vec![
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-3-5-sonnet-20241022",
        ],
        "openai" => vec![
            "gpt-4o",
            "gpt-4.1",
            "o3-mini",
            "o4-mini",
        ],
        _ => vec![],
    }
}

/// All supported providers.
pub const ALL_PROVIDERS: &[&str] = &["gemini", "anthropic", "openai"];

// ── Chat data types ─────────────────────────────────────────────

/// Represents a role in the chat.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

/// Represents a tool call shown in the chat.
#[derive(Clone, Debug)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub output: Option<String>,
    /// When this tool call started (for elapsed time display).
    pub started_at: std::time::Instant,
    /// Pre-formatted elapsed time string (e.g. "1.2s"), updated on completion.
    pub elapsed_display: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolCallStatus {
    Pending,
    /// Waiting for user to approve or reject this tool call.
    WaitingApproval,
    Running,
    Success,
    Error,
    /// User rejected this tool call.
    Rejected,
}

/// A single entry in the chat, with a unique id and version for reactive re-rendering.
#[derive(Clone, Debug)]
pub struct ChatEntry {
    /// Unique entry id (monotonically increasing).
    pub id: u64,
    /// Version counter -- incremented on every mutation so `dyn_stack` re-renders.
    pub version: u64,
    pub kind: ChatEntryKind,
}

#[derive(Clone, Debug)]
pub enum ChatEntryKind {
    Message { role: ChatRole, content: String },
    ToolCall(ChatToolCall),
    /// Agent thinking step (server-side activity).
    ThinkingStep(ChatThinkingStep),
    /// Agent's task plan.
    Plan(ChatPlan),
    /// Server-side tool call (displayed in thinking section).
    ServerToolCall(ChatServerToolCall),
}

/// A thinking step from the agent (server-side activity).
#[derive(Clone, Debug)]
pub struct ChatThinkingStep {
    /// Type: "enriching", "reasoning", "searching", etc.
    pub step_type: String,
    /// Human-readable message
    pub message: String,
    /// Optional detail
    pub detail: Option<String>,
    /// When this step started
    pub started_at: std::time::Instant,
}

/// Agent's task plan.
#[derive(Clone, Debug)]
pub struct ChatPlan {
    pub steps: Vec<ChatPlanStep>,
}

/// A single step in the agent's plan.
#[derive(Clone, Debug)]
pub struct ChatPlanStep {
    pub number: u32,
    pub description: String,
    pub status: ChatPlanStepStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChatPlanStepStatus {
    Pending,
    InProgress,
    Done,
}

/// A server-side tool call (codebase_search, trace_call_chain, etc.).
#[derive(Clone, Debug)]
pub struct ChatServerToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub result_summary: Option<String>,
    pub started_at: std::time::Instant,
    pub elapsed_display: String,
}

impl ChatEntry {
    /// Stable key for `dyn_stack` that changes when content is mutated.
    pub fn key(&self) -> (u64, u64) {
        (self.id, self.version)
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(1);

fn next_entry_id() -> u64 {
    NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn new_message(role: ChatRole, content: String) -> ChatEntry {
    ChatEntry {
        id: next_entry_id(),
        version: 0,
        kind: ChatEntryKind::Message { role, content },
    }
}

pub fn new_tool_call(tc: ChatToolCall) -> ChatEntry {
    ChatEntry {
        id: next_entry_id(),
        version: 0,
        kind: ChatEntryKind::ToolCall(tc),
    }
}

pub fn new_thinking_step(step_type: String, message: String, detail: Option<String>) -> ChatEntry {
    ChatEntry {
        id: next_entry_id(),
        version: 0,
        kind: ChatEntryKind::ThinkingStep(ChatThinkingStep {
            step_type,
            message,
            detail,
            started_at: std::time::Instant::now(),
        }),
    }
}

pub fn new_plan(steps: Vec<ChatPlanStep>) -> ChatEntry {
    ChatEntry {
        id: next_entry_id(),
        version: 0,
        kind: ChatEntryKind::Plan(ChatPlan { steps }),
    }
}

pub fn new_server_tool_call(id: String, name: String, arguments: String) -> ChatEntry {
    ChatEntry {
        id: next_entry_id(),
        version: 0,
        kind: ChatEntryKind::ServerToolCall(ChatServerToolCall {
            id,
            name,
            arguments,
            status: ToolCallStatus::Running,
            result_summary: None,
            started_at: std::time::Instant::now(),
            elapsed_display: String::new(),
        }),
    }
}

// ── AiChatData ──────────────────────────────────────────────────

/// Reactive data for the AI chat panel.
#[derive(Clone, Debug)]
pub struct AiChatData {
    /// Scope for creating ext actions (cross-thread callbacks)
    pub scope: Scope,
    /// The editor for the chat input
    pub editor: EditorData,
    /// Chat history entries
    pub entries: RwSignal<im::Vector<ChatEntry>>,
    /// Whether the agent is currently processing
    pub is_loading: RwSignal<bool>,
    /// Selected provider
    pub provider: RwSignal<String>,
    /// Selected model
    pub model: RwSignal<String>,
    /// Persisted API key configuration
    pub keys_config: RwSignal<AiKeysConfig>,
    /// Whether the model dropdown is currently open
    pub dropdown_open: RwSignal<bool>,
    pub common: Rc<CommonData>,

    // ── Streaming UI signals ────────────────────────────────────
    /// Current in-progress streaming text (plain, not yet markdown-parsed).
    /// Updated on every AgentTextChunk; cleared on done.
    pub streaming_text: RwSignal<String>,
    /// Whether we've received the first text token (controls thinking indicator).
    pub has_first_token: RwSignal<bool>,
    /// Signal to auto-scroll the message list to the bottom.
    pub scroll_to_bottom: RwSignal<Option<Point>>,
    /// Counter incremented each time we want to trigger a scroll.
    /// The view reacts to changes in this signal.
    pub scroll_trigger: RwSignal<u64>,

    // ── Conversation tracking ──────────────────────────────────
    /// Persistent conversation_id for multi-turn chat memory.
    /// Preserved across messages in the same session; reset on clear.
    pub conversation_id: RwSignal<String>,

    // ── Index status ────────────────────────────────────────────
    /// Human-readable codebase index status shown in the header.
    /// e.g. "Not indexed", "Indexing…", "42 files · 318 symbols"
    pub index_status: RwSignal<String>,
    /// Index progress: 0.0..1.0 while indexing, -1.0 when idle.
    pub index_progress: RwSignal<f64>,

    // ── Thinking section state ─────────────────────────────────
    /// Whether the thinking section is collapsed.
    /// Auto-collapses when the final answer arrives.
    pub thinking_collapsed: RwSignal<bool>,
    /// Accumulated thinking steps for the current turn (cleared on new message).
    pub thinking_steps: RwSignal<im::Vector<ChatEntry>>,

    // ── Multimodal input state ──────────────────────────────────
    /// Images pasted/attached by the user (base64-encoded).
    pub attached_images: RwSignal<Vec<lapce_rpc::proxy::AttachedImageData>>,
    /// Whether the mic is currently recording.
    pub is_recording: RwSignal<bool>,
    /// Audio recorder instance.
    pub recorder: crate::audio_recorder::AudioRecorder,
}

impl AiChatData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        let editor = editors.make_local(cx, common.clone());
        let config = AiKeysConfig::load();
        let provider = config.defaults.provider.clone();
        let model = config.defaults.model.clone();

        Self {
            scope: cx,
            editor,
            entries: cx.create_rw_signal(im::Vector::new()),
            is_loading: cx.create_rw_signal(false),
            provider: cx.create_rw_signal(provider),
            model: cx.create_rw_signal(model),
            keys_config: cx.create_rw_signal(config),
            dropdown_open: cx.create_rw_signal(false),
            common,
            streaming_text: cx.create_rw_signal(String::new()),
            has_first_token: cx.create_rw_signal(false),
            scroll_to_bottom: cx.create_rw_signal(None),
            scroll_trigger: cx.create_rw_signal(0),
            index_status: cx.create_rw_signal("Checking…".to_string()),
            index_progress: cx.create_rw_signal(-1.0),
            conversation_id: cx.create_rw_signal(uuid::Uuid::new_v4().to_string()),
            thinking_collapsed: cx.create_rw_signal(false),
            thinking_steps: cx.create_rw_signal(im::Vector::new()),
            attached_images: cx.create_rw_signal(Vec::new()),
            is_recording: cx.create_rw_signal(false),
            recorder: crate::audio_recorder::AudioRecorder::new(),
        }
    }

    /// Whether user can chat — either signed into forge-search or has an API key.
    ///
    /// The forge-auth.json token is saved into the Lapce config directory
    /// (e.g. `~/Library/Application Support/dev.lapce.Lapce-Nightly/`) by the
    /// OAuth callback.  We check that path first, then fall back to checking
    /// whether the user has pasted a raw API key into ai-keys.toml.
    pub fn has_any_key(&self) -> bool {
        // Primary: forge-search auth token in Lapce's own config dir
        if let Some(dir) = lapce_core::directory::Directory::config_directory() {
            if dir.join("forge-auth.json").exists() {
                return true;
            }
        }

        // Also check the forge-agent config dir (dirs::config_dir()/forge-ide/)
        // in case the token was placed there manually for testing.
        if dirs::config_dir()
            .map(|d| d.join("forge-ide").join("forge-auth.json"))
            .map_or(false, |p| p.exists())
        {
            return true;
        }

        // Fallback: check for direct API keys
        self.keys_config.with_untracked(|c| c.has_any_key())
    }

    /// Save a key for a provider and update signals.
    pub fn save_provider_key(&self, provider: &str, key: &str) {
        self.keys_config.update(|c| {
            c.keys.insert(provider.to_string(), key.to_string());
            c.defaults.provider = provider.to_string();
            // Pick the first model for this provider
            if let Some(first_model) = models_for_provider(provider).first() {
                c.defaults.model = first_model.to_string();
            }
            c.save();
        });
        self.provider.set(provider.to_string());
        if let Some(first_model) = models_for_provider(provider).first() {
            self.model.set(first_model.to_string());
        }
    }

    /// Select a model (and its provider) and persist.
    pub fn select_model(&self, provider: &str, model: &str) {
        self.provider.set(provider.to_string());
        self.model.set(model.to_string());
        self.keys_config.update(|c| {
            c.defaults.provider = provider.to_string();
            c.defaults.model = model.to_string();
            c.save();
        });
        self.dropdown_open.set(false);
    }

    /// Get available models grouped by provider (only configured providers).
    pub fn available_models(&self) -> Vec<(String, Vec<&'static str>)> {
        let config = self.keys_config.get_untracked();
        let mut result = Vec::new();
        for &prov in ALL_PROVIDERS {
            if config.key_for(prov).is_some() {
                result.push((prov.to_string(), models_for_provider(prov)));
            }
        }
        result
    }

    pub fn send_message(&self) {
        let text: String = self.editor.doc().buffer.with_untracked(|b| b.to_string());
        if text.trim().is_empty() {
            return;
        }

        tracing::info!("[AI_CHAT] send_message called, text_len={}", text.len());

        // Determine if forge-search auth is available (no API key needed).
        let forge_search_auth = self.is_forge_search_authenticated();
        tracing::info!("[AI_CHAT] forge_search_auth={}", forge_search_auth);

        // Auto-index on first message if not indexed (only for forge-search)
        if forge_search_auth {
            let status = self.index_status.get_untracked();
            let is_indexed = status.contains("symbols indexed");
            let is_indexing = self.index_progress.get_untracked() >= 0.0;
            if !is_indexed && !is_indexing {
                // Trigger background indexing - doesn't block the message
                self.start_indexing();
            }
        }

        // Read current provider/model/key
        let (provider, model, api_key) = if forge_search_auth {
            // Route through forge-search — no API key required.
            ("forge-search".to_string(), "forge-search".to_string(), String::new())
        } else {
            let provider = self.provider.get_untracked();
            let model = self.model.get_untracked();
            let api_key = self
                .keys_config
                .with_untracked(|c| c.key_for(&provider).unwrap_or("").to_string());
            (provider, model, api_key)
        };

        // Add user message
        self.entries.update(|entries| {
            entries.push_back(new_message(ChatRole::User, text.clone()));
        });

        // Clear the editor and reset streaming state
        self.editor.doc().reload(lapce_xi_rope::Rope::from(""), true);
        self.is_loading.set(true);
        self.has_first_token.set(false);
        self.streaming_text.set(String::new());

        // Only check API key when NOT using forge-search
        if !forge_search_auth && api_key.is_empty() {
            self.entries.update(|entries| {
                entries.push_back(new_message(
                    ChatRole::System,
                    format!("No API key configured for {}. Please configure one in the setup.", provider),
                ));
            });
            self.is_loading.set(false);
            return;
        }

        // Send to the proxy via RPC using create_ext_action to safely
        // marshal the response back to the UI thread (RwSignal is !Send).
        // Text content arrives incrementally via CoreNotification::AgentTextChunk.
        // This callback only handles the final completion/error signal.
        let entries = self.entries;
        let is_loading = self.is_loading;

        let send = create_ext_action(self.scope, move |result: Result<lapce_rpc::proxy::ProxyResponse, lapce_rpc::RpcError>| {
            match result {
                Ok(resp) => {
                    use lapce_rpc::proxy::ProxyResponse;
                    match resp {
                        ProxyResponse::AgentDone { .. } => {
                            // Streaming text already arrived via notifications;
                            // just stop the loading indicator.
                        }
                        ProxyResponse::AgentError { error } => {
                            entries.update(|entries| {
                                entries.push_back(new_message(
                                    ChatRole::System,
                                    format!("Error: {}", error),
                                ));
                            });
                        }
                        _ => {}
                    }
                }
                Err(rpc_err) => {
                    entries.update(|entries| {
                        entries.push_back(new_message(
                            ChatRole::System,
                            format!("RPC Error: {}", rpc_err.message),
                        ));
                    });
                }
            }
            is_loading.set(false);
        });

        let conversation_id = self.conversation_id.get_untracked();
        let images = self.attached_images.get_untracked();
        self.attached_images.set(Vec::new()); // Clear after sending
        
        tracing::info!(
            "[AI_CHAT] Sending AgentPrompt via proxy RPC: conv_id={}, provider={}, model={}, images={}",
            conversation_id, provider, model, images.len()
        );
        
        self.common.proxy.request_async(
            lapce_rpc::proxy::ProxyRequest::AgentPrompt {
                prompt: text,
                provider,
                model,
                api_key,
                conversation_id,
                attached_images: images,
            },
            send,
        );
        
        tracing::info!("[AI_CHAT] AgentPrompt request_async called (queued to proxy)");
    }

    /// Add a pasted image (base64-encoded) to the pending attachments.
    pub fn add_image(&self, data: String, mime_type: String) {
        let count = self.attached_images.with_untracked(|imgs| imgs.len());
        let filename = format!("paste-{}.{}", count + 1, if mime_type.contains("jpeg") { "jpg" } else { "png" });
        self.attached_images.update(|imgs| {
            imgs.push(lapce_rpc::proxy::AttachedImageData {
                filename,
                data,
                mime_type,
            });
        });
    }

    /// Remove an attached image by index.
    pub fn remove_image(&self, index: usize) {
        self.attached_images.update(|imgs| {
            if index < imgs.len() {
                imgs.remove(index);
            }
        });
    }

    /// Send audio data to proxy for Whisper transcription.
    /// On success, inserts the transcript text into the editor.
    pub fn transcribe_audio(&self, audio_data: Vec<u8>) {
        let editor = self.editor.clone();
        let is_recording = self.is_recording;
        
        let send = create_ext_action(self.scope, move |result: Result<lapce_rpc::proxy::ProxyResponse, lapce_rpc::RpcError>| {
            is_recording.set(false);
            match result {
                Ok(lapce_rpc::proxy::ProxyResponse::AgentTranscription { text }) => {
                    if !text.is_empty() {
                        // Insert transcript into the editor so user can review/edit before sending
                        let current = editor.doc().buffer.with_untracked(|b| b.to_string());
                        let new_text = if current.trim().is_empty() {
                            text
                        } else {
                            format!("{} {}", current.trim(), text)
                        };
                        editor.doc().reload(lapce_xi_rope::Rope::from(&new_text), true);
                    }
                }
                Ok(lapce_rpc::proxy::ProxyResponse::AgentError { error }) => {
                    tracing::error!("Transcription failed: {}", error);
                }
                _ => {}
            }
        });
        
        self.common.proxy.request_async(
            lapce_rpc::proxy::ProxyRequest::AgentTranscribeAudio { audio_data },
            send,
        );
    }

    /// Toggle audio recording. On start: opens mic. On stop: transcribes via Whisper.
    pub fn toggle_recording(&self) {
        if self.recorder.is_recording() {
            // Stop and transcribe
            let wav_data = self.recorder.stop();
            self.is_recording.set(false);
            if !wav_data.is_empty() {
                self.transcribe_audio(wav_data);
            }
        } else {
            // Start recording
            match self.recorder.start() {
                Ok(()) => {
                    self.is_recording.set(true);
                }
                Err(e) => {
                    tracing::error!("Failed to start recording: {e}");
                    self.entries.update(|entries| {
                        entries.push_back(new_message(
                            ChatRole::System,
                            format!("Microphone error: {e}"),
                        ));
                    });
                }
            }
        }
    }

    /// Check if forge-search auth token exists (file on disk, no network).
    fn is_forge_search_authenticated(&self) -> bool {
        // Check Lapce's own config dir (where OAuth callback saves the token)
        if let Some(dir) = lapce_core::directory::Directory::config_directory() {
            if dir.join("forge-auth.json").exists() {
                return true;
            }
        }
        // Also check the forge-agent config dir
        if dirs::config_dir()
            .map(|d| d.join("forge-ide").join("forge-auth.json"))
            .map_or(false, |p| p.exists())
        {
            return true;
        }
        false
    }

    pub fn clear_chat(&self) {
        self.entries.update(|entries| entries.clear());
        self.streaming_text.set(String::new());
        self.has_first_token.set(false);
        self.is_loading.set(false);
        // New conversation = new conversation_id
        self.conversation_id.set(uuid::Uuid::new_v4().to_string());
    }

    /// Trigger the scroll-to-bottom signal.
    pub fn request_scroll_to_bottom(&self) {
        self.scroll_trigger.update(|v| *v += 1);
    }

    /// Fire a background request to forge-search `/health` to update the
    /// index status badge in the header.  Safe to call multiple times.
    pub fn refresh_index_status(&self) {
        if !self.is_forge_search_authenticated() {
            self.index_status.set("Not connected".to_string());
            return;
        }

        // Read the auth token from disk
        let token = Self::read_forge_token();
        let base_url = std::env::var("FORGE_SEARCH_URL")
            .unwrap_or_else(|_| "https://forge-search-production.up.railway.app".to_string());

        // Derive workspace_id from the open workspace path
        let workspace_name = self
            .common
            .workspace
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());

        let index_status = self.index_status;

        let send = create_ext_action(self.scope, move |status: String| {
            index_status.set(status);
        });

        std::thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
            {
                Ok(c) => c,
                Err(_) => {
                    send("Error".to_string());
                    return;
                }
            };

            // 1. Health check
            let health_url = format!("{}/health", base_url);
            match client.get(&health_url).send() {
                Ok(resp) if resp.status().is_success() => {}
                _ => {
                    send("Server offline".to_string());
                    return;
                }
            }

            // 2. Search with a tiny query to get total_nodes (workspace stats)
            let search_url = format!("{}/search", base_url);
            let mut req = client.post(&search_url).json(&serde_json::json!({
                "workspace_id": workspace_name,
                "query": "__ping__",
                "top_k": 1,
            }));
            if !token.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", token));
            }

            match req.send() {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>() {
                        let total = body["total_nodes"].as_i64().unwrap_or(0);
                        if total > 0 {
                            send(format!("{} symbols indexed", total));
                        } else {
                            send("Not indexed".to_string());
                        }
                    } else {
                        send("Connected".to_string());
                    }
                }
                Ok(resp) if resp.status().as_u16() == 401 => {
                    send("Auth expired".to_string());
                }
                _ => {
                    send("Not indexed".to_string());
                }
            }
        });
    }

    /// Start indexing the workspace codebase via forge-search.
    /// Uses the consolidated proxy RPC to avoid code duplication.
    /// Progress updates arrive via CoreNotification::IndexProgress.
    pub fn start_indexing(&self) {
        // Prevent double-indexing
        if self.index_progress.get_untracked() >= 0.0 {
            return;
        }

        // Set initial progress to indicate indexing started
        self.index_status.set("Starting index...".to_string());
        self.index_progress.set(0.0);

        // Send RPC to proxy - progress updates will come via CoreNotification::IndexProgress
        // which is handled in window_tab.rs handle_core_notification
        self.common.proxy.request_async(
            lapce_rpc::proxy::ProxyRequest::IndexWorkspace {},
            |_result| {
                // Response just confirms indexing started.
                // Actual progress/completion comes via CoreNotification.
            },
        );
    }

    /// Read the forge-search JWT from disk (checks both Lapce and agent config dirs).
    fn read_forge_token() -> String {
        // Lapce config dir first
        if let Some(dir) = Directory::config_directory() {
            let path = dir.join("forge-auth.json");
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(t) = v["token"].as_str() {
                        return t.to_string();
                    }
                }
            }
        }
        // Agent config dir
        if let Some(dir) = dirs::config_dir() {
            let path = dir.join("forge-ide").join("forge-auth.json");
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(t) = v["token"].as_str() {
                        return t.to_string();
                    }
                }
            }
        }
        String::new()
    }
}

impl KeyPressFocus for AiChatData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: crate::keypress::condition::Condition) -> bool {
        matches!(condition, crate::keypress::condition::Condition::PanelFocus)
    }

    fn run_command(
        &self,
        command: &LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                // Enter sends the message
                if let CommandKind::Edit(EditCommand::InsertNewLine) = command.kind {
                    self.send_message();
                    return CommandExecuted::Yes;
                }

                return self.editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::No
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}
