//! db/postgres.rs — PostgreSQL adapter using sqlx

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use async_trait::async_trait;
use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use zeroize::Zeroizing;

pub struct PostgresAdapter {
    pool:        PgPool,
    pub db_name: String,
}

impl PostgresAdapter {
    pub async fn connect(dsn: Zeroizing<String>) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(|conn, _meta| Box::pin(async move {
                use sqlx::Executor;
                conn.execute("SET search_path = public").await?;
                Ok(())
            }))
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

#[async_trait]
impl IDatabaseAdapter for PostgresAdapter {
    fn driver_name(&self) -> &str { "postgresql" }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        let table_rows = sqlx::query(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Schema(e.to_string()))?;

        let mut tables = Vec::new();

        for row in &table_rows {
            let table_name: String = row.get("table_name");

            // ── Columns ────────────────────────────────────────────────────
            let col_rows = sqlx::query(
                r#"
                SELECT
                    c.column_name,
                    c.data_type,
                    c.is_nullable,
                    c.column_default,
                    CASE WHEN pk.column_name IS NOT NULL THEN TRUE ELSE FALSE END AS is_pk,
                    CASE WHEN fk.column_name IS NOT NULL THEN TRUE ELSE FALSE END AS is_fk
                FROM information_schema.columns c
                LEFT JOIN (
                    SELECT kcu.column_name FROM information_schema.table_constraints tc
                    JOIN information_schema.key_column_usage kcu
                      ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
                    WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_name = $1 AND tc.table_schema = 'public'
                ) pk ON pk.column_name = c.column_name
                LEFT JOIN (
                    SELECT kcu.column_name FROM information_schema.table_constraints tc
                    JOIN information_schema.key_column_usage kcu
                      ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
                    WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = $1 AND tc.table_schema = 'public'
                ) fk ON fk.column_name = c.column_name
                WHERE c.table_name = $1 AND c.table_schema = 'public'
                ORDER BY c.ordinal_position
                "#,
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            // ── FK details: constraint name, ref table/col, actions ─────────
            let fk_rows = sqlx::query(
                r#"
                SELECT
                    kcu.column_name,
                    tc.constraint_name,
                    ccu.table_name  AS ref_table,
                    ccu.column_name AS ref_column,
                    rc.delete_rule,
                    rc.update_rule
                FROM information_schema.table_constraints tc
                JOIN information_schema.key_column_usage kcu
                  ON tc.constraint_name = kcu.constraint_name
                 AND tc.table_schema    = kcu.table_schema
                JOIN information_schema.referential_constraints rc
                  ON tc.constraint_name = rc.constraint_name
                 AND tc.constraint_schema = rc.constraint_schema
                JOIN information_schema.constraint_column_usage ccu
                  ON rc.unique_constraint_name   = ccu.constraint_name
                 AND rc.unique_constraint_schema = ccu.constraint_schema
                WHERE tc.constraint_type = 'FOREIGN KEY'
                  AND tc.table_name      = $1
                  AND tc.table_schema    = 'public'
                "#,
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            let columns = col_rows.iter().map(|r| {
                let col_name: String = r.get("column_name");
                let is_fk: bool = r.get("is_fk");
                let fk = if is_fk {
                    fk_rows.iter().find(|fr| {
                        let n: String = fr.get("column_name");
                        n == col_name
                    }).map(|fr| ForeignKeyMeta {
                        constraint_name: fr.get("constraint_name"),
                        ref_table:       fr.get("ref_table"),
                        ref_column:      fr.get("ref_column"),
                        on_delete:       fr.get("delete_rule"),
                        on_update:       fr.get("update_rule"),
                    })
                } else {
                    None
                };
                ColumnMeta {
                    name:        col_name,
                    data_type:   r.get("data_type"),
                    nullable:    r.get::<String, _>("is_nullable") == "YES",
                    is_pk:       r.get("is_pk"),
                    is_fk,
                    fk,
                    default_val: r.get("column_default"),
                }
            }).collect();

            // ── Table-level constraints: CHECK and multi-column UNIQUE ──────
            let con_rows = sqlx::query(
                r#"
                SELECT
                    tc.constraint_name,
                    tc.constraint_type,
                    CASE
                        WHEN tc.constraint_type = 'CHECK'
                             THEN cc.check_clause
                        WHEN tc.constraint_type = 'UNIQUE'
                             THEN string_agg(kcu.column_name, ', ' ORDER BY kcu.ordinal_position)
                        ELSE ''
                    END AS definition
                FROM information_schema.table_constraints tc
                LEFT JOIN information_schema.check_constraints cc
                  ON tc.constraint_name  = cc.constraint_name
                 AND tc.constraint_schema = cc.constraint_schema
                LEFT JOIN information_schema.key_column_usage kcu
                  ON tc.constraint_name  = kcu.constraint_name
                 AND tc.table_schema     = kcu.table_schema
                WHERE tc.table_name   = $1
                  AND tc.table_schema = 'public'
                  AND tc.constraint_type IN ('CHECK', 'UNIQUE')
                  -- exclude the auto-generated NOT NULL check constraints
                  AND tc.constraint_name NOT LIKE '%_not_null'
                GROUP BY tc.constraint_name, tc.constraint_type, cc.check_clause
                ORDER BY tc.constraint_type, tc.constraint_name
                "#,
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            let constraints: Vec<TableConstraint> = con_rows.iter().map(|r| TableConstraint {
                name:            r.get("constraint_name"),
                constraint_type: r.get("constraint_type"),
                definition:      r.get("definition"),
            }).collect();

            // ── Indexes ────────────────────────────────────────────────────
            let idx_rows = sqlx::query(
                r#"
                SELECT
                    i.indexname                                AS index_name,
                    ix.indisunique                             AS is_unique,
                    i.indexdef                                 AS indexdef
                FROM pg_indexes i
                JOIN pg_class ic ON ic.relname = i.indexname
                                 AND ic.relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = i.schemaname)
                JOIN pg_index ix ON ix.indexrelid = ic.oid
                WHERE i.tablename = $1
                  AND i.schemaname = 'public'
                ORDER BY i.indexname
                "#,
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            eprintln!("[get_schema] table '{}' — {} index rows returned", table_name, idx_rows.len());
            let indexes = idx_rows.iter().map(|r| {
                let indexdef: String = r.get("indexdef");
                let columns: Vec<String> = indexdef
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
                    indexdef.split(" USING ").nth(1)
                        .and_then(|s| s.split_whitespace().next())
                        .map(|s| s.to_string())
                } else {
                    Some("btree".to_string())
                };
                let predicate = indexdef.rfind(')').and_then(|close| {
                    let after = indexdef[close + 1..].trim();
                    if after.starts_with("WHERE") {
                        Some(after[5..].trim().to_string())
                    } else {
                        None
                    }
                });
                IndexMeta {
                    name:       r.get("index_name"),
                    unique:     r.get("is_unique"),
                    index_type,
                    predicate,
                    columns,
                }
            }).collect();

            tables.push(TableMeta { name: table_name, columns, indexes, constraints });
        }

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
        dry_run:  bool,
    ) -> Result<TransactionResult, DbError> {
        let mut tx = self.pool.begin().await
            .map_err(|e| DbError::Transaction(e.to_string()))?;

        let mut outcomes   = Vec::new();
        let mut total_rows = 0u64;

        for raw in &commands {
            let stmts: Vec<&str> = raw.split(';')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            for sql in stmts {
                match sqlx::query(sql).persistent(false).execute(&mut *tx).await {
                    Ok(r) => {
                        total_rows += r.rows_affected();
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(), rows_affected: r.rows_affected(), ok: true, error: None,
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(), rows_affected: 0, ok: false, error: Some(msg.clone()),
                        });
                        tx.rollback().await.map_err(|re| DbError::Rollback(re.to_string()))?;
                        return Ok(TransactionResult {
                            success: false, dry_run, rows_affected: total_rows,
                            statements: outcomes, error: Some(msg),
                        });
                    }
                }
            }
        }

