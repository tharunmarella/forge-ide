use std::rc::Rc;

use floem::{
    IntoView, View,
    event::{EventListener, EventPropagation},
    peniko::Color,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::{CursorStyle, Display, FlexDirection, Position},
    views::{
        Decorators, container, dyn_stack, empty, label, scroll, stack, svg, text,
        text_input,
    },
};

use crate::{
    config::{color::LapceColor, icon::LapceIcons},
    database::{ConnectionState, DatabaseViewData, DbViewMode},
    main_split::Editors,
    window_tab::CommonData,
};

use lapce_rpc::db::{DbConnectionConfig, DbQueryResult, DbType};

/// Main Database Manager view -- full TablePlus-like interface
pub fn database_manager_view(
    db_data: DatabaseViewData,
    _editors: Editors,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;

    stack((
        // Main layout: sidebar + content
        stack((
            // Left sidebar: connection tree
            connection_sidebar(db_data.clone(), common.clone()),
            // Main content area
            main_content_area(db_data.clone(), common.clone()),
        ))
        .style(|s| s.width_full().height_full().flex_row()),
        // Overlay: connection form dialog
        connection_form_overlay(db_data.clone(), common.clone()),
    ))
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .height_full()
            .background(config.color(LapceColor::PANEL_BACKGROUND))
    })
}

/// Left sidebar with connection list and tree view
fn connection_sidebar(db_data: DatabaseViewData, common: Rc<CommonData>) -> impl View {
    let config = common.config;
    let connections = db_data.connections;
    let db = db_data.clone();

    container(
        stack((
            // Header with title and add button
            stack((
                label(|| "Connections").style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .font_weight(floem::text::Weight::BOLD)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                        .flex_grow(1.0)
                }),
                // Add connection button
                {
                    let db = db_data.clone();
                    label(|| "+")
                        .style(move |s| {
                            let config = config.get();
                            s.font_size(config.ui.font_size() as f32 * 1.2)
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                .cursor(CursorStyle::Pointer)
                                .padding_horiz(8.0)
                                .padding_vert(2.0)
                                .border_radius(4.0)
                                .hover(|s| {
                                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                })
                        })
                        .on_click_stop(move |_| {
                            db.show_add_connection();
                        })
                },
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .padding(8.0)
                    .items_center()
                    .flex_row()
                    .border_bottom(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
            }),
            // Connection list
            scroll(
                {
                    let db = db_data.clone();
                    dyn_stack(
                        move || connections.get(),
                        move |conn: &ConnectionState| conn.config.id.clone(),
                        move |conn: ConnectionState| {
                            connection_tree_item(conn, db.clone(), config)
                        },
                    )
                    .style(|s| s.flex_col().width_full())
                },
            )
            .style(|s| s.width_full().flex_grow(1.0)),
        ))
        .style(|s| s.flex_col().width_full().height_full()),
    )
    .style(move |s| {
        let config = config.get();
        s.width(220.0)
            .height_full()
            .border_right(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
            .background(config.color(LapceColor::PANEL_BACKGROUND))
            .flex_shrink(0.0)
    })
}

