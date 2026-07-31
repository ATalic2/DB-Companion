//! db/mysql.rs — MySQL adapter (sqlx)

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use async_trait::async_trait;
use anyhow::Result;
use sqlx::{mysql::MySqlPoolOptions, MySqlPool, Row};
use zeroize::Zeroizing;

pub struct MySqlAdapter {
    pool:    MySqlPool,
    db_name: String,
}

impl MySqlAdapter {
    pub async fn connect(dsn: Zeroizing<String>) -> Result<Self, DbError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .connect(&dsn)
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        let db_name: String = sqlx::query_scalar("SELECT DATABASE()")
            .fetch_one(&pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

        Ok(Self { pool, db_name })
    }

    pub fn dsn(host: &str, port: &str, user: &str, password: &str, dbname: &str) -> String {
        format!("mysql://{}:{}@{}:{}/{}", user, password, host, port, dbname)
    }
}

#[async_trait]
impl IDatabaseAdapter for MySqlAdapter {
    fn driver_name(&self) -> &str { "mysql" }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        let table_rows = sqlx::query(
            "SELECT CAST(TABLE_NAME AS CHAR) AS TABLE_NAME \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' \
             ORDER BY TABLE_NAME",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Schema(e.to_string()))?;

        let mut tables = Vec::new();