        if dry_run {
            tx.rollback().await.map_err(|e| DbError::Rollback(e.to_string()))?;
        } else {
            tx.commit().await.map_err(|e| DbError::Transaction(e.to_string()))?;
        }

        Ok(TransactionResult {
            success: true, dry_run, rows_affected: total_rows,
            statements: outcomes, error: None,
        })
    }

    async fn run_query(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        self.run_query_impl(sql).await
    }
}

impl PostgresAdapter {
    pub async fn run_query_impl(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        use sqlx::Column;
        use sqlx::Row;
        use sqlx::TypeInfo;

        let stripped: String = sql
            .lines()
            .map(|line| {
                let mut in_single = false;
                let chars: Vec<char> = line.chars().collect();
                let mut end = chars.len();
                let mut i = 0;
                while i < chars.len() {
                    match chars[i] {
                        '\'' => { in_single = !in_single; i += 1; }
                        '-' if !in_single && i + 1 < chars.len() && chars[i + 1] == '-' => {
                            end = i;
                            break;
                        }
                        _ => { i += 1; }
                    }
                }
                chars[..end].iter().collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        let stmts: Vec<String> = stripped
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| {
                if s.is_empty() { return false; }
                let upper = s.to_uppercase();
                let sql_verbs = [
                    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP",
                    "ALTER", "TRUNCATE", "WITH", "SET", "GRANT", "REVOKE",
                    "BEGIN", "COMMIT", "ROLLBACK", "VACUUM", "ANALYZE",
                    "COMMENT", "DO", "CALL", "COPY",
                ];
                sql_verbs.iter().any(|v| upper.contains(v))
            })
            .collect();

        let mut last_result = crate::commands::query::QueryResult {
            columns: vec![], rows: vec![], rows_affected: 0, execution_ms: 0, error: None,
                capped: false,
            };

        eprintln!("[run_query] {} statements to execute:", stmts.len());
        for (i, s) in stmts.iter().enumerate() {
            eprintln!("  [{i}] {}", &s.chars().take(120).collect::<String>());
        }

        for stmt in &stmts {
            eprintln!("[run_query] executing: {}", &stmt.chars().take(120).collect::<String>());
            match sqlx::query(stmt).persistent(false).fetch_all(&self.pool).await {
                Ok(rows) => {
                    let columns = if rows.is_empty() {
                        vec![]
                    } else {
                        rows[0].columns().iter().map(|c| c.name().to_string()).collect()
                    };

                    let result_rows = rows.iter().map(|row| {
                        (0..row.columns().len()).map(|i| {
                            let col = &row.columns()[i];
                            let type_name = col.type_info().name();
                            match type_name {
                                "INT4" | "INT8" | "INT2" | "OID" => {
                                    row.try_get::<i64, _>(i)
                                        .map(|v| serde_json::Value::Number(v.into()))
                                        .unwrap_or(serde_json::Value::Null)
                                }
                                "FLOAT4" | "FLOAT8" | "NUMERIC" => {
                                    row.try_get::<f64, _>(i)
                                        .ok()
                                        .and_then(|v| serde_json::Number::from_f64(v))
                                        .map(serde_json::Value::Number)
                                        .unwrap_or_else(|| {
                                            row.try_get::<String, _>(i)
                                                .map(serde_json::Value::String)
                                                .unwrap_or(serde_json::Value::Null)
                                        })
                                }
                                "BOOL" => {
                                    row.try_get::<bool, _>(i)
                                        .map(serde_json::Value::Bool)
                                        .unwrap_or(serde_json::Value::Null)
                                }
                                _ => {
                                    row.try_get::<String, _>(i)
                                        .map(serde_json::Value::String)
                                        .unwrap_or(serde_json::Value::Null)
                                }
                            }
                        }).collect::<Vec<_>>()
                    }).collect();

                    last_result = crate::commands::query::QueryResult {
                        columns,
                        rows: result_rows,
                        rows_affected: rows.len() as u64,
                        execution_ms: 0,
                        error: None,
                capped: false,
            };
                }
                Err(fetch_err) => {
                    eprintln!("[run_query] fetch_all failed (expected for DDL/DML): {fetch_err}");
                    let result = sqlx::query(stmt).persistent(false).execute(&self.pool).await
                        .map_err(|e| { eprintln!("[run_query] execute failed: {e}"); DbError::Transaction(e.to_string()) })?;
                    eprintln!("[run_query] execute OK — {} rows affected", result.rows_affected());
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
