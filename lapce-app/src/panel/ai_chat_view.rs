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
    kurbo::{Point, Size},
    reactive::{
        SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty, label, rich_text, scroll, stack,
        svg, text_input,
    },
};

use super::position::PanelPosition;
use crate::{
    ai_chat::{
        AiChatData, ChatEntry, ChatEntryKind, ChatRole, ChatToolCall, ToolCallStatus,
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

    // Reactive: rebuild when keys change
    container(
        dyn_stack(
            move || {
                let has_key = keys_config.with(|c| c.has_any_key());
                // Return a single-element vec with true/false so dyn_stack re-renders
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

    let selected_provider = create_rw_signal("gemini".to_string());
    let api_key_text = create_rw_signal("".to_string());

    let chat_data_save = chat_data.clone();
    let selected_provider_save = selected_provider;
    let api_key_save = api_key_text;

    container(
        stack((
            // ── Title ──
            label(|| "Configure AI Provider".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32 + 4.0)
                    .font_bold()
                    .margin_bottom(16.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            // ── Description ──
            label(|| "Enter an API key to get started with Forge AI.".to_string())
                .style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .margin_bottom(20.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
            // ── Provider selector ──
            label(|| "Provider".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .font_bold()
                    .margin_bottom(8.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            provider_selector(config, selected_provider),
            // ── API Key input ──
            label(|| "API Key".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .font_bold()
                    .margin_top(16.0)
                    .margin_bottom(8.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
            }),
            container(
                text_input(api_key_text)
                    .placeholder("Paste your API key here...")
                    .keyboard_navigable()
                    .style(move |s: floem::style::Style| {
                        let config = config.get();
                        s.width_pct(100.0)
                            .min_height(36.0)
                            .padding_horiz(10.0)
                            .padding_vert(8.0)
                            .background(config.color(LapceColor::EDITOR_BACKGROUND))
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            .font_size(config.ui.font_size() as f32)
                            .cursor(CursorStyle::Text)
                    }),
            )
            .style(move |s| {
                let config = config.get();
                s.width_pct(100.0)
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
            }),
            // ── Help text ──
            {
                let selected = selected_provider;
                label(move || {
                    let prov = selected.get();
                    match prov.as_str() {
                        "gemini" => "Get a Gemini key at ai.google.dev".to_string(),
                        "anthropic" => "Get a Claude key at console.anthropic.com".to_string(),
                        "openai" => "Get an OpenAI key at platform.openai.com".to_string(),
                        _ => String::new(),
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .margin_top(6.0)
                        .margin_bottom(20.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                })
            },
            // ── Save button ──
            label(|| "Save & Start".to_string())
                .on_click_stop(move |_| {
                    let provider = selected_provider_save.get_untracked();
                    let key = api_key_save.get_untracked();
                    if !key.trim().is_empty() {
                        chat_data_save.save_provider_key(&provider, key.trim());
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    s.padding_horiz(24.0)
                        .padding_vert(10.0)
                        .font_bold()
                        .font_size(config.ui.font_size() as f32)
                        .cursor(CursorStyle::Pointer)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        .background(config.color(LapceColor::EDITOR_BACKGROUND))
                        .border(1.0)
                        .border_color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        .hover(|s| {
                            s.background(
                                config.color(LapceColor::LAPCE_ICON_ACTIVE).multiply_alpha(0.2),
                            )
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

    stack((
        // ── Header ──────────────────────────────────────────
        chat_header(config, chat_data_clear),
        // ── Message list (scrollable) with auto-scroll ──────
        chat_message_list(config, chat_data.clone()),
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

/// Header with title, model dropdown, and clear button.
fn chat_header(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let model = chat_data.model;
    let dropdown_open = chat_data.dropdown_open;
    let _chat_data_dropdown = chat_data.clone();
    let chat_data_clear = chat_data.clone();

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
            // Model dropdown trigger
            model_dropdown_trigger(config, model, dropdown_open),
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

/// The clickable model badge + dropdown overlay.
fn model_dropdown_trigger(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    model: floem::reactive::RwSignal<String>,
    dropdown_open: floem::reactive::RwSignal<bool>,
) -> impl View {
    // Just the clickable badge; the dropdown panel is rendered in the chat view overlay
    label(move || {
        let m = model.get();
        format!("{} ▾", m)
    })
    .on_click_stop(move |_| {
        dropdown_open.update(|v| *v = !*v);
    })
    .style(move |s| {
        let config = config.get();
        s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
            .padding_horiz(8.0)
            .padding_vert(3.0)
            .cursor(CursorStyle::Pointer)
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
            .color(config.color(LapceColor::EDITOR_DIM))
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .hover(|s| {
                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

/// Scrollable chat message list with thinking indicator and auto-scroll.
fn chat_message_list(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let entries = chat_data.entries;
    let chat_data_dropdown = chat_data.clone();
    let is_loading = chat_data.is_loading;
    let has_first_token = chat_data.has_first_token;
    let streaming_text = chat_data.streaming_text;
    let scroll_trigger = chat_data.scroll_trigger;

    stack((
        // ── Dropdown overlay (shown when dropdown_open is true) ──
        model_dropdown_panel(config, chat_data_dropdown),
        // ── Messages + thinking indicator ──
        scroll(
            stack((
                // ── Chat entries ──
                dyn_stack(
                    move || {
                        let entries = entries.get();
                        entries.iter().cloned().collect::<Vec<_>>()
                    },
                    |entry: &ChatEntry| entry.key(),
                    move |entry| chat_entry_view(config, entry),
                )
                .style(|s| s.flex_col().width_pct(100.0).min_width(0.0)),

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
) -> impl View {
    match entry.kind {
        ChatEntryKind::Message { role, content } => {
            message_bubble(config, role, content).into_any()
        }
        ChatEntryKind::ToolCall(tc) => tool_call_card(config, tc).into_any(),
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
                            MarkdownContent::MermaidDiagram { svg: svg_str } => {
                                // Render the Mermaid SVG inline
                                container(
                                    svg(move || svg_str.clone())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.width_pct(100.0)
                                                .min_height(80.0)
                                                .max_height(500.0)
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
                            MarkdownContent::Image { .. } => {
                                container(empty()).into_any()
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
        ToolCallStatus::Running => "\u{25CF}",   // filled circle (pulsing via color)
        ToolCallStatus::Success => "\u{2713}",   // checkmark
        ToolCallStatus::Error => "\u{2717}",     // X mark
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
