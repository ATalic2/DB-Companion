//! db/sqlite.rs — SQLite adapter using rusqlite + spawn_blocking

use super::{
    build_schema_text, ColumnMeta, DbError, ForeignKeyMeta, IDatabaseAdapter, IndexMeta,
    SchemaMetadata, StatementOutcome, TableConstraint, TableMeta, TransactionResult,
};
use async_trait::async_trait;
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::{path::PathBuf, sync::{Arc, Mutex}};

pub struct SqliteAdapter {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl SqliteAdapter {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DbError> {
        let path = path.into();
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| DbError::Connection(e.to_string()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| DbError::Connection(e.to_string()))?;

        Ok(Self { conn: Arc::new(Mutex::new(conn)), path })
    }
}

#[async_trait]
impl IDatabaseAdapter for SqliteAdapter {
    fn driver_name(&self) -> &str { "sqlite" }

    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError> {
        let conn    = Arc::clone(&self.conn);
        let db_name = self.path.file_name().unwrap_or_default()
            .to_string_lossy().into_owned();

        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(|e| DbError::Schema(e.to_string()))?;

            let mut stmt = guard
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .map_err(|e| DbError::Schema(e.to_string()))?;

            let table_names: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| DbError::Schema(e.to_string()))?
                .collect::<rusqlite::Result<_>>()
                .map_err(|e| DbError::Schema(e.to_string()))?;

            let mut tables = Vec::new();

            for table_name in table_names {
                // ── Columns via PRAGMA table_info ───────────────────────────
                let mut col_stmt = guard
                    .prepare(&format!("PRAGMA table_info(\"{}\")", table_name))
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                let mut columns: Vec<ColumnMeta> = col_stmt
                    .query_map([], |r| Ok(ColumnMeta {
                        name:        r.get(1)?,
                        data_type:   r.get(2)?,
                        nullable:    r.get::<_, i32>(3)? == 0,
                        is_pk:       r.get::<_, i32>(5)? > 0,
                        is_fk:       false,
                        fk:          None,
                        default_val: r.get(4)?,
                    }))
                    .map_err(|e| DbError::Schema(e.to_string()))?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                // ── FK details via PRAGMA foreign_key_list ──────────────────
                // Columns: id, seq, table, from, to, on_update, on_delete, match
                let mut fk_stmt = guard
                    .prepare(&format!("PRAGMA foreign_key_list(\"{}\")", table_name))
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                struct RawFk {
                    id:        i64,
                    from_col:  String,
                    ref_table: String,
                    ref_col:   String,
                    on_update: String,
                    on_delete: String,
                }
                let raw_fks: Vec<RawFk> = fk_stmt
                    .query_map([], |r| Ok(RawFk {
                        id:        r.get(0)?,
                        from_col:  r.get(3)?,
                        ref_table: r.get(2)?,
                        ref_col:   r.get(4)?,
                        on_update: r.get(5)?,
                        on_delete: r.get(6)?,
                    }))
                    .map_err(|e| DbError::Schema(e.to_string()))?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                for c in &mut columns {
                    if let Some(fk) = raw_fks.iter().find(|f| f.from_col == c.name) {
                        c.is_fk = true;
                        c.fk = Some(ForeignKeyMeta {
                            // SQLite doesn't store constraint names; synthesise one
                            constraint_name: format!("fk_{}_{}_{}_{}", table_name, c.name, fk.ref_table, fk.id),
                            ref_table:       fk.ref_table.clone(),
                            ref_column:      fk.ref_col.clone(),
                            on_delete:       fk.on_delete.clone(),
                            on_update:       fk.on_update.clone(),
                        });
                    }
                }

                // ── CHECK constraints from sqlite_master DDL ────────────────
                // SQLite doesn't have an information_schema for constraints;
                // we parse them out of the CREATE TABLE DDL stored in sqlite_master.
                let ddl: Option<String> = guard
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                        rusqlite::params![&table_name],
                        |r| r.get(0),
                    )
                    .ok();

