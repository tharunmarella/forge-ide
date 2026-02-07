use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use sqlx::postgres::PgPool;
use sqlx::{Column, Row, TypeInfo};

use lapce_rpc::db::{
    DbColumnInfo, DbConnectionConfig, DbQueryResult, DbSchema, DbTableInfo,
    DbTableStructure,
};

use super::DatabaseEngine;

pub struct PostgresAdapter {
    pool: Arc<Mutex<Option<PgPool>>>,
    config: DbConnectionConfig,
}

impl PostgresAdapter {
    pub fn new(config: DbConnectionConfig) -> Self {
        Self {
            pool: Arc::new(Mutex::new(None)),
            config,
        }
    }

    pub fn connect(&self, rt: &tokio::runtime::Runtime) -> Result<()> {
        let url = self.config.connection_url();
        let pool = rt
            .block_on(async {
                PgPool::connect(&url).await
            })
            .context("Failed to connect to PostgreSQL")?;
        *self.pool.lock() = Some(pool);
        Ok(())
    }

    fn get_pool(&self) -> Result<PgPool> {
        self.pool
            .lock()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Not connected to PostgreSQL"))
    }
}

impl DatabaseEngine for PostgresAdapter {
    fn get_schema(&self, rt: &tokio::runtime::Runtime) -> Result<DbSchema> {
        let pool = self.get_pool()?;
        let tables = rt.block_on(async {
            let rows = sqlx::query(
                r#"
                SELECT 
                    t.table_schema,
                    t.table_name,
                    t.table_type,
                    (SELECT reltuples::bigint 
                     FROM pg_class 
                     WHERE oid = (quote_ident(t.table_schema) || '.' || quote_ident(t.table_name))::regclass
                    ) as row_estimate
                FROM information_schema.tables t
                WHERE t.table_schema NOT IN ('pg_catalog', 'information_schema')
                ORDER BY t.table_schema, t.table_name
                "#,
            )
            .fetch_all(&pool)
            .await?;

            let tables: Vec<DbTableInfo> = rows
                .iter()
                .map(|row| {
                    let schema: String = row.get("table_schema");
                    let name: String = row.get("table_name");
                    let table_type: String = row.get("table_type");
                    let row_count: Option<i64> = row.try_get("row_estimate").ok();

                    DbTableInfo {
                        name,
                        schema: Some(schema),
                        row_count: row_count.map(|c| c.max(0) as u64),
                        table_type: match table_type.as_str() {
                            "BASE TABLE" => "table".to_string(),
                            "VIEW" => "view".to_string(),
                            other => other.to_lowercase(),
                        },
                    }
                })
                .collect();

            Ok::<_, anyhow::Error>(tables)
        })?;

        Ok(DbSchema { tables })
    }

    fn get_table_data(
        &self,
        rt: &tokio::runtime::Runtime,
        table: &str,
        offset: u64,
        limit: u64,
    ) -> Result<DbQueryResult> {
        let pool = self.get_pool()?;
        let table_name = table.to_string();

        rt.block_on(async {
            let start = Instant::now();

            // Get total count
            let count_query = format!(
                "SELECT COUNT(*) as count FROM {}",
                quote_identifier(&table_name)
            );
            let count_row = sqlx::query(&count_query).fetch_one(&pool).await?;
            let total_count: i64 = count_row.get("count");

            // Get data
            let data_query = format!(
                "SELECT * FROM {} LIMIT {} OFFSET {}",
                quote_identifier(&table_name),
                limit,
                offset
            );
            let rows = sqlx::query(&data_query).fetch_all(&pool).await?;

            let execution_time_ms = start.elapsed().as_millis() as u64;

            // Build columns from first row metadata
            let columns = if let Some(first_row) = rows.first() {
                first_row
                    .columns()
                    .iter()
                    .map(|col| DbColumnInfo {
                        name: col.name().to_string(),
                        data_type: col.type_info().name().to_string(),
                        nullable: true, // simplified
                        is_primary_key: false,
                        default_value: None,
                    })
                    .collect()
            } else {
                Vec::new()
            };

            // Convert rows to JSON values
            let json_rows: Vec<Vec<serde_json::Value>> = rows
                .iter()
                .map(|row| pg_row_to_json(row))
                .collect();

            let has_more = (offset + limit) < total_count as u64;
            let row_count = json_rows.len() as u64;
            let _ = row_count; // suppress warning

            Ok(DbQueryResult {
                columns,
                rows: json_rows,
                affected_rows: None,
                total_count: Some(total_count as u64),
                execution_time_ms,
                has_more,
            })
        })
    }

