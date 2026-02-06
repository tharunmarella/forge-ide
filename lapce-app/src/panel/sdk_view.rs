//! SDK Manager Panel View
//!
//! This panel provides UI for managing SDKs and toolchains via proto.

use std::rc::Rc;
use std::sync::Arc;

use floem::{
    View,
    reactive::ReadSignal,
    views::{Decorators, container, label},
};

use super::{position::PanelPosition, view::PanelBuilder};
use crate::{
    config::LapceConfig,
    window_tab::WindowTabData,
};

pub fn sdk_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let scope = window_tab_data.scope;
    
    PanelBuilder::new(config, position)
        .add(
            "SDK Status",
            simple_placeholder(),
            scope.create_rw_signal(true),
        )
        .build()
        .debug_name("SDK Panel")
}

/// Minimal placeholder view for testing
fn simple_placeholder() -> impl View {
    container(
        label(|| "SDK Manager - Coming Soon")
    )
    .style(|s| s.padding(10.0))
}
