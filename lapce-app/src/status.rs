use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    event::EventPropagation,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    style::{AlignItems, CursorStyle, Display},
    views::{Decorators, label, stack, svg},
};
use indexmap::IndexMap;
use lapce_core::mode::{Mode, VisualMode};
use lsp_types::{DiagnosticSeverity, ProgressToken};

use crate::{
    command::LapceWorkbenchCommand,
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    editor::EditorData,
    listener::Listener,
    palette::kind::PaletteKind,
    panel::kind::PanelKind,
    source_control::SourceControlData,
    window_tab::{WindowTabData, WorkProgress},
};

pub fn status(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    status_height: RwSignal<f64>,
    _config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let config = window_tab_data.common.config;
    let diagnostics = window_tab_data.main_split.diagnostics;
    let editor = window_tab_data.main_split.active_editor;
    let panel = window_tab_data.panel.clone();
    let palette = window_tab_data.palette.clone();
    let diagnostic_count = create_memo(move |_| {
        let mut errors = 0;
        let mut warnings = 0;
        for (_, diagnostics) in diagnostics.get().iter() {
            for diagnostic in diagnostics.diagnostics.get().iter() {
                if let Some(severity) = diagnostic.severity {
                    match severity {
                        DiagnosticSeverity::ERROR => errors += 1,
                        DiagnosticSeverity::WARNING => warnings += 1,
                        _ => (),
                    }
                }
            }
        }
        (errors, warnings)
    });
    let branch = source_control.branch;
    let file_diffs = source_control.file_diffs;
    let branch = move || {
        format!(
            "{}{}",
            branch.get(),
            if file_diffs.with(|diffs| diffs.is_empty()) {
                ""
            } else {
                "*"
            }
        )
    };

    let progresses = window_tab_data.progresses;
    let window_tab_data_for_click = window_tab_data.clone();
    let mode = create_memo(move |_| window_tab_data.mode());
    let pointer_down = floem::reactive::create_rw_signal(false);

    stack((
        stack((
            label(move || match mode.get() {
                Mode::Normal => "Normal".to_string(),
                Mode::Insert => "Insert".to_string(),
                Mode::Visual(mode) => match mode {
                    VisualMode::Normal => "Visual".to_string(),
                    VisualMode::Linewise => "Visual Line".to_string(),
                    VisualMode::Blockwise => "Visual Block".to_string(),
                },
                Mode::Terminal => "Terminal".to_string(),
            })
            .style(move |s| {
                let config = config.get();
                let display = if config.core.modal {
                    Display::Flex
                } else {
                    Display::None
                };

                let (bg, fg) = match mode.get() {
                    Mode::Normal => (
                        LapceColor::STATUS_MODAL_NORMAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_NORMAL_FOREGROUND,
                    ),
                    Mode::Insert => (
                        LapceColor::STATUS_MODAL_INSERT_BACKGROUND,
                        LapceColor::STATUS_MODAL_INSERT_FOREGROUND,
                    ),
                    Mode::Visual(_) => (
                        LapceColor::STATUS_MODAL_VISUAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_VISUAL_FOREGROUND,
                    ),
                    Mode::Terminal => (
                        LapceColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                        LapceColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                    ),
                };

                let bg = config.color(bg);
                let fg = config.color(fg);

                s.display(display)
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(100.0)
                    .align_items(Some(AlignItems::Center))
                    .selectable(false)
            }),
            stack((
                svg(move || config.get().ui_svg(LapceIcons::GIT_LOG)).style(move |s| {
                    let config = config.get();
                    let icon_size = config.ui.icon_size() as f32;
                    s.size(icon_size, icon_size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
                label(branch).style(move |s| {
                    s.margin_left(10.0)
                        .color(config.get().color(LapceColor::STATUS_FOREGROUND))
                        .selectable(false)
                }),
            ))
            .style(move |s| {
                s.display(if branch().is_empty() {
                    Display::None
                } else {
                    Display::Flex
                })
                .height_pct(100.0)
                .padding_horiz(10.0)
                .align_items(Some(AlignItems::Center))
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
            .on_click_stop({
                let window_tab_data = window_tab_data_for_click.clone();
                let source_control = source_control.clone();
                move |_| {
                    // Open Git Log panel at the bottom
                    eprintln!("DEBUG: Status bar branch clicked - opening GitLog panel");
                    
                    // Load git log commits
                    source_control.load_git_log();
                    
                    // Open the Git Log panel
                    window_tab_data.toggle_panel_visual_at_position(
                        PanelKind::GitLog,
                        crate::panel::position::PanelPosition::BottomLeft,
                    );
                }
            }),
            {
                let panel = panel.clone();
                stack((
                    svg(move || config.get().ui_svg(LapceIcons::ERROR)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        },
                    ),
                    label(move || diagnostic_count.get().0.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(LapceColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                    svg(move || config.get().ui_svg(LapceIcons::WARNING)).style(
                        move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size)
                                .margin_left(5.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        },
                    ),
                    label(move || diagnostic_count.get().1.to_string()).style(
                        move |s| {
                            s.margin_left(5.0)
                                .color(
                                    config
                                        .get()
                                        .color(LapceColor::STATUS_FOREGROUND),
                                )
                                .selectable(false)
                        },
                    ),
                ))
                .on_click_stop(move |_| {
                    panel.show_panel(&PanelKind::Problem);
                })
                .style(move |s| {
                    s.height_pct(100.0)
                        .padding_horiz(10.0)
                        .items_center()
                        .hover(|s| {
                            s.cursor(CursorStyle::Pointer).background(
                                config
                                    .get()
                                    .color(LapceColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
        ))
        .style(|s| {
            s.height_pct(100.0)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0)
                .items_center()
        }),
        // Panel toggle icons moved to title bar
        progress_loader_view(config, progresses),
        stack({
            let palette_clone = palette.clone();
            let cursor_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let mut status = String::new();
                    let cursor = editor.cursor().get();
                    if let Some((line, column, character)) = editor
                        .doc_signal()
                        .get()
                        .buffer
                        .with(|buffer| cursor.get_line_col_char(buffer))
                    {
                        status = format!(
                            "Ln {}, Col {}, Char {}",
                            line + 1,
                            column + 1,
                            character,
                        );
                    }
                    if let Some(selection) = cursor.get_selection() {
                        let selection_range = selection.0.abs_diff(selection.1);

                        if selection.0 != selection.1 {
                            status =
                                format!("{status} ({selection_range} selected)");
                        }
                    }
                    let selection_count = cursor.get_selection_count();
                    if selection_count > 1 {
                        status = format!("{status} {selection_count} selections");
                    }
                    return status;
                }
                String::new()
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Line);
            });
            let palette_clone = palette.clone();
            let line_ending_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.buffer.with(|b| b.line_ending()).as_str()
                } else {
                    ""
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::LineEnding);
            });
            let palette_clone = palette.clone();
            let language_info = status_text(config, editor, move || {
                if let Some(editor) = editor.get() {
                    let doc = editor.doc_signal().get();
                    doc.syntax().with(|s| s.language.name())
                } else {
                    "unknown"
                }
            })
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Language);
            });
            (cursor_info, line_ending_info, language_info)
        })
        .style(|s| {
            s.height_pct(100.0)
                .flex_basis(0.0)
                .flex_grow(1.0)
                .justify_end()
        }),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != status_height.get_untracked() {
            status_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.border_top(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::STATUS_BACKGROUND))
            .flex_basis(config.ui.status_height() as f32)
            .flex_grow(0.0)
            .flex_shrink(0.0)
            .items_center()
    })
    .debug_name("Status/Bottom Bar")
}

fn progress_loader_view(
    config: ReadSignal<Arc<LapceConfig>>,
    progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
) -> impl View {
    let has_progress = create_memo(move |_| progresses.with(|items| !items.is_empty()));
    let progress_text = create_memo(move |_| {
        progresses.with(|items| {
            items.values().next().map(|p| match &p.message {
                Some(message) if !message.is_empty() => {
                    format!("{}: {}", p.title, message)
                }
                _ => p.title.clone(),
            })
        })
    });

    stack((
        svg(move || config.get().ui_svg(LapceIcons::IMAGE_LOADING)).style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.size(size, size)
                .color(config.color(LapceColor::STATUS_FOREGROUND))
        }),
        label(move || {
            progress_text
                .get()
                .unwrap_or_else(|| "Working...".to_string())
        })
        .style(move |s| {
            s.margin_left(6.0)
                .text_ellipsis()
                .selectable(false)
                .color(config.get().color(LapceColor::STATUS_FOREGROUND))
        }),
    ))
    .style(move |s| {
        let display = if has_progress.get() {
            Display::Flex
        } else {
            Display::None
        };
        s.display(display)
            .height_pct(100.0)
            .items_center()
            .padding_horiz(10.0)
    })
}

fn status_text<S: std::fmt::Display + 'static>(
    config: ReadSignal<Arc<LapceConfig>>,
    editor: Memo<Option<EditorData>>,
    text: impl Fn() -> S + 'static,
) -> impl View {
    label(text).style(move |s| {
        let config = config.get();
        let display = if editor
            .get()
            .map(|editor| {
                editor.doc_signal().get().content.with(|c| {
                    use crate::doc::DocContent;
                    matches!(c, DocContent::File { .. } | DocContent::Scratch { .. })
                })
            })
            .unwrap_or(false)
        {
            Display::Flex
        } else {
            Display::None
        };

        s.display(display)
            .height_full()
            .padding_horiz(10.0)
            .items_center()
            .color(config.color(LapceColor::STATUS_FOREGROUND))
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
            })
            .selectable(false)
    })
}
