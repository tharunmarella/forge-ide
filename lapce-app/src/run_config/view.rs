//! Run Configuration Editor Tab View
//!
//! Full configuration editor with:
//! - Left panel: list of configurations with add/remove/duplicate
//! - Right panel: configuration form editor

use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate},
    style::{CursorStyle, Display},
    views::{Decorators, container, dyn_stack, empty, label, scroll, stack, svg},
    ext_event::create_ext_action,
};
use lapce_rpc::{
    dap_types::RunDebugConfig,
    proxy::ProxyResponse,
};

use crate::{
    app::clickable_icon,
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    main_split::Editors,
    window_tab::CommonData,
};

use super::{RunConfigData, ConfigSource, fetch_run_configs};

/// Create the run configuration editor view (opens as an editor tab)
pub fn run_config_editor_view(
    _editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    tracing::info!("run_config_editor_view: Starting to create view");
    
    let config = common.config;
    let scope = Scope::current();
    tracing::info!("run_config_editor_view: Got scope");
    
    let _proxy = common.proxy.clone();
    
    // Local state for the editor
    tracing::info!("run_config_editor_view: Creating RunConfigData");
    let run_config_data = RunConfigData::new(scope);
    tracing::info!("run_config_editor_view: Created RunConfigData");
    
    let selected_for_edit: RwSignal<Option<String>> = scope.create_rw_signal(None);
    let edit_name: RwSignal<String> = scope.create_rw_signal(String::new());
    let edit_command: RwSignal<String> = scope.create_rw_signal(String::new());
    let edit_args: RwSignal<String> = scope.create_rw_signal(String::new());
    let edit_cwd: RwSignal<String> = scope.create_rw_signal(String::new());
    let edit_type: RwSignal<String> = scope.create_rw_signal(String::new());
    let is_user_config: RwSignal<bool> = scope.create_rw_signal(false);
    let save_message: RwSignal<Option<String>> = scope.create_rw_signal(None);
    tracing::info!("run_config_editor_view: Created all signals");
    
    // Fetch configs on load
    tracing::info!("run_config_editor_view: About to fetch configs");
    fetch_run_configs(scope, common.clone(), run_config_data.clone());
    tracing::info!("run_config_editor_view: Fetch configs initiated");
    
    container(
        stack((
            // Header
            container(
                label(|| "Run/Debug Configurations".to_string())
                    .style(move |s| {
                        let cfg = config.get();
                        s.font_size((cfg.ui.font_size() + 4) as f32)
                            .font_bold()
                            .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                    }),
            )
            .style(move |s| {
                let cfg = config.get();
                s.padding(16.0)
                    .border_bottom(1.0)
                    .border_color(cfg.color(LapceColor::LAPCE_BORDER))
            }),
            
            // Main content: left list + right editor
            stack((
                // Left panel: configuration list
                left_panel(
                    config,
                    run_config_data.clone(),
                    selected_for_edit,
                    edit_name,
                    edit_command,
                    edit_args,
                    edit_cwd,
                    edit_type,
                    is_user_config,
                    common.clone(),
                ),
                
                // Separator
                empty().style(move |s| {
                    let cfg = config.get();
                    s.width(1.0)
                        .height_full()
                        .background(cfg.color(LapceColor::LAPCE_BORDER))
                }),
                
                // Right panel: configuration editor
                right_panel(
                    config,
                    selected_for_edit,
                    edit_name,
                    edit_command,
                    edit_args,
                    edit_cwd,
                    edit_type,
                    is_user_config,
                    save_message,
                    common.clone(),
                    run_config_data.clone(),
                ),
            ))
            .style(|s| s.flex_grow(1.0).width_full()),
        ))
        .style(|s| s.flex_col().width_full().height_full()),
    )
    .style(move |s| {
        let cfg = config.get();
        s.width_full()
            .height_full()
            .background(cfg.color(LapceColor::EDITOR_BACKGROUND))
    })
}