/// Individual connection item in the sidebar tree
fn connection_tree_item(
    conn: ConnectionState,
    db_data: DatabaseViewData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let conn_id = conn.config.id.clone();
    let conn_name = conn.config.name.clone();
    let db_type = conn.config.db_type.clone();
    let db_type_icon = db_type.clone();
    let is_connected = conn.connected;
    let is_expanded = conn.expanded;
    let schema = conn.schema.clone();
    let config_clone = conn.config.clone();

    stack((
        // Connection header row
        {
            let db = db_data.clone();
            let cid = conn_id.clone();
            let cfg = config_clone.clone();
            stack((
                // Expand/collapse indicator
                label(move || if is_expanded { "▼" } else { "▶" }).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32 * 0.7)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .width(16.0)
                        .justify_center()
                }),
                // Database icon (type-specific)
                svg(move || {
                    let config = config.get();
                    match &db_type_icon {
                        DbType::Postgres => config.ui_svg(LapceIcons::DATABASE_POSTGRES),
                        DbType::MongoDB => config.ui_svg(LapceIcons::DATABASE_MONGODB),
                    }
                }).style(move |s| {
                    let config = config.get();
                    let color = if is_connected {
                        config.color(LapceColor::LAPCE_ICON_ACTIVE)
                    } else {
                        config.color(LapceColor::EDITOR_DIM)
                    };
                    s.size(14.0, 14.0).color(color).margin_right(6.0)
                }),
                // Connection name
                label(move || conn_name.clone()).style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                        .flex_grow(1.0)
                        .text_ellipsis()
                }),
                // Type badge
                label(move || match &db_type {
                    DbType::Postgres => "PG",
                    DbType::MongoDB => "MG",
                })
                .style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32 * 0.7)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .padding_horiz(4.0)
                }),
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .padding_vert(4.0)
                    .padding_horiz(8.0)
                    .items_center()
                    .flex_row()
                    .cursor(CursorStyle::Pointer)
                    .hover(|s| {
                        s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                    })
            })
            .on_click_stop(move |_| {
                if is_connected {
                    db.toggle_connection_expanded(&cid);
                } else {
                    db.connect(cid.clone());
                }
            })
        },
        // Tables/collections list (shown when expanded)
        {
            let db = db_data.clone();
            let cid = conn_id.clone();
            if is_expanded {
                if let Some(schema) = schema {
                    let tables = schema.tables;
                    container(
                        dyn_stack(
                            move || tables.clone(),
                            move |t| t.name.clone(),
                            move |table_info| {
                                let db = db.clone();
                                let cid = cid.clone();
                                let tname = table_info.name.clone();
                                let ttype = table_info.table_type.clone();
                                let row_count = table_info.row_count;

                                label(move || {
                                    let count_str = row_count
                                        .map(|c| format!(" ({})", c))
                                        .unwrap_or_default();
                                    format!("  {} {}{}", 
                                        if ttype == "collection" { "📁" } else { "📄" },
                                        tname, count_str)
                                })
                                .style(move |s| {
                                    let config = config.get();
                                    s.width_full()
                                        .padding_vert(3.0)
                                        .padding_left(32.0)
                                        .padding_right(8.0)
                                        .font_size(config.ui.font_size() as f32 * 0.9)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .cursor(CursorStyle::Pointer)
                                        .text_ellipsis()
                                        .hover(|s| {
                                            s.background(
                                                config.color(LapceColor::PANEL_HOVERED_BACKGROUND),
                                            )
                                        })
                                })
                                .on_click_stop({
                                    let db = db.clone();
                                    let cid = cid.clone();
                                    let tname = table_info.name.clone();
                                    move |_| {
                                        db.load_table_data(cid.clone(), tname.clone());
                                    }
                                })
                            },
                        )
                        .style(|s| s.flex_col().width_full()),
                    )
                    .into_any()
                } else {
                    empty().into_any()
                }
            } else {
                empty().into_any()
            }
        },
    ))
    .style(|s| s.flex_col().width_full())
}

