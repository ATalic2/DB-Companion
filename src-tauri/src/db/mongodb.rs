//! db/mongodb.rs — MongoDB adapter via mongosh subprocess
//!
//! Instead of using the Rust mongodb driver (which has SCRAM auth issues on
//! Windows), we shell out to mongosh which is known to work reliably.
//! All commands are run as `mongosh <uri> --quiet --eval <js>` and the
//! output is parsed as JSON.

use super::{
    build_schema_text, ColumnMeta, DbError, IDatabaseAdapter, SchemaMetadata, StatementOutcome,
    TableMeta, TransactionResult,
};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command;
use zeroize::Zeroizing;

pub struct MongoAdapter {
    uri: Zeroizing<String>,
    db_name: String,
}

impl MongoAdapter {
    pub async fn connect(uri: Zeroizing<String>, db_name: &str) -> Result<Self, DbError> {
        // Verify mongosh is available and can connect
        let out = mongosh_eval(&uri, db_name, "JSON.stringify(db.runCommand({ping:1}))").await?;
        let val: serde_json::Value = serde_json::from_str(&out).map_err(|e| {
            DbError::Connection(format!("Unexpected ping response: {e} — got: {out}"))
        })?;
        if val.get("ok").and_then(|v| v.as_f64()).unwrap_or(0.0) != 1.0 {
            return Err(DbError::Connection(format!("Ping failed: {out}")));
        }
        Ok(Self {
            uri,
            db_name: db_name.to_string(),
        })
    }
}

/// Run a JS expression in mongosh and return stdout as a String.
async fn mongosh_eval(uri: &str, _db_name: &str, js: &str) -> Result<String, DbError> {
    // Try mongosh first, fall back to mongo
    for bin in &["mongosh", "mongo"] {
        let mut command = Command::new(bin);
        command
            .kill_on_drop(true)
            .args([uri, "--quiet", "--eval", js]);
        let result = command.output().await;

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
        "mongosh not found. Please install it from https://www.mongodb.com/try/download/shell"
            .into(),
    ))
}

/// Run JS and parse the result as a JSON value.
async fn mongosh_json(uri: &str, db_name: &str, js: &str) -> Result<serde_json::Value, DbError> {
    let out = mongosh_eval(uri, db_name, js).await?;
    // mongosh sometimes emits extra lines before the JSON — find the first `{` or `[`
    let start = out.find(|c| c == '{' || c == '[').unwrap_or(0);
    let clean = &out[start..];
    serde_json::from_str(clean)
        .map_err(|e| DbError::Transaction(format!("Failed to parse mongosh output as JSON: {e}")))
}

/// Parse an AI-produced MongoDB command and translate shell-helper names to
/// their `db.runCommand()` equivalents. `createCollection` is a mongosh method,
/// while the command document accepted by the server uses `create`.
fn normalize_mongo_command(input: &str) -> Result<serde_json::Value, DbError> {
    let mut command: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| DbError::Transaction(format!("Invalid MongoDB command JSON: {e}")))?;
    let object = command.as_object_mut().ok_or_else(|| {
        DbError::Transaction("MongoDB commands must be JSON objects".into())
    })?;

    if object.contains_key("createCollection") {
        let fields = std::mem::take(object);
        for (key, value) in fields {
            let key = if key == "createCollection" {
                "create".to_string()
            } else {
                key
            };
            object.entry(key).or_insert(value);
        }
    }

    // MongoDB requires aggregate commands to opt into cursor results. Models
    // commonly produce only the collection and pipeline, so make the valid
    // default explicit when this is a normal (non-explain) aggregation.
    if object.contains_key("aggregate")
        && !object.contains_key("cursor")
        && !object.contains_key("explain")
    {
        object.insert("cursor".to_string(), serde_json::json!({}));
    }

    Ok(command)
}

/// Build a safe JavaScript expression for a command document. EJSON.parse
/// converts Extended JSON values such as `{ "$date": "..." }` into their
/// native BSON types before the command reaches MongoDB.
fn mongo_command_expression(input: &str) -> Result<String, DbError> {
    let command = normalize_mongo_command(input)?;
    let command_json = serde_json::to_string(&command)
        .map_err(|e| DbError::Transaction(format!("Could not serialize MongoDB command: {e}")))?;
    let js_string = serde_json::to_string(&command_json)
        .map_err(|e| DbError::Transaction(format!("Could not quote MongoDB command: {e}")))?;
    Ok(format!("EJSON.parse({js_string})"))
}

#[async_trait]
impl IDatabaseAdapter for MongoAdapter {
    fn driver_name(&self) -> &str {
        "mongodb"
    }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        // Get collection names
        let js = "JSON.stringify(db.getCollectionNames())";
        let names_val = mongosh_json(&self.uri, &self.db_name, js).await?;
        let collection_names: Vec<String> =
            serde_json::from_value(names_val).map_err(|e| DbError::Schema(e.to_string()))?;

