//! db.rs — Database adapter trait + shared types

pub mod postgres;
pub mod sqlite;
pub mod mysql;
pub mod mssql;
pub mod mongodb;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::commands::query::QueryResult;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Schema extraction failed: {0}")]
    Schema(String),
    #[error("Transaction error: {0}")]
    Transaction(String),
    #[error("Rollback triggered — reason: {0}")]
    Rollback(String),
}

/// Full FK detail for a column — target table/column, constraint name, actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyMeta {
    pub constraint_name: String,
    pub ref_table:       String,
    pub ref_column:      String,
    pub on_delete:       String, // NO ACTION | CASCADE | SET NULL | SET DEFAULT | RESTRICT
    pub on_update:       String,
}

/// A table-level constraint (CHECK or UNIQUE spanning multiple columns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConstraint {
    pub name:            String,
    pub constraint_type: String, // "CHECK" | "UNIQUE"
    /// For CHECK: the expression. For UNIQUE: comma-separated column list.
    pub definition:      String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name:        String,
    pub data_type:   String,
    pub nullable:    bool,
    pub is_pk:       bool,
    pub is_fk:       bool,
    /// Full FK details when is_fk = true.
    pub fk:          Option<ForeignKeyMeta>,
    pub default_val: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub name:       String,
    pub columns:    Vec<String>,
    pub unique:     bool,
    pub index_type: Option<String>, // e.g. btree, hash, gin, gist, brin
    pub predicate:  Option<String>, // partial index WHERE clause, if any
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name:        String,
    pub columns:     Vec<ColumnMeta>,
    pub indexes:     Vec<IndexMeta>,
    /// Table-level CHECK and multi-column UNIQUE constraints.
    pub constraints: Vec<TableConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaMetadata {
    pub db_name:     String,
    pub db_type:     String,
    pub tables:      Vec<TableMeta>,
    pub schema_text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatementOutcome {
    pub sql:           String,
    pub rows_affected: u64,
    pub ok:            bool,
    pub error:         Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResult {
    pub success:       bool,
    pub dry_run:       bool,
    pub rows_affected: u64,
    pub statements:    Vec<StatementOutcome>,
    pub error:         Option<String>,
}

#[async_trait]
pub trait IDatabaseAdapter: Send + Sync {
    fn driver_name(&self) -> &str;
    async fn get_schema_metadata(&self) -> Result<SchemaMetadata, DbError>;
    async fn execute_transaction(
        &self, commands: Vec<String>, dry_run: bool,
    ) -> Result<TransactionResult, DbError>;
    async fn run_query(&self, sql: &str) -> Result<QueryResult, DbError>;

    /// Run a multi-statement admin/setup script (e.g. a sequence of
    /// db.createCollection(...)/createIndex(...) calls). Only meaningful
    /// for drivers that support scripting a full session in one shot;
    /// defaults to unsupported for SQL adapters.
    async fn run_script(&self, _script: &str) -> Result<String, DbError> {
        Err(DbError::Transaction(
            "Script mode is only supported for MongoDB connections.".into(),
        ))
    }
}

pub fn build_schema_text(tables: &[TableMeta], driver: &str) -> String {
    let mut out = format!("Database driver: {driver}\n\n");
    for t in tables {
        out.push_str(&format!("TABLE {}\n", t.name));

        for c in &t.columns {
            let mut flags = vec![];
            if c.is_pk { flags.push("PRIMARY KEY".to_string()); }
            if !c.nullable { flags.push("NOT NULL".to_string()); }
            if let Some(d) = &c.default_val {
                flags.push(format!("DEFAULT {d}"));
            }

            let flag_str = if flags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", flags.join(", "))
            };

            out.push_str(&format!("  {} {}{}\n", c.name, c.data_type, flag_str));

            // FK detail on its own indented line so it's unambiguous
            if let Some(fk) = &c.fk {
                out.push_str(&format!(
                    "    FK: CONSTRAINT {} REFERENCES {}.{} ON DELETE {} ON UPDATE {}\n",
                    fk.constraint_name, fk.ref_table, fk.ref_column,
                    fk.on_delete, fk.on_update,
                ));
            }
        }

        // Table-level constraints (CHECK, multi-col UNIQUE)
        if !t.constraints.is_empty() {
            out.push_str("  Constraints:\n");
            for con in &t.constraints {
                out.push_str(&format!(
                    "    {} {} ({})\n",
                    con.constraint_type, con.name, con.definition
                ));
            }
        }

        if !t.indexes.is_empty() {
            out.push_str("  Indexes:\n");
            for idx in &t.indexes {
                let kind = if idx.unique { "UNIQUE" } else { "INDEX" };
                let idx_type = idx.index_type.as_deref().unwrap_or("btree");
                let base = format!("    {} {} USING {} ON ({})",
                    kind, idx.name, idx_type, idx.columns.join(", "));
                let with_pred = match &idx.predicate {
                    Some(pred) => format!("{} WHERE {}\n", base, pred),
                    None       => format!("{}\n", base),
                };
                out.push_str(&with_pred);
            }
        }

        out.push('\n');
    }
    out
}