/// Main content area: shows table data, query results, or welcome screen
fn main_content_area(db_data: DatabaseViewData, common: Rc<CommonData>) -> impl View {
    let config = common.config;
    let view_mode = db_data.view_mode;
    let table_data = db_data.table_data;
    let loading = db_data.loading;
    let status_message = db_data.status_message;
    let query_text = db_data.query_text;
    let active_connection_id = db_data.active_connection_id;
    let db = db_data.clone();

    container(
        stack((
            // Toolbar area with query input + execute
            {
                let db = db_data.clone();
                stack((
                    // Query text input
                    text_input(query_text)
                        .placeholder("Enter SQL query or MongoDB filter JSON...")
                        .style(move |s| {
                            let config = config.get();
                            s.flex_grow(1.0)
                                .height(32.0)
                                .padding_horiz(8.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(config.color(LapceColor::LAPCE_BORDER))
                                .background(config.color(LapceColor::EDITOR_BACKGROUND))
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                .font_size(config.ui.font_size() as f32)
                        }),
                    // Execute button
                    label(|| "Run")
                        .style(move |s| {
                            let config = config.get();
                            s.padding_horiz(16.0)
                                .padding_vert(6.0)
                                .margin_left(8.0)
                                .border_radius(4.0)
                                .font_size(config.ui.font_size() as f32)
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                .background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                })
                        })
                        .on_click_stop(move |_| {
                            db.execute_query();
                        }),
                ))
                .style(move |s| {
                    let config = config.get();
                    s.width_full()
                        .padding(8.0)
                        .items_center()
                        .flex_row()
                        .border_bottom(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                })
            },
            // Status bar
            {
                let loading = loading;
                let status = status_message;
                label(move || {
                    if loading.get() {
                        "Loading...".to_string()
                    } else if let Some(msg) = status.get() {
                        msg
                    } else {
                        String::new()
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    s.width_full()
                        .padding_horiz(8.0)
                        .padding_vert(4.0)
                        .font_size(config.ui.font_size() as f32 * 0.85)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .border_bottom(1.0)
                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                })
            },
            // Data display area
            container(
                {
                    let db = db_data.clone();
                    dyn_stack(
                        move || {
                            let mode = view_mode.get();
                            vec![mode]
                        },
                        move |mode| format!("{:?}", mode),
                        move |mode| {
                            let db = db.clone();
                            match mode {
                                DbViewMode::ConnectionList => {
                                    welcome_view(config).into_any()
                                }
                                DbViewMode::TableData { .. } | DbViewMode::QueryResults { .. } => {
                                    data_grid_view(db.clone(), config).into_any()
                                }
                                DbViewMode::TableStructure { .. } => {
                                    structure_view(db.clone(), config).into_any()
                                }
                            }
                        },
                    )
                    .style(|s| s.width_full().height_full())
                },
            )
            .style(|s| s.width_full().flex_grow(1.0)),
            // Pagination bar
            {
                let db = db_data.clone();
                pagination_bar(db, config)
            },
        ))
        .style(|s| s.flex_col().width_full().height_full()),
    )
    .style(|s| s.flex_grow(1.0).height_full())
}

/// Welcome screen shown when no data is loaded
fn welcome_view(
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    container(
        stack((
            svg(move || config.get().ui_svg(LapceIcons::DATABASE)).style(move |s| {
                let config = config.get();
                s.size(48.0, 48.0)
                    .color(config.color(LapceColor::EDITOR_DIM))
                    .margin_bottom(16.0)
            }),
            label(|| "Database Manager").style(move |s| {
                let config = config.get();
                s.font_size(config.ui.font_size() as f32 * 1.5)
                    .font_weight(floem::text::Weight::BOLD)
                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    .margin_bottom(8.0)
            }),
            label(|| "Connect to a database from the sidebar to browse tables and run queries.")
                .style(move |s| {
                    let config = config.get();
                    s.font_size(config.ui.font_size() as f32)
                        .color(config.color(LapceColor::EDITOR_DIM))
                        .max_width(400.0)
                        .text_ellipsis()
                }),
        ))
        .style(|s| {
            s.flex_col()
                .items_center()
                .justify_center()
        }),
    )
    .style(|s| s.width_full().height_full().items_center().justify_center())
}

/// Data grid view showing query results or table data
fn data_grid_view(
    db_data: DatabaseViewData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let table_data = db_data.table_data;

    container(
        scroll(
            {
                dyn_stack(
                    move || {
                        let data = table_data.get();
                        vec![data]
                    },
                    move |data| {
                        data.as_ref()
                            .map(|d| d.columns.len())
                            .unwrap_or(0)
                    },
                    move |data| {
                        if let Some(result) = data {
                            data_table(result, config).into_any()
                        } else {
                            label(|| "No data").style(move |s| {
                                let config = config.get();
                                s.padding(20.0)
                                    .color(config.color(LapceColor::EDITOR_DIM))
                            }).into_any()
                        }
                    },
                )
                .style(|s| s.width_full().min_width_full())
            },
        )
        .style(|s| s.width_full().height_full()),
    )
    .style(|s| s.width_full().height_full())
}

/// Renders the actual data table with header and rows
fn data_table(
    result: DbQueryResult,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let columns = result.columns.clone();
    let rows = result.rows.clone();
    let col_count = columns.len();

    stack((
        // Header row
        {
            let cols = columns.clone();
            container(
                dyn_stack(
                    move || cols.clone().into_iter().enumerate().collect::<Vec<_>>(),
                    move |&(i, _)| i,
                    move |(i, col)| {
                        let name = col.name.clone();
                        let dtype = col.data_type.clone();
                        label(move || format!("{}\n{}", name, dtype))
                            .style(move |s| {
                                let config = config.get();
                                s.min_width(120.0)
                                    .max_width(300.0)
                                    .padding_horiz(8.0)
                                    .padding_vert(6.0)
                                    .font_size(config.ui.font_size() as f32 * 0.85)
                                    .font_weight(floem::text::Weight::BOLD)
                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                    .border_right(1.0)
                                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                            })
                    },
                )
                .style(|s| s.flex_row()),
            )
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .border_bottom(2.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
            })
        },
        // Data rows
        {
            dyn_stack(
                move || rows.clone().into_iter().enumerate().collect::<Vec<_>>(),
                move |&(i, _)| i,
                move |(row_idx, row)| {
                    let is_even = row_idx % 2 == 0;
                    container(
                        dyn_stack(
                            move || row.clone().into_iter().enumerate().collect::<Vec<_>>(),
                            move |&(i, _)| i,
                            move |(_, val)| {
                                let display_val = json_value_to_display(&val);
                                label(move || display_val.clone())
                                    .style(move |s| {
                                        let config = config.get();
                                        s.min_width(120.0)
                                            .max_width(300.0)
                                            .padding_horiz(8.0)
                                            .padding_vert(4.0)
                                            .font_size(config.ui.font_size() as f32 * 0.85)
                                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                            .border_right(1.0)
                                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                                            .text_ellipsis()
                                    })
                            },
                        )
                        .style(|s| s.flex_row()),
                    )
                    .style(move |s| {
                        let config = config.get();
                        let bg = if is_even {
                            config.color(LapceColor::EDITOR_BACKGROUND)
                        } else {
                            config.color(LapceColor::PANEL_BACKGROUND)
                        };
                        s.width_full()
                            .border_bottom(1.0)
                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                            .background(bg)
                            .hover(|s| {
                                s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                            })
                    })
                },
            )
            .style(|s| s.flex_col().width_full())
        },
    ))
    .style(|s| s.flex_col().width_full())
}

