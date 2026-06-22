use std::rc::Rc;
use std::sync::{Arc, Mutex};
use floem::{
    IntoView, View,
    views::{Decorators, container, label, stack, scroll, svg, Decorators as _, dyn_stack},
    reactive::{create_rw_signal, Scope, RwSignal, SignalGet, SignalUpdate},
    kurbo::Point,
    peniko::Color,
    style::CursorStyle,
    ext_event::create_ext_action,
};
use crate::{
    project_map::ProjectMapData,
    window_tab::WindowTabData,
    config::{color::LapceColor},
};
use super::position::PanelPosition;

fn spawn_map_fetch(
    scope: Scope,
    map_data: Arc<Mutex<ProjectMapData>>,
    workspace_id: String,
    base_url: String,
    token: Option<String>,
    focus_path: Option<String>,
    focus_symbol: Option<String>,
    loaded: RwSignal<bool>,
    loading: RwSignal<bool>,
    error_msg: RwSignal<Option<String>>,
    map_revision: RwSignal<u64>,
) {
    loading.set(true);
    let on_done = create_ext_action(
        scope,
        move |result: Result<ProjectMapData, String>| {
            loading.set(false);
            loaded.set(true);
            match result {
                Ok(data) => {
                    if let Ok(mut md) = map_data.lock() {
                        *md = data;
                    }
                    error_msg.set(None);
                    map_revision.update(|r| *r += 1);
                }
                Err(e) => {
                    error_msg.set(Some(format!("Error: {e}")));
                    map_revision.update(|r| *r += 1);
                }
            }
        },
    );
    std::thread::spawn(move || {
        let mut data = ProjectMapData::new(workspace_id, base_url, token);
        let result = data
            .fetch_map(focus_path, focus_symbol)
            .map(|_| data)
            .map_err(|e| e.to_string());
        on_done(result);
    });
}