        let mut tables = Vec::new();

        for coll_name in &collection_names {
            // Sample documents to infer fields
            let js = format!(
                "JSON.stringify(db.getCollection('{}').aggregate([{{$sample:{{size:20}}}}]).toArray())",
                coll_name.replace('\'', "\\'")
            );
            let docs_val = mongosh_json(&self.uri, &self.db_name, &js)
                .await
                .unwrap_or(serde_json::Value::Array(vec![]));

            let mut seen = std::collections::HashSet::new();
            let mut columns: Vec<ColumnMeta> = Vec::new();

            if let serde_json::Value::Array(docs) = docs_val {
                for doc in &docs {
                    if let serde_json::Value::Object(map) = doc {
                        for (key, val) in map {
                            if seen.insert(key.clone()) {
                                let is_pk = key == "_id";
                                columns.push(ColumnMeta {
                                    name: key.clone(),
                                    data_type: json_type_name(val).to_string(),
                                    nullable: !is_pk,
                                    is_pk,
                                    is_fk: false,
                                    fk: None,
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
            let indexes: Vec<super::IndexMeta> =
                match mongosh_json(&self.uri, &self.db_name, &idx_js).await {
                    Ok(serde_json::Value::Array(specs)) => specs
                        .iter()
                        .filter_map(|spec| {
                            let obj = spec.as_object()?;
                            let name = obj.get("name")?.as_str()?.to_string();
                            let key_obj = obj.get("key")?.as_object()?;
                            let columns: Vec<String> = key_obj.keys().cloned().collect();
                            let unique =
                                obj.get("unique").and_then(|v| v.as_bool()).unwrap_or(false);
                            let predicate =
                                obj.get("partialFilterExpression").map(|v| v.to_string());

                            // Mongo key-spec values reveal special index kinds (text/geo/hashed);
                            // a plain 1/-1 value is a normal ascending/descending field index.
                            let special = key_obj.values().find_map(|v| v.as_str());
                            let index_type = match special {
                                Some("text") => "text",
                                Some("2dsphere") => "2dsphere",
                                Some("2d") => "2d",
                                Some("hashed") => "hashed",
                                _ if columns.len() > 1 => "compound",
                                _ => "single",
                            };

                            Some(super::IndexMeta {
                                name,
                                columns,
                                unique,
                                index_type: Some(index_type.to_string()),
                                predicate,
                            })
                        })
                        .collect(),
                    _ => vec![], // don't fail the whole schema fetch if index lookup has a hiccup
                };

            tables.push(TableMeta {
                name: coll_name.clone(),
                columns,
                indexes,
                constraints: vec![],
            });
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
    ) -> Result<TransactionResult, DbError> {
        let mut outcomes = Vec::new();
        let mut total_rows = 0u64;

        for cmd_str in &commands {
            // Parse as Extended JSON, then pass the resulting command document
            // to MongoDB. This also prevents command text from becoming raw JS.
            let command = mongo_command_expression(cmd_str)?;
            let js = format!("JSON.stringify(db.runCommand({command}))");
            match mongosh_json(&self.uri, &self.db_name, &js).await {
                Ok(result) => {
                    if result.get("ok").and_then(|v| v.as_f64()) != Some(1.0) {
                        let msg = result.get("errmsg").and_then(|v| v.as_str())
                            .unwrap_or("MongoDB reported an unsuccessful command")
                            .to_string();
                        outcomes.push(StatementOutcome {
                            sql: cmd_str.clone(), rows_affected: 0, ok: false,
                            error: Some(msg.clone()),
                        });
                        return Ok(TransactionResult {
                            success: false, rows_affected: total_rows,
                            statements: outcomes, error: Some(msg),
                        });
                    }
                    let n = result.get("n").and_then(|v| v.as_u64()).unwrap_or(0);
                    total_rows += n;
                    outcomes.push(StatementOutcome {
                        sql: cmd_str.clone(),
                        rows_affected: n,
                        ok: true,
                        error: None,
                    });
                }
                Err(e) => {
                    // mongosh process failures are represented as connection
                    // errors by the shared runner, but at this point the app is
                    // already connected and the command itself failed.
                    let msg = match e {
                        DbError::Connection(message) => message,
                        other => other.to_string(),
                    };
                    outcomes.push(StatementOutcome {
                        sql: cmd_str.clone(),
                        rows_affected: 0,
                        ok: false,
                        error: Some(msg.clone()),
                    });
                    return Ok(TransactionResult {
                        success: false,
                        rows_affected: total_rows,
                        statements: outcomes,
                        error: Some(msg),
                    });
                }
            }
        }

        Ok(TransactionResult {
            success: true,
            rows_affected: total_rows,
            statements: outcomes,
            error: None,
        })
    }

    async fn run_query(&self, input: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        // Input can be either a JSON command doc or raw JS expression
        let js = if input.trim_start().starts_with('{') {
            let command = mongo_command_expression(input)?;
            format!("(()=>{{const r=db.runCommand({command});if(r?.cursor?.firstBatch?.length>{})r.cursor.firstBatch=r.cursor.firstBatch.slice(0,{});return JSON.stringify(r)}})()", crate::commands::query::ROW_CAP, crate::commands::query::ROW_CAP + 1)
        } else {
            // Raw JS — wrap result in JSON.stringify
            format!("(()=>{{const r=({input});return JSON.stringify(Array.isArray(r)?r.slice(0,{}):r)}})()", crate::commands::query::ROW_CAP + 1)
        };

        let result = mongosh_json(&self.uri, &self.db_name, &js).await?;
        // find and aggregate return their documents inside cursor.firstBatch.
        // Feed that batch through the same tabular conversion as direct arrays.
        let result = match result.pointer("/cursor/firstBatch") {
            Some(serde_json::Value::Array(documents)) => {
                serde_json::Value::Array(documents.clone())
            }
            _ => result,
        };

        // If result is an array of documents, expand into rows
        if let serde_json::Value::Array(docs) = &result {
            let capped = docs.len() > crate::commands::query::ROW_CAP;
            let docs = &docs[..docs.len().min(crate::commands::query::ROW_CAP)];
            if docs.is_empty() {
                return Ok(crate::commands::query::QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    execution_ms: 0,
                    error: None,
                    capped,
                });
            }
            let columns: Vec<String> = if let serde_json::Value::Object(map) = &docs[0] {
                map.keys().cloned().collect()
            } else {
                vec!["value".into()]
            };
            let rows = docs
                .iter()
                .map(|doc| {
                    if let serde_json::Value::Object(map) = doc {
                        columns
                            .iter()
                            .map(|k| map.get(k).cloned().unwrap_or(serde_json::Value::Null))
                            .collect()
                    } else {
                        vec![doc.clone()]
                    }
                })
                .collect();
            return Ok(crate::commands::query::QueryResult {
                columns,
                rows,
                rows_affected: docs.len() as u64,
                execution_ms: 0,
                error: None,
                capped,
            });
        }

        // Single result document — flatten to one row
        if let serde_json::Value::Object(map) = &result {
            let columns: Vec<String> = map.keys().cloned().collect();
            let row: Vec<serde_json::Value> = map.values().cloned().collect();
            return Ok(crate::commands::query::QueryResult {
                columns,
                rows: vec![row],
                rows_affected: 1,
                execution_ms: 0,
                error: None,
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
            return Ok(
                "(nothing to run — script only contained a `use` statement, \
                        which is unnecessary since the database is already set by \
                        the connection profile)"
                    .to_string(),
            );
        }

        // Run the whole script as-is via mongosh; it will print the return
        // value of each top-level statement, same as pasting it interactively.
        mongosh_eval(&self.uri, &self.db_name, use_re_stripped).await
    }
}

#[cfg(test)]
mod tests {
    use super::{mongo_command_expression, normalize_mongo_command};
    use serde_json::json;

    #[test]
    fn translates_create_collection_helper_to_server_command() {
        let command = normalize_mongo_command(r#"{"createCollection":"users"}"#).unwrap();
        assert_eq!(command, json!({ "create": "users" }));
        assert_eq!(command.as_object().unwrap().keys().next().unwrap(), "create");
    }

    #[test]
    fn preserves_insert_as_the_first_command_field() {
        let command = normalize_mongo_command(
            r#"{"insert":"users","documents":[{"_id":"u1","name":"Ava"}]}"#,
        )
        .unwrap();

        assert_eq!(command.as_object().unwrap().keys().next().unwrap(), "insert");
        let expression = mongo_command_expression(
            r#"{"insert":"users","documents":[{"_id":"u1","name":"Ava"}]}"#,
        )
        .unwrap();
        assert!(expression.find("insert").unwrap() < expression.find("documents").unwrap());
    }

    #[test]
    fn adds_the_required_cursor_to_aggregate_commands() {
        let command = normalize_mongo_command(
            r#"{"aggregate":"orders","pipeline":[{"$limit":5}]}"#,
        )
        .unwrap();

        assert_eq!(command.get("cursor"), Some(&json!({})));
        assert_eq!(command.as_object().unwrap().keys().next().unwrap(), "aggregate");
    }

    #[test]
    fn command_expression_uses_extended_json_parser() {
        let expression = mongo_command_expression(
            r#"{"insert":"events","documents":[{"at":{"$date":"2026-01-01T00:00:00Z"}}]}"#,
        )
        .unwrap();

        assert!(expression.starts_with("EJSON.parse("));
        assert!(expression.contains("$date"));
    }
}

fn json_type_name(val: &serde_json::Value) -> &'static str {
    match val {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(n) => {
            if n.is_f64() {
                "double"
            } else {
                "int"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