        for row in &table_rows {
            let table_name: String = row.get("TABLE_NAME");

            // ── Columns ────────────────────────────────────────────────────
            let col_rows = sqlx::query(
                "SELECT CAST(COLUMN_NAME AS CHAR)    AS COLUMN_NAME, \
                        CAST(DATA_TYPE AS CHAR)       AS DATA_TYPE, \
                        CAST(IS_NULLABLE AS CHAR)     AS IS_NULLABLE, \
                        CAST(COLUMN_DEFAULT AS CHAR)  AS COLUMN_DEFAULT, \
                        CAST(COLUMN_KEY AS CHAR)      AS COLUMN_KEY \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? \
                 ORDER BY ORDINAL_POSITION",
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            // ── FK details ─────────────────────────────────────────────────
            // information_schema.KEY_COLUMN_USAGE has REFERENCED_TABLE_NAME etc.
            let fk_rows = sqlx::query(
                "SELECT \
                    CAST(kcu.COLUMN_NAME AS CHAR)            AS COLUMN_NAME, \
                    CAST(kcu.CONSTRAINT_NAME AS CHAR)        AS CONSTRAINT_NAME, \
                    CAST(kcu.REFERENCED_TABLE_NAME AS CHAR)  AS REF_TABLE, \
                    CAST(kcu.REFERENCED_COLUMN_NAME AS CHAR) AS REF_COLUMN, \
                    CAST(rc.DELETE_RULE AS CHAR)             AS DELETE_RULE, \
                    CAST(rc.UPDATE_RULE AS CHAR)             AS UPDATE_RULE \
                 FROM information_schema.KEY_COLUMN_USAGE kcu \
                 JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
                   ON kcu.CONSTRAINT_NAME   = rc.CONSTRAINT_NAME \
                  AND kcu.TABLE_SCHEMA      = rc.CONSTRAINT_SCHEMA \
                 WHERE kcu.TABLE_SCHEMA = DATABASE() \
                   AND kcu.TABLE_NAME   = ? \
                   AND kcu.REFERENCED_TABLE_NAME IS NOT NULL",
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            let columns = col_rows.iter().map(|r| {
                let col_name: String = r.get("COLUMN_NAME");
                let key: String = r.get("COLUMN_KEY");
                let is_fk = key == "MUL" || fk_rows.iter().any(|fr| {
                    let n: String = fr.get("COLUMN_NAME");
                    n == col_name
                });
                let fk = if is_fk {
                    fk_rows.iter().find(|fr| {
                        let n: String = fr.get("COLUMN_NAME");
                        n == col_name
                    }).map(|fr| ForeignKeyMeta {
                        constraint_name: fr.get("CONSTRAINT_NAME"),
                        ref_table:       fr.get("REF_TABLE"),
                        ref_column:      fr.get("REF_COLUMN"),
                        on_delete:       fr.get("DELETE_RULE"),
                        on_update:       fr.get("UPDATE_RULE"),
                    })
                } else {
                    None
                };
                ColumnMeta {
                    name:        col_name,
                    data_type:   r.get("DATA_TYPE"),
                    nullable:    r.get::<String, _>("IS_NULLABLE") == "YES",
                    is_pk:       key == "PRI",
                    is_fk,
                    fk,
                    default_val: r.get("COLUMN_DEFAULT"),
                }
            }).collect();

            // ── Table-level CHECK constraints (MySQL 8.0.16+) ──────────────
            // MySQL exposes CHECK constraints in information_schema.CHECK_CONSTRAINTS.
            // UNIQUE constraints are already visible through indexes, so we only
            // pull CHECK here to avoid duplication.
            let check_rows = sqlx::query(
                "SELECT \
                    CAST(tc.CONSTRAINT_NAME AS CHAR)  AS CONSTRAINT_NAME, \
                    CAST(cc.CHECK_CLAUSE AS CHAR)     AS CHECK_CLAUSE \
                 FROM information_schema.TABLE_CONSTRAINTS tc \
                 JOIN information_schema.CHECK_CONSTRAINTS cc \
                   ON tc.CONSTRAINT_NAME   = cc.CONSTRAINT_NAME \
                  AND tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA \
                 WHERE tc.TABLE_SCHEMA     = DATABASE() \
                   AND tc.TABLE_NAME       = ? \
                   AND tc.CONSTRAINT_TYPE  = 'CHECK'",
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default(); // older MySQL versions may not have this table

            let constraints: Vec<TableConstraint> = check_rows.iter().map(|r| TableConstraint {
                name:            r.get("CONSTRAINT_NAME"),
                constraint_type: "CHECK".to_string(),
                definition:      r.get("CHECK_CLAUSE"),
            }).collect();

            // ── Indexes ────────────────────────────────────────────────────
            let idx_rows = sqlx::query(
                "SELECT CAST(INDEX_NAME AS CHAR) AS INDEX_NAME, NON_UNIQUE, \
                        CAST(INDEX_TYPE AS CHAR) AS INDEX_TYPE, \
                        CAST(GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) AS CHAR) AS cols \
                 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? \
                 GROUP BY INDEX_NAME, NON_UNIQUE, INDEX_TYPE \
                 ORDER BY INDEX_NAME",
            )
            .bind(&table_name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

            let indexes = idx_rows.iter().map(|r| {
                let cols_str: String = r.get("cols");
                // MySQL reports BTREE / FULLTEXT / HASH / SPATIAL / RTREE — lowercase
                // it to match the convention the other adapters + AI prompt use.
                let index_type: String = r.get("INDEX_TYPE");
                IndexMeta {
                    name:       r.get("INDEX_NAME"),
                    columns:    cols_str.split(',').map(String::from).collect(),
                    unique:     r.get::<i8, _>("NON_UNIQUE") == 0,
                    index_type: Some(index_type.to_lowercase()),
                    predicate:  None,
                }
            }).collect();

            tables.push(TableMeta { name: table_name, columns, indexes, constraints });
        }

        let schema_text = build_schema_text(&tables, "mysql");
        Ok(SchemaMetadata {
            db_name: self.db_name.clone(),
            db_type: "mysql".into(),
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
            let stmts: Vec<String> = strip_sql_comments(raw)
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            for sql in &stmts {
                match sqlx::query(sql).execute(&mut *tx).await {
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
        use sqlx::Row;
        use sqlx::Column;

        let stmts: Vec<String> = strip_sql_comments(sql)
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mut last_result = crate::commands::query::QueryResult {
            columns: vec![], rows: vec![], rows_affected: 0, execution_ms: 0, error: None,
                capped: false,
            };

        for stmt in &stmts {
            match sqlx::query(stmt).persistent(false).fetch_all(&self.pool).await {
                Ok(rows) => {
                    let columns = if rows.is_empty() { vec![] } else {
                        rows[0].columns().iter().map(|c| c.name().to_string()).collect()
                    };
                    let result_rows = rows.iter().map(|row| {
                        (0..row.columns().len()).map(|i| {
                            row.try_get::<String, _>(i)
                                .map(serde_json::Value::String)
                                .unwrap_or(serde_json::Value::Null)
                        }).collect()
                    }).collect();
                    last_result = crate::commands::query::QueryResult {
                        columns, rows: result_rows,
                        rows_affected: rows.len() as u64,
                        execution_ms: 0, error: None,
                capped: false,
            };
                }
                Err(_) => {
                    let r = sqlx::query(stmt).persistent(false).execute(&self.pool).await
                        .map_err(|e| DbError::Transaction(e.to_string()))?;
                    last_result = crate::commands::query::QueryResult {
                        columns: vec![], rows: vec![],
                        rows_affected: r.rows_affected(),
                        execution_ms: 0, error: None,
                capped: false,
            };
                }
            }
        }

        Ok(last_result)
    }
}

fn strip_sql_comments(sql: &str) -> String {
    sql.lines()
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
        .join("\n")
}