pub fn project_map_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let workspace_id = window_tab_data.workspace.path.as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());
    
    let fs_client = forge_agent::forge_search::client();
    let base_url = fs_client.base_url().to_string();
    let scope = window_tab_data.common.scope;

    let map_data = Arc::new(Mutex::new(ProjectMapData::new(
        workspace_id.clone(),
        base_url.clone(),
        None,
    )));
    let config = window_tab_data.common.config;
    let loaded = create_rw_signal(false);
    let loading = create_rw_signal(true);
    let error_msg = create_rw_signal(None::<String>);
    /// Incremented after each fetch so Floem re-reads RefCell-backed map data.
    let map_revision = create_rw_signal(0u64);

    let workspace_id_hdr = workspace_id.clone();
    let base_url_hdr = base_url.clone();

    // Load token + initial map off the UI thread
    {
        let scope_init = scope;
        let map_data_init = map_data.clone();
        let ws_init = workspace_id.clone();
        let bu_init = base_url.clone();
        let on_token = create_ext_action(scope_init, move |token: Option<String>| {
            spawn_map_fetch(
                scope_init,
                map_data_init,
                ws_init,
                bu_init,
                token,
                None,
                None,
                loaded,
                loading,
                error_msg,
                map_revision,
            );
        });
        std::thread::spawn(move || {
            let token = match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.block_on(async { forge_agent::forge_search::client().token().await })
                }
                Err(_) => match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt.block_on(async {
                        forge_agent::forge_search::client().token().await
                    }),
                    Err(e) => {
                        tracing::error!(
                            "Failed to create tokio runtime for project map: {e}"
                        );
                        String::new()
                    }
                },
            };
            let token = if token.is_empty() { None } else { Some(token) };
            on_token(token);
        });
    }

    container(
        stack((
            // Header with breadcrumb navigation
            container(
                stack((
                    label(|| "Project Map".to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(config.ui.font_size() as f32)
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                    
                    // Breadcrumb navigation
                    {
                        let map_data_breadcrumb = map_data.clone();
                        container(
                            stack((
                                // Back button
                                label(|| "← Back".to_string())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size((config.ui.font_size() - 1) as f32)
                                            .color(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                            .padding_horiz(8.0)
                                            .padding_vert(4.0)
                                            .border_radius(3.0)
                                            .cursor(CursorStyle::Pointer)
                                            .hover(|s| s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND)))
                                    })
                                    .on_click_stop({
                                        let map_data_back = map_data_breadcrumb.clone();
                                        let scope_nav = scope;
                                        let ws_back = workspace_id_hdr.clone();
                                        let bu_back = base_url_hdr.clone();
                                        move |_| {
                                            spawn_map_fetch(
                                                scope_nav,
                                                map_data_back.clone(),
                                                ws_back.clone(),
                                                bu_back.clone(),
                                                None,
                                                None,
                                                None,
                                                loaded,
                                                loading,
                                                error_msg,
                                                map_revision,
                                            );
                                        }
                                    }),
                                
                                // Current level indicator
                                {
                                    let map_data_level = map_data_breadcrumb.clone();
                                    label(move || {
                                        let _rev = map_revision.get();
                                        let data = map_data_level.lock().ok();
                                        if let Some(data) = data.as_deref() {
                                            if let Some(response) = &data.response {
                                                if let Some(focus_path) = &response.focus_path {
                                                    format!("/ {}", focus_path)
                                                } else if let Some(focus_symbol) = &response.focus_symbol {
                                                    format!("/ {} (symbol)", focus_symbol)
                                                } else {
                                                    "/ Architecture Overview".to_string()
                                                }
                                            } else {
                                                String::new()
                                            }
                                        } else {
                                            String::new()
                                        }
                                    })
                                    .style(move |s| {
                                        let config = config.get();
                                        s.font_size((config.ui.font_size() - 1) as f32)
                                            .color(config.color(LapceColor::PANEL_FOREGROUND).with_alpha(0.7))
                                            .margin_left(8.0)
                                    })
                                }
                            ))
                            .style(|s| s.flex_row().items_center())
                        )
                        .style(|s| s.margin_top(4.0))
                    }
                ))
                .style(|s| s.flex_col().items_start())
            )
            .style(move |s| {
                let config = config.get();
                s.padding(10.0)
                    .width_pct(100.0)
                    .border_bottom(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            }),
            
            // Content
            container({
                let map_data = map_data.clone();
                
                stack((
                    // Loading state
                    container(
                        label(move || {
                            if loading.get() {
                                "Loading project map...".to_string()
                            } else {
                                String::new()
                            }
                        })
                            .style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32)
                                    .color(config.color(LapceColor::PANEL_FOREGROUND))
                            })
                    )
                    .style(move |s| {
                        s.padding(20.0)
                            .size_pct(100.0, 100.0)
                            .items_center()
                            .justify_center()
                            .apply_if(!loading.get(), |s| s.hide())
                    }),
                    
                    // Interactive Graph Canvas
                    container({
                        let map_data_canvas = map_data.clone();
                        
                        // Error message overlay
                        stack((
                            label(move || error_msg.get().unwrap_or_default())
                                .style(move |s| {
                                    s.color(config.get().color(LapceColor::ERROR_LENS_ERROR_FOREGROUND))
                                        .font_size(config.get().ui.font_size() as f32)
                                        .padding(20.0)
                                        .apply_if(error_msg.get().is_none(), |s| s.hide())
                                }),
                            
                            // Graph visualization
                            interactive_graph_view(
                                map_data_canvas,
                                config,
                                map_revision,
                                scope,
                                loaded,
                                loading,
                                error_msg,
                                Rc::new(workspace_id_hdr.clone()),
                                Rc::new(base_url_hdr.clone()),
                            )
                                .style(move |s| {
                                    s.size_pct(100.0, 100.0)
                                        .apply_if(error_msg.get().is_some(), |s| s.hide())
                                })
                        ))
                    })
                    .style(move |s| {
                        s.size_pct(100.0, 100.0)
                            .apply_if(loading.get() || !loaded.get(), |s| s.hide())
                    })
                ))
            })
            .style(|s| s.flex_grow(1.0).width_pct(100.0))
        ))
        .style(|s| s.flex_col().size_pct(100.0, 100.0))
    )
    .style(|s| s.size_pct(100.0, 100.0))
}

