//! db/postgres.rs — PostgreSQL adapter using sqlx

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::collections::HashMap;
use zeroize::Zeroizing;

pub struct PostgresAdapter {
    pool: PgPool,
    pub db_name: String,
}

impl PostgresAdapter {
    pub async fn connect(dsn: Zeroizing<String>) -> Result<Self, DbError> {
        let dsn = normalize_dsn(&dsn)?;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    use sqlx::Executor;
                    conn.execute("SET search_path = public").await?;
                    Ok(())
                })
            })
            .connect(&dsn)
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        let db_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

        Ok(Self { pool, db_name })
    }
}

/// Accept both URL DSNs and Npgsql-style `Key=Value;Key=Value` strings.
fn normalize_dsn(dsn: &str) -> Result<Zeroizing<String>, DbError> {
    let value = dsn.trim();
    if value.contains("://") {
        return Ok(Zeroizing::new(value.to_string()));
    }

    let mut host = None;
    let mut port = None;
    let mut database = None;
    let mut username = None;
    let mut password = None;

    for part in value.split(';').filter(|part| !part.trim().is_empty()) {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            DbError::Connection(format!(
                "Invalid PostgreSQL connection-string entry: {part}"
            ))
        })?;
        let key = key.trim().to_ascii_lowercase();
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        match key.as_str() {
            "host" | "server" | "data source" => host = Some(value),
            "port" => port = Some(value),
            "database" | "database name" | "dbname" => database = Some(value),
            "username" | "user id" | "user" => username = Some(value),
            "password" | "pwd" => password = Some(value),
            "include error detail" | "commandtimeout" | "timeout" => {}
            _ => {}
        }
    }

    let host = host.ok_or_else(|| {
        DbError::Connection("PostgreSQL connection string is missing Host".into())
    })?;
    let database = database.ok_or_else(|| {
        DbError::Connection("PostgreSQL connection string is missing Database".into())
    })?;
    let user = username.unwrap_or_default();
    let password = password.unwrap_or_default();
    let port = port.unwrap_or_else(|| "5432".into());
    let encoded_user = urlencoding::encode(&user);
    let encoded_password = urlencoding::encode(&password);
    let encoded_host = urlencoding::encode(&host);
    let encoded_database = urlencoding::encode(&database);

    Ok(Zeroizing::new(format!(
        "postgres://{encoded_user}:{encoded_password}@{encoded_host}:{port}/{encoded_database}"
    )))
}

#[async_trait]
impl IDatabaseAdapter for PostgresAdapter {
    fn driver_name(&self) -> &str {
        "postgresql"
    }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        let started = std::time::Instant::now();
        let table_rows = sqlx::query(
            "SELECT table_name FROM information_schema.tables \
                     WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
                     ORDER BY table_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Schema(e.to_string()))?;

        let col_rows = sqlx::query(
                    r#"SELECT c.table_name, c.column_name, c.data_type, c.is_nullable, c.column_default,
                              EXISTS (SELECT 1 FROM information_schema.key_column_usage k
                                      JOIN information_schema.table_constraints tc
                                        ON tc.constraint_name = k.constraint_name AND tc.table_schema = k.table_schema
                                      WHERE tc.constraint_type = 'PRIMARY KEY' AND k.table_name = c.table_name
                                        AND k.column_name = c.column_name AND k.table_schema = 'public') AS is_pk
                       FROM information_schema.columns c
                       WHERE c.table_schema = 'public'
                       ORDER BY c.table_name, c.ordinal_position"#,
                ).fetch_all(&self.pool).await.map_err(|e| DbError::Schema(e.to_string()))?;

        let fk_rows = sqlx::query(
                    r#"SELECT kcu.table_name, kcu.column_name, tc.constraint_name,
                              ccu.table_name AS ref_table, ccu.column_name AS ref_column,
                              rc.delete_rule, rc.update_rule
                       FROM information_schema.table_constraints tc
                       JOIN information_schema.key_column_usage kcu
                         ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
                       JOIN information_schema.referential_constraints rc
                         ON tc.constraint_name = rc.constraint_name AND tc.constraint_schema = rc.constraint_schema
                       JOIN information_schema.constraint_column_usage ccu
                         ON rc.unique_constraint_name = ccu.constraint_name
                        AND rc.unique_constraint_schema = ccu.constraint_schema
                       WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_schema = 'public'"#,
                ).fetch_all(&self.pool).await.map_err(|e| DbError::Schema(e.to_string()))?;

        let con_rows = sqlx::query(
                    r#"SELECT tc.table_name, tc.constraint_name, tc.constraint_type,
                              CASE WHEN tc.constraint_type = 'CHECK' THEN cc.check_clause
                                   WHEN tc.constraint_type = 'UNIQUE' THEN string_agg(kcu.column_name, ', ' ORDER BY kcu.ordinal_position)
                                   ELSE '' END AS definition
                       FROM information_schema.table_constraints tc
                       LEFT JOIN information_schema.check_constraints cc
                         ON tc.constraint_name = cc.constraint_name AND tc.constraint_schema = cc.constraint_schema
                       LEFT JOIN information_schema.key_column_usage kcu
                         ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
                       WHERE tc.table_schema = 'public' AND tc.constraint_type IN ('CHECK', 'UNIQUE')
                         AND tc.constraint_name NOT LIKE '%_not_null'
                       GROUP BY tc.table_name, tc.constraint_name, tc.constraint_type, cc.check_clause
                       ORDER BY tc.table_name, tc.constraint_type, tc.constraint_name"#,
                ).fetch_all(&self.pool).await.map_err(|e| DbError::Schema(e.to_string()))?;

