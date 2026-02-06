use std::rc::Rc;

use floem::{
    View,
    reactive::SignalGet,
    views::{
        Decorators, container, label, scroll, stack, svg,
    },
};

use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    main_split::Editors,
    window_tab::CommonData,
};

/// Database Manager view for editor tab (opened as a tab like SDK Manager)
pub fn database_manager_view(
    _editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;
    
    container(
        scroll(
            container(
                stack((
                    // Header
                    stack((
                        // Icon
                        svg(move || config.get().ui_svg(LapceIcons::DATABASE)).style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32 * 2.5;
                            s.size(size, size)
                                .margin_right(16.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        }),
                        // Title and description
                        stack((
                            label(|| "Database Manager").style(move |s| {
                                let config = config.get();
                                s.font_size(config.ui.font_size() as f32 * 1.5)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            }),
                            label(|| "Manage database connections and queries").style(move |s| {
                                let config = config.get();
                                s.margin_top(4.0)
                                    .font_size(config.ui.font_size() as f32)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                        ))
                        .style(|s| s.flex_col()),
                    ))
                    .style(|s| s.items_center().margin_bottom(24.0)),
                    
                    // Placeholder content
                    container(
                        label(|| "Database Manager is under development. Stay tuned for database connection management features!")
                            .style(move |s| {
                                let config = config.get();
                                s.padding(20.0)
                                    .font_size(config.ui.font_size() as f32)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }),
                    )
                    .style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .border(1.0)
                            .border_radius(8.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    }),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.width_full().max_width(800.0).padding(32.0)),
        )
        .style(|s| s.width_full().height_full()),
    )
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .height_full()
            .items_center()
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}
