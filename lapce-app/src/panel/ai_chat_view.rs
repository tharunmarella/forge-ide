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
    text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
};

use super::position::PanelPosition;
use crate::{
    ai_chat::{
        AiChatData, ChatEntry, ChatEntryKind, ChatRole, ChatToolCall, ToolCallStatus,
        ChatPlan, ChatPlanStep, ChatPlanStepStatus, ChatServerToolCall,
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
        chat_header(config, chat_data_clear, window_tab_data.panel.clone()),
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

/// Header with title, index status badge, clear button, and close button.
fn chat_header(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
    panel: crate::panel::data::PanelData,
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
            // Close button — collapses the right panel
            {
                use crate::panel::position::PanelContainerPosition;
                use crate::panel::kind::PanelKind;
                crate::app::clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        panel.hide_panel(&PanelKind::AiChat);
                        if panel.is_container_shown(&PanelContainerPosition::Right, false) {
                            panel.toggle_container_visual(&PanelContainerPosition::Right);
                        }
                    },
                    || false,
                    || false,
                    || "Close AI Chat",
                    config,
                )
            },
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
    let is_loading = chat_data.is_loading;
    let has_first_token = chat_data.has_first_token;
    let streaming_text = chat_data.streaming_text;
    let scroll_trigger = chat_data.scroll_trigger;

    // Session-level auto-approve flag — shared across all approval cards in this view.
    let auto_approve_session = floem::reactive::create_rw_signal(false);

    // Track the actual pixel width of the panel so text can be pre-wrapped.
    // Scroll containers give children unconstrained horizontal width in taffy,
    // so rich_text's built-in wrapping never fires. We fix this by pre-calling
    // text_layout.set_size(panel_width) before the layout pass.
    let panel_width = floem::reactive::create_rw_signal(0.0_f64);

    stack((
        // ── Dropdown overlay (shown when dropdown_open is true) ──
        model_dropdown_panel(config, chat_data_dropdown),
        // ── Messages + streaming indicator ──
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
                        move |entry| chat_entry_view(config, entry, internal_command, proxy.clone(), panel_width, auto_approve_session)
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0).min_width(0.0)),

                // ── Streaming text preview (rich text, pre-wrapped) ──
                // Shown while the assistant is actively streaming.
                // Uses panel_width to pre-set TextLayout size since scroll containers
                // give children unconstrained width and rich_text can't auto-wrap.
                {
                    container(
                        rich_text(move || {
                            let text = streaming_text.get();
                            if text.is_empty() {
                                return TextLayout::new();
                            }
                            let config = config.get();
                            let font_size = (config.ui.font_size() as f32 - 2.0).max(11.0);
                            let mut text_layout = TextLayout::new();
                            let attrs = Attrs::new()
                                .font_size(font_size)
                                .line_height(LineHeightValue::Normal(1.5))
                                .color(config.color(LapceColor::EDITOR_FOREGROUND));
                            text_layout.set_text(&text, AttrsList::new(attrs), None);
                            // Pre-wrap at the actual panel pixel width (minus padding)
                            let w = panel_width.get() as f32;
                            if w > 40.0 {
                                text_layout.set_size(w - 40.0, f32::MAX);
                            }
                            text_layout
                        })
                        .style(|s| s.width_pct(100.0).min_width(0.0))
                    )
                    .style(move |s| {
                        let config = config.get();
                        let text = streaming_text.get();
                        let loading = is_loading.get();
                        let has_token = has_first_token.get();
                        let show = loading && has_token && !text.is_empty();
                        s.width_pct(100.0)
                            .min_width(0.0)
                            .padding_horiz(10.0)
                            .padding_vert(6.0)
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
    .on_resize(move |rect| {
        // Update the tracked panel width whenever the outer container resizes.
        // This triggers re-computation of all rich_text closures that read panel_width,
        // which pre-calls set_size() on their TextLayouts and requests a re-layout.
        let w = rect.width();
        if (panel_width.get_untracked() - w).abs() > 1.0 {
            panel_width.set(w);
        }
    })
    .style(|s| {
        s.flex_col()
            .flex_grow(1.0)
            .flex_basis(0.0)
            .width_pct(100.0)
            .min_width(0.0)
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
    panel_width: floem::reactive::RwSignal<f64>,
    auto_approve_session: floem::reactive::RwSignal<bool>,
) -> impl View {
    match entry.kind {
        ChatEntryKind::Message { role, content } => {
            message_bubble(config, role, content, panel_width).into_any()
        }
        ChatEntryKind::ToolCall(tc) => {
            // Approval-pending tools get Accept/Reject/Approve-All buttons
            if tc.status == ToolCallStatus::WaitingApproval || tc.status == ToolCallStatus::AwaitingReview {
                return approval_card(config, tc, proxy, internal_command, auto_approve_session).into_any();
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
        ChatEntryKind::ThinkingStep(_) | ChatEntryKind::Plan(_) | ChatEntryKind::ServerToolCall(_) => {
            // These entry types were used by the removed thinking section — render nothing.
            empty().into_any()
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
    panel_width: floem::reactive::RwSignal<f64>,
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
        let chat_font = (cfg.ui.font_size() as f32).max(13.0);
        crate::markdown::parse_markdown_sized(&content, 1.5, &cfg, chat_font)
    } else {
        Vec::new()
    };

    container(
        stack((
            // Role label
            label(move || role_label.to_string()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32).max(12.0))
                    .font_bold()
                    .margin_bottom(4.0)
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
                                rich_text(move || {
                                    // Pre-wrap at actual pixel width to work around scroll
                                    // containers giving children unconstrained horizontal space.
                                    let mut layout = text_layout.clone();
                                    let w = panel_width.get() as f32;
                                    if w > 60.0 {
                                        layout.set_size(w - 60.0, f32::MAX);
                                    }
                                    layout
                                })
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
                        s.font_size((config.ui.font_size() as f32).max(13.0))
                            .width_pct(100.0)
                            .min_width(0.0)
                            .max_width_pct(100.0)
                            .line_height(1.5)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    })
                    .into_any()
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0).min_width(0.0).max_width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .min_width(0.0)
            .max_width_pct(100.0)
            .margin_horiz(8.0)
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
        ToolCallStatus::AwaitingReview => "\u{1F440}", // eyes (reviewing)
        ToolCallStatus::Running => "\u{25CF}",
        ToolCallStatus::Success => "\u{2713}",
        ToolCallStatus::Accepted => "\u{2713}", // same as success
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

fn create_terminal_text_layout(
    text: &str,
    config: &crate::config::LapceConfig,
) -> TextLayout {
    let mut text_layout = TextLayout::new();
    let family: Vec<FamilyOwned> = FamilyOwned::parse_list("monospace").collect();
    let font_size = (config.ui.font_size() as f32 - 1.0).max(11.0);

    let attrs = Attrs::new()
        .family(&family)
        .font_size(font_size)
        .line_height(LineHeightValue::Normal(1.4))
        .color(config.color(LapceColor::EDITOR_FOREGROUND));

    text_layout.set_text(text, AttrsList::new(attrs), None);
    text_layout
}

/// Build a TextLayout for a diff string where lines starting with "+ " are green
/// and lines starting with "- " are red. All other lines use the default foreground color.
fn create_diff_text_layout(
    diff_text: &str,
    config: &crate::config::LapceConfig,
) -> TextLayout {
    let family: Vec<FamilyOwned> = FamilyOwned::parse_list("monospace").collect();
    let font_size = (config.ui.font_size() as f32 - 2.0).max(10.0);
    let default_color = config.color(LapceColor::EDITOR_FOREGROUND);
    let added_color = config.color(LapceColor::TERMINAL_GREEN);
    let removed_color = config.color(LapceColor::TERMINAL_RED);

    let base_attrs = Attrs::new()
        .family(&family)
        .font_size(font_size)
        .line_height(LineHeightValue::Normal(1.4))
        .color(default_color);

    let mut attrs_list = AttrsList::new(base_attrs.clone());

    // Walk through the text byte-by-byte, tracking line spans for coloring.
    let mut byte_pos = 0usize;
    for line in diff_text.split('\n') {
        let line_len = line.len();
        let color_opt = if line.starts_with("+ ") {
            Some(added_color)
        } else if line.starts_with("- ") {
            Some(removed_color)
        } else {
            None
        };
        if let Some(color) = color_opt {
            attrs_list.add_span(
                byte_pos..byte_pos + line_len,
                base_attrs.clone().color(color),
            );
        }
        byte_pos += line_len + 1; // +1 for the '\n' separator
    }

    let mut text_layout = TextLayout::new();
    text_layout.set_text(diff_text, attrs_list, None);
    text_layout
}

/// Approval card — shown when a mutating tool needs user permission.
/// Displays the tool name, summary, and Accept/Reject/View buttons.
fn approval_card(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatToolCall,
    proxy: lapce_rpc::proxy::ProxyRpcHandler,
    internal_command: crate::listener::Listener<crate::command::InternalCommand>,
    auto_approve_session: floem::reactive::RwSignal<bool>,
) -> impl View {
    let tool_name = tc.name.clone();
    let summary = tc.output.clone().unwrap_or_else(|| format!("Execute: {}", tool_name));
    let tc_id = tc.id.clone();
    let tc_id_reject = tc.id.clone();
    let tc_id_approve_all = tc.id.clone();
    let proxy_accept = proxy.clone();
    let proxy_reject = proxy.clone();
    let proxy_approve_all = proxy.clone();

    // Parse arguments once for all subsequent extractions
    let args_value: Option<serde_json::Value> = serde_json::from_str(&tc.arguments).ok();

    // Extract file path from arguments for "View" button (for file-related tools)
    let file_path: Option<std::path::PathBuf> = args_value
        .as_ref()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(std::path::PathBuf::from));
    let has_file_path = file_path.is_some();
    let view_path = file_path.clone();

    // Compute action header: "Verb: filename: description"
    let action_verb = match tc.name.as_str() {
        "replace_in_file" | "apply_patch" => "Edit",
        "write_to_file" => "Write",
        "delete_file" => "Delete",
        "run_command" | "execute_command" | "bash" => "Run",
        "rename_file" | "move_file" => "Rename",
        _ => "Action",
    };
    let short_filename = args_value
        .as_ref()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()))
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p)
                .to_string()
        })
        .unwrap_or_else(|| tool_name.clone());
    let action_desc = match tc.name.as_str() {
        "replace_in_file" => {
            let n = args_value
                .as_ref()
                .and_then(|v| v.get("old_str").and_then(|s| s.as_str()))
                .map(|s| s.lines().count())
                .unwrap_or(0);
            format!("replaced {} line{}", n, if n == 1 { "" } else { "s" })
        }
        "write_to_file" => "created/updated file".to_string(),
        "delete_file" => "deleted file".to_string(),
        "apply_patch" => "applied patch".to_string(),
        "run_command" | "execute_command" | "bash" => args_value
            .as_ref()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()))
            .unwrap_or("command")
            .to_string(),
        _ => tool_name.clone(),
    };
    let header_text = format!("{}: {}: {}", action_verb, short_filename, action_desc);

    // Build diff preview for file-edit tools — all lines, no truncation
    let is_file_edit = matches!(
        tc.name.as_str(),
        "replace_in_file" | "write_to_file" | "apply_patch"
    );
    let diff_preview: Option<String> = if tc.name == "replace_in_file" {
        args_value.as_ref().and_then(|v| {
            let old_str = v.get("old_str").and_then(|s| s.as_str())?;
            let new_str = v.get("new_str").and_then(|s| s.as_str())?;
            let mut preview = String::new();
            for line in old_str.lines() {
                preview.push_str("- ");
                preview.push_str(line);
                preview.push('\n');
            }
            for line in new_str.lines() {
                preview.push_str("+ ");
                preview.push_str(line);
                preview.push('\n');
            }
            Some(preview)
        })
    } else if tc.name == "write_to_file" {
        args_value.as_ref().and_then(|v| {
            let content = v.get("content").and_then(|s| s.as_str())?;
            let mut preview = String::new();
            for line in content.lines() {
                preview.push_str("+ ");
                preview.push_str(line);
                preview.push('\n');
            }
            Some(preview)
        })
    } else {
        None
    };

    // Expanded state for showing diff preview
    let expanded = create_rw_signal(true);
    let diff_preview_clone = diff_preview.clone();

    container(
        stack((
            // Action header: "Verb: filename: description"
            label(move || header_text.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 1.0).max(10.0))
                    .font_bold()
                    .color(config.color(LapceColor::LAPCE_WARN))
            }),
            // Terminal output styled like a scrollable terminal window
            container(
                scroll(
                    {
                        let summary = summary.clone();
                        rich_text(move || {
                            let config = config.get();
                            create_terminal_text_layout(&summary, &config)
                        })
                        .style(|s| s.width_pct(100.0).min_width(0.0))
                    }
                )
                .style(|s| {
                    s.width_pct(100.0)
                        .min_height(50.0)
                        .max_height(300.0)
                })
            )
            .style(move |s| {
                let config = config.get();
                s.width_pct(100.0)
                    .min_width(0.0)
                    .margin_bottom(8.0)
                    .padding(10.0)
                    .border_radius(6.0)
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border(1.0)
                    .border_color(
                        config
                            .color(LapceColor::LAPCE_BORDER)
                            .multiply_alpha(0.8),
                    )
                    .apply_if(is_file_edit, |s| s.hide())
            }),
            // Colored diff view — scrollable, red for removed lines, green for added
            container(
                scroll(
                    rich_text(move || {
                        let config = config.get();
                        create_diff_text_layout(
                            diff_preview_clone.as_deref().unwrap_or(""),
                            &config,
                        )
                    })
                    .style(|s| s.width_pct(100.0).min_width(0.0)),
                )
                .style(|s| s.width_pct(100.0).max_height(500.0)),
            )
            .style(move |s| {
                let is_expanded = expanded.get();
                let has_diff = diff_preview.is_some();
                let config = config.get();
                s.width_pct(100.0)
                    .padding(8.0)
                    .margin_bottom(6.0)
                    .border_radius(4.0)
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .apply_if(!is_expanded || !has_diff, |s| s.hide())
            }),
            // Status message after accept/reject
            {
                let status_msg = match &tc.status {
                    ToolCallStatus::Accepted => Some("✓ Changes accepted"),
                    ToolCallStatus::Rejected => Some("↩ Changes reverted"),
                    _ => None,
                };
                if let Some(msg) = status_msg {
                    label(move || msg.to_string())
                        .style(move |s| {
                            let config = config.get();
                            let color = match tc.status {
                                ToolCallStatus::Accepted => config.color(LapceColor::LAPCE_ICON_ACTIVE),
                                _ => config.color(LapceColor::EDITOR_DIM),
                            };
                            s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                                .color(color)
                                .margin_bottom(4.0)
                        })
                        .into_any()
                } else {
                    empty().into_any()
                }
            },
            // Accept / View / Reject buttons (only show if still awaiting response)
            {
                let show_buttons = matches!(tc.status, ToolCallStatus::WaitingApproval | ToolCallStatus::AwaitingReview);
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

                    // "Approve All Future" — auto-approves everything in this session
                    // except dangerous ops (delete_file). Hidden once already active.
                    label(move || {
                        if auto_approve_session.get() {
                            "✓ Auto-approving".to_string()
                        } else {
                            "Approve All Future".to_string()
                        }
                    })
                    .on_click_stop(move |_| {
                        if !auto_approve_session.get_untracked() {
                            auto_approve_session.set(true);
                            // Approve current tool call + tell proxy to enable session auto-approve
                            proxy_approve_all.request_async(
                                lapce_rpc::proxy::ProxyRequest::AgentApproveToolCall {
                                    tool_call_id: tc_id_approve_all.clone(),
                                },
                                |_| {},
                            );
                            proxy_approve_all.request_async(
                                lapce_rpc::proxy::ProxyRequest::AgentApproveAllFuture {},
                                |_| {},
                            );
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        let active = auto_approve_session.get();
                        s.margin_left(8.0)
                            .padding_horiz(14.0)
                            .padding_vert(4.0)
                            .border_radius(4.0)
                            .font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .border(1.0)
                            .cursor(if active { CursorStyle::Default } else { CursorStyle::Pointer })
                            .apply_if(active, |s| {
                                s.color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                                    .border_color(config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.4))
                            })
                            .apply_if(!active, |s| {
                                s.color(config.color(LapceColor::PANEL_FOREGROUND).multiply_alpha(0.7))
                                    .border_color(config.color(LapceColor::LAPCE_BORDER).multiply_alpha(0.5))
                                    .hover(|s| s.background(
                                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                    ))
                            })
                    }),
                ))
                .style(move |s| {
                    s.flex_row()
                        .items_center()
                        .apply_if(!show_buttons, |s| s.hide())
                })
            },
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
        ToolCallStatus::AwaitingReview => "\u{1F440}", // eyes (reviewing)
        ToolCallStatus::Running => "\u{25CF}",   // filled circle (pulsing via color)
        ToolCallStatus::Success => "\u{2713}",   // checkmark
        ToolCallStatus::Accepted => "\u{2713}", // same as success
        ToolCallStatus::Error => "\u{2717}",     // X mark
        ToolCallStatus::Rejected => "\u{2718}",  // rejected
    };

    let tool_name = tc.name.clone();
    let elapsed = tc.elapsed_display.clone();
    
    // Special handling for show_code and show_diagram tools - always start expanded
    let is_show_code = tool_name == "show_code";
    let is_show_diagram = tool_name == "show_diagram";
    let collapsed = create_rw_signal(!is_show_code && !is_show_diagram);

    // Details (shown when expanded)
    let args_preview: String = tc.arguments.chars().take(200).collect();
    
    // Parse code blocks or mermaid diagrams from output
    let (code_block, code_language, code_title, mermaid_diagram, mermaid_title, regular_output) = 
        if is_show_code {
            let (code, lang, title, remaining) = parse_code_block(&tc.output.clone().unwrap_or_default());
            (code, lang, title, None, None, remaining)
        } else if is_show_diagram {
            let (diagram, title, remaining) = parse_mermaid_block(&tc.output.clone().unwrap_or_default());
            (None, None, None, diagram, title, remaining)
        } else {
            (None, None, None, None, None, tc.output.clone())
        };
    
    // For preview in collapsed state or has_details check, we use truncated output
    let output_preview = regular_output.clone().map(|o| {
        if o.len() > 300 {
            // Use char boundaries instead of byte indexing to avoid panic
            let truncated: String = o.chars().take(300).collect();
            format!("{}...", truncated)
        } else {
            o
        }
    });
    let has_details = !args_preview.is_empty() || output_preview.is_some() || code_block.is_some() || mermaid_diagram.is_some();

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
                                .font_family("monospace".to_string())
                                .width_pct(100.0)
                                .min_width(0.0)
                                .margin_top(4.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .apply_if(args_preview.is_empty(), |s| s.hide())
                        }),
                        // Code block (for show_code tool)
                        {
                            let code = code_block.clone();
                            let lang = code_language.clone();
                            let title = code_title.clone();
                            container(
                                stack((
                                    // Title (if present)
                                    {
                                        let t = title.clone();
                                        label(move || t.clone().unwrap_or_default()).style(move |s| {
                                            let config = config.get();
                                            s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                                                .font_bold()
                                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                                                .margin_bottom(6.0)
                                                .apply_if(title.is_none(), |s| s.hide())
                                        })
                                    },
                                    // Language badge
                                    {
                                        let l = lang.clone();
                                        container(
                                            label(move || l.clone().unwrap_or_else(|| "code".to_string())).style(move |s| {
                                                let config = config.get();
                                                s.font_size((config.ui.font_size() as f32 - 3.0).max(9.0))
                                                    .font_family("monospace".to_string())
                                                    .color(config.color(LapceColor::EDITOR_DIM))
                                            })
                                        )
                                        .style(move |s| {
                                            let config = config.get();
                                            s.padding_horiz(8.0)
                                                .padding_vert(3.0)
                                                .margin_bottom(6.0)
                                                .border_radius(3.0)
                                                .background(
                                                    config
                                                        .color(LapceColor::PANEL_BACKGROUND)
                                                        .multiply_alpha(0.7),
                                                )
                                                .border(1.0)
                                                .border_color(
                                                    config
                                                        .color(LapceColor::LAPCE_BORDER)
                                                        .multiply_alpha(0.5),
                                                )
                                        })
                                    },
                                    // Code content
                                    label(move || code.clone().unwrap_or_default()).style(move |s| {
                                        let config = config.get();
                                        s.font_size(
                                            (config.ui.font_size() as f32 - 1.0).max(11.0),
                                        )
                                        .font_family("monospace".to_string())
                                        .width_pct(100.0)
                                        .min_width(0.0)
                                        .line_height(1.5)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                    }),
                                ))
                                .style(|s| s.flex_col().width_pct(100.0))
                            )
                            .style(move |s| {
                                let config = config.get();
                                s.width_pct(100.0)
                                    .min_width(0.0)
                                    .margin_top(8.0)
                                    .padding(12.0)
                                    .border_radius(8.0)
                                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                    .border(1.0)
                                    .border_color(
                                        config
                                            .color(LapceColor::LAPCE_BORDER)
                                            .multiply_alpha(0.8),
                                    )
                                    .apply_if(code_block.is_none(), |s| s.hide())
                            })
                        },
                        // Mermaid diagram (for show_diagram tool)
                        {
                            let diagram = mermaid_diagram.clone();
                            let diagram_title = mermaid_title.clone();
                            
                            container(
                                stack((
                                    // Title (if present)
                                    {
                                        let t = diagram_title.clone();
                                        label(move || t.clone().unwrap_or_default()).style(move |s| {
                                            let config = config.get();
                                            s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                                                .font_bold()
                                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                                                .margin_bottom(12.0)
                                                .apply_if(diagram_title.is_none(), |s| s.hide())
                                        })
                                    },
                                    // Diagram preview and link
                                    {
                                        let diagram_for_label = diagram.clone();
                                        let diagram_for_click = diagram.clone();
                                        
                                        stack((
                                            label(|| "🎨 Diagram ready".to_string()).style(move |s| {
                                                let config = config.get();
                                                s.font_size((config.ui.font_size() as f32).max(13.0))
                                                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                                                    .margin_bottom(8.0)
                                            }),
                                            label(move || {
                                                if let Some(ref diag) = diagram_for_label {
                                                    format!("Click to open interactive diagram in browser\n\n{}", 
                                                        if diag.len() > 100 { 
                                                            format!("{}...", &diag[..100])
                                                        } else {
                                                            diag.clone()
                                                        }
                                                    )
                                                } else {
                                                    String::new()
                                                }
                                            }).style(move |s| {
                                                let config = config.get();
                                                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                                                    .font_family("monospace".to_string())
                                                    .width_pct(100.0)
                                                    .min_width(0.0)
                                                    .line_height(1.5)
                                                    .color(config.color(LapceColor::EDITOR_DIM))
                                                    .cursor(CursorStyle::Pointer)
                                            }),
                                        ))
                                        .on_click_stop({
                                            let diag = diagram_for_click;
                                            move |_| {
                                                if let Some(ref d) = diag {
                                                    if let Err(e) = open_mermaid_in_browser(d) {
                                                        tracing::error!("Failed to open diagram: {}", e);
                                                    }
                                                }
                                            }
                                        })
                                        .style(|s| s.flex_col())
                                    },
                                ))
                                .style(|s| s.flex_col().width_pct(100.0))
                            )
                            .style(move |s| {
                                let config = config.get();
                                s.width_pct(100.0)
                                    .min_width(0.0)
                                    .margin_top(8.0)
                                    .padding(16.0)
                                    .border_radius(8.0)
                                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                    .border(1.0)
                                    .border_color(
                                        config
                                            .color(LapceColor::LAPCE_BORDER)
                                            .multiply_alpha(0.8),
                                    )
                                    .apply_if(mermaid_diagram.is_none(), |s| s.hide())
                            })
                        },
                        // Regular output - styled like a scrollable terminal window
                        {
                            let out = regular_output.clone();
                            container(
                                scroll(
                                    {
                                        let out = out.clone();
                                        rich_text(move || {
                                            let config = config.get();
                                            create_terminal_text_layout(&out.clone().unwrap_or_default(), &config)
                                        })
                                        .style(|s| s.width_pct(100.0).min_width(0.0))
                                    }
                                )
                                .style(|s| {
                                    s.width_pct(100.0)
                                        .min_height(50.0)
                                        .max_height(300.0)
                                })
                            )
                            .style(move |s| {
                                let config = config.get();
                                s.width_pct(100.0)
                                    .min_width(0.0)
                                    .margin_top(8.0)
                                    .padding(10.0)
                                    .border_radius(6.0)
                                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                    .border(1.0)
                                    .border_color(
                                        config
                                            .color(LapceColor::LAPCE_BORDER)
                                            .multiply_alpha(0.8),
                                    )
                                    .apply_if(regular_output.is_none(), |s| s.hide())
                            })
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

/// View for a thinking step (server-side activity) — kept for pattern matching but not rendered.
#[allow(dead_code)]
fn thinking_step_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    step: crate::ai_chat::ChatThinkingStep,
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
        ToolCallStatus::AwaitingReview => "\u{1F440}", // eyes (reviewing)
        ToolCallStatus::Running => "\u{25CF}",
        ToolCallStatus::Success => "\u{2713}",
        ToolCallStatus::Accepted => "\u{2713}", // same as success
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


/// Chat input area using the shared editor from AiChatData.
fn chat_input_area(
    window_tab_data: Rc<WindowTabData>,
    chat_data: AiChatData,
) -> impl View {
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_loading = chat_data.is_loading;
    let is_recording = chat_data.is_recording;
    let attached_images = chat_data.attached_images;
    let editor = chat_data.editor.clone();

    let is_focused =
        move || focus.get() == Focus::Panel(super::kind::PanelKind::AiChat);

    let text_input_view = TextInputBuilder::new()
        .is_focused(is_focused)
        .build_editor(editor);

    let cursor_x = create_rw_signal(0.0);

    // Clone chat_data before closures consume it
    let chat_data_mic = chat_data.clone();
    let chat_data_attach = chat_data.clone();
    let chat_data_preview = chat_data.clone();
    let chat_data_paste = chat_data.clone();

    // ── Image preview strip (shown above input when images are attached) ──
    let image_preview = dyn_stack(
        move || {
            let imgs = attached_images.get();
            imgs.into_iter().enumerate().collect::<Vec<_>>()
        },
        |item: &(usize, lapce_rpc::proxy::AttachedImageData)| item.0,
        move |(idx, img)| {
            let chat_data_rm = chat_data_preview.clone();
            let filename = img.filename.clone();
            stack((
                // Image icon (SVG)
                svg(|| r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="currentColor" d="M5 21q-.825 0-1.412-.587T3 19V5q0-.825.588-1.412T5 3h14q.825 0 1.413.588T21 5v14q0 .825-.587 1.413T19 21zm1-4h12l-3.75-5l-3 4L9 13z"/></svg>"#.to_string())
                    .style(move |s| {
                        let config = config.get();
                        s.size(14.0, 14.0)
                            .margin_right(4.0)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    }),
                // Filename
                label(move || filename.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size(10.0)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                }),
                // Remove button
                label(|| "\u{2715}".to_string()) // ✕
                    .on_click_stop(move |_| {
                        chat_data_rm.remove_image(idx);
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(10.0)
                            .padding_horiz(4.0)
                            .cursor(CursorStyle::Pointer)
                            .color(config.color(LapceColor::EDITOR_DIM))
                            .hover(|s| s.color(config.color(LapceColor::EDITOR_FOREGROUND)))
                    }),
            ))
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .padding(4.0)
                    .margin_right(4.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
        },
    )
    .style(move |s| {
        let has_images = !attached_images.get().is_empty();
        s.flex_row()
            .padding(4.0)
            .width_pct(100.0)
            .margin_horiz(8.0)
            .apply_if(!has_images, |s| s.hide())
    });

    // ── Input bar: [attach] [text input] [mic] ──
    // Enter sends message, so no Send button needed
    let input_bar = stack((
        // Attach button (paperclip icon)
        svg(move || {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512"><path fill="none" stroke="currentColor" stroke-linecap="round" stroke-miterlimit="10" stroke-width="32" d="M216.08 192v143.85a40.08 40.08 0 0 0 80.15 0l.13-188.55a67.94 67.94 0 1 0-135.87 0v189.82a95.51 95.51 0 1 0 191 0V159.74"/></svg>"#.to_string()
        })
        .on_click_stop(move |_| {
            // Open file picker for images
            use floem::action::open_file;
            use floem::file::FileDialogOptions;
            use base64::{Engine as _, engine::general_purpose::STANDARD};
            
            let chat_data = chat_data_attach.clone();
            let options = FileDialogOptions::new()
                .title("Attach Image");
            
            open_file(options, move |file_info| {
                if let Some(mut file) = file_info {
                    // Read the file and add as base64
                    if let Some(path) = file.path.pop() {
                        if let Ok(data) = std::fs::read(&path) {
                            let mime = if path.extension().and_then(|e| e.to_str()) == Some("png") {
                                "image/png"
                            } else {
                                "image/jpeg"
                            };
                            let base64_data = STANDARD.encode(&data);
                            chat_data.add_image(base64_data, mime.to_string());
                        }
                    }
                }
            });
        })
        .style(move |s| {
            let config = config.get();
            s.width(28.0)
                .height(28.0)
                .padding(6.0)
                .cursor(CursorStyle::Pointer)
                .color(config.color(LapceColor::EDITOR_DIM))
                .hover(|s| s.color(config.color(LapceColor::EDITOR_FOREGROUND)))
        }),
        // Input editor -- fills all remaining width
        scroll(
            text_input_view
                .placeholder(|| "Ask Forge anything...".to_string())
                .on_cursor_pos(move |point| {
                    cursor_x.set(point.x);
                })
                .on_event_stop(EventListener::KeyDown, {
                    let chat_data = chat_data_paste.clone();
                    move |event| {
                        // Intercept Cmd+V / Ctrl+V to check for clipboard images
                        if let floem::event::Event::KeyDown(key_event) = event {
                            use floem::keyboard::Key;
                            let is_v_key = match &key_event.key.logical_key {
                                Key::Character(s) => s.as_ref() as &str == "v",
                                _ => false,
                            };
                            if (key_event.modifiers.meta() || key_event.modifiers.control()) && is_v_key {
                                chat_data.check_clipboard_for_image();
                            }
                        }
                    }
                })
                .style(|s| {
                    s.padding_vert(6.0).padding_horiz(8.0).flex_grow(1.0).min_width(0.0)
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
        // Mic button (SVG mic icon, or stop square when recording)
        {
            let is_rec = is_recording;
            container(
                dyn_stack(
                    move || vec![is_rec.get()],
                    |v| *v,
                    move |recording| {
                        if recording {
                            // Stop square when recording
                            label(|| "\u{25A0}".to_string())
                                .style(move |s| {
                                    let config = config.get();
                                    s.font_size(16.0)
                                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                                })
                                .into_any()
                        } else {
                            // Mic SVG icon
                            svg(move || {
                                r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="currentColor" d="M19 9a1 1 0 0 1 1 1a8 8 0 0 1-6.999 7.938L13 20h3a1 1 0 0 1 0 2H8a1 1 0 0 1 0-2h3v-2.062A8 8 0 0 1 4 10a1 1 0 1 1 2 0a6 6 0 0 0 12 0a1 1 0 0 1 1-1m-7-8a4 4 0 0 1 4 4v5a4 4 0 1 1-8 0V5a4 4 0 0 1 4-4"/></svg>"#.to_string()
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.width(18.0)
                                    .height(18.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            })
                            .into_any()
                        }
                    },
                )
                .style(|s| s.items_center().justify_center()),
            )
            .on_click_stop(move |_| {
                chat_data_mic.toggle_recording();
            })
            .style(move |s| {
                let config = config.get();
                let recording = is_recording.get();
                s.width(32.0)
                    .height_pct(100.0)
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::Pointer)
                    .background(if recording {
                        config.color(LapceColor::LAPCE_ERROR)
                    } else {
                        config.color(LapceColor::EDITOR_BACKGROUND)
                    })
                    .hover(|s| {
                        s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                    })
            })
        },
    ))
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .height(36.0)
            .items_center()
            .margin_horiz(8.0)
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    });

    // Stack: image previews on top, input bar below
    stack((image_preview, input_bar))
        .style(|s| s.flex_col().width_pct(100.0))
}

/// Parse code block from tool output.
/// Returns (code_content, language, title, remaining_output).
fn parse_code_block(output: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    // Check for title line (=== Title ===)
    let (title, rest) = if let Some(title_end) = output.find("\n\n") {
        let first_line = &output[..title_end];
        if first_line.starts_with("=== ") && first_line.ends_with(" ===") {
            let title_text = first_line.trim_start_matches("=== ").trim_end_matches(" ===").to_string();
            (Some(title_text), &output[title_end + 2..])
        } else {
            (None, output)
        }
    } else {
        (None, output)
    };
    
    // Check for code block markers [CODE:language] ... [/CODE]
    if let Some(start_idx) = rest.find("[CODE:") {
        if let Some(end_bracket) = rest[start_idx..].find(']') {
            let language = rest[start_idx + 6..start_idx + end_bracket].to_string();
            let code_start = start_idx + end_bracket + 2; // skip "]\n"
            
            if let Some(end_idx) = rest[code_start..].find("[/CODE]") {
                let code_content = rest[code_start..code_start + end_idx].trim().to_string();
                
                // Remaining output (before code block + after code block)
                let before = &rest[..start_idx];
                let after = &rest[code_start + end_idx + 7..]; // skip "[/CODE]"
                let remaining = format!("{}{}", before.trim(), after.trim()).trim().to_string();
                let remaining_output = if remaining.is_empty() { None } else { Some(remaining) };
                
                return (Some(code_content), Some(language), title, remaining_output);
            }
        }
    }
    
    // No code block found, return original output
    (None, None, title, Some(output.to_string()))
}

/// Parse mermaid diagram from tool output.
/// Returns (diagram_code, title, remaining_output).
fn parse_mermaid_block(output: &str) -> (Option<String>, Option<String>, Option<String>) {
    // Check for title line (=== Title ===)
    let (title, rest) = if let Some(title_end) = output.find("\n\n") {
        let first_line = &output[..title_end];
        if first_line.starts_with("=== ") && first_line.ends_with(" ===") {
            let title_text = first_line.trim_start_matches("=== ").trim_end_matches(" ===").to_string();
            (Some(title_text), &output[title_end + 2..])
        } else {
            (None, output)
        }
    } else {
        (None, output)
    };
    
    // Check for mermaid markers [MERMAID] ... [/MERMAID]
    if let Some(start_idx) = rest.find("[MERMAID]") {
        let diagram_start = start_idx + 10; // skip "[MERMAID]\n"
        
        if let Some(end_idx) = rest[diagram_start..].find("[/MERMAID]") {
            let diagram_code = rest[diagram_start..diagram_start + end_idx].trim().to_string();
            
            // Remaining output (before diagram + after diagram)
            let before = &rest[..start_idx];
            let after = &rest[diagram_start + end_idx + 10..]; // skip "[/MERMAID]"
            let remaining = format!("{}{}", before.trim(), after.trim()).trim().to_string();
            let remaining_output = if remaining.is_empty() { None } else { Some(remaining) };
            
            return (Some(diagram_code), title, remaining_output);
        }
    }
    
    // No mermaid block found
    (None, title, Some(output.to_string()))
}

/// Generate an HTML file with Mermaid.js and open it in the browser.
fn open_mermaid_in_browser(diagram_code: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::PathBuf;
    
    // Create temp directory for diagram files
    let temp_dir = std::env::temp_dir().join("forge-diagrams");
    fs::create_dir_all(&temp_dir)?;
    
    // Generate unique filename based on diagram hash
    let hash = format!("{:x}", md5::compute(diagram_code.as_bytes()));
    let html_path: PathBuf = temp_dir.join(format!("diagram-{}.html", hash));
    
    // HTML template with Mermaid.js CDN
    let html_content = format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Forge IDE - Mermaid Diagram</title>
    <script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #1e1e1e;
            color: #d4d4d4;
            overflow: hidden;
            height: 100vh;
        }}
        .header {{
            background: #252526;
            padding: 12px 20px;
            border-bottom: 1px solid #3e3e42;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }}
        .title {{
            font-size: 14px;
            color: #858585;
        }}
        .controls {{
            display: flex;
            gap: 8px;
        }}
        .btn {{
            background: #3e3e42;
            border: none;
            color: #d4d4d4;
            padding: 6px 12px;
            border-radius: 4px;
            cursor: pointer;
            font-size: 13px;
            transition: background 0.2s;
        }}
        .btn:hover {{
            background: #505050;
        }}
        .btn:active {{
            background: #5a5a5a;
        }}
        .diagram-wrapper {{
            position: relative;
            width: 100%;
            height: calc(100vh - 49px);
            overflow: hidden;
            background: #1e1e1e;
        }}
        .diagram-container {{
            width: 100%;
            height: 100%;
            display: flex;
            justify-content: center;
            align-items: center;
            transform-origin: center center;
            transition: transform 0.2s ease-out;
            cursor: grab;
        }}
        .diagram-container.grabbing {{
            cursor: grabbing;
        }}
        .mermaid {{
            text-align: center;
            background: #252526;
            padding: 40px;
            border-radius: 8px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
        }}
        .zoom-info {{
            position: absolute;
            bottom: 16px;
            right: 16px;
            background: rgba(37, 37, 38, 0.95);
            padding: 6px 12px;
            border-radius: 4px;
            font-size: 12px;
            color: #858585;
            pointer-events: none;
        }}
    </style>
</head>
<body>
    <div class="header">
        <div class="title">Generated by Forge IDE • Interactive Diagram</div>
        <div class="controls">
            <button class="btn" id="zoomIn">Zoom In (+)</button>
            <button class="btn" id="zoomOut">Zoom Out (-)</button>
            <button class="btn" id="resetZoom">Reset (0)</button>
            <button class="btn" id="fullscreen">Fullscreen (F)</button>
        </div>
    </div>
    <div class="diagram-wrapper" id="wrapper">
        <div class="diagram-container" id="diagramContainer">
            <pre class="mermaid">
{}
            </pre>
        </div>
        <div class="zoom-info" id="zoomInfo">100%</div>
    </div>
    <script>
        // Auto-fix common Mermaid syntax issues before rendering
        function fixMermaidSyntax(code) {{
            return code.replace(/([A-Z][A-Za-z0-9]*)\[([^\]]*[():,-][^\]]*)\]/g, (match, nodeId, label) => {{
                if (label.startsWith('"') && label.endsWith('"')) {{
                    return match;
                }}
                return `${{nodeId}}["${{label}}"]`;
            }});
        }}
        
        // Get the mermaid code and fix it
        const mermaidElement = document.querySelector('.mermaid');
        if (mermaidElement) {{
            const originalCode = mermaidElement.textContent;
            const fixedCode = fixMermaidSyntax(originalCode);
            mermaidElement.textContent = fixedCode;
        }}
        
        // Initialize Mermaid
        mermaid.initialize({{ 
            startOnLoad: true,
            theme: 'dark',
            themeVariables: {{
                primaryColor: '#89b4fa',
                primaryTextColor: '#cdd6f4',
                primaryBorderColor: '#585b70',
                lineColor: '#a6adc8',
                secondaryColor: '#313244',
                tertiaryColor: '#1e1e2e',
                background: '#1e1e1e',
                mainBkg: '#1e1e2e',
                secondBkg: '#252526',
                textColor: '#d4d4d4',
                border1: '#3e3e42',
                border2: '#6c757d',
                fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
            }}
        }});
        
        // Simple pan/zoom implementation
        setTimeout(() => {{
            const container = document.getElementById('diagramContainer');
            const wrapper = document.getElementById('wrapper');
            const zoomInfo = document.getElementById('zoomInfo');
            
            let scale = 1;
            let translateX = 0;
            let translateY = 0;
            let isDragging = false;
            let startX = 0;
            let startY = 0;
            
            function updateTransform() {{
                container.style.transform = `translate(${{translateX}}px, ${{translateY}}px) scale(${{scale}})`;
                zoomInfo.textContent = Math.round(scale * 100) + '%';
            }}
            
            function zoom(delta, centerX, centerY) {{
                const oldScale = scale;
                scale = Math.max(0.1, Math.min(5, scale + delta));
                
                // Zoom towards cursor/center
                if (centerX !== undefined && centerY !== undefined) {{
                    const rect = wrapper.getBoundingClientRect();
                    const x = centerX - rect.left;
                    const y = centerY - rect.top;
                    
                    translateX = x - (x - translateX) * (scale / oldScale);
                    translateY = y - (y - translateY) * (scale / oldScale);
                }}
                
                updateTransform();
            }}
            
            // Zoom controls
            document.getElementById('zoomIn').addEventListener('click', () => {{
                zoom(0.2);
            }});
            
            document.getElementById('zoomOut').addEventListener('click', () => {{
                zoom(-0.2);
            }});
            
            document.getElementById('resetZoom').addEventListener('click', () => {{
                scale = 1;
                translateX = 0;
                translateY = 0;
                updateTransform();
            }});
            
            // Fullscreen
            document.getElementById('fullscreen').addEventListener('click', () => {{
                if (!document.fullscreenElement) {{
                    document.documentElement.requestFullscreen();
                }} else {{
                    document.exitFullscreen();
                }}
            }});
            
            // Mouse wheel zoom
            wrapper.addEventListener('wheel', (e) => {{
                e.preventDefault();
                const delta = e.deltaY > 0 ? -0.1 : 0.1;
                zoom(delta, e.clientX, e.clientY);
            }});
            
            // Pan with mouse
            container.addEventListener('mousedown', (e) => {{
                isDragging = true;
                startX = e.clientX - translateX;
                startY = e.clientY - translateY;
                container.classList.add('grabbing');
            }});
            
            document.addEventListener('mousemove', (e) => {{
                if (isDragging) {{
                    translateX = e.clientX - startX;
                    translateY = e.clientY - startY;
                    updateTransform();
                }}
            }});
            
            document.addEventListener('mouseup', () => {{
                isDragging = false;
                container.classList.remove('grabbing');
            }});
            
            // Keyboard shortcuts
            document.addEventListener('keydown', (e) => {{
                if (e.key === '+' || e.key === '=') {{
                    zoom(0.2);
                }} else if (e.key === '-') {{
                    zoom(-0.2);
                }} else if (e.key === '0') {{
                    scale = 1;
                    translateX = 0;
                    translateY = 0;
                    updateTransform();
                }} else if (e.key === 'f' || e.key === 'F') {{
                    if (!document.fullscreenElement) {{
                        document.documentElement.requestFullscreen();
                    }} else {{
                        document.exitFullscreen();
                    }}
                }}
            }});
            
        }}, 500);
    </script>
</body>
</html>"#, diagram_code);
    
    // Write HTML file
    fs::write(&html_path, html_content)?;
    
    // Open in default browser
    opener::open(&html_path)?;
    
    Ok(())
}