    fn get_table_structure(
        &self,
        rt: &tokio::runtime::Runtime,
        table: &str,
    ) -> Result<DbTableStructure> {
        let pool = self.get_pool()?;
        let table_name = table.to_string();

        rt.block_on(async {
            let rows = sqlx::query(
                r#"
                SELECT 
                    c.column_name,
                    c.data_type,
                    c.is_nullable,
                    c.column_default,
                    CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END as is_pk
                FROM information_schema.columns c
                LEFT JOIN (
                    SELECT ku.column_name
                    FROM information_schema.table_constraints tc
                    JOIN information_schema.key_column_usage ku
                        ON tc.constraint_name = ku.constraint_name
                    WHERE tc.constraint_type = 'PRIMARY KEY'
                        AND tc.table_name = $1
                ) pk ON c.column_name = pk.column_name
                WHERE c.table_name = $1
                ORDER BY c.ordinal_position
                "#,
            )
            .bind(&table_name)
            .fetch_all(&pool)
            .await?;

            let columns = rows
                .iter()
                .map(|row| {
                    let name: String = row.get("column_name");
                    let data_type: String = row.get("data_type");
                    let nullable_str: String = row.get("is_nullable");
                    let default_value: Option<String> = row.try_get("column_default").ok();
                    let is_pk: bool = row.try_get("is_pk").unwrap_or(false);

                    DbColumnInfo {
                        name,
                        data_type,
                        nullable: nullable_str == "YES",
                        is_primary_key: is_pk,
                        default_value,
                    }
                })
                .collect();

            Ok(DbTableStructure {
                table_name,
                columns,
            })
        })
    }

    fn execute_query(
        &self,
        rt: &tokio::runtime::Runtime,
        query: &str,
    ) -> Result<DbQueryResult> {
        let pool = self.get_pool()?;
        let query_str = query.to_string();

        rt.block_on(async {
            let start = Instant::now();

            // Try to determine if this is a SELECT or a mutation
            let trimmed = query_str.trim().to_uppercase();
            if trimmed.starts_with("SELECT")
                || trimmed.starts_with("WITH")
                || trimmed.starts_with("SHOW")
                || trimmed.starts_with("EXPLAIN")
            {
                let rows = sqlx::query(&query_str).fetch_all(&pool).await?;
                let execution_time_ms = start.elapsed().as_millis() as u64;

                let columns = if let Some(first_row) = rows.first() {
                    first_row
                        .columns()
                        .iter()
                        .map(|col| DbColumnInfo {
                            name: col.name().to_string(),
                            data_type: col.type_info().name().to_string(),
                            nullable: true,
                            is_primary_key: false,
                            default_value: None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };

                let json_rows: Vec<Vec<serde_json::Value>> = rows
                    .iter()
                    .map(|row| pg_row_to_json(row))
                    .collect();

                let count = json_rows.len() as u64;
                Ok(DbQueryResult {
                    columns,
                    rows: json_rows,
                    affected_rows: None,
                    total_count: Some(count),
                    execution_time_ms,
                    has_more: false,
                })
            } else {
                // INSERT, UPDATE, DELETE, etc.
                let result = sqlx::query(&query_str).execute(&pool).await?;
                let execution_time_ms = start.elapsed().as_millis() as u64;

                Ok(DbQueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    affected_rows: Some(result.rows_affected()),
                    total_count: None,
                    execution_time_ms,
                    has_more: false,
                })
            }
        })
    }

    fn test_connection(&self, rt: &tokio::runtime::Runtime) -> Result<bool> {
        let url = self.config.connection_url();
        rt.block_on(async {
            let pool = PgPool::connect(&url).await?;
            sqlx::query("SELECT 1").fetch_one(&pool).await?;
            pool.close().await;
            Ok(true)
        })
    }

    fn disconnect(&self) {
        if let Some(pool) = self.pool.lock().take() {
            // PgPool closes when dropped, but we can explicitly close
            // This needs a runtime but dropping also works
            drop(pool);
        }
    }
}

/// Quote a PostgreSQL identifier to prevent SQL injection
fn quote_identifier(name: &str) -> String {
    // Simple quoting -- wrap in double quotes and escape any internal double quotes
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Convert a sqlx PgRow to a vector of serde_json::Value
fn pg_row_to_json(row: &sqlx::postgres::PgRow) -> Vec<serde_json::Value> {
    row.columns()
        .iter()
        .map(|col| {
            let type_name = col.type_info().name();
            // Try to get the value as various types, falling back gracefully
            match type_name {
                "BOOL" => row
                    .try_get::<bool, _>(col.ordinal())
                    .ok()
                    .map(serde_json::Value::Bool)
                    .unwrap_or(serde_json::Value::Null),
                "INT2" => row
                    .try_get::<i16, _>(col.ordinal())
                    .ok()
                    .map(|v| serde_json::Value::Number(v.into()))
                    .unwrap_or(serde_json::Value::Null),
                "INT4" => row
                    .try_get::<i32, _>(col.ordinal())
                    .ok()
                    .map(|v| serde_json::Value::Number(v.into()))
                    .unwrap_or(serde_json::Value::Null),
                "INT8" => row
                    .try_get::<i64, _>(col.ordinal())
                    .ok()
                    .map(|v| serde_json::Value::Number(v.into()))
                    .unwrap_or(serde_json::Value::Null),
                "FLOAT4" => row
                    .try_get::<f32, _>(col.ordinal())
                    .ok()
                    .and_then(|v| serde_json::Number::from_f64(v as f64))
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                "FLOAT8" | "NUMERIC" => row
                    .try_get::<f64, _>(col.ordinal())
                    .ok()
                    .and_then(|v| serde_json::Number::from_f64(v))
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                "JSON" | "JSONB" => row
                    .try_get::<serde_json::Value, _>(col.ordinal())
                    .ok()
                    .unwrap_or(serde_json::Value::Null),
                _ => {
                    // Default: try to get as String
                    row.try_get::<String, _>(col.ordinal())
                        .ok()
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null)
                }
            }
        })
        .collect()
}
