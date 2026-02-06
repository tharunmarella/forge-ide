use std::{path::PathBuf, rc::Rc};

use floem::{
    View,
    action::show_context_menu,
    event::{Event, EventListener},
    menu::{Menu, MenuItem},
    peniko::kurbo::Rect,
    prelude::SignalTrack,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    style::{CursorStyle, Style},
    views::{
        Decorators, container, dyn_container, dyn_stack,
        editor::view::{LineRegion, cursor_caret},
        label, scroll, stack, svg,
    },
};
use indexmap::IndexMap;
use lapce_core::buffer::rope_text::RopeText;
use lapce_rpc::source_control::FileDiff;

use super::{
    data::PanelSection, kind::PanelKind, position::PanelPosition,
    view::foldable_panel_section,
};
use crate::{
    command::{CommandKind, InternalCommand, LapceCommand, LapceWorkbenchCommand},
    config::{color::LapceColor, icon::LapceIcons},
    editor::view::editor_view,
    settings::checkbox,
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};

pub fn source_control_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let workbench_command = window_tab_data.common.workbench_command;
    let focus = source_control.common.focus;
    let editor = source_control.editor.clone();
    let doc = editor.doc_signal();
    let cursor = editor.cursor();
    let viewport = editor.viewport();
    let window_origin = editor.window_origin();
    let editor = create_rw_signal(editor);
    let is_active = move |tracked| {
        let focus = if tracked {
            focus.get()
        } else {
            focus.get_untracked()
        };
        focus == Focus::Panel(PanelKind::SourceControl)
    };
    let is_empty = create_memo(move |_| {
        let doc = doc.get();
        doc.buffer.with(|b| b.len() == 0)
    });
    let debug_breakline = create_memo(move |_| None);

    // Tab selection: 0 = Changes, 1 = Untracked
    let selected_tab = create_rw_signal(0u8);
    let file_diffs = source_control.file_diffs;
    let untracked_files = source_control.untracked_files;
    let source_control_for_tabs = source_control.clone();
    
    stack((
        // 1. Horizontal tab bar: Changes | Untracked
        changes_tab_bar(selected_tab, file_diffs, untracked_files, config),
        // 2. Content area based on selected tab
        dyn_container(
            move || selected_tab.get(),
            move |tab| {
                if tab == 0 {
                    // Changes tab - show all tracked file changes
                    Box::new(
                        file_diffs_view(source_control_for_tabs.clone())
                            .style(|s| s.size_pct(100.0, 100.0))
                    ) as Box<dyn View>
                } else {
                    // Untracked tab - show untracked files
                    Box::new(
                        untracked_files_view(source_control_for_tabs.clone())
                            .style(|s| s.size_pct(100.0, 100.0))
                    ) as Box<dyn View>
                }
            },
        )
        .style(|s| s.flex_col().width_pct(100.0).flex_grow(1.0).flex_basis(0.0).min_height(100.0)),
        // 3. Commit message and buttons at the bottom
        stack((
            container({
                scroll({
                    let view = stack((
                        editor_view(
                            editor.get_untracked(),
                            debug_breakline,
                            is_active,
                        ),
                        label(|| "Commit Message".to_string()).style(move |s| {
                            let config = config.get();
                            s.absolute()
                                .items_center()
                                .height(config.editor.line_height() as f32)
                                .color(config.color(LapceColor::EDITOR_DIM))
                                .apply_if(!is_empty.get(), |s| s.hide())
                                .selectable(false)
                        }),
                    ))
                    .style(|s| {
                        s.absolute()
                            .min_size_pct(100.0, 100.0)
                            .padding_left(10.0)
                            .padding_vert(6.0)
                            .hover(|s| s.cursor(CursorStyle::Text))
                    });
                    let id = view.id();
                    view.on_event_cont(EventListener::PointerDown, move |event| {
                        let event = event.clone().offset((10.0, 6.0));
                        if let Event::PointerDown(pointer_event) = event {
                            id.request_active();
                            editor.get_untracked().pointer_down(&pointer_event);
                        }
                    })
                    .on_event_stop(EventListener::PointerMove, move |event| {
                        let event = event.clone().offset((10.0, 6.0));
                        if let Event::PointerMove(pointer_event) = event {
                            editor.get_untracked().pointer_move(&pointer_event);
                        }
                    })
                    .on_event_stop(
                        EventListener::PointerUp,
                        move |event| {
                            let event = event.clone().offset((10.0, 6.0));
                            if let Event::PointerUp(pointer_event) = event {
                                editor.get_untracked().pointer_up(&pointer_event);
                            }
                        },
                    )
                })
                .on_move(move |pos| {
                    window_origin.set(pos + (10.0, 6.0));
                })
                .on_scroll(move |rect| {
                    viewport.set(rect);
                })
                .ensure_visible(move || {
                    let cursor = cursor.get();
                    let offset = cursor.offset();
                    let e_data = editor.get_untracked();
                    e_data.doc_signal().track();
                    e_data.kind.track();
                    let LineRegion { x, width, rvline } = cursor_caret(
                        &e_data.editor,
                        offset,
                        !cursor.is_insert(),
                        cursor.affinity,
                    );
                    let config = config.get_untracked();
                    let line_height = config.editor.line_height();
                    // TODO: is there a way to avoid the calculation of the vline here?
                    let vline = e_data.editor.vline_of_rvline(rvline);
                    Rect::from_origin_size(
                        (x, (vline.get() * line_height) as f64),
                        (width, line_height as f64),
                    )
                    .inflate(30.0, 10.0)
                })
                .style(|s| s.absolute().size_pct(100.0, 100.0))
            })
            .style(move |s| {
                let config = config.get();
                s.width_pct(100.0)
                    .min_height(120.0)
                    .height(120.0)
                    .border(1.0)
                    .padding(-1.0)
                    .border_radius(6.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
            }),
            // Buttons row: Commit and Commit & Push
            stack((
                {
                    let source_control = source_control.clone();
                    label(|| "Commit".to_string())
                        .on_click_stop(move |_| {
                            if !is_empty.get_untracked() {
                                source_control.commit();
                            }
                        })
                        .style(move |s| {
                            let config = config.get();
                            let disabled = is_empty.get();
                            s.line_height(1.6)
                                .flex_grow(1.0)
                                .justify_center()
                                .padding_vert(10.0)
                                .font_size(config.ui.font_size() as f32)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(config.color(LapceColor::LAPCE_BORDER))
                                .apply_if(disabled, |s| {
                                    s.color(config.color(LapceColor::EDITOR_DIM))
                                        .cursor(CursorStyle::Default)
                                })
                                .apply_if(!disabled, |s| {
                                    s.hover(|s| {
                                        s.cursor(CursorStyle::Pointer).background(
                                            config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                        )
                                    })
                                    .active(|s| {
                                        s.background(config.color(
                                            LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                        ))
                                    })
                                })
                                .selectable(false)
                        })
                },
                {
                    let source_control = source_control.clone();
                    label(|| "Commit & Push".to_string())
                        .on_click_stop(move |_| {
                            if !is_empty.get_untracked() {
                                source_control.commit_and_push();
                            }
                        })
                        .style(move |s| {
                            let config = config.get();
                            let disabled = is_empty.get();
                            s.margin_left(8.0)
                                .line_height(1.6)
                                .flex_grow(1.0)
                                .justify_center()
                                .padding_vert(10.0)
                                .font_size(config.ui.font_size() as f32)
                                .border(1.0)
                                .border_radius(6.0)
                                .border_color(config.color(LapceColor::LAPCE_BORDER))
                                .apply_if(disabled, |s| {
                                    s.background(
                                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                                            .multiply_alpha(0.4)
                                    )
                                    .color(
                                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND)
                                            .multiply_alpha(0.5)
                                    )
                                    .cursor(CursorStyle::Default)
                                })
                                .apply_if(!disabled, |s| {
                                    s.background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                        .color(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND))
                                        .hover(|s| {
                                            s.cursor(CursorStyle::Pointer).background(
                                                config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                                                    .multiply_alpha(0.85),
                                            )
                                        })
                                        .active(|s| {
                                            s.background(
                                                config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                                                    .multiply_alpha(0.7),
                                            )
                                        })
                                })
                                .selectable(false)
                        })
                },
            ))
            .style(|s| s.margin_top(12.0).width_pct(100.0)),
        ))
        .style(|s| s.flex_col().width_pct(100.0).padding(12.0)),
    ))
    .on_event_stop(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::SourceControl) {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        }
    })
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
    .debug_name("Source Control Panel")
}

