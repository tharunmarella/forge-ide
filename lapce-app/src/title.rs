use std::{rc::Rc, sync::Arc};

use floem::{
    View, ViewId,
    action::{add_overlay, remove_overlay},
    event::EventListener,
    menu::{Menu, MenuItem},
    peniko::kurbo::{Point, Size},
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo, create_rw_signal,
    },
    style::{AlignItems, CursorStyle, JustifyContent, Position},
    views::{Decorators, clip, container, drag_window_area, dyn_stack, empty, label, scroll, stack, svg},
};
use lapce_core::meta;
#[allow(unused_imports)]
use lapce_rpc::proxy::ProxyStatus;
use tracing::debug;

use crate::{
    alert::AlertButton,
    app::{clickable_icon, not_clickable_icon, window_menu},
    command::{CommandKind, LapceCommand, LapceWorkbenchCommand, WindowCommand},
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    listener::Listener,
    main_split::MainSplitData,
    source_control::SourceControlData,
    update::ReleaseInfo,
    window_tab::WindowTabData,
    workspace::LapceWorkspace,
};

/// Overlay content for project dropdown — rendered at window level so clicks work.
fn project_dropdown_overlay(
    config: ReadSignal<Arc<LapceConfig>>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    window_command: Listener<WindowCommand>,
    recent_workspaces: RwSignal<Vec<LapceWorkspace>>,
    project_dropdown_visible: RwSignal<bool>,
) -> impl View {
    container(
        stack((
            container(
                label(|| "Open Folder...".to_string())
                    .style(move |s| {
                        let config = config.get();
                        s.padding_horiz(10.0)
                            .padding_vert(6.0)
                            .width_full()
                            .font_size(12.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    }),
            )
            .on_click_stop(move |_| {
                debug!("project dropdown: open folder");
                project_dropdown_visible.set(false);
                workbench_command.send(LapceWorkbenchCommand::OpenFolder);
            })
            .style(move |s| {
                let config = config.get();
                s.width_full().hover(|s| {
                    s.cursor(CursorStyle::Pointer)
                        .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
            }),
            empty().style(move |s| {
                let config = config.get();
                s.width_full()
                    .height(1.0)
                    .margin_vert(4.0)
                    .background(config.color(LapceColor::LAPCE_BORDER))
            }),
            label(|| "Recent Projects".to_string()).style(move |s| {
                let config = config.get();
                s.padding_horiz(10.0)
                    .padding_vert(4.0)
                    .font_size(10.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
            }),
            clip(
                scroll(
                    dyn_stack(
                        move || recent_workspaces.get().into_iter().take(15).enumerate().collect::<Vec<_>>(),
                        move |(i, _)| *i,
                        move |(_, ws)| {
                            let path = ws.path.clone();
                            let display_name = path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Unknown".to_string());
                            let path_str = path
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let workspace_clone = ws.clone();
                            container(
                                stack((
                                    svg(move || config.get().ui_svg(LapceIcons::FILE_EXPLORER))
                                        .style(move |s| {
                                            let config = config.get();
                                            s.size(12.0, 12.0)
                                                .margin_right(8.0)
                                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                                        }),
                                    stack((
                                        label(move || display_name.clone())
                                            .style(move |s| {
                                                let config = config.get();
                                                s.font_size(12.0)
                                                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                                            }),
                                        label(move || path_str.clone()).style(move |s| {
                                            let config = config.get();
                                            s.font_size(10.0)
                                                .margin_top(1.0)
                                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                                        }),
                                    ))
                                    .style(|s| s.flex_col().min_width(0.0)),
                                ))
                                .style(|s| s.items_center()),
                            )
                            .on_click_stop(move |_| {
                                debug!("project dropdown: switch workspace");
                                project_dropdown_visible.set(false);
                                window_command.send(WindowCommand::SetWorkspace {
                                    workspace: workspace_clone.clone(),
                                });
                            })
                            .style(move |s| {
                                let config = config.get();
                                s.width_full()
                                    .padding_horiz(10.0)
                                    .padding_vert(6.0)
                                    .hover(|s| {
                                        s.cursor(CursorStyle::Pointer).background(
                                            config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                        )
                                    })
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                )
                .style(|s| s.width_full().max_height(400.0)),
            )
            .style(|s| s.width_full()),
        ))
        .style(|s| s.flex_col().width_full()),
    )
    .keyboard_navigable()
    .request_focus(|| {})
    .on_event_stop(EventListener::FocusLost, move |_| {
        debug!("project dropdown: focus lost -> close");
        project_dropdown_visible.set(false);
    })
    .style(move |s| {
        let config = config.get();
        s.min_width(240.0)
            .max_width(350.0)
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .box_shadow_blur(10.0)
            .box_shadow_color(config.color(LapceColor::LAPCE_DROPDOWN_SHADOW))
    })
}

/// Overlay content for branch dropdown — rendered at window level so clicks work.
fn branch_dropdown_overlay(
    config: ReadSignal<Arc<LapceConfig>>,
    lapce_command: Listener<LapceCommand>,
    source_control: SourceControlData,
    branch_dropdown_visible: RwSignal<bool>,
    window_tab_data: Rc<WindowTabData>,
) -> impl View {
    let branches = source_control.branches;
    let current_branch = source_control.branch;
    
    // Track which branch is selected (clicked) to show the sub-menu
    let selected_branch: RwSignal<Option<String>> = create_rw_signal(None);
    let selected_branch_index: RwSignal<Option<usize>> = create_rw_signal(None);
    let submenu_visible = create_memo(move |_| selected_branch.get().is_some());

    let window_tab_data_for_alert = window_tab_data.clone();
    let show_info: Rc<dyn Fn(&str, &str)> = Rc::new(move |title, msg| {
        window_tab_data_for_alert.show_alert(
            title.to_string(),
            msg.to_string(),
            Vec::<AlertButton>::new(),
        );
    });

    // Menu item helper - full width clickable row
    let menu_item = move |icon: &'static str, text: String, on_click: Rc<dyn Fn()>| {
        container(
            stack((
                svg(move || config.get().ui_svg(icon))
                    .style(move |s| {
                        let config = config.get();
                        s.size(12.0, 12.0)
                            .margin_right(8.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                    }),
                label(move || text.clone())
                    .style(move |s| {
                        let config = config.get();
                        s.flex_grow(1.0)
                            .font_size(12.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    }),
            ))
            .style(|s| s.items_center().width_full()),
        )
        .on_click_stop(move |_| {
            on_click();
        })
        .style(move |s| {
            let config = config.get();
            s.padding_horiz(10.0)
                .padding_vert(6.0)
                .width_full()
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer)
                        .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        })
    };

    // Separator helper
    let separator = move || {
        empty().style(move |s| {
            let config = config.get();
            s.width_full()
                .height(1.0)
                .margin_vert(4.0)
                .background(config.color(LapceColor::LAPCE_BORDER))
        })
    };

    // Update Project action (git pull)
    let update_project_action = {
        let workbench_command = window_tab_data.common.workbench_command;
        Rc::new(move || {
            selected_branch.set(None);
            selected_branch_index.set(None);
            branch_dropdown_visible.set(false);
            workbench_command.send(LapceWorkbenchCommand::GitPull);
        })
    };
    
    // Commit action
    let commit_action = {
        let workbench_command = window_tab_data.common.workbench_command;
        Rc::new(move || {
            selected_branch.set(None);
            selected_branch_index.set(None);
            branch_dropdown_visible.set(false);
            workbench_command.send(LapceWorkbenchCommand::SourceControlCommit);
        })
    };
    
    // Push action
    let push_action = {
        let workbench_command = window_tab_data.common.workbench_command;
        Rc::new(move || {
            selected_branch.set(None);
            selected_branch_index.set(None);
            branch_dropdown_visible.set(false);
            workbench_command.send(LapceWorkbenchCommand::GitPush);
        })
    };

    // Main menu column - simplified to 3 main actions
    let main_menu = container(
        stack((
            // Update Project (git pull)
            menu_item(LapceIcons::SCM, "Update Project".to_string(), update_project_action),
            // Commit
            menu_item(LapceIcons::SCM, "Commit".to_string(), commit_action),
            // Push
            menu_item(LapceIcons::SCM, "Push".to_string(), push_action),
            // Separator
            separator(),
            // Branches list
            clip(
                scroll(
                    dyn_stack(
                        move || branches.get().into_iter().take(50).enumerate().collect::<Vec<_>>(),
                        move |(i, _)| *i,
                        move |(idx, branch_name)| {
                            let is_current = branch_name == current_branch.get_untracked();
                            let branch_for_click = branch_name.clone();
                            let branch_display = branch_name.clone();
                            let branch_for_style = branch_name.clone();
                            container(
                                stack((
                                    svg(move || config.get().ui_svg(LapceIcons::SCM))
                                        .style(move |s| {
                                            let config = config.get();
                                            s.size(12.0, 12.0)
                                                .margin_right(8.0)
                                                .color(if is_current {
                                                    config.color(LapceColor::EDITOR_CARET)
                                                } else {
                                                    config.color(LapceColor::LAPCE_ICON_ACTIVE)
                                                })
                                        }),
                                    label(move || branch_display.clone())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.flex_grow(1.0)
                                                .font_size(12.0)
                                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                                        }),
                                    // Checkmark for current branch
                                    label(move || if is_current { "✓" } else { "" }.to_string())
                                        .style(move |s| {
                                            let config = config.get();
                                            s.margin_right(6.0)
                                                .font_size(11.0)
                                                .color(config.color(LapceColor::EDITOR_CARET))
                                        }),
                                    // Chevron arrow
                                    svg(move || config.get().ui_svg(LapceIcons::ITEM_OPENED))
                                        .style(move |s| {
                                            let config = config.get();
                                            s.size(10.0, 10.0)
                                                .color(config.color(LapceColor::LAPCE_ICON_INACTIVE))
                                        }),
                                ))
                                .style(|s| s.items_center().width_full()),
                            )
                            .on_click_stop(move |_| {
                                // Toggle: if already selected, deselect; otherwise select
                                if selected_branch.get_untracked() == Some(branch_for_click.clone()) {
                                    selected_branch.set(None);
                                    selected_branch_index.set(None);
                                } else {
                                    selected_branch.set(Some(branch_for_click.clone()));
                                    selected_branch_index.set(Some(idx));
                                }
                            })
                            .style(move |s| {
                                let config = config.get();
                                let is_selected = selected_branch.get() == Some(branch_for_style.clone());
                                s.padding_horiz(10.0)
                                    .padding_vert(6.0)
                                    .width_full()
                                    .apply_if(is_selected, |s| {
                                        s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                    })
                                    .hover(|s| {
                                        s.cursor(CursorStyle::Pointer)
                                            .background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                    })
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                )
                .style(|s| s.width_full().max_height(300.0)),
            ),
        ))
        .style(|s| s.flex_col().width_full().padding_vert(4.0)),
    )
    .style(move |s| {
        s.width(180.0)
    });

    // Sub-menu for branch actions (appears on the right when a branch is clicked)
    let submenu = {
        let show_info = show_info.clone();
        container(
            stack((
                // Branch name header
                container(
                    label(move || {
                        selected_branch.get().unwrap_or_else(|| "".to_string())
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.font_bold()
                            .font_size(12.0)
                            .padding_horiz(10.0)
                            .padding_vert(6.0)
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    })
                ),
                separator(),
                // Checkout
                menu_item(
                    LapceIcons::SCM,
                    "Checkout".to_string(),
                    Rc::new({
                        let lapce_command = lapce_command.clone();
                        let selected_branch = selected_branch;
                        let branch_dropdown_visible = branch_dropdown_visible;
                        move || {
                            if let Some(branch) = selected_branch.get_untracked() {
                                branch_dropdown_visible.set(false);
                                lapce_command.send(LapceCommand {
                                    kind: CommandKind::Workbench(
                                        LapceWorkbenchCommand::CheckoutReference,
                                    ),
                                    data: Some(serde_json::json!(branch)),
                                });
                            }
                        }
                    }),
                ),
                // New Branch from
                menu_item(
                    LapceIcons::SCM,
                    "New Branch from...".to_string(),
                    Rc::new({
                        let show_info = show_info.clone();
                        move || {
                            show_info("Not implemented", "New Branch from... is not implemented yet.");
                        }
                    }),
                ),
                // Compare with
                menu_item(
                    LapceIcons::SCM,
                    "Compare with...".to_string(),
                    Rc::new({
                        let show_info = show_info.clone();
                        move || {
                            show_info("Not implemented", "Compare is not implemented yet.");
                        }
                    }),
                ),
                // Show Diff
                menu_item(
                    LapceIcons::SCM,
                    "Show Diff".to_string(),
                    Rc::new({
                        let show_info = show_info.clone();
                        move || {
                            show_info("Not implemented", "Show Diff is not implemented yet.");
                        }
                    }),
                ),
                // Merge into current
                menu_item(
                    LapceIcons::SCM,
                    "Merge into current".to_string(),
                    Rc::new({
                        let show_info = show_info.clone();
                        move || {
                            show_info("Not implemented", "Merge is not implemented yet.");
                        }
                    }),
                ),
                // Delete branch
                menu_item(
                    LapceIcons::CLOSE,
                    "Delete".to_string(),
                    Rc::new({
                        let show_info = show_info.clone();
                        move || {
                            show_info("Not implemented", "Delete branch is not implemented yet.");
                        }
                    }),
                ),
            ))
            .style(|s| s.flex_col().width_full().padding_vert(4.0)),
        )
        .on_click_stop(move |_| {
            // Clicking inside submenu shouldn't close it
        })
        .style(move |s| {
            let config = config.get();
            let visible = submenu_visible.get();
            // Calculate top offset based on selected branch index
            // Each item is approximately 24px (6px padding top + 12px font + 6px padding bottom)
            // Plus we need to account for the top menu items (Commit, Push, Update, separators, New Branch)
            // Top section is roughly: 3 items * 24px + 2 separators * 9px + 1 item * 24px = 114px
            let idx = selected_branch_index.get().unwrap_or(0);
            let item_height = 24.0_f32;
            let top_offset = 114.0_f32; // Offset for top menu items
            let inset_top = top_offset + (idx as f32 * item_height);
            
            s.width(160.0)
                .position(Position::Absolute)
                .inset_left(180.0) // Position to the right of main menu (180px width)
                .inset_top(inset_top)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .box_shadow_blur(10.0)
                .box_shadow_color(config.color(LapceColor::LAPCE_DROPDOWN_SHADOW))
                .apply_if(!visible, |s| s.hide())
        })
    };

    // Container with main menu + absolutely positioned submenu
    stack((
        main_menu,
        submenu,
    ))
    .style(|s| s.position(Position::Relative))
    .keyboard_navigable()
    .request_focus(|| {})
    .on_event_stop(EventListener::FocusLost, move |_| {
        debug!("branch dropdown: focus lost -> close");
        selected_branch.set(None);
        selected_branch_index.set(None);
        branch_dropdown_visible.set(false);
    })
    .style(move |s| {
        let config = config.get();
        s.background(config.color(LapceColor::PANEL_BACKGROUND))
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .box_shadow_blur(12.0)
            .box_shadow_color(config.color(LapceColor::LAPCE_DROPDOWN_SHADOW))
    })
}

fn left(
    workspace: Arc<LapceWorkspace>,
    lapce_command: Listener<LapceCommand>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    _window_command: Listener<WindowCommand>,
    source_control: SourceControlData,
    config: ReadSignal<Arc<LapceConfig>>,
    _proxy_status: RwSignal<Option<ProxyStatus>>,
    num_window_tabs: Memo<usize>,
    project_dropdown_visible: RwSignal<bool>,
    branch_dropdown_visible: RwSignal<bool>,
    _recent_workspaces: RwSignal<Vec<LapceWorkspace>>,
    project_button_origin: RwSignal<Point>,
    project_button_size: RwSignal<Size>,
    branch_button_origin: RwSignal<Point>,
    branch_button_size: RwSignal<Size>,
) -> impl View {
    let is_macos = cfg!(target_os = "macos");
    let local_workspace = workspace.clone();
    let branch = source_control.branch;

    stack((
        empty().style(move |s| {
            let should_hide = if is_macos {
                num_window_tabs.get() > 1
            } else {
                true
            };
            s.width(75.0).apply_if(should_hide, |s| s.hide())
        }),
        container(svg(move || config.get().ui_svg(LapceIcons::LOGO)).style(
            move |s| {
                let config = config.get();
                s.size(16.0, 16.0)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            },
        ))
        .style(move |s| s.margin_horiz(10.0).apply_if(is_macos, |s| s.hide())),
        not_clickable_icon(
            || LapceIcons::MENU,
            || false,
            || false,
            || "Menu",
            config,
        )
        .popout_menu(move || window_menu(lapce_command, workbench_command))
        .style(move |s| {
            s.margin_left(4.0)
                .margin_right(6.0)
                .apply_if(is_macos, |s| s.hide())
        }),
        // Project button wrapper — capture position/size for overlay
        stack((
            svg(move || config.get().ui_svg(LapceIcons::FILE_EXPLORER)).style(
                move |s| {
                    let config = config.get();
                    let icon_size = config.ui.icon_size() as f32;
                    s.size(icon_size, icon_size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                },
            ),
            label(move || {
                if let Some(name) = local_workspace.display() {
                    name
                } else {
                    "Open Project".to_string()
                }
            })
            .style(move |s| {
                let config = config.get();
                s.margin_left(6.0)
                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                    .selectable(false)
            }),
            svg(move || config.get().ui_svg(LapceIcons::DROPDOWN_ARROW)).style(
                move |s| {
                    let config = config.get();
                    s.size(10.0, 10.0)
                        .margin_left(4.0)
                        .color(config.color(LapceColor::LAPCE_ICON_INACTIVE))
                },
            ),
        ))
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .height_pct(100.0)
                .padding_horiz(10.0)
                .border_radius(4.0)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        })
        .on_click_stop(move |_| {
            branch_dropdown_visible.set(false);
            project_dropdown_visible.update(|v| {
                *v = !*v;
                debug!("project dropdown: toggle -> {}", *v);
            });
        })
        .on_move(move |point| {
            project_button_origin.set(point);
        })
        .on_resize(move |rect| {
            project_button_size.set(rect.size());
        }),
        // Branch button wrapper — capture position/size for overlay
        {
            let git_loading = source_control.git_operation_loading;
            stack((
                svg(move || config.get().ui_svg(LapceIcons::SCM)).style(move |s| {
                    let config = config.get();
                    let icon_size = config.ui.icon_size() as f32;
                    s.size(icon_size, icon_size)
                        .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                }),
                label(move || {
                    let b = branch.get();
                    if b.is_empty() {
                        "main".to_string()
                    } else {
                        b
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    s.margin_left(6.0)
                        .color(config.color(LapceColor::PANEL_FOREGROUND))
                        .selectable(false)
                }),
                // Loading spinner - shown when git operation is in progress
                svg(move || config.get().ui_svg(LapceIcons::DEBUG_RESTART)).style(
                    move |s| {
                        let config = config.get();
                        s.size(12.0, 12.0)
                            .margin_left(6.0)
                            .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                            .apply_if(!git_loading.get(), |s| s.hide())
                    },
                ),
                svg(move || config.get().ui_svg(LapceIcons::DROPDOWN_ARROW)).style(
                    move |s| {
                        let config = config.get();
                        s.size(10.0, 10.0)
                            .margin_left(4.0)
                            .color(config.color(LapceColor::LAPCE_ICON_INACTIVE))
                            .apply_if(git_loading.get(), |s| s.hide())
                    },
                ),
            ))
        }
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .height_pct(100.0)
                .padding_horiz(10.0)
                .border_radius(4.0)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        })
        .on_click_stop(move |_| {
            project_dropdown_visible.set(false);
            branch_dropdown_visible.update(|v| {
                *v = !*v;
                debug!("branch dropdown: toggle -> {}", *v);
            });
        })
        .on_move(move |point| {
            branch_button_origin.set(point);
        })
        .on_resize(move |rect| {
            branch_button_size.set(rect.size());
        })
        .style(move |s| {
            s.height_pct(100.0)
                .margin_left(4.0)
                .apply_if(workspace.path.is_none(), |s| s.hide())
        }),

        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
    ))
    .style(move |s| {
        s.height_pct(100.0)
            .flex_basis(0.0)
            .flex_grow(1.0)
            .items_center()
    })
    .debug_name("Left Side of Top Bar")
}

fn middle(
    _workspace: Arc<LapceWorkspace>,
    main_split: MainSplitData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let can_jump_backward = {
        let main_split = main_split.clone();
        create_memo(move |_| main_split.can_jump_location_backward(true))
    };
    let can_jump_forward =
        create_memo(move |_| main_split.can_jump_location_forward(true));

    let jump_backward = move || {
        clickable_icon(
            || LapceIcons::LOCATION_BACKWARD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::JumpLocationBackward);
            },
            || false,
            move || !can_jump_backward.get(),
            || "Jump Backward",
            config,
        )
        .style(move |s| s.margin_horiz(6.0))
    };
    let jump_forward = move || {
        clickable_icon(
            || LapceIcons::LOCATION_FORWARD,
            move || {
                workbench_command.send(LapceWorkbenchCommand::JumpLocationForward);
            },
            || false,
            move || !can_jump_forward.get(),
            || "Jump Forward",
            config,
        )
        .style(move |s| s.margin_right(6.0))
    };

    // Simplified middle section - just navigation buttons and drag area (IntelliJ-style)
    stack((
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
        jump_backward(),
        jump_forward(),
        clickable_icon(
            || LapceIcons::START,
            move || {
                workbench_command.send(LapceWorkbenchCommand::PaletteRunAndDebug)
            },
            || false,
            || false,
            || "Run and Debug",
            config,
        )
        .style(move |s| s.margin_horiz(6.0)),
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
    ))
    .style(|s| {
        s.flex_basis(0)
            .flex_grow(2.0)
            .height_pct(100.0)
            .align_items(Some(AlignItems::Center))
            .justify_content(Some(JustifyContent::Center))
    })
    .debug_name("Middle of Top Bar")
}

fn right(
    window_command: Listener<WindowCommand>,
    workbench_command: Listener<LapceWorkbenchCommand>,
    latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    update_in_progress: RwSignal<bool>,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    let latest_version = create_memo(move |_| {
        let latest_release = latest_release.get();
        let latest_version =
            latest_release.as_ref().as_ref().map(|r| r.version.clone());
        if latest_version.is_some()
            && latest_version.as_deref() != Some(meta::VERSION)
        {
            latest_version
        } else {
            None
        }
    });

    let has_update = move || latest_version.with(|v| v.is_some());

    stack((
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0)),
        stack((
            not_clickable_icon(
                || LapceIcons::SETTINGS,
                || false,
                || false,
                || "Settings",
                config,
            )
            .popout_menu(move || {
                Menu::new("")
                    .entry(MenuItem::new("Command Palette").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::PaletteCommand)
                    }))
                    .separator()
                    .entry(MenuItem::new("Open Settings").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::OpenSettings)
                    }))
                    .entry(MenuItem::new("Open Keyboard Shortcuts").action(
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenKeyboardShortcuts)
                        },
                    ))
                    .entry(MenuItem::new("Open Theme Color Settings").action(
                        move || {
                            workbench_command
                                .send(LapceWorkbenchCommand::OpenThemeColorSettings)
                        },
                    ))
                    .separator()
                    .entry(if let Some(v) = latest_version.get_untracked() {
                        if update_in_progress.get_untracked() {
                            MenuItem::new(format!("Update in progress ({v})"))
                                .enabled(false)
                        } else {
                            MenuItem::new(format!("Restart to update ({v})")).action(
                                move || {
                                    workbench_command
                                        .send(LapceWorkbenchCommand::RestartToUpdate)
                                },
                            )
                        }
                    } else {
                        MenuItem::new("No update available").enabled(false)
                    })
                    .separator()
                    .entry(MenuItem::new("About Lapce").action(move || {
                        workbench_command.send(LapceWorkbenchCommand::ShowAbout)
                    }))
            }),
            container(label(|| "1".to_string()).style(move |s| {
                let config = config.get();
                s.font_size(10.0)
                    .color(config.color(LapceColor::EDITOR_BACKGROUND))
                    .border_radius(100.0)
                    .margin_left(5.0)
                    .margin_top(10.0)
                    .background(config.color(LapceColor::EDITOR_CARET))
            }))
            .style(move |s| {
                let has_update = has_update();
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .justify_end()
                    .items_end()
                    .pointer_events_none()
                    .apply_if(!has_update, |s| s.hide())
            }),
        ))
        .style(move |s| s.margin_horiz(6.0)),
        window_controls_view(
            window_command,
            true,
            num_window_tabs,
            window_maximized,
            config,
        ),
    ))
    .style(|s| {
        s.flex_basis(0)
            .flex_grow(1.0)
            .justify_content(Some(JustifyContent::FlexEnd))
    })
    .debug_name("Right of top bar")
}