/// Table structure view
fn structure_view(
    db_data: DatabaseViewData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let table_structure = db_data.table_structure;

    container(
        scroll(
            dyn_stack(
                move || {
                    let structure = table_structure.get();
                    vec![structure]
                },
                move |s| {
                    s.as_ref()
                        .map(|s| s.columns.len())
                        .unwrap_or(0)
                },
                move |structure| {
                    if let Some(structure) = structure {
                        let cols = structure.columns;
                        stack((
                            // Title
                            label(move || format!("Structure: {}", structure.table_name))
                                .style(move |s| {
                                    let config = config.get();
                                    s.font_size(config.ui.font_size() as f32 * 1.1)
                                        .font_weight(floem::text::Weight::BOLD)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .padding(12.0)
                                }),
                            // Column list
                            dyn_stack(
                                move || cols.clone(),
                                move |col| col.name.clone(),
                                move |col| {
                                    let name = col.name.clone();
                                    let dtype = col.data_type.clone();
                                    let nullable = col.nullable;
                                    let is_pk = col.is_primary_key;
                                    let default = col.default_value.clone();

                                    stack((
                                        label(move || {
                                            if is_pk { "🔑" } else { "  " }
                                        })
                                        .style(move |s| {
                                            let config = config.get();
                                            s.width(24.0)
                                                .font_size(config.ui.font_size() as f32 * 0.85)
                                        }),
                                        label(move || name.clone())
                                            .style(move |s| {
                                                let config = config.get();
                                                s.min_width(150.0)
                                                    .font_size(config.ui.font_size() as f32)
                                                    .font_weight(floem::text::Weight::BOLD)
                                                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                            }),
                                        label(move || dtype.clone())
                                            .style(move |s| {
                                                let config = config.get();
                                                s.min_width(120.0)
                                                    .font_size(config.ui.font_size() as f32 * 0.9)
                                                    .color(config.color(LapceColor::EDITOR_DIM))
                                            }),
                                        label(move || {
                                            if nullable { "NULL" } else { "NOT NULL" }
                                        })
                                        .style(move |s| {
                                            let config = config.get();
                                            s.min_width(80.0)
                                                .font_size(config.ui.font_size() as f32 * 0.85)
                                                .color(config.color(LapceColor::EDITOR_DIM))
                                        }),
                                        label(move || {
                                            default
                                                .clone()
                                                .unwrap_or_default()
                                        })
                                        .style(move |s| {
                                            let config = config.get();
                                            s.font_size(config.ui.font_size() as f32 * 0.85)
                                                .color(config.color(LapceColor::EDITOR_DIM))
                                        }),
                                    ))
                                    .style(move |s| {
                                        let config = config.get();
                                        s.width_full()
                                            .flex_row()
                                            .items_center()
                                            .padding_horiz(12.0)
                                            .padding_vert(4.0)
                                            .border_bottom(1.0)
                                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                                    })
                                },
                            )
                            .style(|s| s.flex_col().width_full()),
                        ))
                        .style(|s| s.flex_col().width_full())
                        .into_any()
                    } else {
                        label(|| "No structure data").style(move |s| {
                            let config = config.get();
                            s.padding(20.0)
                                .color(config.color(LapceColor::EDITOR_DIM))
                        }).into_any()
                    }
                },
            )
            .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.width_full().height_full()),
    )
    .style(|s| s.width_full().height_full())
}

