use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use mongodb::bson::{self, Bson, Document, doc};
use mongodb::options::ClientOptions;
use mongodb::Client;
use parking_lot::Mutex;

use lapce_rpc::db::{
    DbColumnInfo, DbConnectionConfig, DbQueryResult, DbSchema, DbTableInfo,
    DbTableStructure,
};

use super::DatabaseEngine;

pub struct MongoAdapter {
    client: Arc<Mutex<Option<Client>>>,
    config: DbConnectionConfig,
}

impl MongoAdapter {
    pub fn new(config: DbConnectionConfig) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            config,
        }
    }

    pub fn connect(&self, rt: &tokio::runtime::Runtime) -> Result<()> {
        let url = self.config.connection_url();
        let client = rt.block_on(async {
            let options = ClientOptions::parse(&url)
                .await
                .context("Failed to parse MongoDB connection string")?;
            Client::with_options(options).context("Failed to create MongoDB client")
        })?;
        *self.client.lock() = Some(client);
        Ok(())
    }

    fn get_client(&self) -> Result<Client> {
        self.client
            .lock()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Not connected to MongoDB"))
    }

    fn database_name(&self) -> &str {
        &self.config.database
    }
}

impl DatabaseEngine for MongoAdapter {
    fn get_schema(&self, rt: &tokio::runtime::Runtime) -> Result<DbSchema> {
        let client = self.get_client()?;
        let db_name = self.database_name().to_string();

        rt.block_on(async {
            let db = client.database(&db_name);
            let collection_names = db
                .list_collection_names()
                .await
                .context("Failed to list collections")?;

            let mut tables = Vec::new();
            for name in collection_names {
                // Try to get document count
                let collection = db.collection::<Document>(&name);
                let count = collection
                    .estimated_document_count()
                    .await
                    .ok();

                tables.push(DbTableInfo {
                    name,
                    schema: None,
                    row_count: count,
                    table_type: "collection".to_string(),
                });
            }

            tables.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(DbSchema { tables })
        })
    }

