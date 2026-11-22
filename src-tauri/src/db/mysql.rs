//! db/mysql.rs — MySQL adapter (sqlx)

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use sqlx::{mysql::{MySqlPoolOptions, MySqlRow}, Column, MySqlPool, Row, TypeInfo, ValueRef};
use zeroize::Zeroizing;

pub struct MySqlAdapter {
    pool: MySqlPool,
    db_name: String,
}

fn mysql_cell(row: &MySqlRow, index: usize) -> Result<serde_json::Value, DbError> {
    use serde_json::Value;

    let column = &row.columns()[index];
    let type_name = column.type_info().name();
    let decode_error = |error: sqlx::Error| DbError::Transaction(
        format!("Could not read MySQL column '{}' ({type_name}): {error}", column.name())
    );
    if row.try_get_raw(index).map_err(decode_error)?.is_null() {
        return Ok(Value::Null);
    }

    match type_name {
        "BOOLEAN" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" =>
            row.try_get::<i64, _>(index).map(|v| Value::Number(v.into())).map_err(decode_error),
        "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "INT UNSIGNED" | "MEDIUMINT UNSIGNED" | "BIGINT UNSIGNED" =>
            row.try_get::<u64, _>(index).map(|v| Value::Number(v.into())).map_err(decode_error),
        "FLOAT" | "DOUBLE" => {
            let value = row.try_get::<f64, _>(index).map_err(decode_error)?;
            Ok(serde_json::Number::from_f64(value).map(Value::Number)
                .unwrap_or_else(|| Value::String(value.to_string())))
        }
        "DECIMAL" | "JSON" => row.try_get_unchecked::<String, _>(index)
            .map(Value::String).map_err(decode_error),
        "DATE" => row.try_get::<chrono::NaiveDate, _>(index)
            .map(|v| Value::String(v.to_string())).map_err(decode_error),
        "DATETIME" | "TIMESTAMP" => row.try_get_unchecked::<chrono::NaiveDateTime, _>(index)
            .map(|v| Value::String(v.to_string())).map_err(decode_error),
        "TIME" => row.try_get::<chrono::NaiveTime, _>(index)
            .map(|v| Value::String(v.to_string())).map_err(decode_error),
        "YEAR" => row.try_get_unchecked::<i16, _>(index)
            .map(|v| Value::String(v.to_string())).map_err(decode_error),
        "SET" => row.try_get_unchecked::<String, _>(index)
            .map(Value::String).map_err(decode_error),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" =>
            row.try_get_unchecked::<Vec<u8>, _>(index)
                .map(|v| Value::String(format!("0x{}", hex::encode(v)))).map_err(decode_error),
        _ => row.try_get::<String, _>(index).map(Value::String).map_err(decode_error),
    }
}