                let constraints = extract_sqlite_checks(ddl.as_deref(), &table_name);

                // ── Indexes ─────────────────────────────────────────────────
                let mut idx_stmt = guard
                    .prepare(&format!("PRAGMA index_list(\"{}\")", table_name))
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                struct RawIdx { name: String, unique: bool }
                let raw: Vec<RawIdx> = idx_stmt
                    .query_map([], |r| Ok(RawIdx {
                        name:   r.get(1)?,
                        unique: r.get::<_, i32>(2)? == 1,
                    }))
                    .map_err(|e| DbError::Schema(e.to_string()))?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(|e| DbError::Schema(e.to_string()))?;

                let mut indexes = Vec::new();
                for ri in raw {
                    let mut info_stmt = guard
                        .prepare(&format!("PRAGMA index_info(\"{}\")", ri.name))
                        .map_err(|e| DbError::Schema(e.to_string()))?;
                    let cols: Vec<String> = info_stmt
                        .query_map([], |r| r.get(2))
                        .map_err(|e| DbError::Schema(e.to_string()))?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(|e| DbError::Schema(e.to_string()))?;
                    indexes.push(IndexMeta {
                        name:       ri.name,
                        columns:    cols,
                        unique:     ri.unique,
                        index_type: Some("btree".to_string()),
                        predicate:  None,
                    });
                }

                tables.push(TableMeta { name: table_name, columns, indexes, constraints });
            }

            let schema_text = build_schema_text(&tables, "sqlite");
            Ok(SchemaMetadata { db_name, db_type: "sqlite".into(), tables, schema_text })
        })
        .await
        .map_err(|e| DbError::Schema(e.to_string()))?
    }

    async fn execute_transaction(
        &self,
        commands: Vec<String>,
        dry_run:  bool,
    ) -> Result<TransactionResult, DbError> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(|e| DbError::Transaction(e.to_string()))?;

            let sp = "db_companion_sp";
            guard.execute_batch(&format!("SAVEPOINT {sp}"))
                .map_err(|e| DbError::Transaction(e.to_string()))?;

            let mut outcomes   = Vec::new();
            let mut total_rows = 0u64;
            let mut failed     = false;
            let mut fail_msg   = String::new();

            for sql in &commands {
                match guard.execute(sql.as_str(), []) {
                    Ok(rows) => {
                        total_rows += rows as u64;
                        outcomes.push(StatementOutcome {
                            sql: sql.clone(), rows_affected: rows as u64, ok: true, error: None,
                        });
                    }
                    Err(e) => {
                        fail_msg = e.to_string();
                        outcomes.push(StatementOutcome {
                            sql: sql.clone(), rows_affected: 0, ok: false, error: Some(fail_msg.clone()),
                        });
                        failed = true;
                        break;
                    }
                }
            }

            if failed || dry_run {
                guard.execute_batch(&format!("ROLLBACK TO SAVEPOINT {sp}; RELEASE {sp}"))
                    .map_err(|e| DbError::Rollback(e.to_string()))?;
            } else {
                guard.execute_batch(&format!("RELEASE {sp}"))
                    .map_err(|e| DbError::Transaction(e.to_string()))?;
            }

            Ok(TransactionResult {
                success:       !failed,
                dry_run,
                rows_affected: total_rows,
                statements:    outcomes,
                error:         if failed { Some(fail_msg) } else { None },
            })
        })
        .await
        .map_err(|e| DbError::Transaction(e.to_string()))?
    }

    async fn run_query(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        tokio::task::spawn_blocking({
            let adapter = SqliteAdapter { conn: Arc::clone(&self.conn), path: self.path.clone() };
            let sql = sql.to_string();
            move || adapter.run_query_impl(&sql)
        })
        .await
        .map_err(|e| DbError::Transaction(e.to_string()))?
    }
}