fn interactive_graph_view(
    map_data: Arc<Mutex<ProjectMapData>>,
    config: floem::reactive::ReadSignal<Arc<crate::config::LapceConfig>>,
    map_revision: floem::reactive::RwSignal<u64>,
    scope: floem::reactive::Scope,
    loaded: floem::reactive::RwSignal<bool>,
    loading: floem::reactive::RwSignal<bool>,
    error_msg: floem::reactive::RwSignal<Option<String>>,
    workspace_id: Rc<String>,
    base_url: Rc<String>,
) -> impl View {
    container(
        scroll(
            container(
                stack((
                    // Empty graph state
                    {
                        let map_data_empty_label = map_data.clone();
                        let map_data_empty_style = map_data.clone();
                        container(
                        label(move || {
                            if loaded.get() && !loading.get() {
                                let empty = map_data_empty_label
                                    .lock()
                                    .ok()
                                    .and_then(|data| {
                                        data.response
                                            .as_ref()
                                            .map(|response| response.nodes.is_empty())
                                    })
                                    .unwrap_or(true);
                                if empty {
                                    return "No project graph data available".to_string();
                                }
                            }
                            String::new()
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(config.ui.font_size() as f32)
                                .color(config.color(LapceColor::PANEL_FOREGROUND_DIM))
                        }),
                    )
                    .style(move |s| {
                        let show_empty = loaded.get()
                            && !loading.get()
                            && map_data_empty_style
                                .lock()
                                .ok()
                                .and_then(|data| {
                                    data.response
                                        .as_ref()
                                        .map(|response| response.nodes.is_empty())
                                })
                                .unwrap_or(true);
                        s.absolute()
                            .size_pct(100.0, 100.0)
                            .items_center()
                            .justify_center()
                            .apply_if(!show_empty, |s| s.hide())
                    })
                    },
                    // SVG for edges
                    svg({
                        let map_data = map_data.clone();
                        move || {
                            let _rev = map_revision.get();
                            let mut svg_content = String::new();
                            if let Ok(data) = map_data.lock() {
                                if let Some(response) = &data.response {
                                    for edge in &response.edges {
                                        if let (Some(from_pos), Some(to_pos)) = (
                                            data.node_positions.get(&edge.from_id),
                                            data.node_positions.get(&edge.to_id),
                                        ) {
                                            let color = match edge.r#type.as_str() {
                                                "DEPENDS_ON" => "#FF5722",
                                                "CALLS" => "#2196F3",
                                                "IMPORTS" => "#4CAF50",
                                                "BELONGS_TO" => "#9C27B0",
                                                "imports" => "#4CAF50",
                                                "calls" => "#2196F3",
                                                "contains" => "#FF9800",
                                                _ => "#757575",
                                            };
                                            svg_content.push_str(&format!(
                                                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1" opacity="0.5"/>"#,
                                                from_pos.x, from_pos.y, to_pos.x, to_pos.y, color
                                            ));
                                        }
                                    }
                                }
                            }
                            format!(
                                r#"<svg viewBox="0 0 800 600" width="800" height="600">{}</svg>"#,
                                svg_content
                            )
                        }
                    })
                    .style(|s| s.absolute().size_pct(100.0, 100.0)),
                    
                    // Nodes
                    dyn_stack(
                        {
                            let map_data = map_data.clone();
                            move || {
                                let _rev = map_revision.get();
                                map_data
                                    .lock()
                                    .ok()
                                    .and_then(|data| {
                                        data.response
                                            .as_ref()
                                            .map(|response| response.nodes.clone())
                                    })
                                    .unwrap_or_default()
                            }
                        },
                        |node| node.id.clone(),
                        {
                            let map_data = map_data.clone();
                            let scope_nav = scope;
                            let loaded_nav = loaded;
                            let loading_nav = loading;
                            let error_msg_nav = error_msg;
                            let map_revision_nav = map_revision;
                            let workspace_id_nav = workspace_id.clone();
                            let base_url_nav = base_url.clone();
                            move |node| {
                                let pos = map_data
                                    .lock()
                                    .ok()
                                    .and_then(|data| {
                                        data.node_positions.get(&node.id).cloned()
                                    })
                                    .unwrap_or(Point::ZERO);
                                let node_color = get_node_color(&node.kind);
                                let node_name = node.name.clone();
                                let node_kind = node.kind.clone();
                                let node_kind_click = node.kind.clone(); // Separate clone for click handler
                                let file_path = node.file_path.clone();
                                let map_data_click = map_data.clone();
                                let node_id = node.id.clone();
                                let node_id_click = node.id.clone(); // Separate clone for click handler
                                
                                container(
                                    stack((
                                        {
                                            let node_kind = node_kind.clone();
                                            label(move || {
                                                match node_kind.as_str() {
                                                    "architecture_layer" => "🏗️", // High-level architecture layers
                                                    "component" => "📦", // Architecture components
                                                    "service" => "🏢",
                                                    "file" => "📄",
                                                    "function" => "🔧",
                                                    "class" => "🏛️",
                                                    "module" => "📚",
                                                    "variable" => "🔤",
                                                    "struct" => "🏗️",
                                                    "enum" => "🔢",
                                                    "trait" => "⚡",
                                                    _ => "⚪",
                                                }
                                            })
                                        }.style(|s| s.font_size(14.0).margin_right(8.0)),
                                        stack((
                                            label(move || node_name.clone())
                                                .style(move |s| {
                                                    s.font_size(12.0)
                                                        .font_bold()
                                                        .color(Color::from_rgb8(220, 220, 220))
                                                }),
                                            {
                                                let map_data = map_data.clone();
                                                let node_id = node_id.clone();
                                                let node_kind = node_kind.clone();
                                                label(move || {
                                                    let _rev = map_revision.get();
                                                    map_data
                                                        .lock()
                                                        .ok()
                                                        .and_then(|data| {
                                                            data.response.as_ref().and_then(|r| {
                                                                r.nodes
                                                                    .iter()
                                                                    .find(|n| n.id == node_id)
                                                                    .and_then(|n| {
                                                                        if node_kind == "component" {
                                                                            Some(
                                                                                n.description
                                                                                    .clone()
                                                                                    .filter(|d| !d.is_empty())
                                                                                    .unwrap_or_else(
                                                                                        || {
                                                                                            "Architecture Component"
                                                                                                .to_string()
                                                                                        },
                                                                                    ),
                                                                            )
                                                                        } else {
                                                                            n.description.clone()
                                                                        }
                                                                    })
                                                            })
                                                        })
                                                        .unwrap_or_default()
                                                })
                                            }
                                            .style({
                                                let node_kind = node_kind.clone();
                                                move |s| {
                                                    s.font_size(10.0)
                                                        .color(Color::from_rgb8(150, 150, 150))
                                                        .apply_if(node_kind == "service", |s| s.font_size(11.0))
                                                }
                                            })
                                        ))
                                        .style(|s| s.flex_col().items_start())
                                    ))
                                    .style(|s| s.flex_row().items_center().justify_start())
                                )
                                .style(move |s| {
                                    let is_service = node_kind == "service";
                                    let is_component = node_kind == "component";
                                    let is_architecture_layer = node_kind == "architecture_layer";
                                    let is_large_node = is_component || is_service || is_architecture_layer;
                                    s.absolute()
                                        .inset_left(pos.x)
                                        .inset_top(pos.y)
                                        .min_width(if is_large_node { 300.0 } else { 200.0 })
                                        .min_height(if is_large_node { 70.0 } else { 35.0 })
                                        .background(node_color.with_alpha(0.1))
                                        .border_radius(if is_architecture_layer { 12.0 } else if is_component { 8.0 } else { 4.0 })
                                        .border(if is_architecture_layer { 3.0 } else if is_component { 2.0 } else { 1.0 })
                                        .border_color(node_color.with_alpha(if is_architecture_layer { 0.8 } else if is_component { 0.6 } else { 0.3 }))
                                        .padding_horiz(12.0)
                                        .padding_vert(8.0)
                                        .items_center()
                                        .cursor(CursorStyle::Pointer)
                                })
                                .on_click_stop({
                                    let ws_nav = workspace_id_nav.clone();
                                    let bu_nav = base_url_nav.clone();
                                    move |_| {
                                    let (focus_path, focus_symbol) =
                                        match node_kind_click.as_str() {
                                            "architecture_layer" | "component" => {
                                                (Some(node_id_click.clone()), None)
                                            }
                                            "file" => {
                                                (
                                                    file_path.clone(),
                                                    None,
                                                )
                                            }
                                            "function" | "class" | "struct" | "enum" => {
                                                (None, Some(node_id_click.clone()))
                                            }
                                            _ => {
                                                if let Some(path) = &file_path {
                                                    (Some(path.clone()), None)
                                                } else {
                                                    (Some(node_id_click.clone()), None)
                                                }
                                            }
                                        };
                                    spawn_map_fetch(
                                        scope_nav,
                                        map_data_click.clone(),
                                        ws_nav.as_ref().clone(),
                                        bu_nav.as_ref().clone(),
                                        None,
                                        focus_path.clone(),
                                        focus_symbol.clone(),
                                        loaded_nav,
                                        loading_nav,
                                        error_msg_nav,
                                        map_revision_nav,
                                    );
                                    }
                                })
                            }
                        }
                    )
                    .style(|s| s.size_pct(100.0, 100.0))
                ))
                .style(|s| s.min_width(800.0).min_height(600.0))
            )
        )
        .style(move |s| {
            s.size_pct(100.0, 100.0)
                .background(config.get().color(LapceColor::PANEL_BACKGROUND))
        })
    )
}


