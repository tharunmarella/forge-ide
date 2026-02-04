use std::{path::PathBuf, rc::Rc, sync::atomic};

use floem::{
    View,
    event::{Event, EventListener},
    ext_event::create_ext_action,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, clip, dyn_stack, editor::id::EditorId, empty, label, stack, svg,
    },
};
use indexmap::IndexMap;
use lapce_core::buffer::{
    diff::{DiffExpand, DiffLines, expand_diff_lines, rope_diff},
    rope_text::RopeText,
};
use lapce_rpc::{buffer::BufferId, proxy::ProxyResponse, source_control::FileDiff};
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};

use super::{EditorData, EditorViewKind};
use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    doc::{Doc, DocContent},
    id::{DiffEditorId, EditorTabId},
    main_split::{Editors, MainSplitData},
    wave::wave_box,
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct DiffInfo {
    pub is_right: bool,
    pub changes: Vec<DiffLines>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiffEditorInfo {
    pub left_content: DocContent,
    pub right_content: DocContent,
}

impl DiffEditorInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> DiffEditorData {
        let cx = data.scope.create_child();

        let diff_editor_id = DiffEditorId::next();

        let new_doc = {
            let data = data.clone();
            let common = data.common.clone();
            move |content: &DocContent| match content {
                DocContent::File { path, .. } => {
                    let (doc, _) = data.get_doc(path.clone(), None);
                    doc
                }
                DocContent::Local => {
                    Rc::new(Doc::new_local(cx, data.editors, common.clone()))
                }
                DocContent::History(history) => {
                    let doc = Doc::new_history(
                        cx,
                        content.clone(),
                        data.editors,
                        common.clone(),
                    );
                    let doc = Rc::new(doc);

                    {
                        let doc = doc.clone();
                        let send = create_ext_action(cx, move |result| {
                            if let Ok(ProxyResponse::BufferHeadResponse {
                                content,
                                ..
                            }) = result
                            {
                                doc.init_content(Rope::from(content));
                            }
                        });
                        common.proxy.get_buffer_head(
                            history.path.clone(),
                            move |result| {
                                send(result);
                            },
                        );
                    }

                    doc
                }
                DocContent::Scratch { name, .. } => {
                    let doc_content = DocContent::Scratch {
                        id: BufferId::next(),
                        name: name.to_string(),
                    };
                    let doc = Doc::new_content(
                        cx,
                        doc_content,
                        data.editors,
                        common.clone(),
                    );
                    let doc = Rc::new(doc);
                    data.scratch_docs.update(|scratch_docs| {
                        scratch_docs.insert(name.to_string(), doc.clone());
                    });
                    doc
                }
            }
        };

        let left_doc = new_doc(&self.left_content);
        let right_doc = new_doc(&self.right_content);

        let diff_editor_data = DiffEditorData::new(
            cx,
            diff_editor_id,
            editor_tab_id,
            left_doc,
            right_doc,
            data.editors,
            data.common.clone(),
        );

        data.diff_editors.update(|diff_editors| {
            diff_editors.insert(diff_editor_id, diff_editor_data.clone());
        });

        diff_editor_data
    }
}

#[derive(Clone)]
pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub editor_tab_id: RwSignal<EditorTabId>,
    pub scope: Scope,
    pub left: EditorData,
    pub right: EditorData,
    pub confirmed: RwSignal<bool>,
    pub focus_right: RwSignal<bool>,
}

impl DiffEditorData {
    pub fn new(
        cx: Scope,
        id: DiffEditorId,
        editor_tab_id: EditorTabId,
        left_doc: Rc<Doc>,
        right_doc: Rc<Doc>,
        editors: Editors,
        common: Rc<CommonData>,
    ) -> Self {
        let cx = cx.create_child();
        let confirmed = cx.create_rw_signal(false);

        // TODO: ensure that left/right are cleaned up
        let [left, right] = [left_doc, right_doc].map(|doc| {
            editors.make_from_doc(
                cx,
                doc,
                None,
                Some((editor_tab_id, id)),
                Some(confirmed),
                common.clone(),
            )
        });

        let data = Self {
            id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            scope: cx,
            left,
            right,
            confirmed,
            focus_right: cx.create_rw_signal(true),
        };

        data.listen_diff_changes();

        data
    }

    pub fn diff_editor_info(&self) -> DiffEditorInfo {
        DiffEditorInfo {
            left_content: self.left.doc().content.get_untracked(),
            right_content: self.right.doc().content.get_untracked(),
        }
    }