/// Extract CHECK constraints from a SQLite CREATE TABLE DDL string.
/// SQLite stores the original DDL in sqlite_master; we scan it for CHECK(...) clauses.
fn extract_sqlite_checks(ddl: Option<&str>, table_name: &str) -> Vec<TableConstraint> {
    let ddl = match ddl { Some(d) => d, None => return vec![] };
    let mut result = Vec::new();
    let upper = ddl.to_uppercase();
    let mut search = upper.as_str();
    let mut idx = 0usize;
    let mut counter = 0u32;

    while let Some(pos) = search.find("CHECK") {
        let abs = idx + pos;
        // Find the opening paren after CHECK
        let after = &ddl[abs + 5..];
        if let Some(paren_pos) = after.find('(') {
            let start = abs + 5 + paren_pos;
            // Walk chars tracking paren depth to find the matching close
            let mut depth = 0i32;
            let mut end = start;
            for (i, ch) in ddl[start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 { end = start + i + 1; break; }
                    }
                    _ => {}
                }
            }
            if end > start {
                let expr = ddl[start..end].to_string();
                counter += 1;
                result.push(TableConstraint {
                    name:            format!("chk_{}_{}", table_name, counter),
                    constraint_type: "CHECK".to_string(),
                    definition:      expr,
                });
            }
        }
        let advance = pos + 5;
        idx   += advance;
        search = &upper[idx..];
    }
    result
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

impl SqliteAdapter {
    pub fn run_query_impl(&self, sql: &str) -> Result<crate::commands::query::QueryResult, DbError> {
        let conn = Arc::clone(&self.conn);
        let guard = conn.lock().map_err(|e| DbError::Transaction(e.to_string()))?;

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
            match guard.prepare(stmt.as_str()) {
                Ok(mut prepared) => {
                    let col_count = prepared.column_count();
                    let columns: Vec<String> = (0..col_count)
                        .map(|i| prepared.column_name(i).unwrap_or("?").to_string())
                        .collect();

                    match prepared.query_map([], |row| {
                        Ok((0..col_count).map(|i| {
                            match row.get_ref(i) {
                                Ok(rusqlite::types::ValueRef::Integer(n)) => serde_json::Value::Number(n.into()),
                                Ok(rusqlite::types::ValueRef::Real(f))    => serde_json::Number::from_f64(f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
                                Ok(rusqlite::types::ValueRef::Text(t))    => serde_json::Value::String(String::from_utf8_lossy(t).into_owned()),
                                Ok(rusqlite::types::ValueRef::Blob(b))    => serde_json::Value::String(format!("<blob {} bytes>", b.len())),
                                _                                          => serde_json::Value::Null,
                            }
                        }).collect())
                    }) {
                        Ok(mapped) => {
                            let rows = mapped
                                .collect::<rusqlite::Result<Vec<_>>>()
                                .map_err(|e| DbError::Transaction(e.to_string()))?;
                            let count = rows.len() as u64;
                            last_result = crate::commands::query::QueryResult {
                                columns, rows, rows_affected: count, execution_ms: 0, error: None,
                capped: false,
            };
                        }
                        Err(_) => {
                            guard.execute_batch(stmt.as_str())
                                .map_err(|e| DbError::Transaction(e.to_string()))?;
                            last_result = crate::commands::query::QueryResult {
                                columns: vec![], rows: vec![], rows_affected: 0, execution_ms: 0, error: None,
                capped: false,
            };
                        }
                    }
                }
                Err(_) => {
                    guard.execute_batch(stmt.as_str())
                        .map_err(|e| DbError::Transaction(e.to_string()))?;
                    last_result = crate::commands::query::QueryResult {
                        columns: vec![], rows: vec![], rows_affected: 0, execution_ms: 0, error: None,
                capped: false,
            };
                }
            }
        }

        Ok(last_result)
    }
}
