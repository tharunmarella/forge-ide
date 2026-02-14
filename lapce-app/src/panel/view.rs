use std::{rc::Rc, sync::Arc};

use floem::{
    AnyView, IntoView, View,
    event::{Event, EventListener, EventPropagation},
    kurbo::{Point, Size},
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::{CursorStyle, Style},
    taffy::AlignItems,
    unit::PxPctAuto,
    views::{
        Decorators, container, dyn_stack, empty, h_stack, label, stack,
        stack_from_iter, svg, tab, text,
    },
};

use super::{
    ai_chat_view::ai_chat_panel,
    debug_view::debug_panel,
    git_log_view::git_log_panel,
    global_search_view::global_search_panel,
    kind::PanelKind,
    plugin_view::plugin_panel,
    position::{PanelContainerPosition, PanelPosition},
    problem_view::problem_panel,
    project_map_view::project_map_panel,
    sdk_view::sdk_panel,
    source_control_view::source_control_panel,
    terminal_view::terminal_panel,
};
use crate::{
    app::{clickable_icon, clickable_icon_base},
    config::{LapceConfig, color::LapceColor, icon::LapceIcons},
    file_explorer::view::file_explorer_panel,
    panel::{
        call_hierarchy_view::show_hierarchy_panel, document_symbol::symbol_panel,
        implementation_view::implementation_panel,
        references_view::references_panel,
    },
    window_tab::{DragContent, WindowTabData},
};

pub fn foldable_panel_section(
    header: impl View + 'static,
    child: impl View + 'static,
    open: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    stack((
        h_stack((
            clickable_icon_base(
                move || {
                    if open.get() {
                        LapceIcons::PANEL_FOLD_DOWN
                    } else {
                        LapceIcons::PANEL_FOLD_UP
                    }
                },
                None::<Box<dyn Fn()>>,
                || false,
                || false,
                config,
            ),
            header.style(|s| s.align_items(AlignItems::Center).padding_left(3.0)),
        ))
        .style(move |s| {
            s.padding_horiz(10.0)
                .padding_vert(6.0)
                .width_pct(100.0)
                .cursor(CursorStyle::Pointer)
                .background(config.get().color(LapceColor::EDITOR_BACKGROUND))
        })
        .on_click_stop(move |_| {
            open.update(|open| *open = !*open);
        }),
        child.style(move |s| s.apply_if(!open.get(), |s| s.hide())),
    ))
}

/// A builder for creating a foldable panel out of sections
pub struct PanelBuilder {
    views: Vec<AnyView>,
    config: ReadSignal<Arc<LapceConfig>>,
    position: PanelPosition,
}
impl PanelBuilder {
    pub fn new(
        config: ReadSignal<Arc<LapceConfig>>,
        position: PanelPosition,
    ) -> Self {
        Self {
            views: Vec::new(),
            config,
            position,
        }
    }

    fn add_general(
        mut self,
        name: &'static str,
        height: Option<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        let position = self.position;
        let view = foldable_panel_section(
            text(name).style(move |s| s.selectable(false)),
            view,
            open,
            self.config,
        )
        .style(move |s| {
            let s = s.width_full().flex_col();
            // Use the manual height if given, otherwise if we're open behave flex,
            // otherwise, do nothing so that there's no height
            let s = if open.get() {
                if let Some(height) = height {
                    s.height(height)
                } else {
                    s.flex_grow(1.0).flex_basis(0.0)
                }
            } else if position.is_bottom() {
                s.flex_grow(0.3).flex_basis(0.0)
            } else {
                s
            };

            style(s)
        });
        self.views.push(view.into_any());
        self
    }

    /// Add a view to the panel
    pub fn add(
        self,
        name: &'static str,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(name, None, view, open, std::convert::identity)
    }

