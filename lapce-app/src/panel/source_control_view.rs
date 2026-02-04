use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    ext_event::create_ext_action,
    reactive::{SignalGet, SignalUpdate, create_rw_signal},
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty,
        label, scroll, stack, svg,
    },
};
use lapce_rpc::proxy::ProxyResponse;
use lapce_rpc::source_control::GitCommitInfo;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    command::LapceWorkbenchCommand,
    config::{color::LapceColor, icon::LapceIcons},
    source_control::SourceControlData,
    window_tab::{Focus, WindowTabData},
};

pub fn source_control_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let focus = source_control.common.focus;
    
    // State for expanded sections
    let head_expanded = create_rw_signal(true);
    let local_expanded = create_rw_signal(true);
    let remote_expanded = create_rw_signal(true);
    
    // Tab state: "Git" (branches) or "Log" (commit history)
    let active_tab = create_rw_signal(0); // 0 = Git, 1 = Log
    
    // Split pane layout
    stack((
        // Header with tabs
        stack((
            // Git tab
            label(|| "Git".to_string())
                .on_click_stop(move |_| active_tab.set(0))
                .style(move |s| {
                    let config = config.get();
                    let is_active = active_tab.get() == 0;
                    s.padding_horiz(12.0)
                        .padding_vert(6.0)
                        .font_size(12.0)
                        .cursor(CursorStyle::Pointer)
                        .border_bottom(if is_active { 2.0 } else { 0.0 })
                        .border_color(config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE))
                        .color(if is_active {
                            config.color(LapceColor::PANEL_FOREGROUND)
                        } else {
                            config.color(LapceColor::PANEL_FOREGROUND_DIM)
                        })
                }),
            // Log tab
            label(|| "Log".to_string())
                .on_click_stop(move |_| active_tab.set(1))
                .style(move |s| {
                    let config = config.get();
                    let is_active = active_tab.get() == 1;
                    s.padding_horiz(12.0)
                        .padding_vert(6.0)
                        .font_size(12.0)
                        .cursor(CursorStyle::Pointer)
                        .border_bottom(if is_active { 2.0 } else { 0.0 })
                        .border_color(config.color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE))
                        .color(if is_active {
                            config.color(LapceColor::PANEL_FOREGROUND)
                        } else {
                            config.color(LapceColor::PANEL_FOREGROUND_DIM)
                        })
                }),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_pct(100.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
        }),
        
        // Content area based on active tab
        container(
            stack((
                // Git tab content (branch tree)
                branch_tree_view(
                    source_control.clone(),
                    config,
                    head_expanded,
                    local_expanded,
                    remote_expanded,
                )
                .style(move |s| s.size_pct(100.0, 100.0).apply_if(active_tab.get() != 0, |s| s.hide())),
                
                // Log tab content (commit history)
                git_log_view(window_tab_data.clone(), source_control.clone(), config)
                    .style(move |s| s.size_pct(100.0, 100.0).apply_if(active_tab.get() != 1, |s| s.hide())),
            ))
        )
        .style(|s| s.flex_grow(1.0).size_pct(100.0, 100.0)),
    ))
    .on_event_stop(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Panel(PanelKind::SourceControl) {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        }
    })
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
    .debug_name("Source Control Panel")
}

