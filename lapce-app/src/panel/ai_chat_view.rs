//! AI Chat panel view for the right sidebar.
//!
//! Provides a chat interface for interacting with the AI coding agent.
//! Shows a setup view when no API keys are configured.
//!
//! Key UX features:
//! - **Deferred markdown:** streaming text renders as plain text (fast);
//!   markdown is parsed once when streaming completes.
//! - **Thinking indicator:** pulsing label shown between Send and first token.
//! - **Collapsible tool cards:** compact single-line cards with elapsed time.
//! - **Auto-scroll:** scroll follows new content automatically.

use std::rc::Rc;

use std::sync::atomic::AtomicU64;

use floem::{
    IntoView, View,
    event::EventListener,
    ext_event::create_ext_action,
    kurbo::{Point, Size},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty, img, label, rich_text, scroll, stack,
        svg,
    },
};

use super::position::PanelPosition;
use crate::{
    ai_chat::{
        AiChatData, ChatEntry, ChatEntryKind, ChatRole, ChatToolCall, ToolCallStatus,
        ChatThinkingStep, ChatPlan, ChatPlanStep, ChatPlanStepStatus, ChatServerToolCall,
        ALL_PROVIDERS, models_for_provider,
    },
    config::{color::LapceColor, icon::LapceIcons},
    text_input::TextInputBuilder,
    window_tab::{Focus, WindowTabData},
};

// ── View functions ───────────────────────────────────────────────

/// Build the AI chat panel.
/// Shows setup view if no keys configured, otherwise the chat view.
pub fn ai_chat_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let _config = window_tab_data.common.config;
    let chat_data = window_tab_data.ai_chat.clone();

    let keys_config = chat_data.keys_config;

    // Reactive: rebuild when keys or forge-auth change (track keys_config so adding a key updates the view)
    container(
        dyn_stack(
            move || {
                let _ = keys_config.with(|_| ()); // track so UI updates when API keys change
                let has_key = chat_data.has_any_key(); // forge-auth.json (same path as agent) or API keys
                vec![has_key]
            },
            |v| *v,
            {
                let window_tab_data = window_tab_data.clone();
                move |has_key| {
                    if has_key {
                        chat_view(window_tab_data.clone()).into_any()
                    } else {
                        setup_view(window_tab_data.clone()).into_any()
                    }
                }
            },
        )
        .style(|s| s.flex_col().size_pct(100.0, 100.0)),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}

// ── Setup View (first-run API key configuration) ────────────────

fn setup_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let chat_data = window_tab_data.ai_chat.clone();
    let scope = chat_data.scope;
    let keys_config = chat_data.keys_config;
    
    // Signal to show "Waiting for authentication..." status
    let auth_status = scope.create_rw_signal(String::new());

    container(
        stack((
            // ── Title ──
            label(|| "Welcome to Forge AI".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32 + 4.0)
                    .font_bold()
                    .margin_bottom(16.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            // ── Description ──
            label(|| "Sign in to get AI-powered code intelligence.".to_string())
                .style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .margin_bottom(8.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
            label(|| "Semantic search, call tracing, impact analysis, and chat.".to_string())
                .style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .margin_bottom(24.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
            // ── Sign in with GitHub (primary) ──
            stack((
                svg(move || {
                    let svg_str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="currentColor" d="M12 2A10 10 0 0 0 2 12c0 4.42 2.87 8.17 6.84 9.5c.5.08.66-.23.66-.5v-1.69c-2.77.6-3.36-1.34-3.36-1.34c-.46-1.16-1.11-1.47-1.11-1.47c-.91-.62.07-.6.07-.6c1 .07 1.53 1.03 1.53 1.03c.87 1.52 2.34 1.07 2.91.83c.09-.65.35-1.09.63-1.34c-2.22-.25-4.55-1.11-4.55-4.92c0-1.11.38-2 1.03-2.71c-.1-.25-.45-1.29.1-2.64c0 0 .84-.27 2.75 1.02c.79-.22 1.65-.33 2.5-.33s1.71.11 2.5.33c1.91-1.29 2.75-1.02 2.75-1.02c.55 1.35.2 2.39.1 2.64c.65.71 1.03 1.6 1.03 2.71c0 3.82-2.34 4.66-4.57 4.91c.36.31.69.92.69 1.85V21c0 .27.16.59.67.5C19.14 20.16 22 16.42 22 12A10 10 0 0 0 12 2"/></svg>"#;
                    svg_str.to_string()
                }).style(|s| s.width(20.0).height(20.0).margin_right(8.0)),
                label(|| "Sign in with GitHub".to_string()),
            ))
            .style(|s| s.flex_row().items_center().justify_center())
            .on_click_stop(move |_| {
                start_oauth_flow(scope, "github", auth_status, keys_config);
            })
            .style(move |s| {
                let config = config.get();
                s.padding_horiz(24.0)
                    .padding_vert(12.0)
                    .width_pct(100.0)
                    .justify_center()
                    .font_bold()
                    .font_size(config.ui.font_size() as f32)
                    .cursor(CursorStyle::Pointer)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    .hover(|s| {
                        s.background(
                            config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.2),
                        )
                    })
            }),
            // ── Sign in with Google ──
            stack((
                svg(move || {
                    let svg_str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="currentColor" d="M21.35 11.1h-9.17v2.73h6.51c-.33 3.81-3.5 5.44-6.5 5.44C8.36 19.27 5 16.25 5 12c0-4.1 3.2-7.27 7.2-7.27c3.09 0 4.9 1.97 4.9 1.97L19 4.72S16.56 2 12.1 2C6.42 2 2.03 6.8 2.03 12c0 5.05 4.13 10 10.22 10c5.35 0 9.25-3.67 9.25-9.09c0-1.15-.15-1.81-.15-1.81"/></svg>"#;
                    svg_str.to_string()
                }).style(|s| s.width(20.0).height(20.0).margin_right(8.0)),
                label(|| "Sign in with Google".to_string()),
            ))
            .style(|s| s.flex_row().items_center().justify_center())
            .on_click_stop(move |_| {
                start_oauth_flow(scope, "google", auth_status, keys_config);
            })
            .style(move |s| {
                let config = config.get();
                s.padding_horiz(24.0)
                    .padding_vert(12.0)
                    .width_pct(100.0)
                    .justify_center()
                    .margin_top(8.0)
                    .font_size(config.ui.font_size() as f32)
                    .cursor(CursorStyle::Pointer)
                    .color(config.color(LapceColor::EDITOR_DIM))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                        .hover(|s| {
                            s.background(
                                config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.3),
                            )
                        })
                }),
            // ── Auth status message ──
            label(move || auth_status.get())
                .style(move |s| {
                    let config = config.get();
                    let status = auth_status.get();
                    s.margin_top(16.0)
                        .font_size(config.ui.font_size() as f32 - 1.0)
                        .color(if status.contains("Error") {
                            config.color(LapceColor::LAPCE_ERROR)
                        } else {
                            config.color(LapceColor::EDITOR_DIM)
                        })
                        .display(if status.is_empty() {
                            floem::style::Display::None
                        } else {
                            floem::style::Display::Flex
                        })
                }),
        ))
        .style(|s| {
            s.flex_col()
                .width_pct(100.0)
                .max_width(400.0)
                .items_start()
        }),
    )
    .style(move |s| {
        let config = config.get();
        s.size_pct(100.0, 100.0)
            .padding(24.0)
            .justify_center()
            .items_center()
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}

/// Start OAuth flow with polling.
/// Opens browser, then polls server for token.
fn start_oauth_flow(
    scope: Scope,
    provider: &'static str,
    auth_status: floem::reactive::RwSignal<String>,
    keys_config: floem::reactive::RwSignal<crate::ai_chat::AiKeysConfig>,
) {
    use lapce_core::directory::Directory;
    
    // Generate unique session ID
    let session_id = uuid::Uuid::new_v4().to_string();
    
    // Open browser with poll state
    let base = std::env::var("FORGE_SEARCH_URL")
        .unwrap_or_else(|_| "https://forge-search-production.up.railway.app".to_string());
    let url = format!("{}/auth/{}?state=poll-{}", base, provider, session_id);
    
    if let Err(e) = open::that(&url) {
        auth_status.set(format!("Error opening browser: {}", e));
        return;
    }
    
    auth_status.set("Waiting for authentication... (complete sign-in in browser)".to_string());
    
    // Start polling in background
    let poll_url = format!("{}/auth/poll/{}", base, session_id);
    
    let on_result = create_ext_action(scope, move |result: Result<(String, String, String), String>| {
        match result {
            Ok((token, email, name)) => {
                // Save token to disk
                if let Some(dir) = Directory::config_directory() {
                    let auth_data = serde_json::json!({
                        "token": token,
                        "email": email,
                        "name": name,
                    });
                    if let Ok(content) = serde_json::to_string_pretty(&auth_data) {
                        let _ = std::fs::write(dir.join("forge-auth.json"), content);
                    }
                }
                // Trigger keys_config signal so dyn_stack re-evaluates has_any_key()
                // which will now find the forge-auth.json file on disk.
                // A no-op update still notifies all subscribers of the signal.
                keys_config.update(|_| {});
                auth_status.set(format!("Signed in as {}", email));
            }
            Err(e) => {
                auth_status.set(format!("Error: {}", e));
            }
        }
    });
    
    std::thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                on_result(Err(format!("HTTP client error: {}", e)));
                return;
            }
        };
        
        // Poll for up to 5 minutes (60 attempts, 5 seconds apart)
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_secs(5));
            
            match client.get(&poll_url).send() {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>() {
                        let status = json["status"].as_str().unwrap_or("pending");
                        match status {
                            "success" => {
                                let token = json["token"].as_str().unwrap_or("").to_string();
                                let email = json["email"].as_str().unwrap_or("").to_string();
                                let name = json["name"].as_str().unwrap_or("").to_string();
                                on_result(Ok((token, email, name)));
                                return;
                            }
                            "expired" => {
                                on_result(Err("Session expired. Please try again.".to_string()));
                                return;
                            }
                            "pending" => {
                                // Continue polling
                            }
                            _ => {
                                // Continue polling
                            }
                        }
                    }
                }
                Err(_) => {
                    // Network error, continue polling
                }
            }
        }
        
        on_result(Err("Authentication timed out. Please try again.".to_string()));
    });
}