impl MySqlAdapter {
    pub async fn connect(dsn: Zeroizing<String>) -> Result<Self, DbError> {
        let dsn = normalize_dsn(&dsn)?;
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

fn normalize_dsn(dsn: &str) -> Result<Zeroizing<String>, DbError> {
    let value = dsn.trim();
    if value.contains("://") {
        return Ok(Zeroizing::new(value.to_string()));
    }

    let values = super::connection_strings::parse_key_value(value)?;
    let host = super::connection_strings::first_value(&values, &["server", "host", "data source"])
        .ok_or_else(|| DbError::Connection("MySQL connection string is missing Server".into()))?;
    let database =
        super::connection_strings::first_value(&values, &["database", "database name", "db"])
            .unwrap_or_default();
    let user = super::connection_strings::first_value(&values, &["user id", "username", "user"])
        .unwrap_or_default();
    let password =
        super::connection_strings::first_value(&values, &["password", "pwd"]).unwrap_or_default();
    let port =
        super::connection_strings::first_value(&values, &["port"]).unwrap_or_else(|| "3306".into());

    Ok(Zeroizing::new(format!(
        "mysql://{}:{}@{}:{}/{}",
        urlencoding::encode(&user),
        urlencoding::encode(&password),
        urlencoding::encode(&host),
        port,
        urlencoding::encode(&database),
    )))
}

#[async_trait]
impl IDatabaseAdapter for MySqlAdapter {
    fn driver_name(&self) -> &str {
        "mysql"
    }

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

            let columns = col_rows
                .iter()
                .map(|r| {
                    let col_name: String = r.get("COLUMN_NAME");
                    let key: String = r.get("COLUMN_KEY");
                    let is_fk = key == "MUL"
                        || fk_rows.iter().any(|fr| {
                            let n: String = fr.get("COLUMN_NAME");
                            n == col_name
                        });
                    let fk = if is_fk {
                        fk_rows
                            .iter()
                            .find(|fr| {
                                let n: String = fr.get("COLUMN_NAME");
                                n == col_name
                            })
                            .map(|fr| ForeignKeyMeta {
                                constraint_name: fr.get("CONSTRAINT_NAME"),
                                ref_table: fr.get("REF_TABLE"),
                                ref_column: fr.get("REF_COLUMN"),
                                on_delete: fr.get("DELETE_RULE"),
                                on_update: fr.get("UPDATE_RULE"),
                            })
                    } else {
                        None
                    };
                    ColumnMeta {
                        name: col_name,
                        data_type: r.get("DATA_TYPE"),
                        nullable: r.get::<String, _>("IS_NULLABLE") == "YES",
                        is_pk: key == "PRI",
                        is_fk,
                        fk,
                        default_val: r.get("COLUMN_DEFAULT"),
                    }
                })
                .collect();

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

            let constraints: Vec<TableConstraint> = check_rows
                .iter()
                .map(|r| TableConstraint {
                    name: r.get("CONSTRAINT_NAME"),
                    constraint_type: "CHECK".to_string(),
                    definition: r.get("CHECK_CLAUSE"),
                })
                .collect();

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

            let indexes = idx_rows
                .iter()
                .map(|r| {
                    let cols_str: String = r.get("cols");
                    // MySQL reports BTREE / FULLTEXT / HASH / SPATIAL / RTREE — lowercase
                    // it to match the convention the other adapters + AI prompt use.
                    let index_type: String = r.get("INDEX_TYPE");
                    IndexMeta {
                        name: r.get("INDEX_NAME"),
                        columns: cols_str.split(',').map(String::from).collect(),
                        unique: r.get::<i8, _>("NON_UNIQUE") == 0,
                        index_type: Some(index_type.to_lowercase()),
                        predicate: None,
                    }
                })
                .collect();

            tables.push(TableMeta {
                name: table_name,
                columns,
                indexes,
                constraints,
            });
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
    ) -> Result<TransactionResult, DbError> {
        let statements = super::plan_sql_statements(&commands)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))?;

        let mut outcomes = Vec::new();
        let mut total_rows = 0u64;

        for sql in &statements {
            match sqlx::query(sql).execute(&mut *tx).await {
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
        use sqlx::Column;
        use sqlx::Row;

        let stmts = super::split_sql_statements(sql);

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
                    let result_rows = rows.iter()
                        .map(|row| (0..row.columns().len())
                            .map(|i| mysql_cell(row, i))
                            .collect::<Result<Vec<_>, DbError>>())
                        .collect::<Result<Vec<_>, DbError>>()?;
                    last_result = crate::commands::query::QueryResult {
                        columns,
                        rows: result_rows,
                        rows_affected: rows.len() as u64,
                        execution_ms: 0,
                        error: None,
                        capped,
                    };
                }
                Err(_) => {
                    let r = sqlx::query(stmt)
                        .persistent(false)
                        .execute(&self.pool)
                        .await
                        .map_err(|e| DbError::Transaction(e.to_string()))?;
                    last_result = crate::commands::query::QueryResult {
                        columns: vec![],
                        rows: vec![],
                        rows_affected: r.rows_affected(),
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
