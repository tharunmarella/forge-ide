//! Run configuration dropdown for the title bar

use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    event::EventListener,
    peniko::kurbo::{Point, Size},
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate, create_effect},
    style::{CursorStyle, Display},
    views::{Decorators, container, dyn_stack, empty, label, scroll, stack, svg},
    action::{add_overlay, remove_overlay},
    ViewId,
};

use crate::{
    app::clickable_icon,
    command::{InternalCommand, LapceWorkbenchCommand},
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    debug::RunDebugMode,
    listener::Listener,
    window_tab::WindowTabData,
};

use super::{RunConfigData, ConfigSource, fetch_run_configs};

/// Create the run configuration dropdown overlay content
fn run_config_dropdown_overlay(
    config: ReadSignal<Arc<LapceConfig>>,
    run_config_data: RunConfigData,
    workbench_command: Listener<LapceWorkbenchCommand>,
    on_run: Rc<dyn Fn(super::RunConfigItem, RunDebugMode)>,
) -> impl View {
    let selected = run_config_data.selected;
    let dropdown_visible = run_config_data.dropdown_visible;
    let loading = run_config_data.loading;
    let error = run_config_data.error;
    
    container(
        stack((
            // Loading state
            container(
                label(move || {
                    if loading.get() {
                        "Loading configurations...".to_string()
                    } else {
                        String::new()
                    }
                })
                .style(move |s| {
                    let cfg = config.get();
                    s.font_size(cfg.ui.font_size() as f32)
                        .color(cfg.color(LapceColor::PANEL_FOREGROUND_DIM))
                }),
            )
            .style(move |s| {
                s.padding_horiz(12.0)
                    .padding_vert(8.0)
                    .width_full()
                    .apply_if(!loading.get(), |s| s.hide())
            }),
            // Error state
            container(
                label(move || error.get().unwrap_or_default())
                    .style(move |s| {
                        let cfg = config.get();
                        s.font_size(cfg.ui.font_size() as f32)
                            .color(cfg.color(LapceColor::LAPCE_ERROR))
                    }),
            )
            .style(move |s| {
                s.padding_horiz(12.0)
                    .padding_vert(8.0)
                    .width_full()
                    .apply_if(error.get().is_none(), |s| s.hide())
            }),
            // Config list
            scroll(
                dyn_stack(
                    move || run_config_data.all_configs(),
                    |item| item.name.clone(),
                    move |item| {
                        let name = item.name.clone();
                        let name_for_check1 = name.clone();
                        let name_for_check2 = name.clone();
                        let item_clone = item.clone();
                        let on_run = on_run.clone();
                        
                        container(
                            stack((
                                // Selection indicator (use START icon as checkmark)
                                svg(move || config.get().ui_svg(LapceIcons::START))
                                    .style(move |s| {
                                        let cfg = config.get();
                                        let is_sel = selected.get() == Some(name_for_check1.clone());
                                        s.size(12.0, 12.0)
                                            .margin_right(8.0)
                                            .color(cfg.color(LapceColor::LAPCE_ICON_ACTIVE))
                                            .display(if is_sel { Display::Flex } else { Display::None })
                                    }),
                                // Config name
                                label(move || item.name.clone())
                                    .style(move |s| {
                                        let cfg = config.get();
                                        s.flex_grow(1.0)
                                            .font_size(cfg.ui.font_size() as f32)
                                            .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                                    }),
                                // Source badge (detected/user)
                                label(move || {
                                    match item.source {
                                        ConfigSource::Detected => "auto".to_string(),
                                        ConfigSource::User => "user".to_string(),
                                    }
                                })
                                .style(move |s| {
                                    let cfg = config.get();
                                    s.font_size((cfg.ui.font_size() - 2) as f32)
                                        .color(cfg.color(LapceColor::PANEL_FOREGROUND_DIM))
                                        .padding_horiz(6.0)
                                        .padding_vert(2.0)
                                        .border_radius(4.0)
                                        .background(cfg.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                }),
                                // Play button
                                {
                                    let item_for_run = item_clone.clone();
                                    let on_run_clone = on_run.clone();
                                    clickable_icon(
                                        || LapceIcons::START,
                                        move || {
                                            dropdown_visible.set(false);
                                            on_run_clone(item_for_run.clone(), RunDebugMode::Run);
                                        },
                                        || false,
                                        || false,
                                        || "Run",
                                        config,
                                    )
                                },
                            ))
                            .style(|s| s.items_center().width_full()),
                        )
                        .on_click_stop({
                            let name = item_clone.name.clone();
                            move |_| {
                                selected.set(Some(name.clone()));
                            }
                        })
                        .style(move |s| {
                            let cfg = config.get();
                            let is_sel = selected.get() == Some(name_for_check2.clone());
                            let bg = if is_sel {
                                cfg.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND)
                            } else {
                                cfg.color(LapceColor::PANEL_BACKGROUND)
                            };
                            s.padding_horiz(12.0)
                                .padding_vert(8.0)
                                .width_full()
                                .background(bg)
                                .hover(|s| {
                                    s.cursor(CursorStyle::Pointer)
                                        .background(cfg.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                })
                        })
                    },
                )
                .style(|s| s.flex_col().width_full()),
            )
            .style(move |s| {
                s.width_full().max_height(300.0).apply_if(
                    loading.get() || error.get().is_some(),
                    |s| s.hide(),
                )
            }),
            
            // Separator
            empty().style(move |s| {
                let cfg = config.get();
                s.width_full()
                    .height(1.0)
                    .margin_vert(4.0)
                    .background(cfg.color(LapceColor::LAPCE_BORDER))
            }),
            
            // Edit Configurations button
            container(
                label(|| "Edit Configurations...".to_string())
                    .style(move |s| {
                        let cfg = config.get();
                        s.font_size(cfg.ui.font_size() as f32)
                            .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                    }),
            )
            .on_click_stop(move |_| {
                dropdown_visible.set(false);
                workbench_command.send(LapceWorkbenchCommand::OpenRunConfigurations);
            })
            .style(move |s| {
                let cfg = config.get();
                s.padding_horiz(12.0)
                    .padding_vert(8.0)
                    .width_full()
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer)
                            .background(cfg.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                    })
            }),
        ))
        .style(|s| s.flex_col().width_full()),
    )
    .keyboard_navigable()
    .request_focus(|| {})
    .on_event_stop(EventListener::FocusLost, move |_| {
        dropdown_visible.set(false);
    })
    .style(move |s| {
        let cfg = config.get();
        s.min_width(280.0)
            .max_width(400.0)
            .background(cfg.color(LapceColor::PANEL_BACKGROUND))
            .border(1.0)
            .border_radius(6.0)
            .border_color(cfg.color(LapceColor::LAPCE_BORDER))
            .box_shadow_blur(10.0)
            .box_shadow_color(cfg.color(LapceColor::LAPCE_DROPDOWN_SHADOW))
    })
}

