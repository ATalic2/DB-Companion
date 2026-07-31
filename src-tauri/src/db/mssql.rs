//! db/mssql.rs — Microsoft SQL Server adapter using tiberius + rustls

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use async_trait::async_trait;
use anyhow::Result;
use tiberius::{AuthMethod, Client, Config};
use tokio::net::TcpStream;
use tokio_util::compat::{TokioAsyncWriteCompatExt, Compat};
use zeroize::Zeroizing;

pub struct MssqlAdapter {
    host:     String,
    port:     u16,
    user:     String,
    password: Zeroizing<String>,
    dbname:   String,
}

impl MssqlAdapter {
    pub fn new(
        host: &str, port: &str, user: &str,
        password: Zeroizing<String>, dbname: &str,
        _instance: Option<&str>,
    ) -> Result<Self, DbError> {
        let port = port.parse::<u16>().unwrap_or(1433);
        Ok(Self {
            host:     host.to_string(),
            port,
            user:     user.to_string(),
            password,
            dbname:   dbname.to_string(),
        })
    }

    async fn connect(&self) -> Result<Client<Compat<TcpStream>>, DbError> {
        let mut config = Config::new();
        config.host(&self.host);
        config.port(self.port);
        config.database(&self.dbname);
        config.authentication(AuthMethod::sql_server(&self.user, self.password.as_str()));
        config.trust_cert();

        let tcp = TcpStream::connect(config.get_addr())
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;
        tcp.set_nodelay(true).ok();

        Client::connect(config, tcp.compat_write())
            .await
            .map_err(|e| DbError::Connection(e.to_string()))
    }
}

#[async_trait]
impl IDatabaseAdapter for MssqlAdapter {
    fn driver_name(&self) -> &str { "mssql" }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        let mut client = self.connect().await?;

        // ── Tables ─────────────────────────────────────────────────────────
        let table_rows = client.simple_query(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
             WHERE TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME"
        ).await
            .map_err(|e| DbError::Schema(e.to_string()))?
            .into_first_result().await
            .map_err(|e| DbError::Schema(e.to_string()))?;

        let table_names: Vec<String> = table_rows.iter()
            .filter_map(|r| r.get::<&str, _>(0).map(|s| s.to_string()))
            .collect();

        let mut tables = Vec::new();

