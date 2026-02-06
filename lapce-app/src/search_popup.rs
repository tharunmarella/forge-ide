//! Search popup - A modal/popup search dialog similar to VS Code's Ctrl+Shift+F
//!
//! This provides a centered popup window for global search instead of a side panel.

use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    peniko::Color,
    reactive::{SignalGet, SignalUpdate},
    style::{Display, Position},
    views::{Decorators, container, stack},
};

use crate::{
    app::clickable_icon,
    config::{color::LapceColor, icon::LapceIcons},
    panel::{global_search_view::global_search_panel, position::PanelPosition},
    window_tab::{Focus, WindowTabData},
};

/// Creates the search popup view - a centered modal dialog for global search
pub fn search_popup(window_tab_data: Rc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let focus = window_tab_data.common.focus;
    let config = window_tab_data.common.config;
    
    // Check if search popup should be visible (using dedicated SearchPopup focus)
    let is_visible = move || focus.get() == Focus::SearchPopup;
    
    // The popup container with backdrop
    container(
        stack((
            // Header bar with close button
            container(
                stack((
                    container(
                        label_text("Search in Files")
                    )
                    .style(move |s| {
                        s.flex_grow(1.0)
                            .font_weight(floem::text::Weight::SEMIBOLD)
                            .color(config.get().color(LapceColor::EDITOR_FOREGROUND))
                    }),
                    // Close button
                    clickable_icon(
                        || LapceIcons::CLOSE,
                        move || {
                            focus.set(Focus::Workbench);
                        },
                        || false,
                        || false,
                        || "Close (Escape)",
                        config,
                    ),
                ))
                .style(move |s| {
                    let config = config.get();
                    s.width_pct(100.0)
                        .padding(10.0)
                        .items_center()
                        .border_bottom(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                }),
            ),
            // Reuse the existing global search panel content
            container(
                global_search_panel(window_tab_data.clone(), PanelPosition::LeftTop)
            )
            .style(|s| s.flex_grow(1.0).width_pct(100.0).min_height(0.0)),
        ))
        .on_event_stop(EventListener::PointerDown, move |_| {
            // Prevent clicks on the popup from closing it
        })
        .on_event_stop(EventListener::KeyDown, move |event| {
            // Handle Escape to close
            if let floem::event::Event::KeyDown(key_event) = event {
                if key_event.key.logical_key == floem::keyboard::Key::Named(floem::keyboard::NamedKey::Escape) {
                    focus.set(Focus::Workbench);
                }
            }
        })
        .style(move |s| {
            let config = config.get();
            let rect = layout_rect.get();
            let width = (rect.width() * 0.65).min(900.0).max(500.0);
            let height = (rect.height() * 0.75).min(700.0).max(400.0);
            
            s.width(width)
                .height(height)
                .margin_top(40.0)
                .border(1.0)
                .border_radius(8.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .flex_col()
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .box_shadow_blur(25.0)
                .box_shadow_color(Color::BLACK.multiply_alpha(0.4))
                .pointer_events_auto()
        }),
    )
    .on_event_stop(EventListener::PointerDown, move |_| {
        // Click on backdrop closes the popup
        focus.set(Focus::Workbench);
    })
    .style(move |s| {
        s.display(if is_visible() {
            Display::Flex
        } else {
            Display::None
        })
        .position(Position::Absolute)
        .size_full()
        .flex_col()
        .items_center()
        .background(Color::BLACK.multiply_alpha(0.5))
        .pointer_events_auto()
    })
    .debug_name("Search Popup")
}

fn label_text(text: &'static str) -> impl View {
    floem::views::label(move || text.to_string())
}