/// Provider selector: three clickable labels in a row.
fn provider_selector(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    selected: floem::reactive::RwSignal<String>,
) -> impl View {
    let providers: Vec<(&'static str, &'static str)> = vec![
        ("gemini", "Gemini"),
        ("anthropic", "Anthropic"),
        ("openai", "OpenAI"),
    ];

    dyn_stack(
        move || providers.clone(),
        |(id, _)| *id,
        move |(id, display_name)| {
            let id_str = id.to_string();
            let id_click = id_str.clone();
            label(move || display_name.to_string())
                .on_click_stop(move |_| {
                    selected.set(id_click.clone());
                })
                .style(move |s| {
                    let config = config.get();
                    let is_selected = selected.get() == id_str;
                    s.padding_horiz(14.0)
                        .padding_vert(8.0)
                        .font_size(config.ui.font_size() as f32)
                        .cursor(CursorStyle::Pointer)
                        .border(1.0)
                        .border_color(if is_selected {
                            config.color(LapceColor::LAPCE_ICON_ACTIVE)
                        } else {
                            config.color(LapceColor::LAPCE_BORDER)
                        })
                        .color(if is_selected {
                            config.color(LapceColor::LAPCE_ICON_ACTIVE)
                        } else {
                            config.color(LapceColor::PANEL_FOREGROUND)
                        })
                        .apply_if(is_selected, |s| {
                            s.background(
                                config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.15),
                            )
                        })
                        .apply_if(!is_selected, |s| {
                            s.hover(|s| {
                                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                            })
                        })
                })
        },
    )
    .style(|s| s.flex_row().gap(8.0).width_pct(100.0))
}

// ── Chat View (main chat UI) ────────────────────────────────────