    pub fn copy(
        &self,
        cx: Scope,
        editor_tab_id: EditorTabId,
        diff_editor_id: EditorId,
        editors: Editors,
    ) -> Self {
        let cx = cx.create_child();
        let confirmed = cx.create_rw_signal(true);

        let [left, right] = [&self.left, &self.right].map(|editor_data| {
            editors
                .make_copy(
                    editor_data.id(),
                    cx,
                    None,
                    Some((editor_tab_id, diff_editor_id)),
                    Some(confirmed),
                )
                .unwrap()
        });

        let diff_editor = DiffEditorData {
            scope: cx,
            id: diff_editor_id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            focus_right: cx.create_rw_signal(true),
            left,
            right,
            confirmed,
        };

        diff_editor.listen_diff_changes();
        diff_editor
    }

    fn listen_diff_changes(&self) {
        let cx = self.scope;

        let left = self.left.clone();
        let left_doc_rev = {
            let left = left.clone();
            cx.create_memo(move |_| {
                let doc = left.doc_signal().get();
                (doc.content.get(), doc.buffer.with(|b| b.rev()))
            })
        };

        let right = self.right.clone();
        let right_doc_rev = {
            let right = right.clone();
            cx.create_memo(move |_| {
                let doc = right.doc_signal().get();
                (doc.content.get(), doc.buffer.with(|b| b.rev()))
            })
        };

        cx.create_effect(move |_| {
            let (_, left_rev) = left_doc_rev.get();
            let (left_editor_view, left_doc) = (left.kind, left.doc());
            let (left_atomic_rev, left_rope) =
                left_doc.buffer.with_untracked(|buffer| {
                    (buffer.atomic_rev(), buffer.text().clone())
                });

            let (_, right_rev) = right_doc_rev.get();
            let (right_editor_view, right_doc) = (right.kind, right.doc());
            let (right_atomic_rev, right_rope) =
                right_doc.buffer.with_untracked(|buffer| {
                    (buffer.atomic_rev(), buffer.text().clone())
                });

            let send = {
                let right_atomic_rev = right_atomic_rev.clone();
                create_ext_action(cx, move |changes: Option<Vec<DiffLines>>| {
                    let changes = if let Some(changes) = changes {
                        changes
                    } else {
                        return;
                    };

                    if left_atomic_rev.load(atomic::Ordering::Acquire) != left_rev {
                        return;
                    }

                    if right_atomic_rev.load(atomic::Ordering::Acquire) != right_rev
                    {
                        return;
                    }

                    left_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: false,
                        changes: changes.clone(),
                    }));
                    right_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: true,
                        changes,
                    }));
                })
            };

            rayon::spawn(move || {
                let changes = rope_diff(
                    left_rope,
                    right_rope,
                    right_rev,
                    right_atomic_rev.clone(),
                    Some(3),
                );
                send(changes);
            });
        });
    }
}

#[derive(Clone, PartialEq)]
struct DiffShowMoreSection {
    left_actual_line: usize,
    right_actual_line: usize,
    skip_start: usize,
    lines: usize,
}

