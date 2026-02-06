//! SDK Manager Panel View
//!
//! This panel provides UI for managing SDKs and toolchains via proto.

use std::rc::Rc;
use std::sync::Arc;

use floem::{
    View,
    ext_event::create_ext_action,
    reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate},
    style::{AlignItems, CursorStyle, Display, FlexDirection},
    views::{Decorators, container, dyn_stack, label, scroll, stack, svg},
};
use lapce_rpc::proxy::{ProxyResponse, ProtoToolInfo};

use super::{position::PanelPosition, view::PanelBuilder};
use crate::{
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    main_split::Editors,
    window_tab::{CommonData, WindowTabData},
};
use lapce_rpc::proxy::ProxyRpcHandler;

/// Detected tool info
#[derive(Clone, Debug)]
struct DetectedTool {
    name: String,
    suggested_version: String,
    reason: String,
    installing: RwSignal<bool>,
    installed: RwSignal<bool>,
}

/// SDK Manager state
#[derive(Clone)]
struct SdkManagerData {
    scope: Scope,
    proto_installed: RwSignal<Option<bool>>,
    proto_version: RwSignal<Option<String>>,
    installed_tools: RwSignal<Vec<ProtoToolInfo>>,
    detected_tools: RwSignal<Vec<DetectedTool>>,
    loading: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    install_message: RwSignal<Option<String>>,
}

impl SdkManagerData {
    fn new(cx: Scope) -> Self {
        Self {
            scope: cx,
            proto_installed: cx.create_rw_signal(None),
            proto_version: cx.create_rw_signal(None),
            installed_tools: cx.create_rw_signal(Vec::new()),
            detected_tools: cx.create_rw_signal(Vec::new()),
            loading: cx.create_rw_signal(true),
            error: cx.create_rw_signal(None),
            install_message: cx.create_rw_signal(None),
        }
    }
}

pub fn sdk_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let scope = window_tab_data.scope;
    let common = window_tab_data.common.clone();
    
    let sdk_data = SdkManagerData::new(scope);
    
    // Fetch proto status on panel load
    fetch_proto_status(scope, common.clone(), sdk_data.clone());
    
    PanelBuilder::new(config, position)
        .add(
            "SDK Status",
            sdk_panel_content(config, common, sdk_data),
            scope.create_rw_signal(true),
        )
        .build()
        .debug_name("SDK Panel")
}

fn fetch_proto_status(scope: Scope, common: Rc<CommonData>, data: SdkManagerData) {
    data.loading.set(true);
    data.error.set(None);
    
    let proxy = common.proxy.clone();
    
    // Check if proto is installed
    let data_clone = data.clone();
    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
        match result {
            Ok(ProxyResponse::ProtoIsInstalledResponse { installed }) => {
                data_clone.proto_installed.set(Some(installed));
            }
            Err(e) => {
                data_clone.error.set(Some(format!("Failed to check proto: {:?}", e)));
            }
            _ => {}
        }
        data_clone.loading.set(false);
    });
    proxy.proto_is_installed(send);
    
    // Get proto version
    let data_clone = data.clone();
    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
        if let Ok(ProxyResponse::ProtoVersionResponse { version }) = result {
            data_clone.proto_version.set(Some(version));
        }
    });
    proxy.proto_get_version(send);
    
    // Get installed tools first, then detect project tools
    let data_clone = data.clone();
    let proxy_clone = proxy.clone();
    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
        if let Ok(ProxyResponse::ProtoToolsListResponse { tools }) = result {
            data_clone.installed_tools.set(tools.clone());
            
            // Now detect project tools and mark already-installed ones
            let installed_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
            let data_inner = data_clone.clone();
            
            let send_detect = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
                if let Ok(ProxyResponse::ProtoDetectedToolsResponse { tools: detected_list }) = result {
                    let detected: Vec<DetectedTool> = detected_list.into_iter().map(|(name, version)| {
                        let reason = match name.as_str() {
                            "rust" => "Cargo.toml found",
                            "node" => "package.json found",
                            "python" => "Python project files found",
                            "go" => "go.mod found",
                            "deno" => "deno.json found",
                            "bun" => "bun.lockb found",
                            _ => "Project file detected",
                        };
                        // Check if already installed
                        let is_installed = installed_names.contains(&name);
                        DetectedTool {
                            name,
                            suggested_version: version,
                            reason: reason.to_string(),
                            installing: data_inner.scope.create_rw_signal(false),
                            installed: data_inner.scope.create_rw_signal(is_installed),
                        }
                    }).collect();
                    data_inner.detected_tools.set(detected);
                }
            });
            proxy_clone.proto_detect_project_tools(send_detect);
        }
    });
    proxy.proto_list_tools(send);
}