/// Left panel with configuration list
fn left_panel(
    config: ReadSignal<Arc<LapceConfig>>,
    run_config_data: RunConfigData,
    selected_for_edit: RwSignal<Option<String>>,
    edit_name: RwSignal<String>,
    edit_command: RwSignal<String>,
    edit_args: RwSignal<String>,
    edit_cwd: RwSignal<String>,
    edit_type: RwSignal<String>,
    is_user_config: RwSignal<bool>,
    common: Rc<CommonData>,
) -> impl View {
    let scope = Scope::current();
    
    container(
        stack((
            // Toolbar: Add, Remove buttons
            container(
                stack((
                    clickable_icon(
                        || LapceIcons::ADD,
                        {
                            let run_config_data = run_config_data.clone();
                            move || {
                                // Create a new empty config
                                let new_name = "New Configuration".to_string();
                                selected_for_edit.set(Some(new_name.clone()));
                                edit_name.set(new_name);
                                edit_command.set(String::new());
                                edit_args.set(String::new());
                                edit_cwd.set("${workspace}".to_string());
                                edit_type.set("custom".to_string());
                                is_user_config.set(true);
                            }
                        },
                        || false,
                        || false,
                        || "Add Configuration",
                        config,
                    ),
                    clickable_icon(
                        || LapceIcons::CLOSE,
                        {
                            let common = common.clone();
                            let run_config_data = run_config_data.clone();
                            move || {
                                if let Some(name) = selected_for_edit.get() {
                                    // Only delete user configs
                                    if is_user_config.get() {
                                        let common_clone = common.clone();
                                        let run_config_data_clone = run_config_data.clone();
                                        let send = create_ext_action(scope, move |_: Result<ProxyResponse, _>| {
                                            fetch_run_configs(scope, common_clone.clone(), run_config_data_clone.clone());
                                        });
                                        common.proxy.delete_run_config(name, send);
                                        selected_for_edit.set(None);
                                    }
                                }
                            }
                        },
                        || false,
                        || false,
                        || "Delete Configuration",
                        config,
                    ),
                ))
                .style(|s| s.items_center().gap(4.0)),
            )
            .style(move |s| {
                let cfg = config.get();
                s.padding(8.0)
                    .border_bottom(1.0)
                    .border_color(cfg.color(LapceColor::LAPCE_BORDER))
            }),
            
            // Config list
            scroll(
                stack((
                    // Detected configs section
                    config_section(
                        config,
                        "Detected",
                        run_config_data.clone(),
                        selected_for_edit,
                        edit_name,
                        edit_command,
                        edit_args,
                        edit_cwd,
                        edit_type,
                        is_user_config,
                        true,
                    ),
                    
                    // User configs section
                    config_section(
                        config,
                        "User Configurations",
                        run_config_data.clone(),
                        selected_for_edit,
                        edit_name,
                        edit_command,
                        edit_args,
                        edit_cwd,
                        edit_type,
                        is_user_config,
                        false,
                    ),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.flex_grow(1.0).width_full()),
        ))
        .style(|s| s.flex_col().width_full().height_full()),
    )
    .style(move |s| {
        let cfg = config.get();
        s.width(280.0)
            .height_full()
            .background(cfg.color(LapceColor::PANEL_BACKGROUND))
    })
}

/// A section of configs (detected or user)
fn config_section(
    config: ReadSignal<Arc<LapceConfig>>,
    title: &'static str,
    run_config_data: RunConfigData,
    selected_for_edit: RwSignal<Option<String>>,
    edit_name: RwSignal<String>,
    edit_command: RwSignal<String>,
    edit_args: RwSignal<String>,
    edit_cwd: RwSignal<String>,
    edit_type: RwSignal<String>,
    is_user_config: RwSignal<bool>,
    is_detected: bool,
) -> impl View {
    let filter_source = if is_detected { ConfigSource::Detected } else { ConfigSource::User };
    
    stack((
        // Section header
        label(move || title.to_string())
            .style(move |s| {
                let cfg = config.get();
                s.padding_horiz(12.0)
                    .padding_vert(8.0)
                    .font_size((cfg.ui.font_size() - 1) as f32)
                    .font_bold()
                    .color(cfg.color(LapceColor::PANEL_FOREGROUND_DIM))
            }),
        
        // Config items
        dyn_stack(
            move || {
                run_config_data.all_configs()
                    .into_iter()
                    .filter(|c| c.source == filter_source)
                    .collect::<Vec<_>>()
            },
            |item| item.name.clone(),
            move |item| {
                let item_for_icon = item.clone();
                let item_for_label = item.clone();
                let item_for_click = item.clone();
                let name = item.name.clone();
                let name_for_check = name.clone();
                
                container(
                    stack((
                        // Icon based on type - use START icon for all run configs
                        svg(move || {
                            let _type = &item_for_icon.config_type;
                            // Use START (play) icon for all run configurations
                            config.get().ui_svg(LapceIcons::START)
                        })
                        .style(move |s| {
                            let cfg = config.get();
                            s.size(16.0, 16.0)
                                .margin_right(8.0)
                                .color(cfg.color(LapceColor::LAPCE_ICON_ACTIVE))
                        }),
                        
                        // Config name
                        label(move || item_for_label.name.clone())
                            .style(move |s| {
                                let cfg = config.get();
                                s.flex_grow(1.0)
                                    .font_size(cfg.ui.font_size() as f32)
                                    .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                                    .text_ellipsis()
                            }),
                    ))
                    .style(|s| s.items_center().width_full()),
                )
                .on_click_stop({
                    move |_| {
                        selected_for_edit.set(Some(item_for_click.name.clone()));
                        edit_name.set(item_for_click.name.clone());
                        edit_command.set(item_for_click.command.clone());
                        edit_args.set(item_for_click.args.join(" "));
                        edit_cwd.set(item_for_click.cwd.clone().unwrap_or_default());
                        edit_type.set(item_for_click.config_type.clone());
                        is_user_config.set(item_for_click.source == ConfigSource::User);
                    }
                })
                .style(move |s| {
                    let cfg = config.get();
                    let is_sel = selected_for_edit.get() == Some(name_for_check.clone());
                    let bg = if is_sel {
                        cfg.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND)
                    } else {
                        cfg.color(LapceColor::PANEL_BACKGROUND)
                    };
                    s.padding_horiz(12.0)
                        .padding_vert(6.0)
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
    ))
    .style(|s| s.flex_col().width_full())
}

/// Right panel with configuration editor form
fn right_panel(
    config: ReadSignal<Arc<LapceConfig>>,
    selected_for_edit: RwSignal<Option<String>>,
    edit_name: RwSignal<String>,
    edit_command: RwSignal<String>,
    edit_args: RwSignal<String>,
    edit_cwd: RwSignal<String>,
    edit_type: RwSignal<String>,
    is_user_config: RwSignal<bool>,
    save_message: RwSignal<Option<String>>,
    common: Rc<CommonData>,
    run_config_data: RunConfigData,
) -> impl View {
    let scope = Scope::current();
    
    container(
        stack((
            // Show placeholder if nothing selected
            container(
                label(|| "Select a configuration to edit".to_string())
                    .style(move |s| {
                        let cfg = config.get();
                        s.font_size(cfg.ui.font_size() as f32)
                            .color(cfg.color(LapceColor::PANEL_FOREGROUND_DIM))
                    }),
            )
            .style(move |s| {
                s.display(if selected_for_edit.get().is_none() { Display::Flex } else { Display::None })
                    .flex_grow(1.0)
                    .items_center()
                    .justify_center()
            }),
            
            // Editor form
            container(
                scroll(
                    stack((
                        // Name field
                        form_field(config, "Name", edit_name, !is_user_config.get()),
                        
                        // Type field (read-only for detected)
                        form_field(config, "Type", edit_type, true),
                        
                        // Command field
                        form_field(config, "Command", edit_command, !is_user_config.get()),
                        
                        // Arguments field
                        form_field(config, "Arguments", edit_args, !is_user_config.get()),
                        
                        // Working Directory field
                        form_field(config, "Working Directory", edit_cwd, !is_user_config.get()),
                        
                        // Save message
                        label(move || save_message.get().unwrap_or_default())
                            .style(move |s| {
                                let cfg = config.get();
                                s.margin_top(16.0)
                                    .font_size(cfg.ui.font_size() as f32)
                                    .color(cfg.color(LapceColor::LAPCE_ICON_ACTIVE))
                                    .display(if save_message.get().is_some() { Display::Flex } else { Display::None })
                            }),
                        
                        // Save button (only for user configs)
                        container(
                            label(|| "Save Configuration".to_string())
                                .style(move |s| {
                                    let cfg = config.get();
                                    s.font_size(cfg.ui.font_size() as f32)
                                        .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                                }),
                        )
                        .on_click_stop({
                            let common = common.clone();
                            let run_config_data = run_config_data.clone();
                            move |_| {
                                if is_user_config.get() {
                                    let config = RunDebugConfig {
                                        ty: Some(edit_type.get()),
                                        name: edit_name.get(),
                                        program: edit_command.get(),
                                        args: Some(edit_args.get().split_whitespace().map(String::from).collect()),
                                        cwd: Some(edit_cwd.get()),
                                        env: None,
                                        prelaunch: None,
                                        debug_command: None,
                                        dap_id: Default::default(),
                                        tracing_output: false,
                                        config_source: Default::default(),
                                    };
                                    
                                    let common_clone = common.clone();
                                    let run_config_data_clone = run_config_data.clone();
                                    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
                                        match result {
                                            Ok(ProxyResponse::RunConfigSaveResponse { success, message }) => {
                                                save_message.set(Some(message));
                                                if success {
                                                    fetch_run_configs(scope, common_clone.clone(), run_config_data_clone.clone());
                                                }
                                            }
                                            Err(e) => {
                                                save_message.set(Some(format!("Error: {:?}", e)));
                                            }
                                            _ => {}
                                        }
                                    });
                                    common.proxy.save_run_config(config, send);
                                }
                            }
                        })
                        .style(move |s| {
                            let cfg = config.get();
                            s.margin_top(24.0)
                                .padding_horiz(16.0)
                                .padding_vert(8.0)
                                .border_radius(4.0)
                                .background(cfg.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND))
                                .display(if is_user_config.get() { Display::Flex } else { Display::None })
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(cfg.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                })
                        }),
                        
                        // Note for detected configs
                        label(|| "Detected configurations are read-only. Create a user configuration to customize.".to_string())
                            .style(move |s| {
                                let cfg = config.get();
                                s.margin_top(24.0)
                                    .font_size((cfg.ui.font_size() - 1) as f32)
                                    .color(cfg.color(LapceColor::PANEL_FOREGROUND_DIM))
                                    .display(if !is_user_config.get() && selected_for_edit.get().is_some() { Display::Flex } else { Display::None })
                            }),
                    ))
                    .style(|s| s.flex_col().width_full().padding(24.0)),
                )
                .style(|s| s.width_full().height_full()),
            )
            .style(move |s| {
                s.display(if selected_for_edit.get().is_some() { Display::Flex } else { Display::None })
                    .flex_grow(1.0)
                    .width_full()
                    .height_full()
            }),
        ))
        .style(|s| s.width_full().height_full()),
    )
    .style(move |s| {
        let cfg = config.get();
        s.flex_grow(1.0)
            .height_full()
            .background(cfg.color(LapceColor::EDITOR_BACKGROUND))
    })
}

/// A form field with label and text input
fn form_field(
    config: ReadSignal<Arc<LapceConfig>>,
    label_text: &'static str,
    value: RwSignal<String>,
    readonly: bool,
) -> impl View {
    stack((
        label(move || label_text.to_string())
            .style(move |s| {
                let cfg = config.get();
                s.font_size(cfg.ui.font_size() as f32)
                    .font_bold()
                    .color(cfg.color(LapceColor::PANEL_FOREGROUND))
                    .margin_bottom(4.0)
            }),
        container(
            label(move || value.get())
                .style(move |s| {
                    let cfg = config.get();
                    s.width_full()
                        .font_size(cfg.ui.font_size() as f32)
                        .color(if readonly {
                            cfg.color(LapceColor::PANEL_FOREGROUND_DIM)
                        } else {
                            cfg.color(LapceColor::PANEL_FOREGROUND)
                        })
                }),
        )
        .style(move |s| {
            let cfg = config.get();
            s.width_full()
                .padding_horiz(8.0)
                .padding_vert(6.0)
                .border(1.0)
                .border_radius(4.0)
                .border_color(cfg.color(LapceColor::LAPCE_BORDER))
                .background(if readonly {
                    cfg.color(LapceColor::PANEL_BACKGROUND)
                } else {
                    cfg.color(LapceColor::EDITOR_BACKGROUND)
                })
        }),
    ))
    .style(|s| s.flex_col().width_full().margin_bottom(16.0))
}