        for table_name in &table_names {
            // ── Columns with PK/FK flags ────────────────────────────────────
            let col_sql = format!("
                SELECT
                    c.COLUMN_NAME,
                    c.DATA_TYPE,
                    c.IS_NULLABLE,
                    c.COLUMN_DEFAULT,
                    CASE WHEN pk.COLUMN_NAME IS NOT NULL THEN 1 ELSE 0 END AS IS_PK,
                    CASE WHEN fk.COLUMN_NAME IS NOT NULL THEN 1 ELSE 0 END AS IS_FK
                FROM INFORMATION_SCHEMA.COLUMNS c
                LEFT JOIN (
                    SELECT kcu.COLUMN_NAME
                    FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
                    JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                      ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
                    WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_NAME = '{table_name}'
                ) pk ON pk.COLUMN_NAME = c.COLUMN_NAME
                LEFT JOIN (
                    SELECT kcu.COLUMN_NAME
                    FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
                    JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                      ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
                    WHERE tc.CONSTRAINT_TYPE = 'FOREIGN KEY' AND tc.TABLE_NAME = '{table_name}'
                ) fk ON fk.COLUMN_NAME = c.COLUMN_NAME
                WHERE c.TABLE_NAME = '{table_name}'
                ORDER BY c.ORDINAL_POSITION");

            let col_rows = client.simple_query(&col_sql).await
                .map_err(|e| DbError::Schema(e.to_string()))?
                .into_first_result().await
                .map_err(|e| DbError::Schema(e.to_string()))?;

            // ── FK details via sys catalog ──────────────────────────────────
            let fk_sql = format!("
                SELECT
                    fk.name                     AS constraint_name,
                    col.name                    AS from_col,
                    ref_tab.name                AS ref_table,
                    ref_col.name                AS ref_col,
                    fk.delete_referential_action_desc AS on_delete,
                    fk.update_referential_action_desc AS on_update
                FROM sys.foreign_keys fk
                JOIN sys.foreign_key_columns fkc
                  ON fk.object_id = fkc.constraint_object_id
                JOIN sys.tables   tab     ON fk.parent_object_id    = tab.object_id
                JOIN sys.columns  col     ON fkc.parent_object_id   = col.object_id
                                         AND fkc.parent_column_id   = col.column_id
                JOIN sys.tables   ref_tab ON fk.referenced_object_id = ref_tab.object_id
                JOIN sys.columns  ref_col ON fkc.referenced_object_id = ref_col.object_id
                                          AND fkc.referenced_column_id = ref_col.column_id
                WHERE tab.name = '{table_name}'");

            let fk_rows = client.simple_query(&fk_sql).await
                .map_err(|e| DbError::Schema(e.to_string()))?
                .into_first_result().await
                .map_err(|e| DbError::Schema(e.to_string()))?;

            let columns: Vec<ColumnMeta> = col_rows.iter().map(|r| {
                let col_name = r.get::<&str, _>(0).unwrap_or("").to_string();
                let is_fk    = r.get::<i32, _>(5).unwrap_or(0) == 1;
                let fk = if is_fk {
                    fk_rows.iter().find(|fr| {
                        fr.get::<&str, _>(1).unwrap_or("") == col_name.as_str()
                    }).map(|fr| ForeignKeyMeta {
                        constraint_name: fr.get::<&str, _>(0).unwrap_or("").to_string(),
                        ref_table:       fr.get::<&str, _>(2).unwrap_or("").to_string(),
                        ref_column:      fr.get::<&str, _>(3).unwrap_or("").to_string(),
                        on_delete:       fr.get::<&str, _>(4).unwrap_or("NO_ACTION").to_string(),
                        on_update:       fr.get::<&str, _>(5).unwrap_or("NO_ACTION").to_string(),
                    })
                } else {
                    None
                };
                ColumnMeta {
                    name:        col_name,
                    data_type:   r.get::<&str, _>(1).unwrap_or("").to_string(),
                    nullable:    r.get::<&str, _>(2).unwrap_or("YES") == "YES",
                    default_val: r.get::<&str, _>(3).map(|s| s.to_string()),
                    is_pk:       r.get::<i32, _>(4).unwrap_or(0) == 1,
                    is_fk,
                    fk,
                }
            }).collect();

            // ── Table-level constraints: CHECK and multi-col UNIQUE ─────────
            let con_sql = format!("
                SELECT
                    tc.CONSTRAINT_NAME,
                    tc.CONSTRAINT_TYPE,
                    CASE
                        WHEN tc.CONSTRAINT_TYPE = 'CHECK'
                             THEN cc.CHECK_CLAUSE
                        WHEN tc.CONSTRAINT_TYPE = 'UNIQUE'
                             THEN (
                                SELECT STRING_AGG(kcu2.COLUMN_NAME, ', ')
                                       WITHIN GROUP (ORDER BY kcu2.ORDINAL_POSITION)
                                FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu2
                                WHERE kcu2.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
                             )
                        ELSE ''
                    END AS definition
                FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
                LEFT JOIN INFORMATION_SCHEMA.CHECK_CONSTRAINTS cc
                  ON tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME
                WHERE tc.TABLE_NAME      = '{table_name}'
                  AND tc.CONSTRAINT_TYPE IN ('CHECK', 'UNIQUE')
                ORDER BY tc.CONSTRAINT_TYPE, tc.CONSTRAINT_NAME");

            let con_rows = client.simple_query(&con_sql).await
                .map_err(|e| DbError::Schema(e.to_string()))?
                .into_first_result().await
                .map_err(|e| DbError::Schema(e.to_string()))?;

            let constraints: Vec<TableConstraint> = con_rows.iter().map(|r| TableConstraint {
                name:            r.get::<&str, _>(0).unwrap_or("").to_string(),
                constraint_type: r.get::<&str, _>(1).unwrap_or("").to_string(),
                definition:      r.get::<&str, _>(2).unwrap_or("").to_string(),
            }).collect();

            // ── Indexes ────────────────────────────────────────────────────
            let idx_sql = format!("
                SELECT
                    i.name AS index_name,
                    i.is_unique,
                    i.type_desc AS index_type,
                    STRING_AGG(c.name, ',') WITHIN GROUP (ORDER BY ic.key_ordinal) AS cols
                FROM sys.indexes i
                JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id
                JOIN sys.columns c        ON ic.object_id = c.object_id AND ic.column_id = c.column_id
                JOIN sys.tables t         ON i.object_id = t.object_id
                WHERE t.name = '{table_name}' AND i.is_primary_key = 0
                GROUP BY i.name, i.is_unique, i.type_desc");

            let idx_rows = client.simple_query(&idx_sql).await
                .map_err(|e| DbError::Schema(e.to_string()))?
                .into_first_result().await
                .map_err(|e| DbError::Schema(e.to_string()))?;

            let indexes: Vec<IndexMeta> = idx_rows.iter().map(|r| {
                // e.g. CLUSTERED / NONCLUSTERED / XML / SPATIAL / NONCLUSTERED COLUMNSTORE / NONCLUSTERED HASH
                let raw_type = r.get::<&str, _>(2).unwrap_or("NONCLUSTERED");
                IndexMeta {
                    name:       r.get::<&str, _>(0).unwrap_or("").to_string(),
                    unique:     r.get::<bool, _>(1).unwrap_or(false),
                    columns:    r.get::<&str, _>(3).unwrap_or("").split(',')
                                  .map(|s| s.to_string()).collect(),
                    index_type: Some(raw_type.to_lowercase().replace(' ', "_")),
                    predicate:  None,
                }
            }).collect();

            tables.push(TableMeta { name: table_name.clone(), columns, indexes, constraints });
        }

        let schema_text = build_schema_text(&tables, "mssql");
        Ok(SchemaMetadata {
            db_name: self.dbname.clone(),
            db_type: "mssql".into(),
            tables,
            schema_text,
        })
    }

    async fn execute_transaction(
        &self,
        commands: Vec<String>,
        dry_run:  bool,
    ) -> Result<TransactionResult, DbError> {
        let mut client = self.connect().await?;
        let mut outcomes   = Vec::new();
        let mut total_rows = 0u64;

        client.simple_query("BEGIN TRANSACTION").await
            .map_err(|e| DbError::Transaction(e.to_string()))?;

        for raw in &commands {
            let stmts: Vec<&str> = raw.split(';')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            for sql in stmts {
                match client.execute(sql, &[]).await {
                    Ok(r) => {
                        let affected = r.rows_affected().iter().sum::<u64>();
                        total_rows += affected;
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(), rows_affected: affected, ok: true, error: None,
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        outcomes.push(StatementOutcome {
                            sql: sql.to_string(), rows_affected: 0, ok: false, error: Some(msg.clone()),
                        });
                        client.simple_query("ROLLBACK TRANSACTION").await.ok();
                        return Ok(TransactionResult {
                            success: false, dry_run, rows_affected: total_rows,
                            statements: outcomes, error: Some(msg),
                        });
                    }
                }
            }
        }

        if dry_run {
            client.simple_query("ROLLBACK TRANSACTION").await
                .map_err(|e| DbError::Rollback(e.to_string()))?;
        } else {
            client.simple_query("COMMIT TRANSACTION").await
                .map_err(|e| DbError::Transaction(e.to_string()))?;
        }

        Ok(TransactionResult {
            success: true, dry_run, rows_affected: total_rows,
            statements: outcomes, error: None,
        })
    }

    async fn run_query(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        let rows_result = self.fetch_rows(sql).await;

        match rows_result {
            Ok(rows) => {
                if rows.is_empty() {
                    return Ok(crate::commands::query::QueryResult {
                        columns: vec![], rows: vec![], rows_affected: 0,
                        execution_ms: 0, error: None,
                capped: false,
            });
                }

                let columns: Vec<String> = rows[0].columns().iter()
                    .map(|c| c.name().to_string()).collect();

                let result_rows = rows.iter().map(|row| {
                    (0..columns.len()).map(|i| {
                        if let Some(v) = row.get::<i64, _>(i) {
                            return serde_json::Value::Number(v.into());
                        }
                        if let Some(v) = row.get::<f64, _>(i) {
                            return serde_json::Number::from_f64(v)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null);
                        }
                        if let Some(v) = row.get::<bool, _>(i) {
                            return serde_json::Value::Bool(v);
                        }
                        row.get::<&str, _>(i)
                            .map(|s| serde_json::Value::String(s.to_string()))
                            .unwrap_or(serde_json::Value::Null)
                    }).collect::<Vec<_>>()
                }).collect();

                Ok(crate::commands::query::QueryResult {
                    columns,
                    rows: result_rows,
                    rows_affected: rows.len() as u64,
                    execution_ms: 0,
                    error: None,
                capped: false,
            })
            }
            Err(_) => {
                let mut client = self.connect().await?;
                let affected = client.execute(sql, &[]).await
                    .map_err(|e| DbError::Transaction(e.to_string()))?
                    .rows_affected().iter().sum::<u64>();

                Ok(crate::commands::query::QueryResult {
                    columns: vec![], rows: vec![],
                    rows_affected: affected, execution_ms: 0, error: None,
                capped: false,
            })
            }
        }
    }
}

impl MssqlAdapter {
    async fn fetch_rows(&self, sql: &str) -> Result<Vec<tiberius::Row>, DbError> {
        let mut client = self.connect().await?;
        let rows = client.simple_query(sql).await
            .map_err(|e| DbError::Transaction(e.to_string()))?
            .into_first_result().await
            .map_err(|e| DbError::Transaction(e.to_string()))?;
        Ok(rows)
    }
}