fn get_node_color(kind: &str) -> Color {
    match kind {
        "architecture_layer" => Color::from_rgb8(121, 85, 72),  // Brown for high-level architecture layers
        "component" => Color::from_rgb8(63, 81, 181),  // Indigo for architecture components
        "service" => Color::from_rgb8(255, 235, 59),   // Yellow for services
        "file" => Color::from_rgb8(76, 175, 80),      // Green for files
        "function" => Color::from_rgb8(33, 150, 243), // Blue for functions  
        "class" => Color::from_rgb8(255, 152, 0),     // Orange for classes
        "module" => Color::from_rgb8(156, 39, 176),   // Purple for modules
        "variable" => Color::from_rgb8(255, 193, 7),  // Yellow for variables
        "struct" => Color::from_rgb8(233, 30, 99),    // Pink for structs
        "enum" => Color::from_rgb8(103, 58, 183),     // Deep purple for enums
        "trait" => Color::from_rgb8(0, 188, 212),     // Cyan for traits
        _ => Color::from_rgb8(117, 117, 117),         // Gray for unknown
    }
}

fn get_edge_color(edge_type: &str) -> Color {
    match edge_type {
        "DEPENDS_ON" => Color::from_rgb8(255, 87, 34),  // Red-orange for component dependencies
        "PROVIDES_TO" => Color::from_rgb8(0, 188, 212), // Cyan
        "CALLS" => Color::from_rgb8(33, 150, 243),      // Blue for function calls
        "IMPORTS" => Color::from_rgb8(76, 175, 80),     // Green for imports
        "BELONGS_TO" => Color::from_rgb8(156, 39, 176), // Purple for belongs-to
        "imports" => Color::from_rgb8(76, 175, 80),     // Legacy green
        "calls" => Color::from_rgb8(33, 150, 243),      // Legacy blue
        "contains" | "CONTAINS" => Color::from_rgb8(255, 152, 0),  // Orange
        "references" => Color::from_rgb8(156, 39, 176), // Purple
        "implements" => Color::from_rgb8(233, 30, 99),  // Pink
        "extends" => Color::from_rgb8(103, 58, 183),    // Deep purple
        _ => Color::from_rgb8(117, 117, 117),           // Gray
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len.saturating_sub(3)])
    }
}
