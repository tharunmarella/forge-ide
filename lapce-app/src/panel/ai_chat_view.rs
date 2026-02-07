//! AI Chat panel view for the right sidebar.
//!
//! Provides a chat interface for interacting with the AI coding agent.
//! Shows a setup view when no API keys are configured.

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
    let config = window_tab_data.common.config;
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
                s.font_size((config.ui.font_size() as f32 + 4.0))
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

    stack((
        // ── Header ──────────────────────────────────────────
        chat_header(config, chat_data_clear),
        // ── Message list (scrollable) ───────────────────────
        chat_message_list(config, chat_data.clone()),
        // ── Input area at the bottom ────────────────────────
        chat_input_area(window_tab_data, chat_data),
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

/// Header with title, model dropdown, and clear button.
fn chat_header(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let model = chat_data.model;
    let dropdown_open = chat_data.dropdown_open;
    let chat_data_dropdown = chat_data.clone();
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
                    s.font_size(config.ui.font_size() as f32)
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

/// Scrollable chat message list.
fn chat_message_list(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    chat_data: AiChatData,
) -> impl View {
    let entries = chat_data.entries;
    let chat_data_dropdown = chat_data.clone();

    stack((
        // ── Dropdown overlay (shown when dropdown_open is true) ──
        model_dropdown_panel(config, chat_data_dropdown),
        // ── Messages ──
        scroll(
            dyn_stack(
                move || {
                    let entries = entries.get();
                    entries.iter().cloned().collect::<Vec<_>>()
                },
                |entry: &ChatEntry| entry.key(),
                move |entry| chat_entry_view(config, entry),
            )
            .style(|s| s.flex_col().width_pct(100.0).padding_vert(8.0)),
        )
        .style(|s| s.flex_grow(1.0).flex_basis(0.0).width_pct(100.0)),
    ))
    .style(|s| s.flex_col().flex_grow(1.0).flex_basis(0.0).width_pct(100.0))
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
/// Assistant messages are rendered as markdown with syntax-highlighted code blocks.
/// User and system messages are rendered as plain text.
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

    // Pre-parse markdown for assistant messages
    let md_content = if is_assistant {
        let cfg = config.get_untracked();
        crate::markdown::parse_markdown(&content, 1.5, &cfg)
    } else {
        Vec::new()
    };

    container(
        stack((
            // Role label
            label(move || role_label.to_string()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
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
                                rich_text(move || text_layout.clone())
                                    .style(|s| s.width_pct(100.0)),
                            )
                            .style(|s| s.width_pct(100.0))
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
                            MarkdownContent::Image { .. } => {
                                container(empty()).into_any()
                            }
                        }
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0))
                .into_any()
            } else {
                label(move || content.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(config.ui.font_size() as f32)
                            .width_pct(100.0)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    })
                    .into_any()
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding_horiz(12.0)
            .padding_vert(8.0)
            .width_pct(100.0)
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

/// A tool call card showing the tool name, arguments, and status.
fn tool_call_card(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    tc: ChatToolCall,
) -> impl View {
    let status_text = match &tc.status {
        ToolCallStatus::Pending => "Pending",
        ToolCallStatus::Running => "Running...",
        ToolCallStatus::Success => "Done",
        ToolCallStatus::Error => "Error",
    };
    let tool_name = tc.name.clone();
    let args_preview = tc.arguments.chars().take(80).collect::<String>();
    let output_preview = tc.output.clone().map(|o| {
        if o.len() > 200 {
            format!("{}...", &o[..200])
        } else {
            o
        }
    });

    container(
        stack((
            stack((
                label(move || format!("Tool: {}", tool_name)).style(move |s| {
                    let config = config.get();
                    s.font_size((config.ui.font_size() as f32 - 1.0).max(11.0))
                        .font_bold()
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                }),
                empty().style(|s| s.flex_grow(1.0)),
                label(move || status_text.to_string()).style(move |s| {
                    let config = config.get();
                    let color = match tc.status {
                        ToolCallStatus::Pending => config.color(LapceColor::EDITOR_DIM),
                        ToolCallStatus::Running => config.color(LapceColor::LAPCE_ICON_ACTIVE),
                        ToolCallStatus::Success => config.color(LapceColor::EDITOR_FOREGROUND),
                        ToolCallStatus::Error => config.color(LapceColor::LAPCE_ERROR),
                    };
                    s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                        .color(color)
                }),
            ))
            .style(|s| s.items_center().width_pct(100.0)),
            label(move || args_preview.clone()).style(move |s| {
                let config = config.get();
                s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                    .margin_top(4.0)
                    .width_pct(100.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            {
                let output = output_preview.clone();
                container(
                    label(move || output.clone().unwrap_or_default()).style(move |s| {
                        let config = config.get();
                        s.font_size((config.ui.font_size() as f32 - 2.0).max(10.0))
                            .width_pct(100.0)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    }),
                )
                .style(move |s| {
                    s.margin_top(4.0)
                        .apply_if(output_preview.is_none(), |s| s.hide())
                })
            },
        ))
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(move |s| {
        let config = config.get();
        s.padding(8.0)
            .margin_horiz(8.0)
            .margin_vert(4.0)
            .width_pct(100.0)
            .border(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
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
                "...".to_string()
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
