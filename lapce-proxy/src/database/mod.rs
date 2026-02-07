pub mod connection_manager;
pub mod mongo_adapter;
pub mod postgres_adapter;

use anyhow::Result;
use lapce_rpc::db::{DbQueryResult, DbSchema, DbTableStructure};

/// Trait abstracting database operations for both SQL and NoSQL databases.
///
/// All methods take `&self` and a tokio `Runtime` reference. The runtime is used
/// to drive async database drivers (sqlx, mongodb) from within the proxy's
/// synchronous thread model via `runtime.block_on()`.
pub trait DatabaseEngine: Send + Sync {
    /// Retrieve the database schema (list of tables/collections).
    fn get_schema(&self, rt: &tokio::runtime::Runtime) -> Result<DbSchema>;

    /// Fetch paginated data from a specific table or collection.
    fn get_table_data(
        &self,
        rt: &tokio::runtime::Runtime,
        table: &str,
        offset: u64,
        limit: u64,
    ) -> Result<DbQueryResult>;

    /// Get structure information (columns, types) for a table.
    fn get_table_structure(
        &self,
        rt: &tokio::runtime::Runtime,
        table: &str,
    ) -> Result<DbTableStructure>;

    /// Execute a raw query string (SQL for Postgres, JSON filter for MongoDB).
    fn execute_query(
        &self,
        rt: &tokio::runtime::Runtime,
        query: &str,
    ) -> Result<DbQueryResult>;

    /// Test whether the connection is working.
    fn test_connection(&self, rt: &tokio::runtime::Runtime) -> Result<bool>;

    /// Disconnect and release resources.
    fn disconnect(&self);
}