pub fn diff_show_more_section_view(
    left_editor: &EditorData,
    right_editor: &EditorData,
) -> impl View + use<> {
    let left_editor_view = left_editor.kind;
    let right_editor_view = right_editor.kind;
    let right_screen_lines = right_editor.screen_lines();
    let right_scroll_delta = right_editor.editor.scroll_delta;
    let viewport = right_editor.viewport();
    let config = right_editor.common.config;

    let each_fn = move || {
        let editor_view = right_editor_view.get();

        if let EditorViewKind::Diff(diff_info) = editor_view {
            diff_info
                .changes
                .iter()
                .filter_map(|change| {
                    let DiffLines::Both(info) = change else {
                        return None;
                    };

                    let skip = info.skip.as_ref()?;

                    Some(DiffShowMoreSection {
                        left_actual_line: info.left.start,
                        right_actual_line: info.right.start,
                        skip_start: skip.start,
                        lines: skip.len(),
                    })
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    let key_fn = move |section: &DiffShowMoreSection| {
        (
            section.right_actual_line + section.skip_start,
            section.lines,
        )
    };

    let view_fn = move |section: DiffShowMoreSection| {
        stack((
            wave_box().style(move |s| {
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .color(config.get().color(LapceColor::PANEL_BACKGROUND))
            }),
            label(move || format!("{} Hidden Lines", section.lines)),
            label(|| "|".to_string()).style(|s| s.margin_left(10.0)),
            stack((
                svg(move || config.get().ui_svg(LapceIcons::FOLD)).style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                }),
                label(|| "Expand All".to_string()).style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::All,
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::All,
                            true,
                        );
                    }
                });
            })
            .style(|s| {
                s.margin_left(10.0)
                    .height_pct(100.0)
                    .items_center()
                    .hover(|s| s.cursor(CursorStyle::Pointer))
            }),
            label(|| "|".to_string()).style(|s| s.margin_left(10.0)),
            stack((
                svg(move || config.get().ui_svg(LapceIcons::FOLD_UP)).style(
                    move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    },
                ),
                label(|| "Expand Up".to_string()).style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::Up(10),
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::Up(10),
                            true,
                        );
                    }
                });
            })
            .style(move |s| {
                s.margin_left(10.0)
                    .height_pct(100.0)
                    .items_center()
                    .hover(|s| s.cursor(CursorStyle::Pointer))
            }),
            label(|| "|".to_string()).style(|s| s.margin_left(10.0)),
            stack((
                svg(move || config.get().ui_svg(LapceIcons::FOLD_DOWN)).style(
                    move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    },
                ),
                label(|| "Expand Down".to_string()).style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::Down(10),
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::Down(10),
                            true,
                        );
                    }
                });
            })
            .style(move |s| {
                s.margin_left(10.0)
                    .height_pct(100.0)
                    .items_center()
                    .hover(|s| s.cursor(CursorStyle::Pointer))
            }),
        ))
        .on_event_cont(EventListener::PointerWheel, move |event| {
            if let Event::PointerWheel(event) = event {
                right_scroll_delta.set(event.delta);
            }
        })
        .style(move |s| {
            let right_screen_lines = right_screen_lines.get();

            let mut right_line = section.right_actual_line + section.skip_start;
            let is_before_skip = if right_line > 0 {
                right_line -= 1;
                true
            } else {
                right_line += section.lines;
                false
            };

            let Some(line_info) = right_screen_lines.info_for_line(right_line)
            else {
                return s.hide();
            };

            let config = config.get();
            let line_height = config.editor.line_height();

            let mut y = line_info.y - viewport.get().y0;

            if is_before_skip {
                y += line_height as f64
            } else {
                y -= line_height as f64
            }

            s.absolute()
                .width_pct(100.0)
                .height(line_height as f32)
                .justify_center()
                .items_center()
                .margin_top(y)
                .pointer_events_auto()
                .hover(|s| s.cursor(CursorStyle::Default))
        })
    };

    stack((
        empty().style(move |s| {
            s.height(config.get().editor.line_height() as f32 + 1.0)
        }),
        clip(
            dyn_stack(each_fn, key_fn, view_fn)
                .style(|s| s.flex_col().size_pct(100.0, 100.0)),
        )
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .style(|s| {
        s.absolute()
            .flex_col()
            .size_pct(100.0, 100.0)
            .pointer_events_none()
    })
    .debug_name("Diff Show More Section")
}

/// Represents a change block for the diff gutter with checkbox
#[derive(Clone, PartialEq)]
struct DiffHunkBlock {
    /// The actual line number in the right document where change starts
    right_actual_line: usize,
    /// Number of lines in this change on the right side
    right_lines: usize,
    /// Unique index for this hunk
    index: usize,
}

/// SVG checkmark for checkbox (same as settings.rs)
const CHECKBOX_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 16 16"><polygon points="5.19,11.83 0.18,7.44 1.82,5.56 4.81,8.17 10,1.25 12,2.75" /></svg>"#;

/// Gutter view in the middle of the diff editor with checkboxes for each hunk
/// The checkboxes are linked to the file's checked state in file_diffs
pub fn diff_gutter_view(
    _left_editor: &EditorData,
    right_editor: &EditorData,
    file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
) -> impl View + use<> {
    let right_editor_view = right_editor.kind;
    let right_screen_lines = right_editor.screen_lines();
    let right_scroll_delta = right_editor.editor.scroll_delta;
    let viewport = right_editor.viewport();
    let config = right_editor.common.config;
    
    // Get the file path from the right editor's doc
    let doc = right_editor.doc();
    let file_path: Option<PathBuf> = match doc.content.get_untracked() {
        DocContent::File { path, .. } => Some(path),
        DocContent::History(h) => Some(h.path),
        _ => None,
    };

    // Collect hunk blocks from diff changes
    let each_fn = move || {
        let editor_view = right_editor_view.get();

        if let EditorViewKind::Diff(diff_info) = editor_view {
            let mut blocks = Vec::new();
            let mut index = 0;

            for change in diff_info.changes.iter() {
                match change {
                    DiffLines::Left(range) => {
                        // Lines only on left (deleted from right)
                        blocks.push(DiffHunkBlock {
                            right_actual_line: range.start,
                            right_lines: 0,
                            index,
                        });
                        index += 1;
                    }
                    DiffLines::Right(range) => {
                        // Lines only on right (added)
                        blocks.push(DiffHunkBlock {
                            right_actual_line: range.start,
                            right_lines: range.len(),
                            index,
                        });
                        index += 1;
                    }
                    DiffLines::Both(_) => {
                        // Same lines - no checkbox needed
                    }
                }
            }

            blocks
        } else {
            Vec::new()
        }
    };

    let key_fn = move |block: &DiffHunkBlock| {
        (block.right_actual_line, block.right_lines, block.index)
    };

    let view_fn = move |block: DiffHunkBlock| {
        let file_path_for_check = file_path.clone();
        let file_path_for_toggle = file_path.clone();
        let block_for_arrow = block.clone();
        
        // Read the file's checked state from file_diffs
        let is_checked = move || {
            if let Some(ref path) = file_path_for_check {
                file_diffs.with(|diffs| {
                    diffs.get(path).map(|(_, c)| *c).unwrap_or(false)
                })
            } else {
                false
            }
        };
        
        // SVG checkbox - same style as commit panel
        let svg_str = move || if is_checked() { CHECKBOX_SVG } else { "" }.to_string();
        
        // Checkbox for selecting the hunk
        let checkbox_view = svg(svg_str)
            .on_click_stop(move |_| {
                eprintln!("DEBUG: Checkbox clicked for hunk at line {}", block.right_actual_line);
                // Toggle the file's checked state (affects all hunks)
                if let Some(ref path) = file_path_for_toggle {
                    let path = path.clone();
                    eprintln!("DEBUG: Toggling file: {:?}", path);
                    file_diffs.update(|diffs| {
                        if let Some((_, checked)) = diffs.get_mut(&path) {
                            eprintln!("DEBUG: {} -> {}", *checked, !*checked);
                            *checked = !*checked;
                        }
                    });
                }
            })
            .style(move |s| {
                let right_screen_lines = right_screen_lines.get();
                
                // Use the line where the hunk starts
                let line = block.right_actual_line;
                
                let Some(line_info) = right_screen_lines.info_for_line(line) else {
                    return s.hide();
                };

                let config_val = config.get();
                let line_height = config_val.editor.line_height() as f64;
                let size = config_val.ui.font_size() as f64;
                let color = config_val.color(LapceColor::EDITOR_FOREGROUND);

                let y = line_info.y - viewport.get().y0;

                s.absolute()
                    .margin_top(y + (line_height - size) / 2.0)
                    .margin_left((24.0 - size) / 2.0)
                    .min_width(size as f32)
                    .size(size as f32, size as f32)
                    .color(color)
                    .border_color(color)
                    .border(1.0)
                    .border_radius(2.0)
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| s.background(config_val.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
            });
        
        // Arrow button for reverting this hunk (appears below checkbox)
        let arrow_view = label(|| "→".to_string())
            .on_click_stop(move |_| {
                eprintln!("DEBUG: Arrow clicked for hunk at line {}", block_for_arrow.right_actual_line);
                eprintln!("DEBUG: This would revert the change (copy from left/HEAD to right/working)");
                // TODO: Implement actual revert logic
            })
            .style(move |s| {
                let right_screen_lines = right_screen_lines.get();
                
                let line = block_for_arrow.right_actual_line;
                
                let Some(line_info) = right_screen_lines.info_for_line(line) else {
                    return s.hide();
                };

                let config_val = config.get();
                let line_height = config_val.editor.line_height() as f64;
                let size = config_val.ui.font_size() as f64;

                // Position arrow below the checkbox (next line)
                let y = line_info.y - viewport.get().y0 + line_height;

                s.absolute()
                    .margin_top(y + (line_height - size) / 2.0)
                    .margin_left((24.0 - size) / 2.0)
                    .min_width(size as f32)
                    .height(size as f32)
                    .items_center()
                    .justify_center()
                    .font_size(size as f32)
                    .cursor(CursorStyle::Pointer)
                    .color(config_val.color(LapceColor::EDITOR_FOREGROUND))
                    .hover(|s| s.background(config_val.color(LapceColor::PANEL_HOVERED_BACKGROUND)).border_radius(4.0))
            });
        
        // Return both checkbox and arrow
        stack((checkbox_view, arrow_view))
            .style(|s| s.size_pct(100.0, 100.0))
    };

    stack((
        empty().style(move |s| {
            s.height(config.get().editor.line_height() as f32 + 1.0)
        }),
        clip(
            dyn_stack(each_fn, key_fn, view_fn)
                .style(|s| s.flex_col().size_pct(100.0, 100.0)),
        )
        .on_event_cont(EventListener::PointerWheel, move |event| {
            if let Event::PointerWheel(event) = event {
                right_scroll_delta.set(event.delta);
            }
        })
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .style(move |s| {
        let config = config.get();
        s.flex_col()
            .width(24.0)
            .height_pct(100.0)
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .border_left(1.0)
            .border_right(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    })
    .debug_name("Diff Gutter")
}
