use std::collections::HashMap;
use std::sync::Arc;

use floem::ext_event::create_ext_action;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use lapce_rpc::db::{
    DbConnectionConfig, DbQueryResult, DbSchema, DbTableStructure, DbType,
};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

/// Represents the current active view in the database manager
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbViewMode {
    /// Showing the welcome/connection list screen
    ConnectionList,
    /// Showing table data for a specific table
    TableData {
        connection_id: String,
        table: String,
    },
    /// Showing query results
    QueryResults {
        connection_id: String,
    },
    /// Showing table structure
    TableStructure {
        connection_id: String,
        table: String,
    },
}

/// Connection state
#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub config: DbConnectionConfig,
    pub connected: bool,
    pub schema: Option<DbSchema>,
    /// Whether the schema tree is expanded in the sidebar
    pub expanded: bool,
}

/// Reactive state container for the Database Manager feature.
#[derive(Clone)]
pub struct DatabaseViewData {
    pub scope: Scope,
    /// All saved connections (loaded from config)
    pub connections: RwSignal<Vec<ConnectionState>>,
    /// Currently active connection ID
    pub active_connection_id: RwSignal<Option<String>>,
    /// Current view mode
    pub view_mode: RwSignal<DbViewMode>,
    /// Current table data being displayed
    pub table_data: RwSignal<Option<DbQueryResult>>,
    /// Current table structure being displayed
    pub table_structure: RwSignal<Option<DbTableStructure>>,
    /// Current query text in the query editor
    pub query_text: RwSignal<String>,
    /// Whether a query/operation is currently loading
    pub loading: RwSignal<bool>,
    /// Status/error message
    pub status_message: RwSignal<Option<String>>,
    /// Current pagination offset
    pub page_offset: RwSignal<u64>,
    /// Page size (rows per page)
    pub page_size: RwSignal<u64>,
    /// Currently selected table name
    pub selected_table: RwSignal<Option<String>>,
    /// Connection form: whether the add/edit dialog is showing
    pub show_connection_form: RwSignal<bool>,
    /// Connection form: the config being edited
    pub editing_connection: RwSignal<Option<DbConnectionConfig>>,
    /// Proxy RPC handler
    proxy: ProxyRpcHandler,
}

impl DatabaseViewData {
    pub fn new(cx: Scope, proxy: ProxyRpcHandler) -> Self {
        let data = Self {
            scope: cx,
            connections: cx.create_rw_signal(Vec::new()),
            active_connection_id: cx.create_rw_signal(None),
            view_mode: cx.create_rw_signal(DbViewMode::ConnectionList),
            table_data: cx.create_rw_signal(None),
            table_structure: cx.create_rw_signal(None),
            query_text: cx.create_rw_signal(String::new()),
            loading: cx.create_rw_signal(false),
            status_message: cx.create_rw_signal(None),
            page_offset: cx.create_rw_signal(0),
            page_size: cx.create_rw_signal(100),
            selected_table: cx.create_rw_signal(None),
            show_connection_form: cx.create_rw_signal(false),
            editing_connection: cx.create_rw_signal(None),
            proxy,
        };

        // Load saved connections on init
        data.load_connections();

        data
    }

    /// Load all saved connections from the proxy
    pub fn load_connections(&self) {
        let connections = self.connections;
        let send = create_ext_action(self.scope, move |result| {
            if let Ok(ProxyResponse::DbConnectionsListResponse {
                connections: conns,
            }) = result
            {
                connections.set(
                    conns
                        .into_iter()
                        .map(|config| ConnectionState {
                            config,
                            connected: false,
                            schema: None,
                            expanded: false,
                        })
                        .collect(),
                );
            }
        });
        self.proxy.db_list_connections(move |result| {
            send(result);
        });
    }

    /// Save a connection config (add or update)
    pub fn save_connection(&self, config: DbConnectionConfig) {
        let connections = self.connections;
        let send = create_ext_action(self.scope, move |result| {
            if let Ok(ProxyResponse::DbConnectionsListResponse {
                connections: conns,
            }) = result
            {
                connections.set(
                    conns
                        .into_iter()
                        .map(|config| ConnectionState {
                            config,
                            connected: false,
                            schema: None,
                            expanded: false,
                        })
                        .collect(),
                );
            }
        });
        self.proxy.db_save_connection(config, move |result| {
            send(result);
        });
    }

