use serde::{Deserialize, Serialize};

/// Supported database types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DbType {
    Postgres,
    MongoDB,
}

impl std::fmt::Display for DbType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbType::Postgres => write!(f, "PostgreSQL"),
            DbType::MongoDB => write!(f, "MongoDB"),
        }
    }
}

/// A saved database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConnectionConfig {
    /// Unique identifier (UUID string)
    pub id: String,
    /// User-friendly display name
    pub name: String,
    /// Database type
    pub db_type: DbType,
    /// Hostname or IP address
    pub host: String,
    /// Port number (5432 for Postgres, 27017 for Mongo)
    pub port: u16,
    /// Username for authentication
    pub user: String,
    /// Password for authentication
    pub password: String,
    /// Database name (or auth database for Mongo)
    pub database: String,
    /// Optional color label for the connection
    pub color: Option<String>,
}

impl DbConnectionConfig {
    /// Build a connection URL from this config
    pub fn connection_url(&self) -> String {
        match self.db_type {
            DbType::Postgres => {
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    self.user, self.password, self.host, self.port, self.database
                )
            }
            DbType::MongoDB => {
                if self.user.is_empty() {
                    format!("mongodb://{}:{}/{}", self.host, self.port, self.database)
                } else {
                    format!(
                        "mongodb://{}:{}@{}:{}/{}",
                        self.user, self.password, self.host, self.port, self.database
                    )
                }
            }
        }
    }

    /// Default port for a given database type
    pub fn default_port(db_type: &DbType) -> u16 {
        match db_type {
            DbType::Postgres => 5432,
            DbType::MongoDB => 27017,
        }
    }
}

/// Schema information for a connected database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSchema {
    /// List of tables or collections
    pub tables: Vec<DbTableInfo>,
}

/// Information about a single table or collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTableInfo {
    /// Table or collection name
    pub name: String,
    /// Schema name (e.g. "public" for Postgres, None for Mongo)
    pub schema: Option<String>,
    /// Approximate row/document count, if available
    pub row_count: Option<u64>,
    /// Type: "table", "view", "materialized_view", "collection"
    pub table_type: String,
}

/// Information about a column in a table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbColumnInfo {
    /// Column name
    pub name: String,
    /// Data type as reported by the database (e.g. "int4", "varchar", "ObjectId")
    pub data_type: String,
    /// Whether the column is nullable
    pub nullable: bool,
    /// Whether this column is part of the primary key
    pub is_primary_key: bool,
    /// Default value expression, if any
    pub default_value: Option<String>,
}

/// Result of a database query or table data fetch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbQueryResult {
    /// Column metadata
    pub columns: Vec<DbColumnInfo>,
    /// Row data -- each row is a vector of JSON values matching column order
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Number of rows affected (for INSERT/UPDATE/DELETE)
    pub affected_rows: Option<u64>,
    /// Total row count in the table (for pagination)
    pub total_count: Option<u64>,
    /// Query execution time in milliseconds
    pub execution_time_ms: u64,
    /// Whether more rows are available beyond this page
    pub has_more: bool,
}

/// Structure information for a table (columns, indexes, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTableStructure {
    /// Table name
    pub table_name: String,
    /// Column details
    pub columns: Vec<DbColumnInfo>,
}
