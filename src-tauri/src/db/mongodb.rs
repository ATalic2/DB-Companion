//! db/mongodb.rs — MongoDB adapter via mongosh subprocess
//!
//! Instead of using the Rust mongodb driver (which has SCRAM auth issues on
//! Windows), we shell out to mongosh which is known to work reliably.
//! All commands are run as `mongosh <uri> --quiet --eval <js>` and the
//! output is parsed as JSON.

use super::{
    build_schema_text, ColumnMeta, DbError, IDatabaseAdapter, SchemaMetadata,
    StatementOutcome, TableMeta, TransactionResult,
};
use async_trait::async_trait;
use anyhow::Result;
use zeroize::Zeroizing;
use tokio::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub struct MongoAdapter {
    uri:     Zeroizing<String>,
    db_name: String,
}

impl MongoAdapter {
    pub async fn connect(uri: Zeroizing<String>, db_name: &str) -> Result<Self, DbError> {
        // Verify mongosh is available and can connect
        let out = mongosh_eval(&uri, db_name, "JSON.stringify(db.runCommand({ping:1}))").await?;
        let val: serde_json::Value = serde_json::from_str(&out)
            .map_err(|e| DbError::Connection(format!("Unexpected ping response: {e} — got: {out}")))?;
        if val.get("ok").and_then(|v| v.as_f64()).unwrap_or(0.0) != 1.0 {
            return Err(DbError::Connection(format!("Ping failed: {out}")));
        }
        Ok(Self { uri, db_name: db_name.to_string() })
    }
}