        let idx_rows = sqlx::query(
                    r#"SELECT i.tablename AS table_name, i.indexname AS index_name,
                              ix.indisunique AS is_unique, i.indexdef
                       FROM pg_indexes i
                       JOIN pg_class ic ON ic.relname = i.indexname
                        AND ic.relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = i.schemaname)
                       JOIN pg_index ix ON ix.indexrelid = ic.oid
                       WHERE i.schemaname = 'public' ORDER BY i.tablename, i.indexname"#,
                ).fetch_all(&self.pool).await.map_err(|e| DbError::Schema(e.to_string()))?;

        let mut foreign_keys = HashMap::new();
        for row in fk_rows {
            foreign_keys.insert(
                (
                    row.get::<String, _>("table_name"),
                    row.get::<String, _>("column_name"),
                ),
                ForeignKeyMeta {
                    constraint_name: row.get("constraint_name"),
                    ref_table: row.get("ref_table"),
                    ref_column: row.get("ref_column"),
                    on_delete: row.get("delete_rule"),
                    on_update: row.get("update_rule"),
                },
            );
        }

        let mut columns_by_table: HashMap<String, Vec<ColumnMeta>> = HashMap::new();
        for row in col_rows {
            let table_name: String = row.get("table_name");
            let column_name: String = row.get("column_name");
            let fk = foreign_keys
                .get(&(table_name.clone(), column_name.clone()))
                .cloned();
            columns_by_table
                .entry(table_name)
                .or_default()
                .push(ColumnMeta {
                    name: column_name,
                    data_type: row.get("data_type"),
                    nullable: row.get::<String, _>("is_nullable") == "YES",
                    is_pk: row.get("is_pk"),
                    is_fk: fk.is_some(),
                    fk,
                    default_val: row.get("column_default"),
                });
        }

        let mut constraints_by_table: HashMap<String, Vec<TableConstraint>> = HashMap::new();
        for row in con_rows {
            let table_name: String = row.get("table_name");
            constraints_by_table
                .entry(table_name)
                .or_default()
                .push(TableConstraint {
                    name: row.get("constraint_name"),
                    constraint_type: row.get("constraint_type"),
                    definition: row.get("definition"),
                });
        }

        let mut indexes_by_table: HashMap<String, Vec<IndexMeta>> = HashMap::new();
        for row in idx_rows {
            let table_name: String = row.get("table_name");
            let indexdef: String = row.get("indexdef");
            let columns = indexdef
                .find('(')
                .and_then(|open| indexdef.rfind(')').map(|close| (open, close)))
                .map(|(open, close)| {
                    indexdef[open + 1..close]
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            let index_type = if indexdef.contains(" USING ") {
                indexdef
                    .split(" USING ")
                    .nth(1)
                    .and_then(|s| s.split_whitespace().next())
                    .map(String::from)
            } else {
                Some("btree".to_string())
            };
            let predicate = indexdef.rfind(')').and_then(|close| {
                let after = indexdef[close + 1..].trim();
                after
                    .strip_prefix("WHERE")
                    .map(|value| value.trim().to_string())
            });
            indexes_by_table
                .entry(table_name)
                .or_default()
                .push(IndexMeta {
                    name: row.get("index_name"),
                    unique: row.get("is_unique"),
                    index_type,
                    predicate,
                    columns,
                });
        }

        let tables = table_rows
            .into_iter()
            .map(|row| {
                let name: String = row.get("table_name");
                TableMeta {
                    columns: columns_by_table.remove(&name).unwrap_or_default(),
                    indexes: indexes_by_table.remove(&name).unwrap_or_default(),
                    constraints: constraints_by_table.remove(&name).unwrap_or_default(),
                    name,
                }
            })
            .collect::<Vec<_>>();

        eprintln!(
                    "[get_schema] bulk load — {} tables, {} columns, {} foreign keys, {} constraints, {} indexes in {}ms",
                    tables.len(),
                    tables.iter().map(|table| table.columns.len()).sum::<usize>(),
                    foreign_keys.len(),
                    tables.iter().map(|table| table.constraints.len()).sum::<usize>(),
                    tables.iter().map(|table| table.indexes.len()).sum::<usize>(),
                    started.elapsed().as_millis(),
                );

        let schema_text = build_schema_text(&tables, "postgresql");
        Ok(SchemaMetadata {
            db_name: self.db_name.clone(),
            db_type: "postgresql".into(),
            tables,
            schema_text,
        })
    }

    async fn execute_transaction(
        &self,
        commands: Vec<String>,
    ) -> Result<TransactionResult, DbError> {
        let statements = super::plan_sql_statements(&commands)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))?;
        // Keep unqualified names resolvable without placing this session setup
        // ahead of a plan's optional transaction wrapper.
        sqlx::query("SET search_path = public").execute(&mut *tx).await
            .map_err(|e| DbError::Transaction(e.to_string()))?;

        let mut outcomes = Vec::new();
        let mut total_rows = 0u64;

        for sql in statements {
                match sqlx::query(&sql).persistent(false).execute(&mut *tx).await {
                    Ok(r) => {
                        total_rows += r.rows_affected();
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(),
                            rows_affected: r.rows_affected(),
                            ok: true,
                            error: None,
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(),
                            rows_affected: 0,
                            ok: false,
                            error: Some(msg.clone()),
                        });
                        tx.rollback()
                            .await
                            .map_err(|re| DbError::Rollback(re.to_string()))?;
                        return Ok(TransactionResult {
                            success: false,
                            rows_affected: total_rows,
                            statements: outcomes,
                            error: Some(msg),
                        });
                    }
                }
        }

        tx.commit()
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))?;

        Ok(TransactionResult {
            success: true,
            rows_affected: total_rows,
            statements: outcomes,
            error: None,
        })
    }

    async fn run_query(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        self.run_query_impl(sql).await
    }
}