fn install_tool(
    scope: Scope,
    proxy: ProxyRpcHandler,
    tool_name: String,
    tool_version: String,
    installing: RwSignal<bool>,
    installed: RwSignal<bool>,
    message: RwSignal<Option<String>>,
    installed_tools: RwSignal<Vec<ProtoToolInfo>>,
) {
    installing.set(true);
    message.set(Some(format!("Installing {} {}...", tool_name, tool_version)));
    
    let tool_name_clone = tool_name.clone();
    let proxy_clone = proxy.clone();
    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
        installing.set(false);
        match result {
            Ok(ProxyResponse::ProtoInstallResponse { success, message: msg }) => {
                if success {
                    installed.set(true);
                    message.set(Some(format!("Successfully installed {}", tool_name_clone)));
                    
                    // Refresh the installed tools list
                    let send_refresh = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
                        if let Ok(ProxyResponse::ProtoToolsListResponse { tools }) = result {
                            installed_tools.set(tools);
                        }
                    });
                    proxy_clone.proto_list_tools(send_refresh);
                } else {
                    message.set(Some(format!("Failed to install {}: {}", tool_name_clone, msg)));
                }
            }
            Err(e) => {
                message.set(Some(format!("Error installing {}: {:?}", tool_name_clone, e)));
            }
            _ => {}
        }
    });
    
    proxy.proto_install_tool(tool_name, tool_version, send);
}