/// Pagination bar at the bottom
fn pagination_bar(
    db_data: DatabaseViewData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    let table_data = db_data.table_data;
    let page_offset = db_data.page_offset;
    let page_size = db_data.page_size;
    let db_prev = db_data.clone();
    let db_next = db_data.clone();

    stack((
        // Previous button
        label(|| "< Previous")
            .style(move |s| {
                let config = config.get();
                let offset = page_offset.get();
                s.padding_horiz(12.0)
                    .padding_vert(4.0)
                    .font_size(config.ui.font_size() as f32 * 0.85)
                    .color(if offset > 0 {
                        config.color(LapceColor::EDITOR_FOREGROUND)
                    } else {
                        config.color(LapceColor::EDITOR_DIM)
                    })
                    .cursor(if offset > 0 {
                        CursorStyle::Pointer
                    } else {
                        CursorStyle::Default
                    })
            })
            .on_click_stop(move |_| {
                db_prev.prev_page();
            }),
        // Page info
        label(move || {
            let data = table_data.get();
            let offset = page_offset.get();
            let size = page_size.get();
            if let Some(result) = data {
                let total = result.total_count.unwrap_or(0);
                let end = std::cmp::min(offset + size, total);
                format!("Rows {}-{} of {}", offset + 1, end, total)
            } else {
                String::new()
            }
        })
        .style(move |s| {
            let config = config.get();
            s.flex_grow(1.0)
                .justify_center()
                .font_size(config.ui.font_size() as f32 * 0.85)
                .color(config.color(LapceColor::EDITOR_DIM))
        }),
        // Next button
        label(|| "Next >")
            .style(move |s| {
                let config = config.get();
                let has_more = table_data
                    .get()
                    .map(|d| d.has_more)
                    .unwrap_or(false);
                s.padding_horiz(12.0)
                    .padding_vert(4.0)
                    .font_size(config.ui.font_size() as f32 * 0.85)
                    .color(if has_more {
                        config.color(LapceColor::EDITOR_FOREGROUND)
                    } else {
                        config.color(LapceColor::EDITOR_DIM)
                    })
                    .cursor(if has_more {
                        CursorStyle::Pointer
                    } else {
                        CursorStyle::Default
                    })
            })
            .on_click_stop(move |_| {
                db_next.next_page();
            }),
    ))
    .style(move |s| {
        let config = config.get();
        s.width_full()
            .padding_vert(4.0)
            .padding_horiz(8.0)
            .items_center()
            .flex_row()
            .border_top(1.0)
            .border_color(config.color(LapceColor::LAPCE_BORDER))
    })
}