    /// Add a view to the panel with a custom style applied to the overall header+section-content
    pub fn add_style(
        self,
        name: &'static str,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        self.add_general(name, None, view, open, style)
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    pub fn add_height(
        self,
        name: &'static str,
        height: impl Into<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(
            name,
            Some(height.into()),
            view,
            open,
            std::convert::identity,
        )
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    /// and a custom style applied to the overall header+section-content
    pub fn add_height_style(
        self,
        name: &'static str,
        height: impl Into<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        self.add_general(name, Some(height.into()), view, open, style)
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    pub fn add_height_pct(
        self,
        name: &'static str,
        height: f64,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(
            name,
            Some(PxPctAuto::Pct(height)),
            view,
            open,
            std::convert::identity,
        )
    }

    /// Build the panel into a view
    pub fn build(self) -> impl View {
        stack_from_iter(self.views).style(move |s| {
            s.width_full()
                .apply_if(!self.position.is_bottom(), |s| s.flex_col())
        })
    }
}

pub fn panel_container_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelContainerPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let current_size = create_rw_signal(Size::ZERO);
    let available_size = window_tab_data.panel.available_size;
    let is_dragging_panel = move || {
        dragging
            .with(|d| d.as_ref().map(|d| d.is_panel()))
            .unwrap_or(false)
    };
    let drop_view = {
        let panel = panel.clone();
        move |position: PanelPosition| {
            let panel = panel.clone();
            let dragging_over = create_rw_signal(false);
            empty()
                .on_event(EventListener::DragEnter, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(true);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::DragLeave, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(false);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some(DragContent::Panel(kind)) = dragging.get_untracked()
                    {
                        dragging_over.set(false);
                        panel.move_panel_to_position(kind, &position);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .style(move |s| {
                    s.size_pct(100.0, 100.0).apply_if(dragging_over.get(), |s| {
                        s.background(
                            config
                                .get()
                                .color(LapceColor::EDITOR_DRAG_DROP_BACKGROUND),
                        )
                    })
                })
        }
    };

    let resize_drag_view = {
        let panel = panel.clone();
        let panel_size = panel.size;
        move |position: PanelContainerPosition| {
            panel.panel_info();
            let view = empty();
            let view_id = view.id();
            let drag_start: RwSignal<Option<Point>> = create_rw_signal(None);
            view.on_event_stop(EventListener::PointerDown, move |event| {
                view_id.request_active();
                if let Event::PointerDown(pointer_event) = event {
                    drag_start.set(Some(pointer_event.pos));
                }
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    if let Some(drag_start_point) = drag_start.get_untracked() {
                        let current_size = current_size.get_untracked();
                        let available_size = available_size.get_untracked();
                        match position {
                            PanelContainerPosition::Left => {
                                let new_size = current_size.width
                                    + pointer_event.pos.x
                                    - drag_start_point.x;
                                let current_panel_size = panel_size.get_untracked();
                                let new_size = new_size
                                    .max(150.0)
                                    .min(available_size.width - 150.0 - 150.0);
                                if new_size != current_panel_size.left {
                                    panel_size.update(|size| {
                                        size.left = new_size;
                                        size.right = size.right.min(
                                            available_size.width - new_size - 150.0,
                                        )
                                    })
                                }
                            }
                            PanelContainerPosition::Bottom => {
                                let new_size = current_size.height
                                    - (pointer_event.pos.y - drag_start_point.y);
                                let maximized = panel.panel_bottom_maximized(false);
                                if (maximized
                                    && new_size < available_size.height - 50.0)
                                    || (!maximized
                                        && new_size > available_size.height - 50.0)
                                {
                                    panel.toggle_bottom_maximize();
                                }

                                let new_size = new_size
                                    .max(100.0)
                                    .min(available_size.height - 100.0);
                                let current_size =
                                    panel_size.with_untracked(|s| s.bottom);
                                if current_size != new_size {
                                    panel_size.update(|size| {
                                        size.bottom = new_size;
                                    })
                                }
                            }
                            PanelContainerPosition::Right => {
                                let new_size = current_size.width
                                    - (pointer_event.pos.x - drag_start_point.x);
                                let current_panel_size = panel_size.get_untracked();
                                let new_size = new_size
                                    .max(150.0)
                                    .min(available_size.width - 150.0 - 150.0);
                                if new_size != current_panel_size.right {
                                    panel_size.update(|size| {
                                        size.right = new_size;
                                        size.left = size.left.min(
                                            available_size.width - new_size - 150.0,
                                        )
                                    })
                                }
                            }
                        }
                    }
                }
            })
            .on_event_stop(EventListener::PointerUp, move |_| {
                drag_start.set(None);
            })
            .style(move |s| {
                let is_dragging = drag_start.get().is_some();
                let current_size = current_size.get();
                let config = config.get();
                s.absolute()
                    .apply_if(position == PanelContainerPosition::Bottom, |s| {
                        s.width_pct(100.0).height(4.0).margin_top(-2.0)
                    })
                    .apply_if(position == PanelContainerPosition::Left, |s| {
                        s.width(4.0)
                            .margin_left(current_size.width as f32 - 2.0)
                            .height_pct(100.0)
                    })
                    .apply_if(position == PanelContainerPosition::Right, |s| {
                        s.width(4.0).margin_left(-2.0).height_pct(100.0)
                    })
                    .apply_if(is_dragging, |s| {
                        s.background(config.color(LapceColor::EDITOR_CARET))
                            .apply_if(
                                position == PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::RowResize),
                            )
                            .apply_if(
                                position != PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::ColResize),
                            )
                            .z_index(2)
                    })
                    .hover(|s| {
                        s.background(config.color(LapceColor::EDITOR_CARET))
                            .apply_if(
                                position == PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::RowResize),
                            )
                            .apply_if(
                                position != PanelContainerPosition::Bottom,
                                |s| s.cursor(CursorStyle::ColResize),
                            )
                            .z_index(2)
                    })
            })
        }
    };