/// Run a JS expression in mongosh and return stdout as a String.
async fn mongosh_eval(uri: &str, _db_name: &str, js: &str) -> Result<String, DbError> {
    // Try mongosh first, fall back to mongo
    for bin in &["mongosh", "mongo"] {
        let mut cmd = Command::new(bin);
        cmd.args([
            uri,
            "--quiet",
            "--eval", js,
        ]);

        // On Windows, prevent a console window from flashing up every time
        // we shell out to mongosh/mongo.
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let result = cmd.output().await;

        match result {
            Ok(out) => {
                if out.status.success() {
                    return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    return Err(DbError::Connection(stderr));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(DbError::Connection(format!("Failed to run {bin}: {e}"))),
        }
    }
    Err(DbError::Connection(
        "mongosh not found. Please install it from https://www.mongodb.com/try/download/shell".into()
    ))
}

/// Run JS and parse the result as a JSON value.
async fn mongosh_json(uri: &str, db_name: &str, js: &str) -> Result<serde_json::Value, DbError> {
    let out = mongosh_eval(uri, db_name, js).await?;
    // mongosh sometimes emits extra lines before the JSON — find the first `{` or `[`
    let start = out.find(|c| c == '{' || c == '[').unwrap_or(0);
    let clean = &out[start..];
    serde_json::from_str(clean)
        .map_err(|e| DbError::Transaction(format!("Failed to parse mongosh output as JSON: {e}\nOutput was:\n{out}")))
}

#[async_trait]
impl IDatabaseAdapter for MongoAdapter {
    fn driver_name(&self) -> &str { "mongodb" }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        // Get collection names
        let js = "JSON.stringify(db.getCollectionNames())";
        let names_val = mongosh_json(&self.uri, &self.db_name, js).await?;
        let collection_names: Vec<String> = serde_json::from_value(names_val)
            .map_err(|e| DbError::Schema(e.to_string()))?;

        let mut tables = Vec::new();

        for coll_name in &collection_names {
            // Sample documents to infer fields
            let js = format!(
                "JSON.stringify(db.getCollection('{}').aggregate([{{$sample:{{size:20}}}}]).toArray())",
                coll_name.replace('\'', "\\'")
            );
            let docs_val = mongosh_json(&self.uri, &self.db_name, &js).await
                .unwrap_or(serde_json::Value::Array(vec![]));

            let mut seen    = std::collections::HashSet::new();
            let mut columns: Vec<ColumnMeta> = Vec::new();

            if let serde_json::Value::Array(docs) = docs_val {
                for doc in &docs {
                    if let serde_json::Value::Object(map) = doc {
                        for (key, val) in map {
                            if seen.insert(key.clone()) {
                                let is_pk = key == "_id";
                                columns.push(ColumnMeta {
                                    name:        key.clone(),
                                    data_type:   json_type_name(val).to_string(),
                                    nullable:    !is_pk,
                                    is_pk,
                                    is_fk:       false,
                                    fk:          None,
                                    default_val: None,
                                });
                            }
                        }
                    }
                }
            }

            columns.sort_by_key(|c| if c.is_pk { 0 } else { 1 });

            // Fetch real index definitions via getIndexes() — Mongo exposes
            // this as data (not a shell-only feature), so there's no reason
            // to leave it empty.
            let idx_js = format!(
                "JSON.stringify(db.getCollection('{}').getIndexes())",
                coll_name.replace('\'', "\\'")
            );
            let indexes: Vec<super::IndexMeta> = match mongosh_json(&self.uri, &self.db_name, &idx_js).await {
                Ok(serde_json::Value::Array(specs)) => specs.iter().filter_map(|spec| {
                    let obj = spec.as_object()?;
                    let name = obj.get("name")?.as_str()?.to_string();
                    let key_obj = obj.get("key")?.as_object()?;
                    let columns: Vec<String> = key_obj.keys().cloned().collect();
                    let unique = obj.get("unique").and_then(|v| v.as_bool()).unwrap_or(false);
                    let predicate = obj.get("partialFilterExpression").map(|v| v.to_string());

                    // Mongo key-spec values reveal special index kinds (text/geo/hashed);
                    // a plain 1/-1 value is a normal ascending/descending field index.
                    let special = key_obj.values().find_map(|v| v.as_str());
                    let index_type = match special {
                        Some("text")      => "text",
                        Some("2dsphere")  => "2dsphere",
                        Some("2d")        => "2d",
                        Some("hashed")    => "hashed",
                        _ if columns.len() > 1 => "compound",
                        _                       => "single",
                    };

                    Some(super::IndexMeta {
                        name, columns, unique,
                        index_type: Some(index_type.to_string()),
                        predicate,
                    })
                }).collect(),
                _ => vec![], // don't fail the whole schema fetch if index lookup has a hiccup
            };

            tables.push(TableMeta { name: coll_name.clone(), columns, indexes, constraints: vec![] });
        }

        let schema_text = build_schema_text(&tables, "mongodb");
        Ok(SchemaMetadata {
            db_name: self.db_name.clone(),
            db_type: "mongodb".into(),
            tables,
            schema_text,
        })
    }

    async fn execute_transaction(
        &self,
        commands: Vec<String>,
        dry_run:  bool,
    ) -> Result<TransactionResult, DbError> {
        if dry_run {
            let outcomes = commands.iter().map(|cmd| StatementOutcome {
                sql: cmd.clone(), rows_affected: 0, ok: true, error: None,
            }).collect();
            return Ok(TransactionResult {
                success: true, dry_run: true, rows_affected: 0,
                statements: outcomes, error: None,
            });
        }

        let mut outcomes   = Vec::new();
        let mut total_rows = 0u64;

        for cmd_str in &commands {
            // Wrap the JSON command in a db.runCommand() call
            let js = format!("JSON.stringify(db.runCommand({cmd_str}))");
            match mongosh_json(&self.uri, &self.db_name, &js).await {
                Ok(result) => {
                    let n = result.get("n").and_then(|v| v.as_u64()).unwrap_or(0);
                    total_rows += n;
                    outcomes.push(StatementOutcome {
                        sql: cmd_str.clone(), rows_affected: n, ok: true, error: None,
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    outcomes.push(StatementOutcome {
                        sql: cmd_str.clone(), rows_affected: 0, ok: false, error: Some(msg.clone()),
                    });
                    return Ok(TransactionResult {
                        success: false, dry_run, rows_affected: total_rows,
                        statements: outcomes, error: Some(msg),
                    });
                }
            }
        }

        Ok(TransactionResult {
            success: true, dry_run, rows_affected: total_rows,
            statements: outcomes, error: None,
        })
    }

    async fn run_query(&self, input: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        // Input can be either a JSON command doc or raw JS expression
        let js = if input.trim_start().starts_with('{') {
            format!("JSON.stringify(db.runCommand({input}))")
        } else {
            // Raw JS — wrap result in JSON.stringify
            format!("JSON.stringify({input})")
        };

        let result = mongosh_json(&self.uri, &self.db_name, &js).await?;

        // If result is an array of documents, expand into rows
        if let serde_json::Value::Array(docs) = &result {
            if docs.is_empty() {
                return Ok(crate::commands::query::QueryResult {
                    columns: vec![], rows: vec![], rows_affected: 0,
                    execution_ms: 0, error: None,
                capped: false,
            });
            }
            let columns: Vec<String> = if let serde_json::Value::Object(map) = &docs[0] {
                map.keys().cloned().collect()
            } else {
                vec!["value".into()]
            };
            let rows = docs.iter().map(|doc| {
                if let serde_json::Value::Object(map) = doc {
                    columns.iter().map(|k| map.get(k).cloned().unwrap_or(serde_json::Value::Null)).collect()
                } else {
                    vec![doc.clone()]
                }
            }).collect();
            return Ok(crate::commands::query::QueryResult {
                columns, rows, rows_affected: docs.len() as u64,
                execution_ms: 0, error: None,
                capped: false,
            });
        }

        // Single result document — flatten to one row
        if let serde_json::Value::Object(map) = &result {
            let columns: Vec<String> = map.keys().cloned().collect();
            let row: Vec<serde_json::Value> = map.values().cloned().collect();
            return Ok(crate::commands::query::QueryResult {
                columns, rows: vec![row], rows_affected: 1,
                execution_ms: 0, error: None,
                capped: false,
            });
        }

        Ok(crate::commands::query::QueryResult {
            columns: vec!["result".into()],
            rows: vec![vec![result]],
            rows_affected: 1,
            execution_ms: 0,
            error: None,
                capped: false,
            })
    }

    async fn run_script(&self, script: &str) -> Result<String, DbError> {
        // Strip a leading `use <dbname>;` line if present — the target database
        // is already fixed by the connection profile, so this is a no-op we
        // silently accept rather than reject (mongosh shell scripts commonly
        // start with one when pasted from setup docs).
        let use_re_stripped = {
            let trimmed = script.trim_start();
            if let Some(rest) = trimmed.strip_prefix("use ") {
                match rest.find(['\n', ';']) {
                    Some(idx) => rest[idx..].trim_start_matches(';').trim_start(),
                    None => "", // the whole script was just `use dbname`
                }
            } else {
                trimmed
            }
        };

        if use_re_stripped.trim().is_empty() {
            return Ok("(nothing to run — script only contained a `use` statement, \
                        which is unnecessary since the database is already set by \
                        the connection profile)".to_string());
        }

        // Run the whole script as-is via mongosh; it will print the return
        // value of each top-level statement, same as pasting it interactively.
        mongosh_eval(&self.uri, &self.db_name, use_re_stripped).await
    }
}

fn json_type_name(val: &serde_json::Value) -> &'static str {
    match val {
        serde_json::Value::Null      => "null",
        serde_json::Value::Bool(_)   => "bool",
        serde_json::Value::Number(n) =>
            if n.is_f64() { "double" } else { "int" },
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_)  => "array",
        serde_json::Value::Object(_) => "object",
    }
}