    /// Delete a connection
    pub fn delete_connection(&self, id: String) {
        let connections = self.connections;
        let active = self.active_connection_id;
        let view_mode = self.view_mode;
        let conn_id = id.clone();
        let send = create_ext_action(self.scope, move |result| {
            if let Ok(ProxyResponse::DbConnectionsListResponse {
                connections: conns,
            }) = result
            {
                connections.set(
                    conns
                        .into_iter()
                        .map(|config| ConnectionState {
                            config,
                            connected: false,
                            schema: None,
                            expanded: false,
                        })
                        .collect(),
                );
                // If we deleted the active connection, reset
                if active.get() == Some(conn_id.clone()) {
                    active.set(None);
                    view_mode.set(DbViewMode::ConnectionList);
                }
            }
        });
        self.proxy.db_delete_connection(id, move |result| {
            send(result);
        });
    }

    /// Test a connection config
    pub fn test_connection(&self, config: DbConnectionConfig) {
        let status = self.status_message;
        let loading = self.loading;
        loading.set(true);

        let send = create_ext_action(self.scope, move |result| {
            loading.set(false);
            if let Ok(ProxyResponse::DbTestConnectionResponse {
                success,
                message,
            }) = result
            {
                if success {
                    status.set(Some("Connection successful!".to_string()));
                } else {
                    status.set(Some(format!("Connection failed: {}", message)));
                }
            } else {
                status.set(Some("Connection test failed".to_string()));
            }
        });
        self.proxy.db_test_connection(config, move |result| {
            send(result);
        });
    }

    /// Connect to a database
    pub fn connect(&self, connection_id: String) {
        let connections = self.connections;
        let active = self.active_connection_id;
        let loading = self.loading;
        let status = self.status_message;
        let conn_id = connection_id.clone();

        loading.set(true);

        let send = create_ext_action(self.scope, move |result| {
            loading.set(false);
            if let Ok(ProxyResponse::DbConnectResponse {
                success,
                schema,
                message,
            }) = result
            {
                if success {
                    active.set(Some(conn_id.clone()));
                    // Update the connection state
                    connections.update(|conns| {
                        if let Some(conn) = conns.iter_mut().find(|c| c.config.id == conn_id) {
                            conn.connected = true;
                            conn.schema = schema;
                            conn.expanded = true;
                        }
                    });
                    status.set(Some("Connected".to_string()));
                } else {
                    status.set(Some(format!("Connection failed: {}", message)));
                }
            } else {
                status.set(Some("Connection failed".to_string()));
            }
        });
        self.proxy.db_connect(connection_id, move |result| {
            send(result);
        });
    }

    /// Disconnect from a database
    pub fn disconnect(&self, connection_id: String) {
        let connections = self.connections;
        let active = self.active_connection_id;
        let view_mode = self.view_mode;
        let conn_id = connection_id.clone();

        self.proxy.db_disconnect(
            connection_id,
            move |_result| {
                // Fire and forget
            },
        );

        // Update local state immediately
        connections.update(|conns| {
            if let Some(conn) = conns.iter_mut().find(|c| c.config.id == conn_id) {
                conn.connected = false;
                conn.schema = None;
                conn.expanded = false;
            }
        });
        if active.get() == Some(conn_id) {
            active.set(None);
            view_mode.set(DbViewMode::ConnectionList);
        }
    }

    /// Load table data
    pub fn load_table_data(&self, connection_id: String, table: String) {
        let table_data = self.table_data;
        let view_mode = self.view_mode;
        let loading = self.loading;
        let status = self.status_message;
        let selected_table = self.selected_table;
        let page_offset = self.page_offset;
        let page_size = self.page_size;

        let offset = page_offset.get();
        let limit = page_size.get();
        let conn_id = connection_id.clone();
        let tbl = table.clone();

        loading.set(true);
        selected_table.set(Some(table.clone()));

        let send = create_ext_action(self.scope, move |result| {
            loading.set(false);
            if let Ok(ProxyResponse::DbQueryResponse { result }) = result {
                table_data.set(Some(result));
                view_mode.set(DbViewMode::TableData {
                    connection_id: conn_id,
                    table: tbl,
                });
                status.set(None);
            } else {
                status.set(Some("Failed to load table data".to_string()));
            }
        });
        self.proxy.db_get_table_data(
            connection_id,
            table,
            offset,
            limit,
            move |result| {
                send(result);
            },
        );
    }

