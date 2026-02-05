use std::rc::Rc;

use chrono::{Local, TimeZone};
use floem::{
    ext_event::create_ext_action,
    event::{Event, EventListener},
    kurbo::Point,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    views::{container, dyn_stack, empty, label, scroll, stack, svg, Decorators},
    View,
};
use lapce_rpc::source_control::GitFileDiff;

use crate::{
    app::clickable_icon,
    config::{color::LapceColor, icon::LapceIcons},
    panel::{kind::PanelKind, position::PanelPosition},
    window_tab::WindowTabData,
};

/// Format a Unix timestamp into a human-readable date string
fn format_timestamp(timestamp: i64) -> String {
    if let Some(dt) = Local.timestamp_opt(timestamp, 0).single() {
        dt.format("%m/%d/%y, %I:%M %p").to_string()
    } else {
        "Unknown date".to_string()
    }
}

pub fn git_log_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let commits = source_control.commits;
    let proxy = window_tab_data.common.proxy.clone();
    let scope = window_tab_data.scope;
    
    let selected_commit_index: RwSignal<Option<usize>> = create_rw_signal(None);
    let selected_commit_files: RwSignal<Vec<GitFileDiff>> = create_rw_signal(Vec::new());
    let files_loading: RwSignal<bool> = create_rw_signal(false);
    
    // Split ratio (0.0 to 1.0) - start at 30%
    let split_ratio: RwSignal<f64> = create_rw_signal(0.30);
    let total_width: RwSignal<f64> = create_rw_signal(800.0);
    let drag_start: RwSignal<Option<(Point, f64)>> = create_rw_signal(None);
    
    // Smaller sizes for bottom panel
    let bottom_icon_size = move || (config.get().ui.icon_size() as f32 - 2.0).max(12.0);
    let bottom_font_size = move || (config.get().ui.font_size() as f32 - 1.0).max(11.0);
    
    // For closing the panel
    let panel = window_tab_data.panel.clone();
    
    // Main layout: header + content
    stack((
        // Header with title and close button
        container(
            stack((
                // Title
                stack((
                    svg(move || config.get().ui_svg(LapceIcons::GIT_LOG))
                        .style(move |s| {
                            let config = config.get();
                            let icon_size = bottom_icon_size();
                            s.size(icon_size, icon_size)
                                .margin_right(6.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        }),
                    label(|| "Git Log".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(bottom_font_size())
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .style(|s| s.items_center().flex_grow(1.0)),
                // Close button
                clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        panel.hide_panel(&PanelKind::GitLog);
                    },
                    || false,
                    || false,
                    || "Close",
                    config,
                )
                .style(|s| s.margin_right(4.0)),
            ))
            .style(|s| s.items_center().width_pct(100.0)),
        )
        .style(move |s| {
            let config = config.get();
            s.width_pct(100.0)
                .padding_vert(4.0)
                .padding_horiz(8.0)
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        }),
        // Content: Split view with commit list and details
        stack((
            // Left side: Commit list (30%)
            scroll(
            dyn_stack(
                move || {
                    let commits_list = commits.get();
                    commits_list.into_iter().enumerate().collect::<Vec<_>>()
                },
                move |(index, commit)| (commit.id.clone(), *index),
                move |(index, commit)| {
                    let commit_clone = commit.clone();
                    let commit_id_for_click = commit.id.clone();
                    let proxy_for_click = proxy.clone();
                    let is_selected = move || {
                        selected_commit_index.get() == Some(index)
                    };
                    
                    container(
                        stack((
                            // Commit icon/indicator
                            container(
                                svg(move || config.get().ui_svg(LapceIcons::GIT_COMMIT))
                                    .style(move |s| {
                                        let config = config.get();
                                        s.size(10.0, 10.0)
                                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                    }),
                            )
                            .style(move |s| {
                                let config = config.get();
                                s.width(20.0)
                                    .height(20.0)
                                    .items_center()
                                    .justify_center()
                                    .border_radius(10.0)
                                    .background(config.color(LapceColor::LAPCE_BORDER))
                                    .margin_right(8.0)
                            }),
                            // Commit info
                            stack((
                                // Summary (first line of commit message)
                                label(move || commit_clone.summary.clone())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(bottom_font_size())
                                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                                            .text_ellipsis()
                                            .max_width_pct(100.0)
                                    }),
                                // Author and date
                                {
                                    let author = commit.author_name.clone();
                                    let timestamp = commit.timestamp;
                                    label(move || {
                                        format!("{} - {}", author, format_timestamp(timestamp))
                                    })
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(bottom_font_size() - 1.0)
                                            .margin_top(2.0)
                                            .color(config.color(LapceColor::EDITOR_DIM))
                                    })
                                },
                            ))
                            .style(|s| s.flex_col().flex_grow(1.0).min_width(0.0)),
                            // Short hash on the right
                            {
                                let short_id = commit.short_id.clone();
                                label(move || short_id.clone())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(bottom_font_size() - 2.0)
                                            .font_family("monospace".to_string())
                                            .padding_horiz(6.0)
                                            .padding_vert(2.0)
                                            .border_radius(3.0)
                                            .background(config.color(LapceColor::LAPCE_BORDER))
                                            .color(config.color(LapceColor::EDITOR_DIM))
                                    })
                            },
                        ))
                        .style(|s| s.items_center().width_pct(100.0)),
                    )
                    .on_click_stop(move |_| {
                        selected_commit_index.set(Some(index));
                        
                        // Fetch changed files for this commit
                        files_loading.set(true);
                        selected_commit_files.set(Vec::new());
                        
                        let commit_id = commit_id_for_click.clone();
                        let send = create_ext_action(scope, move |result: Result<lapce_rpc::proxy::ProxyResponse, lapce_rpc::RpcError>| {
                            files_loading.set(false);
                            match result {
                                Ok(lapce_rpc::proxy::ProxyResponse::GitCommitDiffResponse { result }) => {
                                    selected_commit_files.set(result.files);
                                }
                                _ => {}
                            }
                        });
                        
                        proxy_for_click.git_get_commit_diff(commit_id, send);
                    })
                    .style(move |s| {
                        let config = config.get();
                        let selected = is_selected();
                        s.padding(6.0)
                            .width_pct(100.0)
                            .border_bottom(1.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .cursor(CursorStyle::Pointer)
                            .apply_if(selected, |s| {
                                s.background(config.color(LapceColor::PANEL_CURRENT_BACKGROUND))
                            })
                            .hover(|s| {
                                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                            })
                    })
                },
            )
            .style(|s| s.flex_col().width_pct(100.0)),
        )
        .style(move |s| {
            let config = config.get();
            let ratio = split_ratio.get();
            let width = total_width.get() * ratio;
            s.width(width as f32)
                .height_pct(100.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        }),
        // Draggable divider
        {
            let divider_view = empty();
            let view_id = divider_view.id();
            divider_view
                .on_event_stop(EventListener::PointerDown, move |event| {
                    view_id.request_active();
                    if let Event::PointerDown(pointer_event) = event {
                        drag_start.set(Some((pointer_event.pos, split_ratio.get_untracked())));
                    }
                })
                .on_event_stop(EventListener::PointerMove, move |event| {
                    if let Event::PointerMove(pointer_event) = event {
                        if let Some((start_pos, start_ratio)) = drag_start.get_untracked() {
                            let total = total_width.get_untracked();
                            let delta = pointer_event.pos.x - start_pos.x;
                            let new_ratio = (start_ratio + delta / total).clamp(0.15, 0.60);
                            split_ratio.set(new_ratio);
                        }
                    }
                })
                .on_event_stop(EventListener::PointerUp, move |_| {
                    drag_start.set(None);
                })
                .style(move |s| {
                    let config = config.get();
                    s.width(4.0)
                        .height_pct(100.0)
                        .cursor(CursorStyle::ColResize)
                        .background(config.color(LapceColor::LAPCE_BORDER))
                        .hover(|s| s.background(config.color(LapceColor::EDITOR_CARET)))
                })
        },
        // Right side: Commit details and changed files (70%)
        scroll(
            container({
                let commits_for_details = commits;
                stack((
                    // Commit details section
                    dyn_stack(
                        move || {
                            if let Some(idx) = selected_commit_index.get() {
                                let commits_list = commits_for_details.get();
                                if let Some(commit) = commits_list.get(idx) {
                                    return vec![commit.clone()];
                                }
                            }
                            vec![]
                        },
                        |commit| commit.id.clone(),
                        move |commit| {
                            let commit_clone = commit.clone();
                            stack((
                                // Commit hash
                                stack((
                                    svg(move || config.get().ui_svg(LapceIcons::GIT_COMMIT))
                                        .style(move |s| {
                                            let config = config.get();
                                            let icon_size = bottom_icon_size();
                                            s.size(icon_size, icon_size)
                                                .margin_right(6.0)
                                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                                        }),
                                    {
                                        let short_id = commit.short_id.clone();
                                        label(move || short_id.clone())
                                            .style(move |s| {
                                                let config = config.get();
                                                s.font_size(bottom_font_size())
                                                    .font_family("monospace".to_string())
                                                    .color(config.color(LapceColor::EDITOR_LINK))
                                            })
                                    },
                                ))
                                .style(|s| s.items_center().margin_bottom(8.0)),
                                // Author info
                                stack((
                                    label(|| "Author:".to_string())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.font_size(bottom_font_size())
                                                .font_bold()
                                                .width(55.0)
                                                .color(config.color(LapceColor::EDITOR_DIM))
                                        }),
                                    {
                                        let author_name = commit.author_name.clone();
                                        let author_email = commit.author_email.clone();
                                        label(move || format!("{} <{}>", author_name, author_email))
                                            .style(move |s| {
                                                let config = config.get();
                                                s.font_size(bottom_font_size())
                                                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                                            })
                                    },
                                ))
                                .style(|s| s.items_center().margin_bottom(4.0)),
                                // Date
                                stack((
                                    label(|| "Date:".to_string())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.font_size(bottom_font_size())
                                                .font_bold()
                                                .width(55.0)
                                                .color(config.color(LapceColor::EDITOR_DIM))
                                        }),
                                    {
                                        let timestamp = commit.timestamp;
                                        label(move || format_timestamp(timestamp))
                                            .style(move |s| {
                                                let config = config.get();
                                                s.font_size(bottom_font_size())
                                                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                                            })
                                    },
                                ))
                                .style(|s| s.items_center().margin_bottom(8.0)),
                                // Commit message
                                label(|| "Message:".to_string())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(bottom_font_size())
                                            .font_bold()
                                            .margin_bottom(4.0)
                                            .color(config.color(LapceColor::EDITOR_DIM))
                                    }),
                                {
                                    let message = commit_clone.message.clone();
                                    label(move || message.clone())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.font_size(bottom_font_size())
                                                .padding(6.0)
                                                .width_pct(100.0)
                                                .border_radius(4.0)
                                                .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                                        })
                                },
                            ))
                            .style(|s| s.flex_col().width_pct(100.0))
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                    // Changed files section
                    container(
                        stack((
                            // Section header
                            label(move || {
                                let count = selected_commit_files.with(|f| f.len());
                                if files_loading.get() {
                                    "Loading...".to_string()
                                } else if count > 0 {
                                    format!("Changed Files ({})", count)
                                } else {
                                    "".to_string()
                                }
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.font_size(bottom_font_size())
                                    .font_bold()
                                    .margin_top(8.0)
                                    .margin_bottom(4.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                            // Files list
                            dyn_stack(
                                move || selected_commit_files.get().into_iter().enumerate().collect::<Vec<_>>(),
                                |(idx, file)| (*idx, file.new_path.clone()),
                                move |(_, file)| {
                                    let file_path = file.new_path.clone()
                                        .or(file.old_path.clone())
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "Unknown file".to_string());
                                    let status = file.status;
                                    
                                    container(
                                        stack((
                                            // Status indicator
                                            label(move || {
                                                match status {
                                                    lapce_rpc::source_control::FileDiffKind::Modified => "M",
                                                    lapce_rpc::source_control::FileDiffKind::Added => "A",
                                                    lapce_rpc::source_control::FileDiffKind::Deleted => "D",
                                                    lapce_rpc::source_control::FileDiffKind::Renamed => "R",
                                                }.to_string()
                                            })
                                            .style(move |s| {
                                                let config = config.get();
                                                let color = match status {
                                                    lapce_rpc::source_control::FileDiffKind::Modified => config.color(LapceColor::SOURCE_CONTROL_MODIFIED),
                                                    lapce_rpc::source_control::FileDiffKind::Added => config.color(LapceColor::SOURCE_CONTROL_ADDED),
                                                    lapce_rpc::source_control::FileDiffKind::Deleted => config.color(LapceColor::SOURCE_CONTROL_REMOVED),
                                                    lapce_rpc::source_control::FileDiffKind::Renamed => config.color(LapceColor::SOURCE_CONTROL_MODIFIED),
                                                };
                                                s.font_size(bottom_font_size() - 1.0)
                                                    .font_bold()
                                                    .width(14.0)
                                                    .color(color)
                                            }),
                                            // File path
                                            label(move || file_path.clone())
                                                .style(move |s| {
                                                    let config = config.get();
                                                    s.font_size(bottom_font_size() - 1.0)
                                                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                                                        .text_ellipsis()
                                                }),
                                        ))
                                        .style(|s| s.items_center().gap(4.0)),
                                    )
                                    .style(move |s| {
                                        let config = config.get();
                                        s.padding_vert(2.0)
                                            .padding_horiz(4.0)
                                            .width_pct(100.0)
                                            .border_radius(2.0)
                                            .hover(|s| {
                                                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                                    .cursor(CursorStyle::Pointer)
                                            })
                                    })
                                },
                            )
                            .style(|s| s.flex_col().width_pct(100.0)),
                        ))
                        .style(|s| s.flex_col().width_pct(100.0)),
                    )
                    .style(|s| s.width_pct(100.0)),
                ))
                .style(|s| s.flex_col().width_pct(100.0))
            })
            .style(|s| s.padding(8.0).width_pct(100.0)),
        )
        .style(move |s| {
            let config = config.get();
            s.flex_grow(1.0)
                .height_pct(100.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        }),
        ))
        .on_resize(move |rect| {
            let width = rect.width();
            if total_width.get_untracked() != width {
                total_width.set(width);
            }
        })
        .style(|s| s.flex_grow(1.0).width_pct(100.0)),
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
    .debug_name("Git Log Panel")
}