/// Branch tree view showing HEAD, Local branches, and Remote branches
fn branch_tree_view(
    source_control: SourceControlData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    head_expanded: floem::reactive::RwSignal<bool>,
    local_expanded: floem::reactive::RwSignal<bool>,
    remote_expanded: floem::reactive::RwSignal<bool>,
) -> impl View {
    let current_branch = source_control.branch;
    let branches = source_control.branches;
    let lapce_command = source_control.common.lapce_command;
    
    scroll(
        stack((
            // HEAD (Current Branch) section
            stack((
                // Section header
                stack((
                    svg(move || config.get().ui_svg(if head_expanded.get() { 
                        LapceIcons::ITEM_OPENED 
                    } else { 
                        LapceIcons::ITEM_CLOSED 
                    }))
                    .style(move |s| {
                        let config = config.get();
                        s.size(10.0, 10.0)
                            .margin_right(4.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    label(|| "HEAD (Current Branch)".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .on_click_stop(move |_| head_expanded.update(|v| *v = !*v))
                .style(move |s| {
                    let config = config.get();
                    s.items_center()
                        .padding_horiz(8.0)
                        .padding_vert(4.0)
                        .width_pct(100.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                }),
                // HEAD content
                container(
                    stack((
                        svg(move || config.get().ui_svg(LapceIcons::SCM))
                            .style(move |s| {
                                let config = config.get();
                                s.size(12.0, 12.0)
                                    .margin_right(6.0)
                                    .color(config.color(LapceColor::EDITOR_CARET))
                            }),
                        label(move || {
                            let b = current_branch.get();
                            if b.is_empty() { "main".to_string() } else { b }
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(12.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                    ))
                    .style(|s| s.items_center()),
                )
                .style(move |s| {
                    s.padding_left(24.0)
                        .padding_vert(4.0)
                        .width_pct(100.0)
                        .apply_if(!head_expanded.get(), |s| s.hide())
                }),
            ))
            .style(|s| s.flex_col().width_pct(100.0)),
            
            // Local branches section
            stack((
                // Section header
                stack((
                    svg(move || config.get().ui_svg(if local_expanded.get() { 
                        LapceIcons::ITEM_OPENED 
                    } else { 
                        LapceIcons::ITEM_CLOSED 
                    }))
                    .style(move |s| {
                        let config = config.get();
                        s.size(10.0, 10.0)
                            .margin_right(4.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    label(|| "Local".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .on_click_stop(move |_| local_expanded.update(|v| *v = !*v))
                .style(move |s| {
                    let config = config.get();
                    s.items_center()
                        .padding_horiz(8.0)
                        .padding_vert(4.0)
                        .width_pct(100.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                }),
                // Local branches list
                container(
                    dyn_stack(
                        move || {
                            branches.get()
                                .into_iter()
                                .filter(|b| !b.starts_with("origin/") && !b.starts_with("remotes/"))
                                .enumerate()
                                .collect::<Vec<_>>()
                        },
                        |(i, _)| *i,
                        move |(_, branch_name)| {
                            let is_current = branch_name == current_branch.get_untracked();
                            let branch_for_checkout = branch_name.clone();
                            let branch_display = branch_name.clone();
                            
                            stack((
                                svg(move || config.get().ui_svg(LapceIcons::SCM))
                                    .style(move |s| {
                                        let config = config.get();
                                        s.size(12.0, 12.0)
                                            .margin_right(6.0)
                                            .color(if is_current {
                                                config.color(LapceColor::EDITOR_CARET)
                                            } else {
                                                config.color(LapceColor::LAPCE_ICON_ACTIVE)
                                            })
                                    }),
                                label(move || branch_display.clone())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(12.0)
                                            .flex_grow(1.0)
                                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                                    }),
                                // Checkmark for current branch
                                label(move || if is_current { "✓" } else { "" }.to_string())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(11.0)
                                            .color(config.color(LapceColor::EDITOR_CARET))
                                    }),
                            ))
                            .on_double_click_stop(move |_| {
                                // Double-click to checkout
                                lapce_command.send(crate::command::LapceCommand {
                                    kind: crate::command::CommandKind::Workbench(
                                        LapceWorkbenchCommand::CheckoutReference,
                                    ),
                                    data: Some(serde_json::json!(branch_for_checkout.clone())),
                                });
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.items_center()
                                    .padding_left(24.0)
                                    .padding_right(8.0)
                                    .padding_vert(3.0)
                                    .width_pct(100.0)
                                    .cursor(CursorStyle::Pointer)
                                    .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                )
                .style(move |s| s.width_pct(100.0).apply_if(!local_expanded.get(), |s| s.hide())),
            ))
            .style(|s| s.flex_col().width_pct(100.0)),
            
            // Remote branches section
            stack((
                // Section header
                stack((
                    svg(move || config.get().ui_svg(if remote_expanded.get() { 
                        LapceIcons::ITEM_OPENED 
                    } else { 
                        LapceIcons::ITEM_CLOSED 
                    }))
                    .style(move |s| {
                        let config = config.get();
                        s.size(10.0, 10.0)
                            .margin_right(4.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                    label(|| "Remote".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .on_click_stop(move |_| remote_expanded.update(|v| *v = !*v))
                .style(move |s| {
                    let config = config.get();
                    s.items_center()
                        .padding_horiz(8.0)
                        .padding_vert(4.0)
                        .width_pct(100.0)
                        .cursor(CursorStyle::Pointer)
                        .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                }),
                // Remote branches list
                container(
                    dyn_stack(
                        move || {
                            branches.get()
                                .into_iter()
                                .filter(|b| b.starts_with("origin/") || b.starts_with("remotes/"))
                                .enumerate()
                                .collect::<Vec<_>>()
                        },
                        |(i, _)| *i,
                        move |(_, branch_name)| {
                            let branch_display = branch_name.clone();
                            let branch_for_checkout = branch_name.clone();
                            
                            stack((
                                svg(move || config.get().ui_svg(LapceIcons::SCM))
                                    .style(move |s| {
                                        let config = config.get();
                                        s.size(12.0, 12.0)
                                            .margin_right(6.0)
                                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                                    }),
                                label(move || branch_display.clone())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(12.0)
                                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                                    }),
                            ))
                            .on_double_click_stop(move |_| {
                                // Double-click to checkout
                                lapce_command.send(crate::command::LapceCommand {
                                    kind: crate::command::CommandKind::Workbench(
                                        LapceWorkbenchCommand::CheckoutReference,
                                    ),
                                    data: Some(serde_json::json!(branch_for_checkout.clone())),
                                });
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.items_center()
                                    .padding_left(24.0)
                                    .padding_right(8.0)
                                    .padding_vert(3.0)
                                    .width_pct(100.0)
                                    .cursor(CursorStyle::Pointer)
                                    .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                )
                .style(move |s| s.width_pct(100.0).apply_if(!remote_expanded.get(), |s| s.hide())),
            ))
            .style(|s| s.flex_col().width_pct(100.0)),
        ))
        .style(|s| s.flex_col().width_pct(100.0).padding_vert(4.0)),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}

/// Git log view showing commit history
fn git_log_view(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let commits = source_control.commits;
    let commits_loading = source_control.commits_loading;
    let selected_commit = create_rw_signal::<Option<String>>(None);
    let search_text = create_rw_signal(String::new());
    let proxy = source_control.common.proxy.clone();
    let scope = window_tab_data.scope;
    
    // Fetch commits on load
    {
        let commits = commits;
        let commits_loading = commits_loading;
        let commits_total_count = source_control.commits_total_count;
        let proxy = proxy.clone();
        
        // Use create_ext_action for thread-safe callback
        let action = create_ext_action(scope, move |result: Result<ProxyResponse, lapce_rpc::RpcError>| {
            commits_loading.set(false);
            if let Ok(ProxyResponse::GitLogResponse { result }) = result {
                commits.set(result.commits.into_iter().collect());
                commits_total_count.set(result.total_count);
            }
        });
        
        commits_loading.set(true);
        proxy.git_log(100, 0, None, None, None, action);
    }
    
    let refresh_commits = {
        let commits = commits;
        let commits_loading = commits_loading;
        let commits_total_count = source_control.commits_total_count;
        let proxy = proxy.clone();
        
        move || {
            let action = create_ext_action(scope, move |result: Result<ProxyResponse, lapce_rpc::RpcError>| {
                commits_loading.set(false);
                if let Ok(ProxyResponse::GitLogResponse { result }) = result {
                    commits.set(result.commits.into_iter().collect());
                    commits_total_count.set(result.total_count);
                }
            });
            
            commits_loading.set(true);
            proxy.git_log(100, 0, None, None, None, action);
        }
    };
    
    stack((
        // Filter bar
        stack((
            // Refresh button
            svg(move || config.get().ui_svg(LapceIcons::DEBUG_RESTART))
                .on_click_stop({
                    let refresh = refresh_commits.clone();
                    move |_| refresh()
                })
                .style(move |s| {
                    let config = config.get();
                    s.size(16.0, 16.0)
                        .margin_right(8.0)
                        .cursor(CursorStyle::Pointer)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
            // Search placeholder
            label(move || {
                if commits_loading.get() {
                    "Loading...".to_string()
                } else {
                    format!("{} commits", commits.get().len())
                }
            })
            .style(move |s| {
                let config = config.get();
                s.font_size(11.0)
                    .flex_grow(1.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .padding_horiz(8.0)
                .padding_vert(6.0)
                .width_pct(100.0)
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
        }),
        
        // Commit list with graph
        scroll(
            dyn_stack(
                move || {
                    commits.get()
                        .into_iter()
                        .enumerate()
                        .collect::<Vec<_>>()
                },
                |(i, c)| format!("{}-{}", *i, c.id.clone()),
                move |(idx, commit)| {
                    commit_row_view(
                        commit,
                        idx,
                        config,
                        selected_commit,
                    )
                },
            )
            .style(|s| s.flex_col().width_pct(100.0)),
        )
        .style(|s| s.flex_grow(1.0).size_pct(100.0, 100.0)),
        
        // Commit details panel (when a commit is selected)
        container(
            dyn_stack(
                move || selected_commit.get().into_iter().collect::<Vec<_>>(),
                |id| id.clone(),
                move |commit_id| {
                    // Find the commit
                    let commit = commits.get().iter().find(|c| c.id == commit_id).cloned();
                    if let Some(commit) = commit {
                        Box::new(commit_details_view(commit, config, selected_commit)) as Box<dyn View>
                    } else {
                        Box::new(empty()) as Box<dyn View>
                    }
                },
            )
            .style(|s| s.width_pct(100.0)),
        )
        .style(move |s| {
            let config = config.get();
            let has_selection = selected_commit.get().is_some();
            s.width_pct(100.0)
                .apply_if(!has_selection, |s| s.hide())
                .border_top(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .max_height(200.0)
        }),
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

/// A single commit row in the log view
fn commit_row_view(
    commit: GitCommitInfo,
    idx: usize,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    selected_commit: floem::reactive::RwSignal<Option<String>>,
) -> impl View {
    let commit_id = commit.id.clone();
    let commit_id_for_click = commit.id.clone();
    let short_id = commit.short_id.clone();
    let summary = commit.summary.clone();
    let author_name = commit.author_name.clone();
    let timestamp = commit.timestamp;
    let is_head = commit.is_head;
    let branches = commit.branches.clone();
    let tags = commit.tags.clone();
    let num_parents = commit.parents.len();
    
    stack((
        // Graph column (simplified)
        container(
            stack((
                // Graph node
                container(empty())
                    .style(move |s| {
                        let config = config.get();
                        s.size(8.0, 8.0)
                            .border_radius(4.0)
                            .background(if is_head {
                                config.color(LapceColor::EDITOR_CARET)
                            } else if num_parents > 1 {
                                // Merge commit
                                config.color(LapceColor::TERMINAL_MAGENTA)
                            } else {
                                config.color(LapceColor::TERMINAL_BLUE)
                            })
                    }),
            ))
            .style(|s| s.items_center().justify_center()),
        )
        .style(|s| s.width(24.0).height(24.0).items_center().justify_center()),
        
        // Commit details
        stack((
            // First line: summary + branch/tag labels
            stack((
                label(move || summary.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(12.0)
                            .text_ellipsis()
                            .max_width(400.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    }),
                // Branch labels
                dyn_stack(
                    move || branches.clone().into_iter().enumerate().collect::<Vec<_>>(),
                    |(i, _)| *i,
                    move |(_, branch)| {
                        label(move || branch.clone())
                            .style(move |s| {
                                let config = config.get();
                                s.font_size(9.0)
                                    .padding_horiz(4.0)
                                    .padding_vert(1.0)
                                    .margin_left(4.0)
                                    .border_radius(3.0)
                                    .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                    .color(config.color(LapceColor::TERMINAL_GREEN))
                            })
                    },
                )
                .style(|s| s.items_center()),
                // Tag labels
                dyn_stack(
                    move || tags.clone().into_iter().enumerate().collect::<Vec<_>>(),
                    |(i, _)| *i,
                    move |(_, tag)| {
                        label(move || tag.clone())
                            .style(move |s| {
                                let config = config.get();
                                s.font_size(9.0)
                                    .padding_horiz(4.0)
                                    .padding_vert(1.0)
                                    .margin_left(4.0)
                                    .border_radius(3.0)
                                    .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                    .color(config.color(LapceColor::TERMINAL_YELLOW))
                            })
                    },
                )
                .style(|s| s.items_center()),
            ))
            .style(|s| s.items_center().flex_grow(1.0)),
            
            // Second line: hash + author + date
            stack((
                label(move || short_id.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(10.0)
                            .font_family("monospace".to_string())
                            .color(config.color(LapceColor::TERMINAL_CYAN))
                    }),
                label(move || format!(" · {}", author_name.clone()))
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(10.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                    }),
                label(move || format!(" · {}", format_timestamp(timestamp)))
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(10.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                    }),
            ))
            .style(|s| s.items_center()),
        ))
        .style(|s| s.flex_col().flex_grow(1.0).padding_vert(2.0)),
    ))
    .on_click_stop(move |_| {
        let current = selected_commit.get();
        if current.as_ref() == Some(&commit_id_for_click) {
            selected_commit.set(None);
        } else {
            selected_commit.set(Some(commit_id_for_click.clone()));
        }
    })
    .style(move |s| {
        let config = config.get();
        let is_selected = selected_commit.get().as_ref() == Some(&commit_id);
        s.items_center()
            .padding_horiz(8.0)
            .padding_vert(4.0)
            .width_pct(100.0)
            .cursor(CursorStyle::Pointer)
            .apply_if(is_selected, |s| s.background(config.color(LapceColor::PANEL_CURRENT_BACKGROUND)))
            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
    })
}

/// Commit details panel shown when a commit is selected
fn commit_details_view(
    commit: GitCommitInfo,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    selected_commit: floem::reactive::RwSignal<Option<String>>,
) -> impl View {
    let full_message = commit.message.clone();
    let author_name = commit.author_name.clone();
    let author_email = commit.author_email.clone();
    let short_id = commit.short_id.clone();
    let full_id = commit.id.clone();
    let timestamp = commit.timestamp;
    let parents = commit.parents.clone();
    
    scroll(
        stack((
            // Header with close button
            stack((
                label(|| "Commit Details".to_string())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(11.0)
                            .font_bold()
                            .flex_grow(1.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    }),
                svg(move || config.get().ui_svg(LapceIcons::CLOSE))
                    .on_click_stop(move |_| selected_commit.set(None))
                    .style(move |s| {
                        let config = config.get();
                        s.size(14.0, 14.0)
                            .cursor(CursorStyle::Pointer)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
            ))
            .style(move |s| {
                let config = config.get();
                s.items_center()
                    .width_pct(100.0)
                    .padding(8.0)
                    .border_bottom(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
            }),
            
            // Commit info
            stack((
                // Hash
                stack((
                    label(|| "Commit: ".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                        }),
                    label(move || full_id.clone())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .font_family("monospace".to_string())
                                .color(config.color(LapceColor::TERMINAL_CYAN))
                        }),
                ))
                .style(|s| s.items_center()),
                
                // Author
                stack((
                    label(|| "Author: ".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                        }),
                    label(move || format!("{} <{}>", author_name.clone(), author_email.clone()))
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .style(|s| s.items_center()),
                
                // Date
                stack((
                    label(|| "Date: ".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                        }),
                    label(move || format_timestamp_full(timestamp))
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .style(|s| s.items_center()),
                
                // Parents
                stack((
                    label(|| "Parents: ".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                        }),
                    label(move || {
                        parents.iter()
                            .map(|p| p.chars().take(7).collect::<String>())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(11.0)
                            .font_family("monospace".to_string())
                            .color(config.color(LapceColor::TERMINAL_CYAN))
                    }),
                ))
                .style(|s| s.items_center()),
                
                // Full message
                container(
                    label(move || full_message.clone())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(11.0)
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                )
                .style(move |s| {
                    let config = config.get();
                    s.margin_top(8.0)
                        .padding(8.0)
                        .width_pct(100.0)
                        .border_radius(4.0)
                        .background(config.color(LapceColor::EDITOR_BACKGROUND))
                }),
            ))
            .style(|s| s.flex_col().gap(4.0).padding(8.0).width_pct(100.0)),
        ))
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(|s| s.size_pct(100.0, 100.0))
}

/// Format timestamp as relative time (e.g., "2 hours ago")
fn format_timestamp(timestamp: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    
    let diff = now - timestamp;
    
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else if diff < 604800 {
        format!("{} days ago", diff / 86400)
    } else if diff < 2592000 {
        format!("{} weeks ago", diff / 604800)
    } else if diff < 31536000 {
        format!("{} months ago", diff / 2592000)
    } else {
        format!("{} years ago", diff / 31536000)
    }
}

/// Format timestamp as full date/time
fn format_timestamp_full(timestamp: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    
    let datetime = UNIX_EPOCH + Duration::from_secs(timestamp as u64);
    // Simple formatting - in a real app you'd use chrono
    format!("{:?}", datetime)
}