/// Create the run configuration dropdown widget for the title bar
pub fn run_config_dropdown(
    window_tab_data: Rc<WindowTabData>,
    run_config_data: RunConfigData,
) -> impl View {
    let config = window_tab_data.common.config;
    let workbench_command = window_tab_data.common.workbench_command;
    let scope = run_config_data.scope;
    let common = window_tab_data.common.clone();
    
    // Overlay management
    let overlay_id: RwSignal<Option<ViewId>> = scope.create_rw_signal(None);
    let button_origin: RwSignal<Point> = scope.create_rw_signal(Point::ZERO);
    let button_size: RwSignal<Size> = scope.create_rw_signal(Size::ZERO);
    
    let dropdown_visible = run_config_data.dropdown_visible;
    let selected = run_config_data.selected;
    
    // Run action callback — opens terminal / debugger via internal command
    let internal_command = common.internal_command;
    let on_run: Rc<dyn Fn(super::RunConfigItem, RunDebugMode)> =
        Rc::new(move |item: super::RunConfigItem, mode: RunDebugMode| {
            let config = item.to_run_debug_config();
            internal_command.send(InternalCommand::RunAndDebug { mode, config });
        });
    
    // Fetch configs on mount
    fetch_run_configs(scope, common.clone(), run_config_data.clone());
    
    // Manage overlay visibility
    let run_config_data_for_overlay = run_config_data.clone();
    let on_run_for_overlay = on_run.clone();
    create_effect(move |_| {
        if dropdown_visible.get() {
            let origin = button_origin.get();
            let size = button_size.get();
            let point = Point::new(origin.x, origin.y + size.height);
            
            let data_clone = run_config_data_for_overlay.clone();
            let on_run_clone = on_run_for_overlay.clone();
            let id = add_overlay(point, move |_| {
                run_config_dropdown_overlay(
                    config,
                    data_clone.clone(),
                    workbench_command,
                    on_run_clone.clone(),
                )
            });
            overlay_id.set(Some(id));
        } else {
            if let Some(id) = overlay_id.get_untracked() {
                remove_overlay(id);
                overlay_id.set(None);
            }
        }
    });
    
    // The dropdown button
    let run_config_data_for_toggle = run_config_data.clone();
    let run_config_data_for_play = run_config_data.clone();
    let run_config_data_for_debug = run_config_data.clone();
    
    stack((
        // Selected config name
        container(
            label(move || {
                selected.get().unwrap_or_else(|| "No Config".to_string())
            })
            .style(move |s| {
                let cfg = config.get();
                s.font_size(cfg.ui.font_size() as f32)
                    .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                    .text_ellipsis()
                    .max_width(120.0)
            }),
        )
        .on_click_stop(move |_| {
            run_config_data_for_toggle.toggle_dropdown();
        })
        .on_move(move |point| {
            button_origin.set(point);
        })
        .on_resize(move |rect| {
            button_size.set(rect.size());
        })
        .style(move |s| {
            let cfg = config.get();
            s.padding_horiz(8.0)
                .padding_vert(4.0)
                .border_radius(4.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(cfg.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                })
        }),
        
        // Dropdown arrow
        svg(move || config.get().ui_svg(LapceIcons::ITEM_OPENED))
            .style(move |s| {
                let cfg = config.get();
                s.size(10.0, 10.0)
                    .margin_right(8.0)
                    .color(cfg.color(LapceColor::LAPCE_ICON_ACTIVE))
            }),
        
        // Play button
        clickable_icon(
            || LapceIcons::START,
            {
                let on_run = on_run.clone();
                move || {
                    if let Some(item) = run_config_data_for_play.get_selected_config() {
                        on_run(item, RunDebugMode::Run);
                    }
                }
            },
            || false,
            || false,
            || "Run",
            config,
        ),
        
        // Debug button
        clickable_icon(
            || LapceIcons::DEBUG,
            {
                let on_run = on_run.clone();
                move || {
                    if let Some(item) = run_config_data_for_debug.get_selected_config() {
                        on_run(item, RunDebugMode::Debug);
                    }
                }
            },
            || false,
            || false,
            || "Debug",
            config,
        ),
    ))
    .style(|s| s.items_center())
}