fn sdk_panel_content(
    config: ReadSignal<Arc<LapceConfig>>,
    common: Rc<CommonData>,
    data: SdkManagerData,
) -> impl View {
    let loading = data.loading;
    let proto_installed = data.proto_installed;
    let proto_version = data.proto_version;
    let installed_tools = data.installed_tools;
    let detected_tools = data.detected_tools;
    let error = data.error;
    
    scroll(
        container(
            stack((
                // Loading indicator
                label(|| "Loading...").style(move |s| {
                    let config = config.get();
                    s.display(if loading.get() { Display::Flex } else { Display::None })
                        .padding(10.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
                
                // Error message
                label(move || error.get().unwrap_or_default()).style(move |s| {
                    let config = config.get();
                    let has_error = error.get().is_some();
                    s.display(if has_error { Display::Flex } else { Display::None })
                        .padding(10.0)
                        .color(config.color(LapceColor::LAPCE_ERROR))
                }),
                
                // Proto status section
                container(
                    stack((
                        // Proto installed status
                        label(move || {
                            match proto_installed.get() {
                                Some(true) => format!(
                                    "Proto: Installed {}",
                                    proto_version.get().map(|v| format!("({})", v)).unwrap_or_default()
                                ),
                                Some(false) => "Proto: Not installed".to_string(),
                                None => "Proto: Checking...".to_string(),
                            }
                        }).style(move |s| {
                            let config = config.get();
                            let installed = proto_installed.get().unwrap_or(false);
                            s.font_size(config.ui.font_size() as f32)
                                .font_weight(floem::text::Weight::BOLD)
                                .padding(8.0)
                                .color(if installed {
                                    config.color(LapceColor::EDITOR_FOREGROUND)
                                } else {
                                    config.color(LapceColor::LAPCE_ERROR)
                                })
                        }),
                        
                        // Install instructions when not installed
                        label(|| "Install proto: curl -fsSL https://moonrepo.dev/install/proto.sh | bash")
                            .style(move |s| {
                                let config = config.get();
                                let show = proto_installed.get() == Some(false);
                                s.display(if show { Display::Flex } else { Display::None })
                                    .font_size(config.ui.font_size() as f32 * 0.9)
                                    .padding(8.0)
                                    .margin_left(16.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                    ))
                    .style(|s| s.flex_direction(FlexDirection::Column))
                ).style(move |s| {
                    let config = config.get();
                    s.width_full()
                        .margin_bottom(16.0)
                        .border_bottom(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                }),
                
                // Detected tools section  
                container(
                    stack((
                        label(|| "Detected Tools").style(move |s| {
                            let config = config.get();
                            s.font_size(config.ui.font_size() as f32)
                                .font_weight(floem::text::Weight::BOLD)
                                .padding(8.0)
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                        }),
                        
                        // Detected tools list
                        dyn_stack(
                            move || detected_tools.get(),
                            |tool| tool.name.clone(),
                            move |tool| {
                                detected_tool_item(config, common.clone(), data.clone(), tool)
                            }
                        ).style(|s| s.flex_direction(FlexDirection::Column).width_full()),
                        
                        // Empty state
                        label(|| "No tools detected for this project").style(move |s| {
                            let config = config.get();
                            let has_tools = !detected_tools.get().is_empty();
                            s.display(if has_tools { Display::None } else { Display::Flex })
                                .padding(8.0)
                                .margin_left(16.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                        }),
                    ))
                    .style(|s| s.flex_direction(FlexDirection::Column).width_full())
                ).style(move |s| {
                    let config = config.get();
                    let installed = proto_installed.get() == Some(true);
                    s.display(if installed { Display::Flex } else { Display::None })
                        .width_full()
                        .margin_bottom(16.0)
                        .border_bottom(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                }),
                
                // Installed tools section
                container(
                    stack((
                        label(|| "Installed Tools").style(move |s| {
                            let config = config.get();
                            s.font_size(config.ui.font_size() as f32)
                                .font_weight(floem::text::Weight::BOLD)
                                .padding(8.0)
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                        }),
                        
                        // Tools list
                        dyn_stack(
                            move || installed_tools.get(),
                            |tool| format!("{}-{}", tool.name, tool.version),
                            move |tool| {
                                tool_item_view(config, tool)
                            }
                        ).style(|s| s.flex_direction(FlexDirection::Column).width_full()),
                        
                        // Empty state
                        label(|| "No tools installed").style(move |s| {
                            let config = config.get();
                            let has_tools = !installed_tools.get().is_empty();
                            s.display(if has_tools { Display::None } else { Display::Flex })
                                .padding(8.0)
                                .margin_left(16.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                        }),
                    ))
                    .style(|s| s.flex_direction(FlexDirection::Column).width_full())
                ).style(move |s| {
                    let installed = proto_installed.get() == Some(true);
                    s.display(if installed { Display::Flex } else { Display::None })
                        .width_full()
                }),
            ))
            .style(|s| s.flex_direction(FlexDirection::Column).width_full())
        )
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding(8.0)
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        })
    )
    .style(|s| s.size_full())
}

fn detected_tool_item(
    config: ReadSignal<Arc<LapceConfig>>,
    common: Rc<CommonData>,
    data: SdkManagerData,
    tool: DetectedTool,
) -> impl View {
    let name = tool.name.clone();
    let version = tool.suggested_version.clone();
    let reason = tool.reason.clone();
    let installing = tool.installing;
    let installed = tool.installed;
    
    let name_for_click = name.clone();
    let version_for_click = version.clone();
    let proxy = common.proxy.clone();
    let scope = data.scope;
    let message = data.install_message;
    let installed_tools = data.installed_tools;
    
    container(
        stack((
            // Tool icon
            svg(move || config.get().ui_svg(LapceIcons::SDK)).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_right(8.0)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            }),
            // Tool name and reason
            stack((
                label(move || name.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .font_weight(floem::text::Weight::BOLD)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                }),
                label(move || format!("{} ({})", reason.clone(), version.clone())).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32 * 0.85)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
            ))
            .style(|s| s.flex_direction(FlexDirection::Column).flex_grow(1.0)),
            // Install button
            container(
                label(move || {
                    if installed.get() {
                        "Installed".to_string()
                    } else if installing.get() {
                        "Installing...".to_string()
                    } else {
                        "Install".to_string()
                    }
                })
            )
            .on_click_stop(move |_| {
                if !installing.get() && !installed.get() {
                    install_tool(
                        scope,
                        proxy.clone(),
                        name_for_click.clone(),
                        version_for_click.clone(),
                        installing,
                        installed,
                        message,
                        installed_tools,
                    );
                }
            })
            .style(move |s| {
                let config = config.get();
                let is_installed = installed.get();
                let is_installing = installing.get();
                s.font_size(config.ui.font_size() as f32 * 0.9)
                    .padding_left(12.0)
                    .padding_right(12.0)
                    .padding_top(4.0)
                    .padding_bottom(4.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .cursor(if is_installed || is_installing { CursorStyle::Default } else { CursorStyle::Pointer })
                    .border_color(if is_installed {
                        config.color(LapceColor::LAPCE_BORDER)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                    })
                    .background(if is_installed {
                        config.color(LapceColor::PANEL_BACKGROUND)
                    } else if is_installing {
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                    })
                    .color(if is_installed {
                        config.color(LapceColor::EDITOR_DIM)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND)
                    })
                    .hover(move |s| {
                        if !is_installed && !is_installing {
                            s.background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                        } else {
                            s
                        }
                    })
            }),
        ))
        .style(|s| s.flex_direction(FlexDirection::Row).align_items(AlignItems::Center).width_full())
    )
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .padding(8.0)
            .margin_left(16.0)
            .border_radius(4.0)
            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
    })
}

fn tool_item_view(
    config: ReadSignal<Arc<LapceConfig>>,
    tool: ProtoToolInfo,
) -> impl View {
    let name = tool.name.clone();
    let version = tool.version.clone();
    let is_default = tool.is_default;
    
    container(
        stack((
            // Tool icon
            svg(move || config.get().ui_svg(LapceIcons::SDK)).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_right(8.0)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            }),
            // Tool name
            label(move || name.clone()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
            }),
            // Version
            label(move || version.clone()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .margin_left(8.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            // Default badge
            label(|| "default").style(move |s| {
                let config = config.get();
                s.display(if is_default { Display::Flex } else { Display::None })
                    .font_size(config.ui.font_size() as f32 * 0.8)
                    .margin_left(8.0)
                    .padding_left(4.0)
                    .padding_right(4.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
        ))
        .style(|s| s.flex_direction(FlexDirection::Row).align_items(AlignItems::Center))
    )
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .padding(8.0)
            .margin_left(16.0)
            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
    })
}

/// SDK Manager view for editor tab (opened as a tab like Settings)
pub fn sdk_manager_view(
    _editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;
    let scope = Scope::current();
    
    let sdk_data = SdkManagerData::new(scope);
    
    // Fetch proto status on view load
    fetch_proto_status(scope, common.clone(), sdk_data.clone());
    
    let loading = sdk_data.loading;
    let proto_installed = sdk_data.proto_installed;
    let proto_version = sdk_data.proto_version;
    let installed_tools = sdk_data.installed_tools;
    let detected_tools = sdk_data.detected_tools;
    let error = sdk_data.error;
    let install_message = sdk_data.install_message;
    
    container(
        scroll(
            container(
                stack((
                    // Header
                    stack((
                        // Icon
                        svg(move || config.get().ui_svg(LapceIcons::SDK)).style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32 * 2.5;
                            s.size(size, size)
                                .margin_right(16.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        }),
                        // Title and description
                        stack((
                            label(|| "SDK Manager").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 1.5)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            }),
                            label(|| "Manage SDKs, toolchains, and runtime versions with proto").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32)
                                    .margin_top(4.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                        ))
                        .style(|s| s.flex_direction(FlexDirection::Column)),
                    ))
                    .style(|s| {
                        s.flex_direction(FlexDirection::Row)
                            .align_items(AlignItems::Center)
                            .margin_bottom(24.0)
                    }),
                    
                    // Loading indicator
                    label(|| "Loading...").style(move |s| {
                        let config = config.get();
                        s.display(if loading.get() { Display::Flex } else { Display::None })
                            .padding(16.0)
                            .color(config.color(LapceColor::EDITOR_DIM))
                    }),
                    
                    // Error message
                    container(
                        label(move || error.get().unwrap_or_default())
                    ).style(move |s| {
                        let config = config.get();
                        let has_error = error.get().is_some();
                        s.display(if has_error { Display::Flex } else { Display::None })
                            .padding(16.0)
                            .margin_bottom(16.0)
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_ERROR))
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                            .color(config.color(LapceColor::LAPCE_ERROR))
                    }),
                    
                    // Install message
                    container(
                        label(move || install_message.get().unwrap_or_default())
                    ).style(move |s| {
                        let config = config.get();
                        let has_msg = install_message.get().is_some();
                        s.display(if has_msg { Display::Flex } else { Display::None })
                            .padding(12.0)
                            .margin_bottom(16.0)
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    }),
                    
                    // Proto Status Card
                    container(
                        stack((
                            label(|| "Proto Status").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 1.1)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .margin_bottom(12.0)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            }),
                            
                            // Status row
                            stack((
                                label(|| "Status:").style(move |s| {
                                    let config = config.get();
                                    s.font_size(config.ui.font_size() as f32)
                                        .width(100.0)
                                        .color(config.color(LapceColor::EDITOR_DIM))
                                }),
                                label(move || {
                                    match proto_installed.get() {
                                        Some(true) => "Installed".to_string(),
                                        Some(false) => "Not Installed".to_string(),
                                        None => "Checking...".to_string(),
                                    }
                                }).style(move |s| {
                                    let config = config.get();
                                    let installed = proto_installed.get().unwrap_or(false);
                                    s.font_size(config.ui.font_size() as f32)
                                        .font_weight(floem::text::Weight::BOLD)
                                        .color(if installed {
                                            config.color(LapceColor::EDITOR_FOREGROUND)
                                        } else {
                                            config.color(LapceColor::LAPCE_ERROR)
                                        })
                                }),
                            ))
                            .style(|s| s.flex_direction(FlexDirection::Row).margin_bottom(8.0)),
                            
                            // Version row
                            stack((
                                label(|| "Version:").style(move |s| {
                                    let config = config.get();
                                    s.font_size(config.ui.font_size() as f32)
                                        .width(100.0)
                                        .color(config.color(LapceColor::EDITOR_DIM))
                                }),
                                label(move || proto_version.get().unwrap_or_else(|| "-".to_string()))
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size(config.ui.font_size() as f32)
                                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                    }),
                            ))
                            .style(move |s| {
                                let show = proto_installed.get() == Some(true);
                                s.display(if show { Display::Flex } else { Display::None })
                                    .flex_direction(FlexDirection::Row)
                            }),
                            
                            // Install instructions
                            container(
                                stack((
                                    label(|| "To install proto, run:").style(move |s| {
                                        let config = config.get();
                                        s.font_size(config.ui.font_size() as f32)
                                            .margin_bottom(8.0)
                                            .color(config.color(LapceColor::EDITOR_DIM))
                                    }),
                                    label(|| "curl -fsSL https://moonrepo.dev/install/proto.sh | bash")
                                        .style(move |s| {
                                            let config = config.get();
                                            s.font_size(config.ui.font_size() as f32)
                                                .padding(12.0)
                                                .border_radius(4.0)
                                                .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        }),
                                ))
                                .style(|s| s.flex_direction(FlexDirection::Column))
                            ).style(move |s| {
                                let show = proto_installed.get() == Some(false);
                                s.display(if show { Display::Flex } else { Display::None })
                                    .margin_top(16.0)
                            }),
                        ))
                        .style(|s| s.flex_direction(FlexDirection::Column))
                    ).style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .padding(16.0)
                            .margin_bottom(16.0)
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                    }),
                    
                    // Detected Tools Card
                    container(
                        stack((
                            label(|| "Detected Tools").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 1.1)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .margin_bottom(4.0)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            }),
                            label(|| "Based on project files, these tools are recommended:").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 0.9)
                                    .margin_bottom(12.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                            
                            // Detected tools list
                            dyn_stack(
                                move || detected_tools.get(),
                                |tool| tool.name.clone(),
                                move |tool| {
                                    detected_tool_row(config, common.clone(), sdk_data.clone(), tool)
                                }
                            ).style(|s| s.flex_direction(FlexDirection::Column).width_full()),
                            
                            // Empty state
                            label(|| "No tools detected for this project").style(move |s| {
                                let config = config.get();
                                let has_tools = !detected_tools.get().is_empty();
                                s.display(if has_tools { Display::None } else { Display::Flex })
                                    .padding(16.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                        ))
                        .style(|s| s.flex_direction(FlexDirection::Column).width_full())
                    ).style(move |s| {
                        let config = config.get();
                        let installed = proto_installed.get() == Some(true);
                        s.display(if installed { Display::Flex } else { Display::None })
                            .width_full()
                            .padding(16.0)
                            .margin_bottom(16.0)
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                    }),
                    
                    // Installed Tools Card
                    container(
                        stack((
                            label(|| "Installed Tools").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 1.1)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .margin_bottom(12.0)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            }),
                            
                            // Tools list
                            dyn_stack(
                                move || installed_tools.get(),
                                |tool| format!("{}-{}", tool.name, tool.version),
                                move |tool| {
                                    tool_row_view(config, tool)
                                }
                            ).style(|s| s.flex_direction(FlexDirection::Column).width_full()),
                            
                            // Empty state
                            label(|| "No tools installed yet").style(move |s| {
                                let config = config.get();
                                let has_tools = !installed_tools.get().is_empty();
                                s.display(if has_tools { Display::None } else { Display::Flex })
                                    .padding(16.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                        ))
                        .style(|s| s.flex_direction(FlexDirection::Column).width_full())
                    ).style(move |s| {
                        let config = config.get();
                        let installed = proto_installed.get() == Some(true);
                        s.display(if installed { Display::Flex } else { Display::None })
                            .width_full()
                            .padding(16.0)
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::PANEL_BACKGROUND))
                    }),
                ))
                .style(|s| s.flex_direction(FlexDirection::Column).width_full())
            )
            .style(move |s| {
                s.width_full()
                    .max_width(800.0)
                    .padding(32.0)
            })
        )
        .style(|s| s.size_full())
    )
    .style(move |s| {
        let config = config.get();
        s.size_full()
            .align_items(AlignItems::Center)
            .background(config.color(LapceColor::EDITOR_BACKGROUND))
    })
    .debug_name("SDK Manager View")
}

