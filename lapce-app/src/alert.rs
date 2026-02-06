use std::{
    fmt,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

use floem::{
    View,
    event::EventListener,
    reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};

use crate::{
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct AlertButton {
    pub text: String,
    pub action: Rc<dyn Fn()>,
}

impl fmt::Debug for AlertButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("AlertButton");
        s.field("text", &self.text);
        s.finish()
    }
}

#[derive(Clone)]
pub struct AlertBoxData {
    pub active: RwSignal<bool>,
    pub title: RwSignal<String>,
    pub msg: RwSignal<String>,
    pub buttons: RwSignal<Vec<AlertButton>>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl AlertBoxData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            title: cx.create_rw_signal("".to_string()),
            msg: cx.create_rw_signal("".to_string()),
            buttons: cx.create_rw_signal(Vec::new()),
            config: common.config,
        }
    }
}

pub fn alert_box(alert_data: AlertBoxData) -> impl View {
    let config = alert_data.config;
    let active = alert_data.active;
    let title = alert_data.title;
    let msg = alert_data.msg;
    let buttons = alert_data.buttons;
    let button_id = AtomicU64::new(0);

    container({
        container({
            stack((
                // Icon with circular background
                container(
                    svg(move || config.get().ui_svg(LapceIcons::WARNING)).style(
                        move |s| {
                            s.size(32.0, 32.0)
                                .color(config.get().color(LapceColor::LAPCE_WARN))
                        },
                    ),
                )
                .style(move |s| {
                    let config = config.get();
                    s.size(64.0, 64.0)
                        .items_center()
                        .justify_center()
                        .border_radius(32.0)
                        .background(
                            config
                                .color(LapceColor::LAPCE_WARN)
                                .multiply_alpha(0.15),
                        )
                }),
                // Title
                label(move || title.get()).style(move |s| {
                    let config = config.get();
                    s.margin_top(24.0)
                        .width_pct(100.0)
                        .justify_center()
                        .font_bold()
                        .font_size((config.ui.font_size() + 4) as f32)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                }),
                // Message
                label(move || msg.get()).style(move |s| {
                    let config = config.get();
                    s.width_pct(100.0)
                        .margin_top(12.0)
                        .justify_center()
                        .line_height(1.5)
                        .font_size(config.ui.font_size() as f32)
                        .color(
                            config
                                .color(LapceColor::EDITOR_FOREGROUND)
                                .multiply_alpha(0.7),
                        )
                }),
                // Spacer before buttons
                empty().style(|s| s.height(24.0)),
                // Action buttons
                dyn_stack(
                    move || {
                        let btns = buttons.get();
                        btns.into_iter().enumerate().collect::<Vec<_>>()
                    },
                    move |(_idx, _button)| {
                        button_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    },
                    move |(idx, button)| {
                        let is_primary = idx == 0;
                        label(move || button.text.clone())
                            .on_click_stop(move |_| {
                                (button.action)();
                            })
                            .style(move |s| {
                                let config = config.get();
                                let base = s
                                    .margin_top(8.0)
                                    .width_pct(100.0)
                                    .justify_center()
                                    .padding_vert(10.0)
                                    .font_size((config.ui.font_size() + 1) as f32)
                                    .border_radius(8.0)
                                    .cursor(CursorStyle::Pointer);

                                if is_primary {
                                    // Primary button - filled with accent color
                                    base.background(
                                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND),
                                    )
                                    .color(
                                        config.color(LapceColor::LAPCE_BUTTON_PRIMARY_FOREGROUND),
                                    )
                                    .font_bold()
                                    .hover(|s| {
                                        s.background(
                                            config
                                                .color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                                                .multiply_alpha(0.85),
                                        )
                                    })
                                    .active(|s| {
                                        s.background(
                                            config
                                                .color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND)
                                                .multiply_alpha(0.7),
                                        )
                                    })
                                } else {
                                    // Secondary button - outlined
                                    base.border(1.0)
                                        .border_color(
                                            config.color(LapceColor::LAPCE_BORDER),
                                        )
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .hover(|s| {
                                            s.background(
                                                config.color(
                                                    LapceColor::PANEL_HOVERED_BACKGROUND,
                                                ),
                                            )
                                            .border_color(
                                                config
                                                    .color(LapceColor::LAPCE_BORDER)
                                                    .multiply_alpha(1.5),
                                            )
                                        })
                                        .active(|s| {
                                            s.background(
                                                config.color(
                                                    LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND,
                                                ),
                                            )
                                        })
                                }
                            })
                    },
                )
                .style(|s| s.flex_col().width_pct(100.0)),
                // Cancel button - subtle text button
                label(|| "Cancel".to_string())
                    .on_click_stop(move |_| {
                        active.set(false);
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.margin_top(16.0)
                            .width_pct(100.0)
                            .justify_center()
                            .padding_vert(10.0)
                            .font_size((config.ui.font_size() + 1) as f32)
                            .border_radius(8.0)
                            .cursor(CursorStyle::Pointer)
                            .color(
                                config
                                    .color(LapceColor::EDITOR_FOREGROUND)
                                    .multiply_alpha(0.6),
                            )
                            .hover(|s| {
                                s.background(
                                    config
                                        .color(LapceColor::PANEL_HOVERED_BACKGROUND)
                                        .multiply_alpha(0.5),
                                )
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            })
                            .active(|s| {
                                s.background(
                                    config.color(LapceColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                                )
                            })
                    }),
            ))
            .style(|s| s.flex_col().items_center().width_pct(100.0))
        })
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            let config = config.get();
            s.padding(32.0)
                .width(340.0)
                .border_radius(16.0)
                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                .background(config.color(LapceColor::PANEL_BACKGROUND))
                .box_shadow_blur(40.0)
                .box_shadow_color(
                    config
                        .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                        .multiply_alpha(0.3),
                )
        })
    })
    .on_event_stop(EventListener::PointerDown, move |_| {})
    .style(move |s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .items_center()
            .justify_center()
            .apply_if(!active.get(), |s| s.hide())
            .background(
                config
                    .get()
                    .color(LapceColor::LAPCE_DROPDOWN_SHADOW)
                    .multiply_alpha(0.6),
            )
    })
    .debug_name("Alert Box")
}