fn chat_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let chat_data = window_tab_data.ai_chat.clone();
    let chat_data_clear = chat_data.clone();
    let wtd = window_tab_data.clone();

    let internal_command = window_tab_data.common.internal_command;
    let proxy = window_tab_data.common.proxy.clone();

    stack((
        // ── Header ──────────────────────────────────────────
        chat_header(config, chat_data_clear),
        // ── Message list (scrollable) with auto-scroll ──────
        chat_message_list(config, chat_data.clone(), internal_command, proxy),
        // ── AI Diff toolbar (only shown when pending diffs exist) ──
        ai_diff_toolbar(wtd),
        // ── Input area at the bottom ────────────────────────
        chat_input_area(window_tab_data, chat_data),
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

/// Toolbar showing Accept All / Reject All buttons when there are pending AI diffs.
fn ai_diff_toolbar(window_tab_data: Rc<WindowTabData>) -> impl View {
    let config = window_tab_data.common.config;
    let ai_diffs = window_tab_data.ai_diffs.clone();
    let has_pending = ai_diffs.has_pending;
    let version = ai_diffs.version;
    let proxy_accept = window_tab_data.common.proxy.clone();
    let proxy_reject = window_tab_data.common.proxy.clone();
    let ai_diffs_accept = ai_diffs.clone();
    let ai_diffs_reject = ai_diffs.clone();

    container(
        stack((
            // Diff count label
            label(move || {
                let _v = version.get(); // re-trigger on changes
                let count = ai_diffs.diffs.with(|d| d.len());
                format!("{} pending diff(s)", count)
            })
            .style(move |s| {
                let config = config.get();
                s.flex_grow(1.0)
                    .padding_horiz(8.0)
                    .font_size(config.ui.font_size() as f32 - 1.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            // Accept All button
            label(|| "Accept All".to_string())
                .on_click_stop(move |_| {
                    ai_diffs_accept.accept_all();
                    proxy_accept.request_async(
                        lapce_rpc::proxy::ProxyRequest::AgentDiffAcceptAll {},
                        |_| {},
                    );
                })
                .style(move |s| {
                    let config = config.get();
                    s.padding_horiz(10.0)
                        .padding_vert(3.0)
                        .margin_right(4.0)
                        .border_radius(4.0)
                        .font_size(config.ui.font_size() as f32 - 1.0)
                        .font_bold()
                        .cursor(CursorStyle::Pointer)
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                        .background(config.color(LapceColor::COMPLETION_CURRENT))
                        .hover(|s| {
                            s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                        })
                }),
            // Reject All button
            label(|| "Reject All".to_string())
                .on_click_stop(move |_| {
                    ai_diffs_reject.reject_all();
                    proxy_reject.request_async(
                        lapce_rpc::proxy::ProxyRequest::AgentDiffRejectAll {},
                        |_| {},
                    );
                })
                .style(move |s| {
                    let config = config.get();
                    s.padding_horiz(10.0)
                        .padding_vert(3.0)
                        .border_radius(4.0)
                        .font_size(config.ui.font_size() as f32 - 1.0)
                        .font_bold()
                        .cursor(CursorStyle::Pointer)
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                        .background(config.color(LapceColor::LAPCE_ERROR))
                        .hover(|s| {
                            s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                        })
                }),
        ))
        .style(|s| {
            s.flex_row()
                .items_center()
                .width_pct(100.0)
                .padding(6.0)
        }),
    )
    .style(move |s| {
        let config = config.get();
        let visible = has_pending.get();
        s.width_pct(100.0)
            .border_top(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .apply_if(!visible, |s| s.display(floem::style::Display::None))
    })
}

/// Header with title, index status badge, and clear button.
fn chat_header(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let chat_data_clear = chat_data.clone();
    let chat_data_badge = chat_data.clone();

    // Kick off a background index status check
    chat_data.refresh_index_status();

    container(
        stack((
            // Title
            stack((
                svg(move || config.get().ui_svg(LapceIcons::AI_CHAT))
                    .style(move |s| {
                        let config = config.get();
                        let icon_size = config.ui.icon_size() as f32;
                        s.size(icon_size, icon_size)
                            .margin_right(6.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                label(|| "Forge AI".to_string()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                        .font_bold()
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                }),
            ))
            .style(|s| s.items_center()),
            // Index status badge with Index button + progress bar
            index_status_badge(config, chat_data_badge),
            // Spacer
            empty().style(|s| s.flex_grow(1.0)),
            // Clear button
            label(|| "Clear".to_string())
                .style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                        .padding_horiz(8.0)
                        .padding_vert(2.0)
                        .cursor(CursorStyle::Pointer)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .hover(|s| {
                            s.background(
                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
                .on_click_stop(move |_| {
                    chat_data_clear.clear_chat();
                }),
        ))
        .style(|s| s.items_center().gap(8.0).width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}

/// Index status area: shows status label and progress bar during indexing.
/// Auto-indexing happens on first message, so no manual button needed.
fn index_status_badge(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let index_status = chat_data.index_status;
    let index_progress = chat_data.index_progress;

    stack((
        // ── Layer 1: status label (hidden during indexing) ──
        label(move || index_status.get())
            .style(move |s| {
                let config = config.get();
                let status = index_status.get();
                let progress = index_progress.get();
                let is_indexing = progress >= 0.0;
                // "N symbols indexed" = success; otherwise dim
                let is_indexed = status.contains("symbols indexed");
                let color = if is_indexed {
                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                } else {
                    config.color(LapceColor::EDITOR_DIM)
                };
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .padding_horiz(8.0)
                    .padding_vert(3.0)
                    .color(color)
                    .apply_if(is_indexing, |s| s.hide())
            }),

        // ── Layer 2: progress bar (visible only during indexing) ──
        index_progress_view(config, index_status, index_progress),
    ))
    .style(|s| s.flex_row().items_center())
}

/// Progress bar shown during codebase indexing.
fn index_progress_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    index_status: floem::reactive::RwSignal<String>,
    index_progress: floem::reactive::RwSignal<f64>,
) -> impl View {
    stack((
        // Status text (e.g. "Indexing… 23/120 files")
        label(move || index_status.get())
            .style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    .margin_right(6.0)
            }),
        // Progress bar track
        container(
            // Progress bar fill
            empty().style(move |s| {
                let progress = index_progress.get().max(0.0).min(1.0);
                let config = config.get();
                s.height(4.0)
                    .width_pct(progress * 100.0)
                    .background(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    .border_radius(2.0)
            }),
        )
        .style(move |s| {
            let config = config.get();
            s.width(80.0)
                .height(4.0)
                .background(config.color(LapceColor::LAPCE_BORDER))
                .border_radius(2.0)
        }),
    ))
    .style(move |s| {
        let is_indexing = index_progress.get() >= 0.0;
        s.flex_row().items_center()
            .apply_if(!is_indexing, |s| s.hide())
    })
}

/// Scrollable chat message list with thinking section and auto-scroll.
fn chat_message_list(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
    proxy: lapce_rpc::proxy::ProxyRpcHandler,
) -> impl View {
    let entries = chat_data.entries;
    let chat_data_dropdown = chat_data.clone();
    let chat_data_thinking = chat_data.clone();
    let is_loading = chat_data.is_loading;
    let has_first_token = chat_data.has_first_token;
    let streaming_text = chat_data.streaming_text;
    let scroll_trigger = chat_data.scroll_trigger;

    stack((
        // ── Dropdown overlay (shown when dropdown_open is true) ──
        model_dropdown_panel(config, chat_data_dropdown),
        // ── Messages + thinking section + streaming indicator ──
        scroll(
            stack((
                // ── Chat entries ──
                dyn_stack(
                    move || {
                        let entries = entries.get();
                        entries.iter().cloned().collect::<Vec<_>>()
                    },
                    |entry: &ChatEntry| entry.key(),
                    {
                        let proxy = proxy.clone();
                        move |entry| chat_entry_view(config, entry, internal_command, proxy.clone())
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0).min_width(0.0)),

                // ── Collapsible thinking section ──
                // Shows server-side activity (thinking steps, server tool calls, plan).
                // Auto-collapses when the final answer arrives.
                thinking_section(config, chat_data_thinking, internal_command, proxy.clone()),

                // ── Streaming text preview (plain text, fast) ──
                // Shown while the assistant is actively streaming.
                // This avoids re-parsing markdown on every chunk.
                {
                    label(move || {
                        let text = streaming_text.get();
                        if text.is_empty() {
                            String::new()
                        } else {
                            text
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let text = streaming_text.get();
                        let loading = is_loading.get();
                        let has_token = has_first_token.get();
                        let show = loading && has_token && !text.is_empty();
                        s.font_size((config.ui.font_size() as f32 - 2.0).max(11.0))
                            .width_pct(100.0)
                            .min_width(0.0)
                            .padding_horiz(10.0)
                            .padding_vert(6.0)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            .background(config.color(LapceColor::EDITOR_BACKGROUND))
                            .apply_if(!show, |s| s.hide())
                    })
                },

                // ── Thinking indicator ──
                // Shown when loading but no text has arrived yet.
                thinking_indicator(config, is_loading, has_first_token),
            ))
            .style(|s| {
                s.flex_col()
                    .width_pct(100.0)
                    .min_width(0.0) // Allow shrinking below content width
                    .padding_vert(8.0)
            }),
        )
        .scroll_to(move || {
            // React to scroll_trigger changes to auto-scroll to bottom
            let _trigger = scroll_trigger.get();
            let loading = is_loading.get();
            if loading {
                Some(Point::new(0.0, f64::MAX))
            } else {
                None
            }
        })
        .style(|s| {
            s.flex_grow(1.0)
                .flex_basis(0.0)
                .width_pct(100.0)
                .min_width(0.0) // Allow shrinking below content width
                // Prevent horizontal scroll — constrain child width to panel
                .set(floem::style::OverflowX, floem::taffy::style::Overflow::Hidden)
        }),
    ))
    .style(|s| {
        s.flex_col()
            .flex_grow(1.0)
            .flex_basis(0.0)
            .width_pct(100.0)
            .min_width(0.0) // Allow shrinking below content width
    })
}

/// "Forge is thinking..." indicator with pulsing dots.
fn thinking_indicator(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    is_loading: floem::reactive::RwSignal<bool>,
    has_first_token: floem::reactive::RwSignal<bool>,
) -> impl View {
    container(
        stack((
            // Pulsing dot
            label(|| "\u{25CF}".to_string()) // filled circle
                .style(move |s| {
                    let config = config.get();
                    s.font_size(8.0)
                        .margin_right(8.0)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
            label(|| "Forge is thinking...".to_string())
                .style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .font_style(floem::text::Style::Italic)
                }),
        ))
        .style(|s| s.items_center()),
    )
    .style(move |s| {
        let loading = is_loading.get();
        let has_token = has_first_token.get();
        let show = loading && !has_token;
        s.padding_horiz(12.0)
            .padding_vert(10.0)
            .width_pct(100.0)
            .apply_if(!show, |s| s.hide())
    })
}

/// The dropdown panel that appears below the header when model is clicked.
fn model_dropdown_panel(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let dropdown_open = chat_data.dropdown_open;
    let keys_config = chat_data.keys_config;
    let current_model = chat_data.model;

    container(
        dyn_stack(
            move || {
                if !dropdown_open.get() {
                    return vec![];
                }
                let config_val = keys_config.get();
                let mut items: Vec<(String, String, bool)> = Vec::new();
                for &prov in ALL_PROVIDERS {
                    if config_val.key_for(prov).is_some() {
                        for model in models_for_provider(prov) {
                            let is_current = current_model.get() == model;
                            items.push((prov.to_string(), model.to_string(), is_current));
                        }
                    }
                }
                items
            },
            |(prov, model, _)| format!("{}/{}", prov, model),
            {
                let chat_data = chat_data.clone();
                move |(prov, model, is_current): (String, String, bool)| {
                    let chat_data = chat_data.clone();
                    let prov_click = prov.clone();
                    let model_click = model.clone();
                    let model_display = model.clone();
                    let prov_display = prov.clone();

                    label(move || format!("{} / {}", prov_display, model_display))
                        .on_click_stop(move |_| {
                            chat_data.select_model(&prov_click, &model_click);
                        })
                        .style(move |s: floem::style::Style| {
                            let config = config.get();
                            s.width_pct(100.0)
                                .padding_horiz(12.0)
                                .padding_vert(6.0)
                                .font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                                .cursor(CursorStyle::Pointer)
                                .color(if is_current {
                                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                                } else {
                                    config.color(LapceColor::PANEL_FOREGROUND)
                                })
                                .apply_if(is_current, |s| {
                                    s.font_bold()
                                        .background(
                                            config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.1),
                                        )
                                })
                                .hover(|s| {
                                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                })
                        })
                }
            },
        )
        .style(|s: floem::style::Style| s.flex_col().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        let is_open = dropdown_open.get();
        s.width_pct(100.0)
            .border_bottom(if is_open { 1.0 } else { 0.0 })
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .apply_if(!is_open, |s| s.hide())
    })
}

/// View for a single chat entry (message or tool call).
fn chat_entry_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    entry: ChatEntry,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
    proxy: lapce_rpc::proxy::ProxyRpcHandler,
) -> impl View {
    match entry.kind {
        ChatEntryKind::Message { role, content } => {
            message_bubble(config, role, content).into_any()
        }
        ChatEntryKind::ToolCall(tc) => {
            // Approval-pending tools get Accept/Reject buttons
            if tc.status == ToolCallStatus::WaitingApproval {
                return approval_card(config, tc, proxy, internal_command).into_any();
            }
            // File-related tools get a special clickable file block
            let is_file_tool = matches!(
                tc.name.as_str(),
                "read_file" | "write_to_file" | "replace_in_file" | "apply_patch" | "delete_file"
            );
            if is_file_tool {
                file_tool_card(config, tc, internal_command).into_any()
            } else {
                tool_call_card(config, tc).into_any()
            }
        }
        ChatEntryKind::ThinkingStep(step) => {
            thinking_step_view(config, step).into_any()
        }
        ChatEntryKind::Plan(plan) => {
            plan_view(config, plan).into_any()
        }
        ChatEntryKind::ServerToolCall(tc) => {
            server_tool_call_view(config, tc).into_any()
        }
    }
}

/// A chat message bubble.
///
/// For completed assistant messages, content is rendered as markdown.
/// User and system messages are plain text.
///
/// Note: During active streaming, the streaming_text label (above, in
/// chat_message_list) shows the in-progress text. The finalized entry
/// is only created when streaming completes, so this function always
/// gets the final content and can safely parse markdown once.
fn message_bubble(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    role: ChatRole,
    content: String,
) -> impl View {
    let is_user = role == ChatRole::User;
    let is_system = role == ChatRole::System;
    let is_assistant = role == ChatRole::Assistant;
    let role_label = match role {
        ChatRole::User => "You",
        ChatRole::Assistant => "Forge",
        ChatRole::System => "System",
    };

    // Parse markdown once for assistant messages (only called for finalized content)
    let md_content = if is_assistant {
        let cfg = config.get_untracked();
        let chat_font = (cfg.ui.font_size() as f32 - 2.0).max(11.0);
        crate::markdown::parse_markdown_sized(&content, 1.4, &cfg, chat_font)
    } else {
        Vec::new()
    };

    container(
        stack((
            // Role label
            label(move || role_label.to_string()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .font_bold()
                    .margin_bottom(3.0)
                    .color(if is_user {
                        config.color(LapceColor::LAPCE_ICON_ACTIVE)
                    } else if is_system {
                        config.color(LapceColor::LAPCE_WARN)
                    } else {
                        config.color(LapceColor::PANEL_FOREGROUND)
                    })
            }),
            // Message content: markdown for assistant, plain text otherwise
            if is_assistant {
                let id_counter = AtomicU64::new(0);
                dyn_stack(
                    move || md_content.clone(),
                    move |_| id_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    move |md_item| {
                        use crate::markdown::MarkdownContent;
                        match md_item {
                            MarkdownContent::Text(text_layout) => container(
                                rich_text(move || text_layout.clone())
                                    .style(|s| s.width_pct(100.0).min_width(0.0)),
                            )
                            .style(|s| s.width_pct(100.0).min_width(0.0))
                            .into_any(),
                            MarkdownContent::Separator => container(
                                empty().style(move |s| {
                                    let config = config.get();
                                    s.width_pct(100.0)
                                        .margin_vert(4.0)
                                        .height(1.0)
                                        .background(
                                            config.color(LapceColor::LAPCE_BORDER),
                                        )
                                }),
                            )
                            .style(|s| s.width_pct(100.0))
                            .into_any(),
                            MarkdownContent::Image { url, .. } => {
                                // Handle both data URIs and regular URLs
                                if url.starts_with("data:image/") {
                                    // Data URI image (e.g., mermaid diagrams from server)
                                    // Format: data:image/png;base64,{base64_data}
                                    let png_data = if let Some(base64_start) = url.find("base64,") {
                                        let base64_str = &url[base64_start + 7..];
                                        use base64::{Engine as _, engine::general_purpose::STANDARD};
                                        STANDARD.decode(base64_str).unwrap_or_default()
                                    } else {
                                        Vec::new()
                                    };
                                    
                                    if png_data.is_empty() {
                                        // Decoding failed, show placeholder
                                        container(empty()).into_any()
                                    } else {
                                        container(
                                            img(move || png_data.clone())
                                                .style(move |s| {
                                                    let config = config.get();
                                                    s.width_pct(100.0)
                                                        .min_height(80.0)
                                                        .padding(8.0)
                                                        .border_radius(6.0)
                                                        .border(1.0)
                                                        .border_color(
                                                            config.color(LapceColor::LAPCE_BORDER),
                                                        )
                                                        .background(
                                                            config.color(LapceColor::EDITOR_BACKGROUND),
                                                        )
                                                }),
                                        )
                                        .style(|s| s.width_pct(100.0).margin_vert(6.0))
                                        .into_any()
                                    }
                                } else {
                                    // Regular URL - placeholder for now
                                    // TODO: Could fetch async and display
                                    container(empty()).into_any()
                                }
                            }
                        }
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0).min_width(0.0))
                .into_any()
            } else {
                label(move || content.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size((config.ui.font_size() as f32 - 2.0).max(11.0))
                            .width_pct(100.0)
                            .min_width(0.0)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    })
                    .into_any()
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0).min_width(0.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border_bottom(1.0)
            .border_color(
                config
                    .color(LapceColor::LAPCE_BORDER)
                    .multiply_alpha(0.3),
            )
            .apply_if(!is_user, |s| {
                s.background(config.color(LapceColor::EDITOR_BACKGROUND))
            })
    })
}

/// A clickable file block for file-related tool calls (read, write, replace, etc.).
///
/// Shows:  [file icon]  filename  [status icon]
///
/// Clicking opens the file in the editor.
fn file_tool_card(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatToolCall,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
) -> impl View {
    let is_success = tc.status == ToolCallStatus::Success;
    let is_error = tc.status == ToolCallStatus::Error;
    let is_running = tc.status == ToolCallStatus::Running;

    // Extract file path from the JSON arguments
    let file_path: Option<std::path::PathBuf> = serde_json::from_str::<serde_json::Value>(&tc.arguments)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(std::path::PathBuf::from));

    // Display: just the filename (or relative path if short enough)
    let display_name = file_path
        .as_ref()
        .map(|p| {
            let s = p.to_string_lossy();
            if s.len() > 40 {
                // Show …/last_two_components
                let components: Vec<_> = p.components().collect();
                if components.len() > 2 {
                    format!(
                        "…/{}",
                        components[components.len() - 2..]
                            .iter()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/")
                    )
                } else {
                    s.to_string()
                }
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| tc.name.clone());

    // Action label
    let action = match tc.name.as_str() {
        "read_file" => "Read",
        "write_to_file" => "Created",
        "replace_in_file" => "Edited",
        "apply_patch" => "Patched",
        "delete_file" => "Deleted",
        _ => "",
    };

    // Status icon
    let status_icon = match &tc.status {
        ToolCallStatus::Pending => "\u{25CB}",
        ToolCallStatus::WaitingApproval => "\u{26A0}",
        ToolCallStatus::Running => "\u{25CF}",
        ToolCallStatus::Success => "\u{2713}",
        ToolCallStatus::Error => "\u{2717}",
        ToolCallStatus::Rejected => "\u{2718}",
    };

    let click_path = file_path.clone();

    container(
        stack((
            // File icon (SVG)
            {
                let file_icon_cfg = config;
                svg(move || {
                    let cfg = file_icon_cfg.get();
                    cfg.ui_svg(LapceIcons::FILE)
                })
                .style(move |s| {
                    let config = config.get();
                    s.size(14.0, 14.0)
                        .margin_right(6.0)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                })
            },
            // File name
            label(move || display_name.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                    .font_bold()
                    .flex_grow(1.0)
                    .min_width(0.0)
                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
            }),
            // Action label
            label(move || action.to_string()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .margin_right(6.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            // Status icon
            label(move || status_icon.to_string()).style(move |s| {
                let config = config.get();
                let color = if is_running {
                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                } else if is_success {
                    config.color(LapceColor::EDITOR_FOREGROUND)
                } else if is_error {
                    config.color(LapceColor::LAPCE_ERROR)
                } else {
                    config.color(LapceColor::EDITOR_DIM)
                };
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .color(color)
            }),
        ))
        .on_click_stop(move |_| {
            if let Some(ref path) = click_path {
                internal_command.send(crate::command::InternalCommand::OpenFile {
                    path: path.clone(),
                });
            }
        })
        .style(move |s| {
            s.items_center()
                .width_pct(100.0)
                .cursor(if file_path.is_some() {
                    CursorStyle::Pointer
                } else {
                    CursorStyle::Default
                })
        }),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .margin_horiz(8.0)
            .margin_vert(2.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border_radius(6.0)
            .border(1.0)
            .border_color(
                config
                    .color(LapceColor::LAPCE_BORDER)
                    .multiply_alpha(0.5),
            )
            .background(
                config
                    .color(LapceColor::PANEL_BACKGROUND)
                    .multiply_alpha(0.6),
            )
            .hover(|s| {
                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

/// Approval card — shown when a mutating tool needs user permission.
/// Displays the tool name, summary, and Accept/Reject/View buttons.
fn approval_card(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatToolCall,
    proxy: lapce_rpc::proxy::ProxyRpcHandler,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
) -> impl View {
    let tool_name = tc.name.clone();
    let summary = tc.output.clone().unwrap_or_else(|| format!("Execute: {}", tool_name));
    let tc_id = tc.id.clone();
    let tc_id_reject = tc.id.clone();
    let proxy_accept = proxy.clone();
    let proxy_reject = proxy.clone();

    // Extract file path from arguments for "View" button (for file-related tools)
    let file_path: Option<std::path::PathBuf> = serde_json::from_str::<serde_json::Value>(&tc.arguments)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(std::path::PathBuf::from));
    let has_file_path = file_path.is_some();
    let view_path = file_path.clone();

    // For replace_in_file, also extract old_str to show in diff preview
    let is_replace = tc.name == "replace_in_file";
    let diff_preview: Option<String> = if is_replace {
        serde_json::from_str::<serde_json::Value>(&tc.arguments)
            .ok()
            .and_then(|v| {
                let old_str = v.get("old_str").and_then(|s| s.as_str())?;
                let new_str = v.get("new_str").and_then(|s| s.as_str())?;
                // Create a simple diff preview
                let old_lines: Vec<&str> = old_str.lines().take(5).collect();
                let new_lines: Vec<&str> = new_str.lines().take(5).collect();
                let mut preview = String::new();
                for line in &old_lines {
                    preview.push_str(&format!("- {}\n", line));
                }
                if old_str.lines().count() > 5 {
                    preview.push_str("  ...\n");
                }
                for line in &new_lines {
                    preview.push_str(&format!("+ {}\n", line));
                }
                if new_str.lines().count() > 5 {
                    preview.push_str("  ...\n");
                }
                Some(preview)
            })
    } else {
        None
    };

    // Expanded state for showing diff preview
    let expanded = create_rw_signal(false);
    let diff_preview_clone = diff_preview.clone();

    container(
        stack((
            // Tool name + summary
            label(move || format!("Approve {}?", tool_name)).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 1.0).max(10.0))
                    .font_bold()
                    .color(config.color(LapceColor::LAPCE_WARN))
            }),
            label(move || summary.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                    .margin_bottom(6.0)
            }),
            // Diff preview (shown when expanded)
            container(
                label(move || diff_preview_clone.clone().unwrap_or_default())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size((config.ui.font_size() as f32 - 3.0).max(9.0))
                            .font_family("monospace".to_string())
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            .width_pct(100.0)
                    })
            )
            .style(move |s| {
                let is_expanded = expanded.get();
                let has_diff = diff_preview.is_some();
                let config = config.get();
                s.width_pct(100.0)
                    .padding(6.0)
                    .margin_bottom(6.0)
                    .border_radius(4.0)
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .apply_if(!is_expanded || !has_diff, |s| s.hide())
            }),
            // Accept / View / Reject buttons
            stack((
                label(|| "Accept".to_string())
                    .on_click_stop(move |_| {
                        proxy_accept.request_async(
                            lapce_rpc::proxy::ProxyRequest::AgentApproveToolCall {
                                tool_call_id: tc_id.clone(),
                            },
                            |_| {},
                        );
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(14.0)
                            .padding_vert(4.0)
                            .margin_right(8.0)
                            .border_radius(4.0)
                            .font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .font_bold()
                            .color(config.color(LapceColor::PANEL_BACKGROUND))
                            .background(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                            .cursor(CursorStyle::Pointer)
                            .hover(|s| s.background(
                                config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.85)
                            ))
                    }),
                // View button - opens file or toggles diff preview
                label(|| "View".to_string())
                    .on_click_stop(move |_| {
                        if let Some(ref path) = view_path {
                            // Open the file in editor
                            internal_command.send(crate::command::InternalCommand::OpenFile {
                                path: path.clone(),
                            });
                        }
                        // Also toggle the diff preview
                        expanded.update(|v| *v = !*v);
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(14.0)
                            .padding_vert(4.0)
                            .margin_right(8.0)
                            .border_radius(4.0)
                            .font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .font_bold()
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                            .border(1.0)
                            .border_color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                            .cursor(CursorStyle::Pointer)
                            .apply_if(!has_file_path && !is_replace, |s| s.hide())
                            .hover(|s| s.background(
                                config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.15)
                            ))
                    }),
                label(|| "Reject".to_string())
                    .on_click_stop(move |_| {
                        proxy_reject.request_async(
                            lapce_rpc::proxy::ProxyRequest::AgentRejectToolCall {
                                tool_call_id: tc_id_reject.clone(),
                            },
                            |_| {},
                        );
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(14.0)
                            .padding_vert(4.0)
                            .border_radius(4.0)
                            .font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .font_bold()
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                            .border(1.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .cursor(CursorStyle::Pointer)
                            .hover(|s| s.background(
                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND)
                            ))
                    }),
            ))
            .style(|s| s.flex_row().items_center()),
        ))
        .style(|s| s.flex_col().gap(4.0).width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .padding(10.0)
            .margin_vert(2.0)
            .border_radius(6.0)
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_WARN).multiply_alpha(0.5))
            .background(config.color(LapceColor::LAPCE_WARN).multiply_alpha(0.08))
    })
}

/// A compact, collapsible tool call card.
///
/// **Collapsed (default):** Single line showing:
///   [status icon] tool_name  elapsed_time
///
/// **Expanded (on click):** Reveals arguments preview and output preview.
fn tool_call_card(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatToolCall,
) -> impl View {
    let is_running = tc.status == ToolCallStatus::Running;
    let is_success = tc.status == ToolCallStatus::Success;
    let is_error = tc.status == ToolCallStatus::Error;

    // Status icon character
    let status_icon = match &tc.status {
        ToolCallStatus::Pending => "\u{25CB}",   // open circle
        ToolCallStatus::WaitingApproval => "\u{26A0}", // warning
        ToolCallStatus::Running => "\u{25CF}",   // filled circle (pulsing via color)
        ToolCallStatus::Success => "\u{2713}",   // checkmark
        ToolCallStatus::Error => "\u{2717}",     // X mark
        ToolCallStatus::Rejected => "\u{2718}",  // rejected
    };

    let tool_name = tc.name.clone();
    let elapsed = tc.elapsed_display.clone();

    // Collapse/expand state
    let collapsed = create_rw_signal(true);

    // Details (shown when expanded)
    let args_preview: String = tc.arguments.chars().take(200).collect();
    let output_preview = tc.output.clone().map(|o| {
        if o.len() > 300 {
            format!("{}...", &o[..300])
        } else {
            o
        }
    });
    let has_details = !args_preview.is_empty() || output_preview.is_some();

    container(
        stack((
            // ── Header row (always visible, clickable) ──
            stack((
                // Status icon
                label(move || status_icon.to_string()).style(move |s| {
                    let config = config.get();
                    let color = if is_running {
                        config.color(LapceColor::LAPCE_ICON_ACTIVE)
                    } else if is_success {
                        config.color(LapceColor::EDITOR_FOREGROUND)
                    } else if is_error {
                        config.color(LapceColor::LAPCE_ERROR)
                    } else {
                        config.color(LapceColor::EDITOR_DIM)
                    };
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .margin_right(6.0)
                        .color(color)
                }),
                // Tool name
                label(move || tool_name.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .font_bold()
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                }),
                // Spacer
                empty().style(|s| s.flex_grow(1.0)),
                // Elapsed time
                label(move || elapsed.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
                // Expand/collapse chevron
                label(move || {
                    if collapsed.get() { "\u{25B6}" } else { "\u{25BC}" }.to_string()
                })
                .style(move |s| {
                    let config = config.get();
                    s.font_size(8.0)
                        .margin_left(6.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .apply_if(!has_details, |s| s.hide())
                }),
            ))
            .on_click_stop(move |_| {
                if has_details {
                    collapsed.update(|v| *v = !*v);
                }
            })
            .style(move |s| {
                s.items_center()
                    .width_pct(100.0)
                    .cursor(if has_details {
                        CursorStyle::Pointer
                    } else {
                        CursorStyle::Default
                    })
            }),

            // ── Details (shown when expanded) ──
            {
                let args = args_preview.clone();
                let output = output_preview.clone();
                container(
                    stack((
                        // Arguments
                        label(move || args.clone()).style(move |s| {
                            let config = config.get();
                            s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                                .width_pct(100.0)
                                .min_width(0.0)
                                .margin_top(4.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .apply_if(args_preview.is_empty(), |s| s.hide())
                        }),
                        // Output
                        {
                            let out = output.clone();
                            label(move || out.clone().unwrap_or_default()).style(
                                move |s| {
                                    let config = config.get();
                                    s.font_size(
                                        (config.ui.font_size() as f32 - 2.0).max(10.0),
                                    )
                                    .width_pct(100.0)
                                    .min_width(0.0)
                                    .margin_top(4.0)
                                    .padding(4.0)
                                    .background(
                                        config
                                            .color(LapceColor::PANEL_BACKGROUND)
                                            .multiply_alpha(0.5),
                                    )
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                    .apply_if(output_preview.is_none(), |s| s.hide())
                                },
                            )
                        },
                    ))
                    .style(|s| s.flex_col().width_pct(100.0)),
                )
                .style(move |s| {
                    let is_collapsed = collapsed.get();
                    s.width_pct(100.0)
                        .apply_if(is_collapsed, |s| s.hide())
                })
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0).min_width(0.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(8.0)
            .padding_vert(6.0)
            .margin_horiz(8.0)
            .margin_vert(2.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border(1.0)
            .border_color(
                config
                    .color(LapceColor::LAPCE_BORDER)
                    .multiply_alpha(0.6),
            )
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}

/// View for a thinking step (server-side activity).
fn thinking_step_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    step: ChatThinkingStep,
) -> impl View {
    // Icon based on step type
    let icon = match step.step_type.as_str() {
        "enriching" | "searching" => "\u{1F50D}", // magnifying glass
        "reasoning" => "\u{1F4AD}",                // thought bubble
        "plan" => "\u{1F4CB}",                     // clipboard
        "tool" => "\u{1F527}",                     // wrench
        _ => "\u{2022}",                           // bullet
    };
    let message = step.message.clone();
    let detail = step.detail.clone();

    container(
        stack((
            // Icon
            label(move || icon.to_string()).style(move |s| {
                let config = config.get();
                s.font_size(10.0)
                    .margin_right(6.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            // Message
            label(move || message.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .color(config.color(LapceColor::EDITOR_DIM))
                    .flex_grow(1.0)
            }),
            // Optional detail (truncated)
            {
                let detail_text = detail.clone().map(|d| {
                    if d.len() > 50 { format!("{}...", &d[..50]) } else { d }
                }).unwrap_or_default();
                let has_detail = detail.is_some();
                label(move || detail_text.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 3.0).max(9.0))
                        .color(config.color(LapceColor::EDITOR_DIM).multiply_alpha(0.7))
                        .margin_left(8.0)
                        .apply_if(!has_detail, |s| s.hide())
                })
            },
        ))
        .style(|s| s.items_center().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(8.0)
            .padding_vert(3.0)
            .margin_horiz(8.0)
            .width_pct(100.0)
            .min_width(0.0)
            .background(config.color(LapceColor::PANEL_BACKGROUND).multiply_alpha(0.3))
    })
}

/// View for the agent's task plan.
fn plan_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    plan: ChatPlan,
) -> impl View {
    let steps = plan.steps;

    container(
        stack((
            // Header
            label(move || "Plan".to_string()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                    .font_bold()
                    .margin_bottom(4.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            // Steps list - use dyn_stack with cloneable data
            dyn_stack(
                move || steps.clone(),
                |step: &ChatPlanStep| step.number,
                move |step| plan_step_view(config, step),
            )
            .style(|s| s.flex_col().width_pct(100.0)),
        ))
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding(8.0)
            .margin_horiz(8.0)
            .margin_vert(4.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.5))
            .background(config.color(LapceColor::PANEL_BACKGROUND).multiply_alpha(0.5))
    })
}

/// View for a single plan step.
fn plan_step_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    step: ChatPlanStep,
) -> impl View {
    let status_icon = match step.status {
        ChatPlanStepStatus::Pending => "\u{25CB}",   // open circle
        ChatPlanStepStatus::InProgress => "\u{25CF}", // filled circle
        ChatPlanStepStatus::Done => "\u{2713}",      // checkmark
    };
    let is_in_progress = step.status == ChatPlanStepStatus::InProgress;
    let is_done = step.status == ChatPlanStepStatus::Done;
    let description = step.description.clone();
    let number = step.number;

    stack((
        // Status icon
        label(move || status_icon.to_string()).style(move |s| {
            let config = config.get();
            let color = if is_done {
                config.color(LapceColor::EDITOR_FOREGROUND)
            } else if is_in_progress {
                config.color(LapceColor::LAPCE_ICON_ACTIVE)
            } else {
                config.color(LapceColor::EDITOR_DIM)
            };
            s.font_size(10.0)
                .min_width(16.0)
                .color(color)
        }),
        // Step number and description
        label(move || format!("{}. {}", number, description.clone())).style(move |s| {
            let config = config.get();
            s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                .color(if is_done {
                    config.color(LapceColor::EDITOR_DIM)
                } else {
                    config.color(LapceColor::EDITOR_FOREGROUND)
                })
                .apply_if(is_done, |s| {
                    // Strikethrough for completed steps (simulated with opacity)
                    s.color(config.color(LapceColor::EDITOR_DIM))
                })
        }),
    ))
    .style(|s| s.items_center().padding_vert(2.0))
}

/// View for a server-side tool call.
fn server_tool_call_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatServerToolCall,
) -> impl View {
    let is_running = tc.status == ToolCallStatus::Running;
    let is_success = tc.status == ToolCallStatus::Success;
    let is_error = tc.status == ToolCallStatus::Error;

    let status_icon = match &tc.status {
        ToolCallStatus::Pending => "\u{25CB}",
        ToolCallStatus::WaitingApproval => "\u{26A0}",
        ToolCallStatus::Running => "\u{25CF}",
        ToolCallStatus::Success => "\u{2713}",
        ToolCallStatus::Error => "\u{2717}",
        ToolCallStatus::Rejected => "\u{2718}",
    };

    let tool_name = tc.name.clone();
    let elapsed = tc.elapsed_display.clone();
    let result_summary = tc.result_summary.clone();

    container(
        stack((
            // Status icon
            label(move || status_icon.to_string()).style(move |s| {
                let config = config.get();
                let color = if is_running {
                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                } else if is_success {
                    config.color(LapceColor::EDITOR_FOREGROUND)
                } else if is_error {
                    config.color(LapceColor::LAPCE_ERROR)
                } else {
                    config.color(LapceColor::EDITOR_DIM)
                };
                s.font_size(10.0)
                    .margin_right(6.0)
                    .color(color)
            }),
            // Tool name (with "server" prefix to distinguish from IDE tools)
            label(move || format!("⚙ {}", tool_name.clone())).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .font_bold()
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            // Result summary (if available)
            {
                let summary_text = result_summary.clone().map(|s| {
                    if s.len() > 40 { format!("{}...", &s[..40]) } else { s }
                }).unwrap_or_default();
                let has_summary = result_summary.is_some();
                label(move || summary_text.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .margin_left(8.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .apply_if(!has_summary, |s| s.hide())
                })
            },
            // Spacer
            empty().style(|s| s.flex_grow(1.0)),
            // Elapsed time
            label(move || elapsed.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
        ))
        .style(|s| s.items_center().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(8.0)
            .padding_vert(4.0)
            .margin_horiz(8.0)
            .margin_vert(1.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.4))
            .background(config.color(LapceColor::PANEL_BACKGROUND).multiply_alpha(0.4))
    })
}

/// Collapsible thinking section that groups all thinking steps and server tool calls.
fn thinking_section(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
    proxy: lapce_rpc::proxy::ProxyRpcHandler,
) -> impl View {
    let thinking_steps = chat_data.thinking_steps;
    let thinking_collapsed = chat_data.thinking_collapsed;

    container(
        stack((
            // ── Header (clickable to toggle collapse) ──
            stack((
                // Chevron
                label(move || {
                    if thinking_collapsed.get() { "\u{25B6}" } else { "\u{25BC}" }.to_string()
                })
                .style(move |s| {
                    let config = config.get();
                    s.font_size(8.0)
                        .margin_right(6.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
                // Title
                label(move || "Thinking...".to_string()).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                        .font_bold()
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
                // Summary (shown when collapsed)
                {
                    label(move || {
                        let steps = thinking_steps.get();
                        let count = steps.len();
                        if count == 0 {
                            String::new()
                        } else {
                            format!("({} steps)", count)
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let collapsed = thinking_collapsed.get();
                        s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .margin_left(8.0)
                            .color(config.color(LapceColor::EDITOR_DIM).multiply_alpha(0.7))
                            .apply_if(!collapsed, |s| s.hide())
                    })
                },
            ))
            .on_click_stop(move |_| {
                thinking_collapsed.update(|v| *v = !*v);
            })
            .style(move |s| {
                s.items_center()
                    .width_pct(100.0)
                    .padding_vert(4.0)
                    .cursor(CursorStyle::Pointer)
            }),

            // ── Content (hidden when collapsed) ──
            {
                container(
                    dyn_stack(
                        move || {
                            let steps = thinking_steps.get();
                            steps.iter().cloned().collect::<Vec<_>>()
                        },
                        |entry: &ChatEntry| entry.key(),
                        {
                            let proxy = proxy.clone();
                            move |entry| chat_entry_view(config, entry, internal_command, proxy.clone())
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                )
                .style(move |s| {
                    let collapsed = thinking_collapsed.get();
                    s.width_pct(100.0)
                        .apply_if(collapsed, |s| s.hide())
                })
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        let steps = thinking_steps.get();
        let is_empty = steps.is_empty();
        s.padding_horiz(8.0)
            .padding_vert(4.0)
            .margin_horiz(4.0)
            .margin_vert(4.0)
            .width_pct(100.0)
            .min_width(0.0)
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.3))
            .background(config.color(LapceColor::PANEL_BACKGROUND).multiply_alpha(0.2))
            .apply_if(is_empty, |s| s.hide())
    })
}

/// Chat input area using the shared editor from AiChatData.
fn chat_input_area(
    window_tab_data: Rc<WindowTabData>,
    chat_data: AiChatData,
) -> impl View {
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_loading = chat_data.is_loading;
    let editor = chat_data.editor.clone();

    let is_focused =
        move || focus.get() == Focus::Panel(super::kind::PanelKind::AiChat);

    let text_input_view = TextInputBuilder::new()
        .is_focused(is_focused)
        .build_editor(editor);

    let cursor_x = create_rw_signal(0.0);

    let chat_data_btn = chat_data.clone();

    // The entire bottom bar is one bordered rectangle.
    // Input fills all available space, Send button is flush on the right.
    stack((
        // Input editor -- fills all remaining width and full height
        scroll(
            text_input_view
                .placeholder(|| "Ask Forge anything...".to_string())
                .on_cursor_pos(move |point| {
                    cursor_x.set(point.x);
                })
                .style(|s| {
                    s.padding_vert(6.0).padding_horiz(8.0).min_width_pct(100.0)
                }),
        )
        .ensure_visible(move || {
            Size::new(20.0, 0.0)
                .to_rect()
                .with_origin(Point::new(cursor_x.get(), 0.0))
        })
        .on_event_cont(EventListener::PointerDown, move |_| {
            focus.set(Focus::Panel(super::kind::PanelKind::AiChat));
        })
        .scroll_style(|s| s.hide_bars(true))
        .style(move |s| {
            let config = config.get();
            s.flex_grow(1.0)
                .height_pct(100.0)
                .cursor(CursorStyle::Text)
                .items_center()
                .background(config.color(LapceColor::EDITOR_BACKGROUND))
        }),
        // Send button -- flush right, full height
        label(move || {
            if is_loading.get() {
                "Stop".to_string()
            } else {
                "Send".to_string()
            }
        })
        .on_click_stop(move |_| {
            if !is_loading.get_untracked() {
                chat_data_btn.send_message();
            }
        })
        .style(move |s| {
            let config = config.get();
            let loading = is_loading.get();
            s.padding_horiz(14.0)
                .height_pct(100.0)
                .items_center()
                .justify_center()
                .cursor(if loading {
                    CursorStyle::Default
                } else {
                    CursorStyle::Pointer
                })
                .font_bold()
                .font_size(config.ui.font_size() as f32)
                .color(config.color(LapceColor::PANEL_FOREGROUND))
                .background(config.color(LapceColor::EDITOR_BACKGROUND))
                .border_left(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .apply_if(!loading, |s| {
                    s.hover(|s| {
                        s.background(
                            config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
                })
                .apply_if(loading, |s| {
                    s.color(config.color(LapceColor::EDITOR_DIM))
                })
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .height(32.0)
            .items_center()
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
}
