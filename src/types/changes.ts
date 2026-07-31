// src/types/changes.ts — mirrors Rust structs exactly

export interface ForeignKeyMeta {
  constraint_name: string;
  ref_table:       string;
  ref_column:      string;
  on_delete:       string;
  on_update:       string;
}

export interface ColumnMeta {
  name:        string;
  data_type:   string;
  nullable:    boolean;
  is_pk:       boolean;
  is_fk:       boolean;
  fk:          ForeignKeyMeta | null;
  default_val: string | null;
}

export interface IndexMeta {
  name:       string;
  columns:    string[];
  unique:     boolean;
  index_type: string | null;
  predicate:  string | null;
}

export interface TableConstraint {
  name:            string;
  constraint_type: string; // "CHECK" | "UNIQUE"
  definition:      string;
}

export interface TableMeta {
  name:        string;
  columns:     ColumnMeta[];
  indexes:     IndexMeta[];
  constraints: TableConstraint[];
}

export interface SchemaMetadata {
  db_name:     string;
  db_type:     string;
  tables:      TableMeta[];
  schema_text: string;
}

export interface ChangeStep {
  type:        "SQL" | "CLI";
  command:     string;
  description: string;
}

export interface ChangePlan {
  summary:    string;
  risk_score: number; // 1–10
  steps:      ChangeStep[];
}

export interface StatementOutcome {
  sql:           string;
  rows_affected: number;
  ok:            boolean;
  error:         string | null;
}

export interface TransactionResult {
  success:       boolean;
  dry_run:       boolean;
  rows_affected: number;
  statements:    StatementOutcome[];
  error:         string | null;
}

export type DbType = "postgresql" | "sqlite";

export interface Connection {
  id:      string;
  label:   string;
  db_type: DbType;
  dsn:     string;  // postgres DSN or sqlite file path
}

export type Step = 0 | 1 | 2 | 3;