pub fn title(window_tab_data: Rc<WindowTabData>) -> impl View {
    let workspace = window_tab_data.workspace.clone();
    let lapce_command = window_tab_data.common.lapce_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_command = window_tab_data.common.window_common.window_command;
    let latest_release = window_tab_data.common.window_common.latest_release;
    let proxy_status = window_tab_data.common.proxy_status;
    let num_window_tabs = window_tab_data.common.window_common.num_window_tabs;
    let window_maximized = window_tab_data.common.window_common.window_maximized;
    let title_height = window_tab_data.title_height;
    let update_in_progress = window_tab_data.update_in_progress;
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    
    // Dropdown state signals
    let project_dropdown_visible = window_tab_data.common.scope.create_rw_signal(false);
    let branch_dropdown_visible = window_tab_data.common.scope.create_rw_signal(false);
    let recent_workspaces: RwSignal<Vec<LapceWorkspace>> = window_tab_data.common.scope.create_rw_signal(Vec::new());
    let project_overlay_id: RwSignal<Option<ViewId>> = window_tab_data.common.scope.create_rw_signal(None);
    let branch_overlay_id: RwSignal<Option<ViewId>> = window_tab_data.common.scope.create_rw_signal(None);
    let project_button_origin: RwSignal<Point> = window_tab_data.common.scope.create_rw_signal(Point::ZERO);
    let project_button_size: RwSignal<Size> = window_tab_data.common.scope.create_rw_signal(Size::ZERO);
    let branch_button_origin: RwSignal<Point> = window_tab_data.common.scope.create_rw_signal(Point::ZERO);
    let branch_button_size: RwSignal<Size> = window_tab_data.common.scope.create_rw_signal(Size::ZERO);

    // Load recent workspaces when project dropdown opens
    create_effect(move |_| {
        if project_dropdown_visible.get() {
            let db: Arc<crate::db::LapceDb> = floem::reactive::use_context().unwrap();
            let workspaces = db.recent_workspaces().unwrap_or_default();
            recent_workspaces.set(workspaces);
        }
    });

    // Project dropdown: add/remove overlay so it receives hit tests at window level
    create_effect(move |_| {
        if project_dropdown_visible.get() {
            let origin = project_button_origin.get();
            let size = project_button_size.get();
            let point = Point::new(origin.x, origin.y + size.height);
            debug!(
                "project dropdown: open overlay at ({:.1}, {:.1})",
                point.x, point.y
            );
            let id = add_overlay(point, move |_| {
                project_dropdown_overlay(
                    config,
                    workbench_command,
                    window_command,
                    recent_workspaces,
                    project_dropdown_visible,
                )
            });
            project_overlay_id.set(Some(id));
        } else {
            if let Some(id) = project_overlay_id.get_untracked() {
                debug!("project dropdown: remove overlay");
                remove_overlay(id);
                project_overlay_id.set(None);
            }
        }
    });

    // Branch dropdown: add/remove overlay
    {
        let source_control_for_branch = source_control.clone();
        let window_tab_data_for_branch = window_tab_data.clone();
        create_effect(move |_| {
            if branch_dropdown_visible.get() {
                let origin = branch_button_origin.get();
                let size = branch_button_size.get();
                let point = Point::new(origin.x, origin.y + size.height);
                let sc = source_control_for_branch.clone();
                debug!(
                    "branch dropdown: open overlay at ({:.1}, {:.1})",
                    point.x, point.y
                );
                let wtd = window_tab_data_for_branch.clone();
                let id = add_overlay(point, move |_| {
                    branch_dropdown_overlay(
                        config,
                        lapce_command,
                        sc.clone(),
                        branch_dropdown_visible,
                        wtd.clone(),
                    )
                });
                branch_overlay_id.set(Some(id));
            } else {
                if let Some(id) = branch_overlay_id.get_untracked() {
                    debug!("branch dropdown: remove overlay");
                    remove_overlay(id);
                    branch_overlay_id.set(None);
                }
            }
        });
    }

    stack((
        stack((
            left(
                workspace.clone(),
                lapce_command,
                workbench_command,
                window_command,
                source_control.clone(),
                config,
                proxy_status,
                num_window_tabs,
                project_dropdown_visible,
                branch_dropdown_visible,
                recent_workspaces,
                project_button_origin,
                project_button_size,
                branch_button_origin,
                branch_button_size,
            ),
            middle(
                workspace,
                window_tab_data.main_split.clone(),
                workbench_command,
                config,
            ),
            right(
                window_command,
                workbench_command,
                latest_release,
                update_in_progress,
                num_window_tabs,
                window_maximized,
                config,
            ),
        ))
        .on_click(move |_| {
            if project_dropdown_visible.get_untracked() || branch_dropdown_visible.get_untracked() {
                project_dropdown_visible.set(false);
                branch_dropdown_visible.set(false);
            }
            floem::event::EventPropagation::Continue
        })
        .style(|s| s.width_full().height_full().items_center()),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != title_height.get_untracked() {
            title_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .height(37.0)
            .items_center()
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .border_bottom(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    })
    .debug_name("Title / Top Bar")
}

pub fn window_controls_view(
    window_command: Listener<WindowCommand>,
    is_title: bool,
    num_window_tabs: Memo<usize>,
    window_maximized: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack((
        clickable_icon(
            || LapceIcons::WINDOW_MINIMIZE,
            || {
                floem::action::minimize_window();
            },
            || false,
            || false,
            || "Minimize",
            config,
        )
        .style(|s| s.margin_right(16.0).margin_left(10.0)),
        clickable_icon(
            move || {
                if window_maximized.get() {
                    LapceIcons::WINDOW_RESTORE
                } else {
                    LapceIcons::WINDOW_MAXIMIZE
                }
            },
            move || {
                floem::action::set_window_maximized(
                    !window_maximized.get_untracked(),
                );
            },
            || false,
            || false,
            || "Maximize",
            config,
        )
        .style(|s| s.margin_right(16.0)),
        clickable_icon(
            || LapceIcons::WINDOW_CLOSE,
            move || {
                window_command.send(WindowCommand::CloseWindow);
            },
            || false,
            || false,
            || "Close Window",
            config,
        )
        .style(|s| s.margin_right(6.0)),
    ))
    .style(move |s| {
        s.apply_if(
            cfg!(target_os = "macos")
                || !config.get_untracked().core.custom_titlebar
                || (is_title && num_window_tabs.get() > 1),
            |s| s.hide(),
        )
    })
}