    fn get_table_data(
        &self,
        rt: &tokio::runtime::Runtime,
        table: &str,
        offset: u64,
        limit: u64,
    ) -> Result<DbQueryResult> {
        let client = self.get_client()?;
        let db_name = self.database_name().to_string();
        let collection_name = table.to_string();

        rt.block_on(async {
            let start = Instant::now();
            let db = client.database(&db_name);
            let collection = db.collection::<Document>(&collection_name);

            // Get total count
            let total_count = collection
                .estimated_document_count()
                .await
                .unwrap_or(0);

            // Fetch documents with pagination
            let mut cursor = collection
                .find(doc! {})
                .skip(offset)
                .limit(limit as i64)
                .await
                .context("Failed to query collection")?;

            let mut documents = Vec::new();
            while let Some(result) = cursor.next().await {
                match result {
                    Ok(doc) => documents.push(doc),
                    Err(e) => {
                        tracing::warn!("Error reading document: {}", e);
                    }
                }
            }

            let execution_time_ms = start.elapsed().as_millis() as u64;

            // Collect all unique keys across documents to build columns
            let mut all_keys = BTreeSet::new();
            // Always put _id first
            all_keys.insert("_id".to_string());
            for doc in &documents {
                for key in doc.keys() {
                    all_keys.insert(key.clone());
                }
            }

            let columns: Vec<DbColumnInfo> = all_keys
                .iter()
                .map(|key| DbColumnInfo {
                    name: key.clone(),
                    data_type: "bson".to_string(),
                    nullable: true,
                    is_primary_key: key == "_id",
                    default_value: None,
                })
                .collect();

            // Convert documents to rows (aligned to column order)
            let column_keys: Vec<&str> = all_keys.iter().map(|s| s.as_str()).collect();
            let rows: Vec<Vec<serde_json::Value>> = documents
                .iter()
                .map(|doc| {
                    column_keys
                        .iter()
                        .map(|key| {
                            doc.get(*key)
                                .map(bson_to_json)
                                .unwrap_or(serde_json::Value::Null)
                        })
                        .collect()
                })
                .collect();

            let has_more = (offset + limit) < total_count;

            Ok(DbQueryResult {
                columns,
                rows,
                affected_rows: None,
                total_count: Some(total_count),
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
        let client = self.get_client()?;
        let db_name = self.database_name().to_string();
        let collection_name = table.to_string();

        rt.block_on(async {
            let db = client.database(&db_name);
            let collection = db.collection::<Document>(&collection_name);

            // Sample a few documents to infer the schema
            let mut cursor = collection
                .find(doc! {})
                .limit(100)
                .await
                .context("Failed to sample collection")?;

            let mut field_types: std::collections::BTreeMap<String, BTreeSet<String>> =
                std::collections::BTreeMap::new();

            while let Some(result) = cursor.next().await {
                if let Ok(doc) = result {
                    for (key, value) in &doc {
                        let type_name = bson_type_name(value);
                        field_types
                            .entry(key.clone())
                            .or_default()
                            .insert(type_name);
                    }
                }
            }

            let columns = field_types
                .into_iter()
                .map(|(name, types)| {
                    let data_type = types.into_iter().collect::<Vec<_>>().join(" | ");
                    DbColumnInfo {
                        name: name.clone(),
                        data_type,
                        nullable: true,
                        is_primary_key: name == "_id",
                        default_value: None,
                    }
                })
                .collect();

            Ok(DbTableStructure {
                table_name: collection_name,
                columns,
            })
        })
    }

    fn execute_query(
        &self,
        rt: &tokio::runtime::Runtime,
        query: &str,
    ) -> Result<DbQueryResult> {
        let client = self.get_client()?;
        let db_name = self.database_name().to_string();

        // Parse the query as JSON. Expected format:
        // { "collection": "users", "filter": { "age": { "$gt": 25 } } }
        // or just a filter document if a table is already selected:
        // { "age": { "$gt": 25 } }
        let query_doc: Document = serde_json::from_str(query)
            .map(|v: serde_json::Value| {
                bson::to_document(&v).unwrap_or_default()
            })
            .unwrap_or_default();

        let collection_name = query_doc
            .get_str("collection")
            .unwrap_or("test")
            .to_string();
        let filter = query_doc
            .get_document("filter")
            .cloned()
            .unwrap_or_else(|_| {
                // If no "collection" key, treat the entire doc as a filter
                if query_doc.get("collection").is_none() {
                    query_doc.clone()
                } else {
                    Document::new()
                }
            });

        rt.block_on(async {
            let start = Instant::now();
            let db = client.database(&db_name);
            let collection = db.collection::<Document>(&collection_name);

            let mut cursor = collection
                .find(filter)
                .limit(1000) // Safety limit
                .await
                .context("Failed to execute MongoDB query")?;

            let mut documents = Vec::new();
            while let Some(result) = cursor.next().await {
                if let Ok(doc) = result {
                    documents.push(doc);
                }
            }

            let execution_time_ms = start.elapsed().as_millis() as u64;

            // Build columns from all documents
            let mut all_keys = BTreeSet::new();
            all_keys.insert("_id".to_string());
            for doc in &documents {
                for key in doc.keys() {
                    all_keys.insert(key.clone());
                }
            }

            let columns: Vec<DbColumnInfo> = all_keys
                .iter()
                .map(|key| DbColumnInfo {
                    name: key.clone(),
                    data_type: "bson".to_string(),
                    nullable: true,
                    is_primary_key: key == "_id",
                    default_value: None,
                })
                .collect();

            let column_keys: Vec<&str> = all_keys.iter().map(|s| s.as_str()).collect();
            let rows: Vec<Vec<serde_json::Value>> = documents
                .iter()
                .map(|doc| {
                    column_keys
                        .iter()
                        .map(|key| {
                            doc.get(*key)
                                .map(bson_to_json)
                                .unwrap_or(serde_json::Value::Null)
                        })
                        .collect()
                })
                .collect();

            let row_count = rows.len() as u64;

            Ok(DbQueryResult {
                columns,
                rows,
                affected_rows: None,
                total_count: Some(row_count),
                execution_time_ms,
                has_more: false,
            })
        })
    }

    fn test_connection(&self, rt: &tokio::runtime::Runtime) -> Result<bool> {
        let url = self.config.connection_url();
        rt.block_on(async {
            let options = ClientOptions::parse(&url)
                .await
                .context("Failed to parse MongoDB URI")?;
            let client =
                Client::with_options(options).context("Failed to create client")?;
            // Ping the database
            let db = client.database(&self.config.database);
            db.run_command(doc! { "ping": 1 })
                .await
                .context("Ping failed")?;
            Ok(true)
        })
    }

    fn disconnect(&self) {
        self.client.lock().take();
    }
}

/// Convert a BSON value to a serde_json::Value
fn bson_to_json(bson: &Bson) -> serde_json::Value {
    match bson {
        Bson::Null => serde_json::Value::Null,
        Bson::Boolean(b) => serde_json::Value::Bool(*b),
        Bson::Int32(i) => serde_json::Value::Number((*i).into()),
        Bson::Int64(i) => serde_json::Value::Number((*i).into()),
        Bson::Double(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Bson::String(s) => serde_json::Value::String(s.clone()),
        Bson::ObjectId(oid) => serde_json::Value::String(oid.to_hex()),
        Bson::DateTime(dt) => {
            serde_json::Value::String(dt.to_string())
        }
        Bson::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(bson_to_json).collect())
        }
        Bson::Document(doc) => {
            let map: serde_json::Map<String, serde_json::Value> = doc
                .iter()
                .map(|(k, v)| (k.clone(), bson_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        Bson::Binary(bin) => {
            serde_json::Value::String(format!("<binary {} bytes>", bin.bytes.len()))
        }
        Bson::RegularExpression(regex) => {
            serde_json::Value::String(format!("/{}/{}", regex.pattern, regex.options))
        }
        Bson::Timestamp(ts) => {
            serde_json::Value::String(format!("Timestamp({}, {})", ts.time, ts.increment))
        }
        _ => serde_json::Value::String(format!("{:?}", bson)),
    }
}

/// Get a human-readable type name for a BSON value
fn bson_type_name(value: &Bson) -> String {
    match value {
        Bson::Null => "null".to_string(),
        Bson::Boolean(_) => "bool".to_string(),
        Bson::Int32(_) => "int32".to_string(),
        Bson::Int64(_) => "int64".to_string(),
        Bson::Double(_) => "double".to_string(),
        Bson::String(_) => "string".to_string(),
        Bson::ObjectId(_) => "ObjectId".to_string(),
        Bson::DateTime(_) => "DateTime".to_string(),
        Bson::Array(_) => "Array".to_string(),
        Bson::Document(_) => "Document".to_string(),
        Bson::Binary(_) => "Binary".to_string(),
        Bson::RegularExpression(_) => "Regex".to_string(),
        Bson::Timestamp(_) => "Timestamp".to_string(),
        _ => "unknown".to_string(),
    }
}