impl PostgresAdapter {
    pub async fn run_query_impl(
        &self,
        sql: &str,
    ) -> Result<crate::commands::query::QueryResult, DbError> {
        use sqlx::Column;
        use sqlx::Row;
        use sqlx::TypeInfo;

        let stmts: Vec<String> = super::split_sql_statements(sql)
            .into_iter()
            .filter(|s| {
                if s.is_empty() {
                    return false;
                }
                let upper = s.to_uppercase();
                let sql_verbs = [
                    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "TRUNCATE",
                    "WITH", "SET", "GRANT", "REVOKE", "BEGIN", "COMMIT", "ROLLBACK", "VACUUM",
                    "ANALYZE", "COMMENT", "DO", "CALL", "COPY",
                ];
                sql_verbs.iter().any(|v| upper.contains(v))
            })
            .collect();

        let mut last_result = crate::commands::query::QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_ms: 0,
            error: None,
            capped: false,
        };

        for stmt in &stmts {
            let mut stream = sqlx::query(stmt).persistent(false).fetch(&self.pool);
            let mut rows = Vec::new();
            let fetch_result: Result<(), sqlx::Error> = async {
                while rows.len() <= crate::commands::query::ROW_CAP {
                    match stream.try_next().await? {
                        Some(row) => rows.push(row),
                        None => break,
                    }
                }
                Ok(())
            }
            .await;
            drop(stream);
            match fetch_result {
                Ok(()) => {
                    let capped = rows.len() > crate::commands::query::ROW_CAP;
                    rows.truncate(crate::commands::query::ROW_CAP);
                    let columns = if rows.is_empty() {
                        vec![]
                    } else {
                        rows[0]
                            .columns()
                            .iter()
                            .map(|c| c.name().to_string())
                            .collect()
                    };

                    let result_rows = rows
                        .iter()
                        .map(|row| {
                            (0..row.columns().len())
                                .map(|i| {
                                    let col = &row.columns()[i];
                                    let type_name = col.type_info().name();
                                    match type_name {
                                        "INT4" | "INT8" | "INT2" | "OID" => row
                                            .try_get::<i64, _>(i)
                                            .map(|v| serde_json::Value::Number(v.into()))
                                            .unwrap_or(serde_json::Value::Null),
                                        "FLOAT4" | "FLOAT8" | "NUMERIC" => row
                                            .try_get::<f64, _>(i)
                                            .ok()
                                            .and_then(|v| serde_json::Number::from_f64(v))
                                            .map(serde_json::Value::Number)
                                            .unwrap_or_else(|| {
                                                row.try_get::<String, _>(i)
                                                    .map(serde_json::Value::String)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }),
                                        "BOOL" => row
                                            .try_get::<bool, _>(i)
                                            .map(serde_json::Value::Bool)
                                            .unwrap_or(serde_json::Value::Null),
                                        _ => row
                                            .try_get::<String, _>(i)
                                            .map(serde_json::Value::String)
                                            .unwrap_or(serde_json::Value::Null),
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect();

                    last_result = crate::commands::query::QueryResult {
                        columns,
                        rows: result_rows,
                        rows_affected: rows.len() as u64,
                        execution_ms: 0,
                        error: None,
                        capped,
                    };
                }
                Err(fetch_err) => {
                    let _ = fetch_err;
                    let result = sqlx::query(stmt)
                        .persistent(false)
                        .execute(&self.pool)
                        .await
                        .map_err(|e| DbError::Transaction(e.to_string()))?;
                    last_result = crate::commands::query::QueryResult {
                        columns: vec![],
                        rows: vec![],
                        rows_affected: result.rows_affected(),
                        execution_ms: 0,
                        error: None,
                        capped: false,
                    };
                }
            }
        }

        Ok(last_result)
    }
}