fn detected_tool_row(
    config: ReadSignal<Arc<LapceConfig>>,
    common: Rc<CommonData>,
    data: SdkManagerData,
    tool: DetectedTool,
) -> impl View {
    let name = tool.name.clone();
    let version = tool.suggested_version.clone();
    let reason = tool.reason.clone();
    let installing = tool.installing;
    let installed = tool.installed;
    
    let name_for_click = name.clone();
    let version_for_click = version.clone();
    let proxy = common.proxy.clone();
    let scope = data.scope;
    let message = data.install_message;
    let installed_tools = data.installed_tools;
    
    container(
        stack((
            // Tool icon
            svg(move || config.get().ui_svg(LapceIcons::SDK)).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_right(12.0)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            }),
            // Tool name and reason
            stack((
                label(move || name.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .font_weight(floem::text::Weight::MEDIUM)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                }),
                label(move || format!("{} • Suggested: {}", reason.clone(), version.clone())).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32 * 0.85)
                        .margin_top(2.0)
                        .color(config.color(LapceColor::EDITOR_DIM))
                }),
            ))
            .style(|s| s.flex_direction(FlexDirection::Column).flex_grow(1.0)),
            // Install button
            container(
                label(move || {
                    if installed.get() {
                        "Installed ✓".to_string()
                    } else if installing.get() {
                        "Installing...".to_string()
                    } else {
                        "Install".to_string()
                    }
                })
            )
            .on_click_stop(move |_| {
                if !installing.get() && !installed.get() {
                    install_tool(
                        scope,
                        proxy.clone(),
                        name_for_click.clone(),
                        version_for_click.clone(),
                        installing,
                        installed,
                        message,
                        installed_tools,
                    );
                }
            })
            .style(move |s| {
                let config = config.get();
                let is_installed = installed.get();
                let is_installing = installing.get();
                s.font_size(config.ui.font_size() as f32 * 0.9)
                    .padding_left(16.0)
                    .padding_right(16.0)
                    .padding_top(6.0)
                    .padding_bottom(6.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .cursor(if is_installed || is_installing { CursorStyle::Default } else { CursorStyle::Pointer })
                    .border_color(if is_installed {
                        config.color(LapceColor::LAPCE_BORDER)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                    })
                    .background(if is_installed {
                        config.color(LapceColor::PANEL_BACKGROUND)
                    } else if is_installing {
                        config.color(LapceColor::PANEL_HOVERED_BACKGROUND)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                    })
                    .color(if is_installed {
                        config.color(LapceColor::EDITOR_DIM)
                    } else {
                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND)
                    })
            }),
        ))
        .style(|s| s.flex_direction(FlexDirection::Row).align_items(AlignItems::Center).width_full())
    )
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .padding(12.0)
            .border_radius(4.0)
            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
    })
}

fn tool_row_view(
    config: ReadSignal<Arc<LapceConfig>>,
    tool: ProtoToolInfo,
) -> impl View {
    let name = tool.name.clone();
    let version = tool.version.clone();
    let is_default = tool.is_default;
    
    container(
        stack((
            // Tool icon
            svg(move || config.get().ui_svg(LapceIcons::SDK)).style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_right(12.0)
                    .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
            }),
            // Tool name
            label(move || name.clone()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .font_weight(floem::text::Weight::MEDIUM)
                    .min_width(120.0)
                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
            }),
            // Version
            label(move || version.clone()).style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32)
                    .min_width(100.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
            // Default badge
            container(
                label(|| "default")
            ).style(move |s| {
                let config = config.get();
                s.display(if is_default { Display::Flex } else { Display::None })
                    .font_size(config.ui.font_size() as f32 * 0.8)
                    .padding_left(6.0)
                    .padding_right(6.0)
                    .padding_top(2.0)
                    .padding_bottom(2.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .color(config.color(LapceColor::EDITOR_DIM))
            }),
        ))
        .style(|s| s.flex_direction(FlexDirection::Row).align_items(AlignItems::Center))
    )
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .padding(12.0)
            .border_radius(4.0)
            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
    })
}