/// Horizontal tab bar with "Changes | Untracked" tabs (IntelliJ style)
fn changes_tab_bar(
    selected_tab: RwSignal<u8>,
    file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
    untracked_files: RwSignal<im::Vector<PathBuf>>,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let changes_count = create_memo(move |_| file_diffs.with(|d| d.len()));
    let untracked_count = create_memo(move |_| untracked_files.with(|f| f.len()));
    
    stack((
        // Changes tab
        label(move || {
            let count = changes_count.get();
            if count > 0 {
                format!("Changes ({})", count)
            } else {
                "Changes".to_string()
            }
        })
        .on_click_stop(move |_| {
            selected_tab.set(0);
        })
        .style(move |s| {
            let config = config.get();
            let is_selected = selected_tab.get() == 0;
            s.padding_horiz(12.0)
                .padding_vert(6.0)
                .cursor(CursorStyle::Pointer)
                .border_bottom(if is_selected { 2.0 } else { 0.0 })
                .border_color(if is_selected {
                    config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                } else {
                    floem::peniko::Color::TRANSPARENT
                })
                .color(if is_selected {
                    config.color(LapceColor::EDITOR_FOREGROUND)
                } else {
                    config.color(LapceColor::EDITOR_DIM)
                })
                .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
        }),
        // Untracked tab
        label(move || {
            let count = untracked_count.get();
            if count > 0 {
                format!("Untracked ({})", count)
            } else {
                "Untracked".to_string()
            }
        })
        .on_click_stop(move |_| {
            selected_tab.set(1);
        })
        .style(move |s| {
            let config = config.get();
            let is_selected = selected_tab.get() == 1;
            s.padding_horiz(12.0)
                .padding_vert(6.0)
                .cursor(CursorStyle::Pointer)
                .border_bottom(if is_selected { 2.0 } else { 0.0 })
                .border_color(if is_selected {
                    config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE)
                } else {
                    floem::peniko::Color::TRANSPARENT
                })
                .color(if is_selected {
                    config.color(LapceColor::EDITOR_FOREGROUND)
                } else {
                    config.color(LapceColor::EDITOR_DIM)
                })
                .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .items_center()
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}

fn file_diffs_view(source_control: SourceControlData) -> impl View {
    let file_diffs = source_control.file_diffs;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let lapce_command = source_control.common.lapce_command;
    let internal_command = source_control.common.internal_command;

    let view_fn = move |(path, (diff, _checked)): (PathBuf, (FileDiff, bool))| {
        let diff_for_style = diff.clone();
        let full_path = path.clone();
        let full_path_for_checkbox = full_path.clone();
        let full_path_for_toggle = full_path.clone();
        let diff_for_menu = diff.clone();
        let path_for_click = full_path.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
            path.strip_prefix(workspace_path)
                .unwrap_or(&full_path)
                .to_path_buf()
        } else {
            path
        };
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let style_path = path.clone();
        
        // Reactively read the checked state from file_diffs signal
        let is_checked = move || {
            let result = file_diffs.with(|diffs| {
                diffs.get(&full_path_for_checkbox).map(|(_, c)| *c).unwrap_or(false)
            });
            result
        };
        
        stack((
            container(checkbox(is_checked, config))
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.font_size() as f32 + 8.0;
                    s.size(size, size)
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                })
                .on_event_stop(EventListener::PointerDown, {
                    move |_| {
                        file_diffs.update(|diffs| {
                            if let Some((_, checked)) = diffs.get_mut(&full_path_for_toggle) {
                                *checked = !*checked;
                            }
                        });
                    }
                }),
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let max_width = panel_width.get() as f32
                    - 10.0
                    - size
                    - 6.0
                    - size
                    - 6.0
                    - 10.0
                    - size
                    - 6.0;
                s.text_ellipsis()
                    .margin_right(6.0)
                    .max_width(max_width)
                    .selectable(false)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0)
                    .flex_basis(0.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(0.0)
                    .selectable(false)
            }),
            container({
                svg(move || {
                    let svg = match &diff {
                        FileDiff::Modified(_) => LapceIcons::SCM_DIFF_MODIFIED,
                        FileDiff::Added(_) => LapceIcons::SCM_DIFF_ADDED,
                        FileDiff::Deleted(_) => LapceIcons::SCM_DIFF_REMOVED,
                        FileDiff::Renamed(_, _) => LapceIcons::SCM_DIFF_RENAMED,
                    };
                    config.get().ui_svg(svg)
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = match &diff_for_style {
                        FileDiff::Modified(_) => LapceColor::SOURCE_CONTROL_MODIFIED,
                        FileDiff::Added(_) => LapceColor::SOURCE_CONTROL_ADDED,
                        FileDiff::Deleted(_) => LapceColor::SOURCE_CONTROL_REMOVED,
                        FileDiff::Renamed(_, _) => {
                            LapceColor::SOURCE_CONTROL_MODIFIED
                        }
                    };
                    let color = config.color(color);
                    s.min_width(size).size(size, size).color(color)
                })
            })
            .style(|s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .padding_right(20.0)
                    .items_center()
                    .justify_end()
                    .pointer_events_none()  // Don't intercept clicks - let them pass through
            }),
        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .on_event_cont(EventListener::PointerDown, move |event| {
            let diff_for_menu = diff_for_menu.clone();

            let discard = move || {
                lapce_command.send(LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges,
                    ),
                    data: Some(serde_json::json!(diff_for_menu.clone())),
                });
            };

            if let Event::PointerDown(pointer_event) = event {
                if pointer_event.button.is_secondary() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Discard Changes").action(discard));
                    show_context_menu(menu, None);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.padding_left(10.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        scroll({
            dyn_stack(
                move || file_diffs.get(),
                |(path, (diff, _checked))| {
                    // Don't include checked in key - only path and diff type
                    (path.to_path_buf(), diff.clone())
                },
                view_fn,
            )
            .style(|s| s.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

/// Section title for "Staged Changes" with count badge
fn staged_section_title(source_control: SourceControlData) -> impl View {
    let staged_diffs = source_control.staged_diffs;
    stack((
        label(|| "Staged Changes".to_string()),
        label(move || {
            let count = staged_diffs.with(|diffs| diffs.len());
            if count > 0 {
                format!(" ({})", count)
            } else {
                String::new()
            }
        })
        .style(|s| s.margin_left(4.0)),
    ))
    .style(|s| s.items_center())
}

/// Section title for "Changes" (unstaged) with count badge
fn unstaged_section_title(source_control: SourceControlData) -> impl View {
    let unstaged_diffs = source_control.unstaged_diffs;
    stack((
        label(|| "Changes".to_string()),
        label(move || {
            let count = unstaged_diffs.with(|diffs| diffs.len());
            if count > 0 {
                format!(" ({})", count)
            } else {
                String::new()
            }
        })
        .style(|s| s.margin_left(4.0)),
    ))
    .style(|s| s.items_center())
}

/// Section title for "Untracked Files" with count badge
fn untracked_section_title(source_control: SourceControlData) -> impl View {
    let untracked_files = source_control.untracked_files;
    stack((
        label(|| "Untracked Files".to_string()),
        label(move || {
            let count = untracked_files.with(|files| files.len());
            if count > 0 {
                format!(" ({})", count)
            } else {
                String::new()
            }
        })
        .style(|s| s.margin_left(4.0)),
    ))
    .style(|s| s.items_center())
}

/// View for staged changes (files in the git index)
fn staged_diffs_view(source_control: SourceControlData) -> impl View {
    let file_diffs = source_control.staged_diffs;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let lapce_command = source_control.common.lapce_command;
    let internal_command = source_control.common.internal_command;

    let view_fn = move |(path, (diff, _checked)): (PathBuf, (FileDiff, bool))| {
        let diff_for_style = diff.clone();
        let full_path = path.clone();
        let full_path_for_checkbox = full_path.clone();
        let full_path_for_toggle = full_path.clone();
        let diff_for_menu = diff.clone();
        let path_for_click = full_path.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
            path.strip_prefix(workspace_path)
                .unwrap_or(&full_path)
                .to_path_buf()
        } else {
            path
        };
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let style_path = path.clone();
        
        let is_checked = move || {
            file_diffs.with(|diffs| {
                diffs.get(&full_path_for_checkbox).map(|(_, c)| *c).unwrap_or(false)
            })
        };
        
        stack((
            container(checkbox(is_checked, config))
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.font_size() as f32 + 8.0;
                    s.size(size, size)
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                })
                .on_event_stop(EventListener::PointerDown, {
                    move |_| {
                        file_diffs.update(|diffs| {
                            if let Some((_, checked)) = diffs.get_mut(&full_path_for_toggle) {
                                *checked = !*checked;
                            }
                        });
                    }
                }),
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let max_width = panel_width.get() as f32 - 10.0 - size - 6.0 - size - 6.0 - 10.0 - size - 6.0;
                s.text_ellipsis()
                    .margin_right(6.0)
                    .max_width(max_width)
                    .selectable(false)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0)
                    .flex_basis(0.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(0.0)
                    .selectable(false)
            }),
            container({
                svg(move || {
                    let svg = match &diff {
                        FileDiff::Modified(_) => LapceIcons::SCM_DIFF_MODIFIED,
                        FileDiff::Added(_) => LapceIcons::SCM_DIFF_ADDED,
                        FileDiff::Deleted(_) => LapceIcons::SCM_DIFF_REMOVED,
                        FileDiff::Renamed(_, _) => LapceIcons::SCM_DIFF_RENAMED,
                    };
                    config.get().ui_svg(svg)
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = match &diff_for_style {
                        FileDiff::Modified(_) => LapceColor::SOURCE_CONTROL_MODIFIED,
                        FileDiff::Added(_) => LapceColor::SOURCE_CONTROL_ADDED,
                        FileDiff::Deleted(_) => LapceColor::SOURCE_CONTROL_REMOVED,
                        FileDiff::Renamed(_, _) => LapceColor::SOURCE_CONTROL_MODIFIED,
                    };
                    let color = config.color(color);
                    s.min_width(size).size(size, size).color(color)
                })
            })
            .style(|s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .padding_right(20.0)
                    .items_center()
                    .justify_end()
                    .pointer_events_none()
            }),
        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .on_event_cont(EventListener::PointerDown, move |event| {
            let diff_for_menu = diff_for_menu.clone();

            let unstage = move || {
                lapce_command.send(LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges,
                    ),
                    data: Some(serde_json::json!(diff_for_menu.clone())),
                });
            };

            if let Event::PointerDown(pointer_event) = event {
                if pointer_event.button.is_secondary() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Unstage").action(unstage));
                    show_context_menu(menu, None);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.padding_left(10.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        scroll({
            dyn_stack(
                move || file_diffs.get(),
                |(path, (diff, _checked))| (path.to_path_buf(), diff.clone()),
                view_fn,
            )
            .style(|s| s.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

/// View for unstaged changes (modified but not in index)
fn unstaged_diffs_view(source_control: SourceControlData) -> impl View {
    let file_diffs = source_control.unstaged_diffs;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let lapce_command = source_control.common.lapce_command;
    let internal_command = source_control.common.internal_command;

    let view_fn = move |(path, (diff, _checked)): (PathBuf, (FileDiff, bool))| {
        let diff_for_style = diff.clone();
        let full_path = path.clone();
        let full_path_for_checkbox = full_path.clone();
        let full_path_for_toggle = full_path.clone();
        let diff_for_menu = diff.clone();
        let path_for_click = full_path.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
            path.strip_prefix(workspace_path)
                .unwrap_or(&full_path)
                .to_path_buf()
        } else {
            path
        };
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let style_path = path.clone();
        
        let is_checked = move || {
            file_diffs.with(|diffs| {
                diffs.get(&full_path_for_checkbox).map(|(_, c)| *c).unwrap_or(false)
            })
        };
        
        stack((
            container(checkbox(is_checked, config))
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.font_size() as f32 + 8.0;
                    s.size(size, size)
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                })
                .on_event_stop(EventListener::PointerDown, {
                    move |_| {
                        file_diffs.update(|diffs| {
                            if let Some((_, checked)) = diffs.get_mut(&full_path_for_toggle) {
                                *checked = !*checked;
                            }
                        });
                    }
                }),
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let max_width = panel_width.get() as f32 - 10.0 - size - 6.0 - size - 6.0 - 10.0 - size - 6.0;
                s.text_ellipsis()
                    .margin_right(6.0)
                    .max_width(max_width)
                    .selectable(false)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0)
                    .flex_basis(0.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(0.0)
                    .selectable(false)
            }),
            container({
                svg(move || {
                    let svg = match &diff {
                        FileDiff::Modified(_) => LapceIcons::SCM_DIFF_MODIFIED,
                        FileDiff::Added(_) => LapceIcons::SCM_DIFF_ADDED,
                        FileDiff::Deleted(_) => LapceIcons::SCM_DIFF_REMOVED,
                        FileDiff::Renamed(_, _) => LapceIcons::SCM_DIFF_RENAMED,
                    };
                    config.get().ui_svg(svg)
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = match &diff_for_style {
                        FileDiff::Modified(_) => LapceColor::SOURCE_CONTROL_MODIFIED,
                        FileDiff::Added(_) => LapceColor::SOURCE_CONTROL_ADDED,
                        FileDiff::Deleted(_) => LapceColor::SOURCE_CONTROL_REMOVED,
                        FileDiff::Renamed(_, _) => LapceColor::SOURCE_CONTROL_MODIFIED,
                    };
                    let color = config.color(color);
                    s.min_width(size).size(size, size).color(color)
                })
            })
            .style(|s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .padding_right(20.0)
                    .items_center()
                    .justify_end()
                    .pointer_events_none()
            }),
        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .on_event_cont(EventListener::PointerDown, move |event| {
            let diff_for_menu = diff_for_menu.clone();

            let discard = move || {
                lapce_command.send(LapceCommand {
                    kind: CommandKind::Workbench(
                        LapceWorkbenchCommand::SourceControlDiscardTargetFileChanges,
                    ),
                    data: Some(serde_json::json!(diff_for_menu.clone())),
                });
            };

            if let Event::PointerDown(pointer_event) = event {
                if pointer_event.button.is_secondary() {
                    let menu = Menu::new("")
                        .entry(MenuItem::new("Discard Changes").action(discard));
                    show_context_menu(menu, None);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.padding_left(10.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        scroll({
            dyn_stack(
                move || file_diffs.get(),
                |(path, (diff, _checked))| (path.to_path_buf(), diff.clone()),
                view_fn,
            )
            .style(|s| s.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}

/// View for untracked files (new files not tracked by git)
fn untracked_files_view(source_control: SourceControlData) -> impl View {
    let untracked_files = source_control.untracked_files;
    let config = source_control.common.config;
    let workspace = source_control.common.workspace.clone();
    let panel_rect = create_rw_signal(Rect::ZERO);
    let panel_width = create_memo(move |_| panel_rect.get().width());
    let internal_command = source_control.common.internal_command;

    let view_fn = move |full_path: PathBuf| {
        let path_for_click = full_path.clone();

        let path = if let Some(workspace_path) = workspace.path.as_ref() {
            full_path.strip_prefix(workspace_path)
                .unwrap_or(&full_path)
                .to_path_buf()
        } else {
            full_path.clone()
        };
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let folder = path
            .parent()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let style_path = path.clone();
        
        stack((
            svg(move || config.get().file_svg(&path).0).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let color = config.file_svg(&style_path).1;
                s.min_width(size)
                    .size(size, size)
                    .margin(6.0)
                    .apply_opt(color, Style::color)
            }),
            label(move || file_name.clone()).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                let max_width = panel_width.get() as f32 - 10.0 - size - 6.0 - size - 6.0 - 10.0 - size - 6.0;
                s.text_ellipsis()
                    .margin_right(6.0)
                    .max_width(max_width)
                    .selectable(false)
            }),
            label(move || folder.clone()).style(move |s| {
                s.text_ellipsis()
                    .flex_grow(1.0)
                    .flex_basis(0.0)
                    .color(config.get().color(LapceColor::EDITOR_DIM))
                    .min_width(0.0)
                    .selectable(false)
            }),
            // Untracked indicator (? icon or similar)
            container({
                svg(move || config.get().ui_svg(LapceIcons::SCM_DIFF_ADDED))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        let color = config.color(LapceColor::SOURCE_CONTROL_ADDED);
                        s.min_width(size).size(size, size).color(color)
                    })
            })
            .style(|s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .padding_right(20.0)
                    .items_center()
                    .justify_end()
                    .pointer_events_none()
            }),
        ))
        .on_click_stop(move |_| {
            internal_command.send(InternalCommand::OpenFileChanges {
                path: path_for_click.clone(),
            });
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.padding_left(10.0)
                .padding_right(10.0 + size + 6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    container({
        scroll({
            dyn_stack(
                move || untracked_files.get(),
                |path| path.clone(),
                view_fn,
            )
            .style(|s| s.line_height(1.6).flex_col().width_pct(100.0))
        })
        .style(|s| s.absolute().size_pct(100.0, 100.0))
    })
    .on_resize(move |rect| {
        panel_rect.set(rect);
    })
    .style(|s| s.size_pct(100.0, 100.0))
}