/// Connection form overlay dialog
fn connection_form_overlay(
    db_data: DatabaseViewData,
    common: Rc<CommonData>,
) -> impl View {
    let config = common.config;
    let show_form = db_data.show_connection_form;
    let editing = db_data.editing_connection;
    let db = db_data.clone();

    // Form state signals
    let scope = db_data.scope;
    let form_connection_string = scope.create_rw_signal(String::new());
    let form_name = scope.create_rw_signal(String::new());
    let form_host = scope.create_rw_signal("localhost".to_string());
    let form_port = scope.create_rw_signal("5432".to_string());
    let form_user = scope.create_rw_signal(String::new());
    let form_password = scope.create_rw_signal(String::new());
    let form_database = scope.create_rw_signal(String::new());
    let form_db_type = scope.create_rw_signal(DbType::Postgres);

    // Parse connection string and populate fields
    {
        let form_connection_string = form_connection_string;
        let form_db_type = form_db_type;
        let form_host = form_host;
        let form_port = form_port;
        let form_user = form_user;
        let form_password = form_password;
        let form_database = form_database;
        
        scope.create_effect(move |_| {
            let conn_str = form_connection_string.get();
            if conn_str.is_empty() {
                return;
            }
            
            if let Some(parsed) = parse_connection_string(&conn_str) {
                form_db_type.set(parsed.db_type);
                form_host.set(parsed.host);
                form_port.set(parsed.port.to_string());
                form_user.set(parsed.user);
                form_password.set(parsed.password);
                form_database.set(parsed.database);
            }
        });
    }

    // Sync form state when editing changes
    {
        let editing = editing;
        scope.create_effect(move |_| {
            if let Some(cfg) = editing.get() {
                form_name.set(cfg.name);
                form_host.set(cfg.host);
                form_port.set(cfg.port.to_string());
                form_user.set(cfg.user);
                form_password.set(cfg.password);
                form_database.set(cfg.database);
                form_db_type.set(cfg.db_type);
            }
        });
    }

    container(
        stack((
            // Backdrop
            empty()
                .style(move |s| {
                    s.position(Position::Absolute)
                        .inset(0.0)
                        .background(Color::from_rgba8(0, 0, 0, 128))
                        .z_index(0)
                })
                .on_click_stop({
                    let db = db.clone();
                    move |_| {
                        db.hide_connection_form();
                    }
                }),
            // Dialog box
            container(
                stack((
                    // Title
                    label(move || {
                        if editing.get().map(|e| e.name.is_empty()).unwrap_or(true) {
                            "New Connection".to_string()
                        } else {
                            "Edit Connection".to_string()
                        }
                    })
                    .style(move |s| {
                        let config = config.get();
                        s.font_size(config.ui.font_size() as f32 * 1.3)
                            .font_weight(floem::text::Weight::BOLD)
                            .color(config.color(LapceColor::EDITOR_FOREGROUND))
                            .margin_bottom(16.0)
                    }),
                    // DB Type selector
                    stack((
                        label(|| "Type:").style(move |s| {
                            let config = config.get();
                            s.width(100.0)
                                .font_size(config.ui.font_size() as f32)
                                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                        }),
                        // PostgreSQL button
                        {
                            stack((
                                svg(move || config.get().ui_svg(LapceIcons::DATABASE_POSTGRES))
                                    .style(|s| s.size(32.0, 32.0)),
                                // Grayscale overlay for unselected state
                                empty().style(move |s| {
                                    let selected = form_db_type.get() == DbType::Postgres;
                                    s.position(Position::Absolute)
                                        .inset(0.0)
                                        .background(Color::from_rgba8(200, 200, 200, 180))
                                        .apply_if(selected, |s| s.hide())
                                }),
                            ))
                            .style(move |s| {
                                let config = config.get();
                                let selected = form_db_type.get() == DbType::Postgres;
                                s.size(48.0, 48.0)
                                    .margin_right(8.0)
                                    .border_radius(4.0)
                                    .cursor(CursorStyle::Pointer)
                                    .items_center()
                                    .justify_center()
                                    .apply_if(selected, |s| {
                                        s.background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                    })
                                    .apply_if(!selected, |s| {
                                        s.border(1.0)
                                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                                    })
                            })
                            .on_click_stop(move |_| {
                                form_db_type.set(DbType::Postgres);
                                form_port.set("5432".to_string());
                            })
                        },
                        // MongoDB button
                        {
                            stack((
                                svg(move || config.get().ui_svg(LapceIcons::DATABASE_MONGODB))
                                    .style(|s| s.size(32.0, 32.0)),
                                // Grayscale overlay for unselected state
                                empty().style(move |s| {
                                    let selected = form_db_type.get() == DbType::MongoDB;
                                    s.position(Position::Absolute)
                                        .inset(0.0)
                                        .background(Color::from_rgba8(200, 200, 200, 180))
                                        .apply_if(selected, |s| s.hide())
                                }),
                            ))
                            .style(move |s| {
                                let config = config.get();
                                let selected = form_db_type.get() == DbType::MongoDB;
                                s.size(48.0, 48.0)
                                    .border_radius(4.0)
                                    .cursor(CursorStyle::Pointer)
                                    .items_center()
                                    .justify_center()
                                    .apply_if(selected, |s| {
                                        s.background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                    })
                                    .apply_if(!selected, |s| {
                                        s.border(1.0)
                                            .border_color(config.color(LapceColor::LAPCE_BORDER))
                                    })
                            })
                            .on_click_stop(move |_| {
                                form_db_type.set(DbType::MongoDB);
                                form_port.set("27017".to_string());
                            })
                        },
                    ))
                    .style(|s| s.flex_row().items_center().margin_bottom(8.0)),
                    // Connection String field (optional - auto-fills other fields)
                    form_field("Connection String:", form_connection_string, "postgresql://user:pass@host:port/db", config),
                    // Divider text
                    container(
                        label(|| "— OR —")
                    )
                    .style(move |s| {
                        let config = config.get();
                        s.width_full()
                            .justify_center()
                            .font_size(config.ui.font_size() as f32 * 0.9)
                            .color(config.color(LapceColor::EDITOR_DIM))
                            .margin_vert(8.0)
                    }),
                    // Form fields
                    form_field("Name:", form_name, "My Database", config),
                    form_field("Host:", form_host, "localhost", config),
                    form_field("Port:", form_port, "5432", config),
                    form_field("User:", form_user, "postgres / admin", config),
                    form_field("Password:", form_password, "", config),
                    form_field("Database:", form_database, "mydb", config),
                    // Action buttons
                    stack((
                        // Test button
                        {
                            let db = db.clone();
                            label(|| "Test Connection")
                                .style(move |s| {
                                    let config = config.get();
                                    s.padding_horiz(16.0)
                                        .padding_vert(6.0)
                                        .margin_right(8.0)
                                        .border_radius(4.0)
                                        .border(1.0)
                                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                                        .font_size(config.ui.font_size() as f32)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .cursor(CursorStyle::Pointer)
                                        .hover(|s| {
                                            s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                        })
                                })
                                .on_click_stop(move |_| {
                                    let config = build_config(
                                        editing.get().map(|e| e.id).unwrap_or_default(),
                                        form_name.get(),
                                        form_db_type.get(),
                                        form_host.get(),
                                        form_port.get(),
                                        form_user.get(),
                                        form_password.get(),
                                        form_database.get(),
                                    );
                                    db.test_connection(config);
                                })
                        },
                        // Save button
                        {
                            let db = db.clone();
                            label(|| "Save")
                                .style(move |s| {
                                    let config = config.get();
                                    s.padding_horiz(16.0)
                                        .padding_vert(6.0)
                                        .margin_right(8.0)
                                        .border_radius(4.0)
                                        .font_size(config.ui.font_size() as f32)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .background(config.color(LapceColor::LAPCE_BUTTON_PRIMARY_BACKGROUND))
                                        .cursor(CursorStyle::Pointer)
                                        .hover(|s| {
                                            s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                        })
                                })
                                .on_click_stop(move |_| {
                                    let config = build_config(
                                        editing.get().map(|e| e.id).unwrap_or_default(),
                                        form_name.get(),
                                        form_db_type.get(),
                                        form_host.get(),
                                        form_port.get(),
                                        form_user.get(),
                                        form_password.get(),
                                        form_database.get(),
                                    );
                                    db.save_connection(config);
                                    db.hide_connection_form();
                                })
                        },
                        // Cancel button
                        {
                            let db = db.clone();
                            label(|| "Cancel")
                                .style(move |s| {
                                    let config = config.get();
                                    s.padding_horiz(16.0)
                                        .padding_vert(6.0)
                                        .border_radius(4.0)
                                        .border(1.0)
                                        .border_color(config.color(LapceColor::LAPCE_BORDER))
                                        .font_size(config.ui.font_size() as f32)
                                        .color(config.color(LapceColor::EDITOR_FOREGROUND))
                                        .cursor(CursorStyle::Pointer)
                                        .hover(|s| {
                                            s.background(config.color(LapceColor::PANEL_HOVERED_BACKGROUND))
                                        })
                                })
                                .on_click_stop(move |_| {
                                    db.hide_connection_form();
                                })
                        },
                    ))
                    .style(|s| {
                        s.flex_row()
                            .justify_end()
                            .margin_top(16.0)
                            .width_full()
                    }),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(move |s| {
                let config = config.get();
                s.width(450.0)
                    .padding(24.0)
                    .border_radius(8.0)
                    .background(config.color(LapceColor::PANEL_BACKGROUND))
                    .border(1.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .z_index(1)
            }),
        ))
        .style(|s| {
            s.width_full()
                .height_full()
                .items_center()
                .justify_center()
        }),
    )
    .style(move |s| {
        let show = show_form.get();
        s.position(Position::Absolute)
            .inset(0.0)
            .display(if show { Display::Flex } else { Display::None })
            .items_center()
            .justify_center()
    })
}

/// Helper to create a form field row
fn form_field(
    label_text: &'static str,
    signal: floem::reactive::RwSignal<String>,
    placeholder: &'static str,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::LapceConfig>>,
) -> impl View {
    stack((
        label(move || label_text).style(move |s| {
            let config = config.get();
            s.width(100.0)
                .font_size(config.ui.font_size() as f32)
                .color(config.color(LapceColor::EDITOR_FOREGROUND))
                .items_start()
                .padding_top(6.0)
        }),
        text_input(signal)
            .placeholder(placeholder)
            .keyboard_navigable()
            .style(move |s: floem::style::Style| {
                let config = config.get();
                s.flex_grow(1.0)
                    .min_height(32.0)
                    .padding_horiz(8.0)
                    .padding_vert(6.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.color(LapceColor::LAPCE_BORDER))
                    .background(config.color(LapceColor::EDITOR_BACKGROUND))
                    .color(config.color(LapceColor::EDITOR_FOREGROUND))
                    .font_size(config.ui.font_size() as f32)
                    .cursor(CursorStyle::Text)
                    .set(floem::style::CursorColor, config.color(LapceColor::TERMINAL_CURSOR))
            }),
    ))
    .style(|s| s.flex_row().items_start().margin_bottom(12.0).width_full())
}

/// Build a DbConnectionConfig from form values
fn build_config(
    id: String,
    name: String,
    db_type: DbType,
    host: String,
    port: String,
    user: String,
    password: String,
    database: String,
) -> DbConnectionConfig {
    let port = port.parse::<u16>().unwrap_or_else(|_| {
        DbConnectionConfig::default_port(&db_type)
    });
    DbConnectionConfig {
        id,
        name,
        db_type,
        host,
        port,
        user,
        password,
        database,
        color: None,
    }
}

/// Convert a JSON value to a display string
fn json_value_to_display(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            serde_json::to_string(arr).unwrap_or_else(|_| "[]".to_string())
        }
        serde_json::Value::Object(obj) => {
            serde_json::to_string(obj).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

/// Parse a connection string and extract database connection details
/// Supports PostgreSQL and MongoDB connection string formats
fn parse_connection_string(conn_str: &str) -> Option<ParsedConnection> {
    let conn_str = conn_str.trim();
    
    // Try PostgreSQL format: postgresql://[user[:password]@]host[:port][/database]
    if conn_str.starts_with("postgresql://") || conn_str.starts_with("postgres://") {
        return parse_postgres_connection_string(conn_str);
    }
    
    // Try MongoDB format: mongodb://[username:password@]host[:port][/database]
    if conn_str.starts_with("mongodb://") || conn_str.starts_with("mongodb+srv://") {
        return parse_mongodb_connection_string(conn_str);
    }
    
    None
}

struct ParsedConnection {
    db_type: DbType,
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

fn parse_postgres_connection_string(conn_str: &str) -> Option<ParsedConnection> {
    // Remove protocol prefix
    let without_protocol = conn_str
        .strip_prefix("postgresql://")
        .or_else(|| conn_str.strip_prefix("postgres://"))?;
    
    // Split by @ to separate credentials from host/db
    let (credentials, host_and_db) = if let Some(at_pos) = without_protocol.find('@') {
        let (creds, rest) = without_protocol.split_at(at_pos);
        (Some(creds), &rest[1..]) // Skip the @ character
    } else {
        (None, without_protocol)
    };
    
    // Parse credentials (user:password)
    let (user, password) = if let Some(creds) = credentials {
        if let Some(colon_pos) = creds.find(':') {
            let (u, p) = creds.split_at(colon_pos);
            (u.to_string(), p[1..].to_string()) // Skip the : character
        } else {
            (creds.to_string(), String::new())
        }
    } else {
        (String::new(), String::new())
    };
    
    // Split host/port from database
    let (host_port, database) = if let Some(slash_pos) = host_and_db.find('/') {
        let (hp, db) = host_and_db.split_at(slash_pos);
        (hp, db[1..].to_string()) // Skip the / character
    } else {
        (host_and_db, String::new())
    };
    
    // Parse host and port
    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let (h, p) = host_port.split_at(colon_pos);
        let port = p[1..].parse::<u16>().unwrap_or(5432);
        (h.to_string(), port)
    } else {
        (host_port.to_string(), 5432)
    };
    
    Some(ParsedConnection {
        db_type: DbType::Postgres,
        host,
        port,
        user,
        password,
        database,
    })
}

fn parse_mongodb_connection_string(conn_str: &str) -> Option<ParsedConnection> {
    // Remove protocol prefix
    let without_protocol = conn_str
        .strip_prefix("mongodb+srv://")
        .or_else(|| conn_str.strip_prefix("mongodb://"))?;
    
    // Split by @ to separate credentials from host/db
    let (credentials, host_and_db) = if let Some(at_pos) = without_protocol.find('@') {
        let (creds, rest) = without_protocol.split_at(at_pos);
        (Some(creds), &rest[1..]) // Skip the @ character
    } else {
        (None, without_protocol)
    };
    
    // Parse credentials (username:password)
    let (user, password) = if let Some(creds) = credentials {
        if let Some(colon_pos) = creds.find(':') {
            let (u, p) = creds.split_at(colon_pos);
            (u.to_string(), p[1..].to_string()) // Skip the : character
        } else {
            (creds.to_string(), String::new())
        }
    } else {
        (String::new(), String::new())
    };
    
    // Remove query parameters if present
    let host_and_db = if let Some(question_pos) = host_and_db.find('?') {
        &host_and_db[..question_pos]
    } else {
        host_and_db
    };
    
    // Split host/port from database
    let (host_port, database) = if let Some(slash_pos) = host_and_db.find('/') {
        let (hp, db) = host_and_db.split_at(slash_pos);
        (hp, db[1..].to_string()) // Skip the / character
    } else {
        (host_and_db, String::new())
    };
    
    // Parse host and port (MongoDB can have multiple hosts, just take the first one)
    let first_host = host_port.split(',').next().unwrap_or(host_port);
    let (host, port) = if let Some(colon_pos) = first_host.rfind(':') {
        let (h, p) = first_host.split_at(colon_pos);
        let port = p[1..].parse::<u16>().unwrap_or(27017);
        (h.to_string(), port)
    } else {
        (first_host.to_string(), 27017)
    };
    
    Some(ParsedConnection {
        db_type: DbType::MongoDB,
        host,
        port,
        user,
        password,
        database,
    })
}
