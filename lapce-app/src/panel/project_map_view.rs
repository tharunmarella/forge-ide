use std::rc::Rc;
use std::cell::RefCell;
use floem::{
    IntoView, View,
    views::{Decorators, container, label, stack, scroll, svg, Decorators as _, dyn_stack, empty},
    reactive::{create_rw_signal, SignalGet, SignalUpdate},
    event::{EventListener, EventPropagation},
    kurbo::{Point, Size},
    peniko::Color,
};
use crate::{
    project_map::ProjectMapData,
    window_tab::WindowTabData,
    config::{color::LapceColor},
};
use super::position::PanelPosition;

pub fn project_map_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let workspace_id = window_tab_data.workspace.path.as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());
    
    // TODO: Get actual base URL from config
    let base_url = "http://localhost:8080".to_string();
    
    let map_data = Rc::new(RefCell::new(ProjectMapData::new(workspace_id, base_url)));
    let config = window_tab_data.common.config;
    let loaded = create_rw_signal(false);

    container(
        stack((
            // Header
            container(
                label(|| "Project Map".to_string())
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(config.ui.font_size() as f32)
                            .font_bold()
                            .color(config.color(LapceColor::PANEL_FOREGROUND))
                    })
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
                
                container(
                    stack((
                        // Load button
                        container(
                            label(|| "Click to load project map".to_string())
                                .on_click_stop({
                                    let map_data = map_data.clone();
                                    let loaded = loaded.clone();
                                    move |_| {
                                        let mut data = map_data.borrow_mut();
                                        data.fetch_map(None, None);
                                        loaded.set(true);
                                    }
                                })
                                .style(|s| s.cursor(floem::style::CursorStyle::Pointer))
                        )
                        .style(move |s| {
                            s.padding(20.0)
                                .size_pct(100.0, 100.0)
                                .items_center()
                                .justify_center()
                                .apply_if(loaded.get(), |s| s.hide())
                        }),
                        
                        // Map view
                        container({
                            let map_data = map_data.clone();
                            label(move || {
                                let data = map_data.borrow();
                                if let Some(resp) = &data.response {
                                    format!("Nodes: {} | Edges: {}", resp.nodes.len(), resp.edges.len())
                                } else {
                                    "Loading...".to_string()
                                }
                            })
                        })
                        .style(move |s| {
                            s.padding(20.0)
                                .size_pct(100.0, 100.0)
                                .apply_if(!loaded.get(), |s| s.hide())
                        })
                    ))
                )
            })
            .style(|s| s.flex_grow(1.0).width_pct(100.0))
        ))
        .style(|s| s.flex_col().size_pct(100.0, 100.0))
    )
    .style(|s| s.size_pct(100.0, 100.0))
}