    let is_bottom = position.is_bottom();
    
    // For left/right panels: icons on the edge, content fills the rest
    // For bottom panel: original stacked layout
    if !is_bottom {
        // Left/Right panels: horizontal layout [icons | content]
        stack((
            panel_picker(window_tab_data.clone(), position.first()),
            stack((
                panel_view(window_tab_data.clone(), position.first()),
                panel_view(window_tab_data.clone(), position.second()),
            )).style(|s| s.flex_col().flex_grow(1.0).height_pct(100.0)),
            resize_drag_view(position),
            stack((drop_view(position.first()), drop_view(position.second()))).style(
                move |s| {
                    let is_dragging_panel = is_dragging_panel();
                    s.absolute()
                        .size_pct(100.0, 100.0)
                        .flex_col()
                        .apply_if(!is_dragging_panel, |s| s.pointer_events_none())
                },
            ),
        ))
    } else {
        // Bottom panel layout
        stack((
            panel_picker(window_tab_data.clone(), position.first()),
            panel_view(window_tab_data.clone(), position.first()),
            panel_view(window_tab_data.clone(), position.second()),
            panel_picker(window_tab_data.clone(), position.second()),
            resize_drag_view(position),
            stack((drop_view(position.first()), drop_view(position.second()))).style(
                move |s| {
                    let is_dragging_panel = is_dragging_panel();
                    s.absolute()
                        .size_pct(100.0, 100.0)
                        .apply_if(!is_dragging_panel, |s| s.pointer_events_none())
                },
            ),
        ))
    }
    .on_resize(move |rect| {
        let size = rect.size();
        if size != current_size.get_untracked() {
            current_size.set(size);
        }
    })
    .style(move |s| {
        let size = panel.size.with(|s| match position {
            PanelContainerPosition::Left => s.left,
            PanelContainerPosition::Bottom => s.bottom,
            PanelContainerPosition::Right => s.right,
        });
        let is_maximized = panel.panel_bottom_maximized(true);
        let config = config.get();
        s.apply_if(!panel.is_container_shown(&position, true), |s| s.hide())
            .apply_if(position == PanelContainerPosition::Bottom, |s| {
                s.width_pct(100.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
                    .apply_if(!is_maximized, |s| {
                        s.border_top(2.0).height(size as f32)
                    })
                    .apply_if(is_maximized, |s| s.flex_grow(1.0))
            })
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.border_right(1.0)
                    .width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.border_left(1.0)
                    .width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .color(config.color(LapceColor::PANEL_FOREGROUND))
    })
    .debug_name(format!("{:?} Pannel Container View", position))
}

fn panel_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let is_bottom = position.is_bottom();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        active_fn,
        panels,
        |p| *p,
        move |kind| {
            let view = match kind {
                // Terminal and GitLog have their own headers, don't wrap them
                PanelKind::Terminal => {
                    terminal_panel(window_tab_data.clone()).into_any()
                }
                PanelKind::GitLog => {
                    git_log_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                // Left sidebar panels - no header needed
                PanelKind::FileExplorer => {
                    file_explorer_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::SourceControl => {
                    source_control_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::Plugin => {
                    plugin_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::SdkManager => {
                    sdk_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::DatabaseManager => {
                    // DatabaseManager opens as an editor tab, not a panel
                    // This case should not be reached, but provide a placeholder
                    floem::views::empty().into_any()
                }
                PanelKind::DocumentSymbol => {
                    symbol_panel(window_tab_data.clone(), position).into_any()
                }
                // Bottom panels that need headers with close buttons
                PanelKind::Search => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            global_search_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        global_search_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::Problem => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            problem_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        problem_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::Debug => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            debug_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        debug_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::CallHierarchy => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            show_hierarchy_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        show_hierarchy_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::References => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            references_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        references_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::Implementation => {
                    if is_bottom {
                        bottom_panel_with_header(
                            window_tab_data.clone(),
                            kind,
                            implementation_panel(window_tab_data.clone(), position),
                        ).into_any()
                    } else {
                        implementation_panel(window_tab_data.clone(), position).into_any()
                    }
                }
                PanelKind::AiChat => {
                    ai_chat_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::ProjectMap => {
                    project_map_panel(window_tab_data.clone(), position).into_any()
                }
            };
            view.style(|s| s.size_pct(100.0, 100.0))
        },
    )
    .style(move |s| {
        s.size_pct(100.0, 100.0).apply_if(
            !panel.is_position_shown(&position, true)
                || panel.is_position_empty(&position, true),
            |s| s.hide(),
        )
    })
}

pub fn panel_header(
    header: String,
    config: ReadSignal<Arc<LapceConfig>>,
) -> impl View {
    container(label(move || header.clone())).style(move |s| {
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .background(config.get().color(LapceColor::EDITOR_BACKGROUND))
    })
}

/// Creates a bottom panel with a header containing title and close button
fn bottom_panel_with_header(
    window_tab_data: Rc<WindowTabData>,
    kind: PanelKind,
    content: impl View + 'static,
) -> impl View {
    let config = window_tab_data.common.config;
    let panel = window_tab_data.panel.clone();
    let focus = window_tab_data.common.focus;
    
    let title = match kind {
        PanelKind::Terminal => "Terminal",
        PanelKind::Problem => "Problems",
        PanelKind::Debug => "Debug",
        PanelKind::Search => "Search",
        PanelKind::CallHierarchy => "Call Hierarchy",
        PanelKind::References => "References",
        PanelKind::Implementation => "Implementation",
        PanelKind::AiChat => "Forge AI",
        PanelKind::ProjectMap => "Project Map",
        _ => "Panel",
    };
    let icon = kind.svg_name();
    
    // Smaller sizes for bottom panel
    let bottom_icon_size = move || (config.get().ui.icon_size() as f32 - 2.0).max(12.0);
    let bottom_font_size = move || (config.get().ui.font_size() as f32 - 1.0).max(11.0);
    
    stack((
        // Header with title and close button
        container(
            stack((
                // Title with icon
                stack((
                    svg(move || config.get().ui_svg(icon))
                        .style(move |s| {
                            let config = config.get();
                            let icon_size = bottom_icon_size();
                            s.size(icon_size, icon_size)
                                .margin_right(6.0)
                                .color(config.color(LapceColor::LAPCE_ICON_ACTIVE))
                        }),
                    label(move || title.to_string())
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(bottom_font_size())
                                .font_bold()
                                .color(config.color(LapceColor::PANEL_FOREGROUND))
                        }),
                ))
                .style(|s| s.items_center().flex_grow(1.0)),
                // Close button
                clickable_icon(
                    || LapceIcons::CLOSE,
                    move || {
                        panel.hide_panel(&kind);
                        focus.set(crate::window_tab::Focus::Workbench);
                    },
                    || false,
                    || false,
                    || "Close",
                    config,
                )
                .style(|s| s.margin_right(4.0)),
            ))
            .style(|s| s.items_center().width_pct(100.0)),
        )
        .style(move |s| {
            let config = config.get();
            s.padding_horiz(8.0)
                .padding_vert(4.0)
                .width_pct(100.0)
                .border_bottom(1.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .background(config.color(LapceColor::PANEL_BACKGROUND))
        }),
        // Content
        content,
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

/// Helper to create a panel icon button
fn panel_icon_button(
    window_tab_data: Rc<WindowTabData>,
    p: PanelKind,
    position: PanelPosition,
    config: ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
    dragging: RwSignal<Option<DragContent>>,
) -> impl View {
    let is_bottom = position.is_bottom();
    let is_first = position.is_first();
    let tooltip = match p {
        PanelKind::Terminal => "Terminal",
        PanelKind::FileExplorer => "File Explorer",
        PanelKind::SourceControl => "Source Control",
        PanelKind::GitLog => "Git Log",
        PanelKind::Plugin => "Plugins",
        PanelKind::SdkManager => "SDK Manager",
        PanelKind::DatabaseManager => "Database Manager",
        PanelKind::Search => "Search",
        PanelKind::Problem => "Problems",
        PanelKind::Debug => "Debug",
        PanelKind::CallHierarchy => "Call Hierarchy",
        PanelKind::DocumentSymbol => "Document Symbol",
        PanelKind::References => "References",
        PanelKind::Implementation => "Implementation",
        PanelKind::AiChat => "Forge AI",
        PanelKind::ProjectMap => "Project Map",
    };
    let icon = p.svg_name();
    let is_active = {
        let window_tab_data = window_tab_data.clone();
        move || {
            // Check if this panel is active at its position
            if let Some((active_panel, shown)) = window_tab_data
                .panel
                .active_panel_at_position(&position, true)
            {
                shown && active_panel == p
            } else {
                false
            }
        }
    };
    container(stack((
        clickable_icon(
            || icon,
            move || {
                if p == PanelKind::Terminal {
                    // Terminal opens at bottom, not in left panel
                    window_tab_data.toggle_panel_visual_at_position(p, PanelPosition::BottomLeft);
                } else if p == PanelKind::DatabaseManager {
                    // DatabaseManager opens as editor tab, not as a panel
                    window_tab_data.main_split.open_database_manager();
                } else if p == PanelKind::ProjectMapPage {
                    // ProjectMapPage opens as editor tab, not as a panel
                    window_tab_data.main_split.open_project_map();
                } else {
                    window_tab_data.toggle_panel_visual(p);
                }
            },
            || false,
            || false,
            move || tooltip,
            config,
        )
        .draggable()
        .on_event_stop(EventListener::DragStart, move |_| {
            dragging.set(Some(DragContent::Panel(p)));
        })
        .on_event_stop(EventListener::DragEnd, move |_| {
            dragging.set(None);
        })
        .dragging_style(move |s| {
            let config = config.get();
            s.border(1.0)
                .border_radius(6.0)
                .border_color(config.color(LapceColor::LAPCE_BORDER))
                .padding(6.0)
                .background(
                    config
                        .color(LapceColor::PANEL_BACKGROUND)
                        .multiply_alpha(0.7),
                )
        })
        .style(|s| s.padding(1.0)),
        label(|| "".to_string()).style(move |s| {
            s.selectable(false)
                .pointer_events_none()
                .absolute()
                .size_pct(100.0, 100.0)
                .apply_if(!is_bottom && is_first, |s| s.margin_top(2.0))
                .apply_if(!is_bottom && !is_first, |s| s.margin_top(-2.0))
                .apply_if(is_bottom && is_first, |s| s.margin_left(-2.0))
                .apply_if(is_bottom && !is_first, |s| s.margin_left(2.0))
                .apply_if(is_active(), |s| {
                    s.apply_if(!is_bottom && is_first, |s| {
                        s.border_bottom(2.0)
                    })
                    .apply_if(!is_bottom && !is_first, |s| s.border_top(2.0))
                    .apply_if(is_bottom && is_first, |s| s.border_left(2.0))
                    .apply_if(is_bottom && !is_first, |s| {
                        s.border_right(2.0)
                    })
                })
                .border_color(
                    config
                        .get()
                        .color(LapceColor::LAPCE_TAB_ACTIVE_UNDERLINE),
                )
        }),
    )))
    .style(|s| s.padding(6.0))
}

fn panel_picker(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = panel.panels;
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let is_bottom = position.is_bottom();
    let is_first = position.is_first();
    
    // Clone for use in closures
    let window_tab_data_for_dyn = window_tab_data.clone();
    
    dyn_stack(
        move || {
            panel
                .panels
                .with(|panels| panels.get(&position).cloned().unwrap_or_default())
        },
        |p| *p,
        move |p| {
            panel_icon_button(
                window_tab_data_for_dyn.clone(),
                p,
                position,
                config,
                dragging,
            )
        },
    )
    .style(move |s| {
        let config = config.get();
        s.flex_col()
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .apply_if(
                panels.with(|p| {
                    p.get(&position).map(|p| p.is_empty()).unwrap_or(true)
                }),
                |s| s.hide(),
            )
            // Bottom panel icon borders
            .apply_if(is_bottom && is_first, |s| s.border_right(1.0))
            .apply_if(is_bottom && !is_first, |s| s.border_left(1.0))
            // Left/Right panel icon borders - add right border to separate from content
            .apply_if(!is_bottom && is_first, |s| s.border_right(1.0))
            .apply_if(!is_bottom && !is_first, |s| s.border_left(1.0))
    })
}