    /// Load table structure
    pub fn load_table_structure(&self, connection_id: String, table: String) {
        let table_structure = self.table_structure;
        let view_mode = self.view_mode;
        let loading = self.loading;
        let status = self.status_message;
        let conn_id = connection_id.clone();
        let tbl = table.clone();

        loading.set(true);

        let send = create_ext_action(self.scope, move |result| {
            loading.set(false);
            if let Ok(ProxyResponse::DbTableStructureResponse { structure }) = result {
                table_structure.set(Some(structure));
                view_mode.set(DbViewMode::TableStructure {
                    connection_id: conn_id,
                    table: tbl,
                });
                status.set(None);
            } else {
                status.set(Some("Failed to load table structure".to_string()));
            }
        });
        self.proxy.db_get_table_structure(
            connection_id,
            table,
            move |result| {
                send(result);
            },
        );
    }

    /// Execute a query
    pub fn execute_query(&self) {
        let query = self.query_text.get();
        let active_id = match self.active_connection_id.get() {
            Some(id) => id,
            None => {
                self.status_message
                    .set(Some("No active connection".to_string()));
                return;
            }
        };

        if query.trim().is_empty() {
            self.status_message
                .set(Some("Query is empty".to_string()));
            return;
        }

        let table_data = self.table_data;
        let view_mode = self.view_mode;
        let loading = self.loading;
        let status = self.status_message;
        let conn_id = active_id.clone();

        loading.set(true);

        let send = create_ext_action(self.scope, move |result| {
            loading.set(false);
            if let Ok(ProxyResponse::DbQueryResponse { result }) = result {
                let msg = if let Some(affected) = result.affected_rows {
                    Some(format!(
                        "{} rows affected ({} ms)",
                        affected, result.execution_time_ms
                    ))
                } else {
                    Some(format!(
                        "{} rows returned ({} ms)",
                        result.rows.len(),
                        result.execution_time_ms
                    ))
                };
                table_data.set(Some(result));
                view_mode.set(DbViewMode::QueryResults {
                    connection_id: conn_id,
                });
                status.set(msg);
            } else {
                status.set(Some("Query execution failed".to_string()));
            }
        });
        self.proxy.db_execute_query(
            active_id,
            query,
            move |result| {
                send(result);
            },
        );
    }

    /// Navigate to next page
    pub fn next_page(&self) {
        let current = self.page_offset.get();
        let size = self.page_size.get();
        self.page_offset.set(current + size);

        // Reload current table data
        if let DbViewMode::TableData {
            connection_id,
            table,
        } = self.view_mode.get()
        {
            self.load_table_data(connection_id, table);
        }
    }

    /// Navigate to previous page
    pub fn prev_page(&self) {
        let current = self.page_offset.get();
        let size = self.page_size.get();
        if current >= size {
            self.page_offset.set(current - size);
        } else {
            self.page_offset.set(0);
        }

        // Reload current table data
        if let DbViewMode::TableData {
            connection_id,
            table,
        } = self.view_mode.get()
        {
            self.load_table_data(connection_id, table);
        }
    }

    /// Toggle connection tree expansion
    pub fn toggle_connection_expanded(&self, connection_id: &str) {
        self.connections.update(|conns| {
            if let Some(conn) = conns.iter_mut().find(|c| c.config.id == connection_id) {
                conn.expanded = !conn.expanded;
            }
        });
    }

    /// Show the connection form for adding a new connection
    pub fn show_add_connection(&self) {
        self.editing_connection.set(Some(DbConnectionConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            db_type: DbType::Postgres,
            host: "localhost".to_string(),
            port: 5432,
            user: String::new(),
            password: String::new(),
            database: String::new(),
            color: None,
        }));
        self.show_connection_form.set(true);
    }

    /// Show the connection form for editing an existing connection
    pub fn show_edit_connection(&self, config: DbConnectionConfig) {
        self.editing_connection.set(Some(config));
        self.show_connection_form.set(true);
    }

    /// Hide the connection form
    pub fn hide_connection_form(&self) {
        self.show_connection_form.set(false);
        self.editing_connection.set(None);
    }
}
