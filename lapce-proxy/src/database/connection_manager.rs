use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use lapce_core::directory::Directory;
use lapce_rpc::db::{
    DbConnectionConfig, DbQueryResult, DbSchema, DbTableStructure, DbType,
};

use super::DatabaseEngine;
use super::mongo_adapter::MongoAdapter;
use super::postgres_adapter::PostgresAdapter;

const CONNECTIONS_FILE: &str = "db_connections.json";

/// Manages database connections and configuration persistence.
pub struct ConnectionManager {
    /// Active database engine instances, keyed by connection ID
    active_connections: HashMap<String, Box<dyn DatabaseEngine>>,
    /// Shared tokio runtime for async database operations
    runtime: tokio::runtime::Runtime,
}

impl ConnectionManager {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for database manager");

        Self {
            active_connections: HashMap::new(),
            runtime,
        }
    }

    /// Get the path to the connections config file
    fn connections_file_path() -> Option<PathBuf> {
        Directory::config_directory().map(|dir| dir.join(CONNECTIONS_FILE))
    }

    /// Load all saved connection configs from disk
    pub fn load_connections() -> Result<Vec<DbConnectionConfig>> {
        let path = Self::connections_file_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)
            .context("Failed to read connections file")?;
        let connections: Vec<DbConnectionConfig> = serde_json::from_str(&content)
            .context("Failed to parse connections file")?;
        Ok(connections)
    }

    /// Save a connection config to disk (add or update)
    pub fn save_connection(config: DbConnectionConfig) -> Result<()> {
        let mut connections = Self::load_connections().unwrap_or_default();

        // Update existing or add new
        if let Some(existing) = connections.iter_mut().find(|c| c.id == config.id) {
            *existing = config;
        } else {
            connections.push(config);
        }

        Self::write_connections(&connections)
    }

    /// Delete a connection config from disk
    pub fn delete_connection(id: &str) -> Result<()> {
        let mut connections = Self::load_connections().unwrap_or_default();
        connections.retain(|c| c.id != id);
        Self::write_connections(&connections)
    }

    /// Write the full connections list to disk
    fn write_connections(connections: &[DbConnectionConfig]) -> Result<()> {
        let path = Self::connections_file_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        let content = serde_json::to_string_pretty(connections)
            .context("Failed to serialize connections")?;
        fs::write(&path, content).context("Failed to write connections file")?;
        Ok(())
    }

    /// Connect to a database using a saved connection config
    pub fn connect(&mut self, connection_id: &str) -> Result<DbSchema> {
        // Load config
        let connections = Self::load_connections()?;
        let config = connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not found: {}", connection_id))?
            .clone();

        // Disconnect existing connection with this ID if any
        self.disconnect(connection_id);

        // Create and connect the appropriate adapter
        let engine: Box<dyn DatabaseEngine> = match config.db_type {
            DbType::Postgres => {
                let adapter = PostgresAdapter::new(config);
                adapter.connect(&self.runtime)?;
                Box::new(adapter)
            }
            DbType::MongoDB => {
                let adapter = MongoAdapter::new(config);
                adapter.connect(&self.runtime)?;
                Box::new(adapter)
            }
        };

        // Fetch schema
        let schema = engine.get_schema(&self.runtime)?;

        // Store the active connection
        self.active_connections
            .insert(connection_id.to_string(), engine);

        Ok(schema)
    }

    /// Disconnect from a database
    pub fn disconnect(&mut self, connection_id: &str) {
        if let Some(engine) = self.active_connections.remove(connection_id) {
            engine.disconnect();
        }
    }

    /// Get schema for an active connection
    pub fn get_schema(&self, connection_id: &str) -> Result<DbSchema> {
        let engine = self
            .active_connections
            .get(connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not active: {}", connection_id))?;
        engine.get_schema(&self.runtime)
    }

    /// Fetch paginated table data
    pub fn get_table_data(
        &self,
        connection_id: &str,
        table: &str,
        offset: u64,
        limit: u64,
    ) -> Result<DbQueryResult> {
        let engine = self
            .active_connections
            .get(connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not active: {}", connection_id))?;
        engine.get_table_data(&self.runtime, table, offset, limit)
    }

    /// Get table/collection structure
    pub fn get_table_structure(
        &self,
        connection_id: &str,
        table: &str,
    ) -> Result<DbTableStructure> {
        let engine = self
            .active_connections
            .get(connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not active: {}", connection_id))?;
        engine.get_table_structure(&self.runtime, table)
    }

    /// Execute a raw query
    pub fn execute_query(
        &self,
        connection_id: &str,
        query: &str,
    ) -> Result<DbQueryResult> {
        let engine = self
            .active_connections
            .get(connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not active: {}", connection_id))?;
        engine.execute_query(&self.runtime, query)
    }

    /// Test a connection config without saving it
    pub fn test_connection(&self, config: &DbConnectionConfig) -> Result<bool> {
        let engine: Box<dyn DatabaseEngine> = match config.db_type {
            DbType::Postgres => Box::new(PostgresAdapter::new(config.clone())),
            DbType::MongoDB => Box::new(MongoAdapter::new(config.clone())),
        };
        engine.test_connection(&self.runtime)
    }
}
