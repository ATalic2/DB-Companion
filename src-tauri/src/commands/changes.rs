//! commands/changes.rs

use crate::{
    ai::{get_provider, ChatMessage, ChatRole, ChangePlan, ProviderCredentials},
    db::TransactionResult,
    AppState,
};
use serde::{Deserialize, Serialize};
use tauri::{State, Emitter};
use zeroize::Zeroizing;

// ── Structured chat response ──────────────────────────────────────────────────

/// What the AI returns from every chat message.
/// response_type = "answer" → plain conversational reply
/// response_type = "plan"   → change plan card with confirm/execute flow
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub response_type: String,
    #[serde(default)]
    pub message:       String, // default empty string if Gemini omits it
    pub plan:          Option<ChangePlan>,
}

// ── Chat ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatArgs {
    pub message:     String,
    pub history:     Vec<ChatMessageDto>,
    pub api_key:     String,
    pub provider_id: Option<String>,
    pub model:       Option<String>,
}

#[derive(Deserialize)]
pub struct ChatMessageDto {
    pub role:    String,
    pub content: String,
}

/// System prompt injected into every chat call.
/// Instructs the AI to return structured JSON so we always know
/// whether to render a plain answer or a plan card.
const MAX_HISTORY: usize = 20;

fn index_guidance() -> &'static str {
    r#"=== INDEX GUIDANCE ===

1. LOCK THE DIALECT FIRST
Before writing any DDL, explicitly state which database engine you are targeting. Verify every keyword, clause, and feature is valid for that engine specifically — do not reuse syntax from other engines. Infer the dialect from query syntax already shown (e.g. dbo. prefixes, NONCLUSTERED, ISNULL → SQL Server; CONCURRENTLY, ::cast, RETURNING → PostgreSQL; LIMIT/IFNULL → MySQL) and confirm it before writing anything. A SQL Server index with PostgreSQL syntax, or vice versa, is a hard error.

2. COMPOSITE INDEX COLUMN ORDERING — HARD RULE
Order columns in any composite index as: equality-filter columns first, then at most one range/inequality column last.
- Never place a BETWEEN, >, <, !=, or non-leading IS NOT NULL column before a later equality column — columns after the first range column cannot be used as index seek conditions.
- If a query has both equality and range filters, the equality column must come first regardless of how the WHERE clause is written.
- Order multiple equality columns by selectivity (most selective first) unless a specific ORDER BY forces otherwise.

3. FILTER + SORT COMPOSITE INDEXES — STATE STRUCTURAL LIMITS, DO NOT HEDGE
When a query has a WHERE filter on column A and an ORDER BY on column B:
- If A is an equality filter (=): a composite index (A, B DESC) serves both the filter and the sort. State this.
- If A is a range filter (BETWEEN, >, <): the index satisfies the range seek but the sort step WILL remain. This is a structural limitation, not a data question — do not hedge it as "depends on data." Say explicitly: "a sort step will remain regardless of data distribution."
- Do not claim a range-filter + sort composite index "handles both the filter and the sort." It does not.
- For paginated endpoints with range + ORDER BY, suggest keyset/cursor pagination and explain why offset pagination compounds the problem.

4. CONSISTENT STRATEGY ACROSS IDENTICAL QUERY SHAPES
If one query in a batch gets a filtered/partial index for a boolean-flag or IS NOT NULL pattern, every other query in the same batch with the same shape must get the same treatment. Solving the same pattern two different ways in one response is a correctness error, not a style choice.

5. COVERING INDEXES MUST EXACTLY MATCH THE SELECT LIST
When building a covering index, list every column in the query's SELECT clause and confirm each is either a key column or in the INCLUDE/covering list. When revising a previously-given index, explicitly diff it against the earlier version and confirm no covered column was silently dropped.

6. ENGINE-SPECIFIC FEATURE AVAILABILITY — FLAG, DO NOT APPLY SILENTLY
Never assume a feature is available. State availability explicitly before using it:
- Partial/filtered indexes (WHERE on CREATE INDEX): PostgreSQL ✓, SQLite ✓, SQL Server ✓ (filtered indexes), MySQL ✗.
- INCLUDE columns: PostgreSQL ✓, SQL Server ✓, MySQL ✗ — in MySQL, covering columns must go in the key itself, which increases write cost. Flag this tradeoff rather than silently applying an INCLUDE.
- CONCURRENTLY (non-locking build): PostgreSQL only.
- Descending index keys: PostgreSQL ✓, MySQL 8+ ✓, SQL Server ✓, SQLite ✗, older MySQL ✗.
- Expression/function indexes: PostgreSQL ✓, SQLite ✓, SQL Server ✓ (computed columns), MySQL 8+ ✓ (generated columns), older MySQL ✗.
If unsure whether a feature is available for the connected engine version, say so and offer a fallback.

7. STRUCTURAL IMPOSSIBILITY VS DATA-DEPENDENT UNCERTAINTY — NEVER CONFLATE THEM
- STRUCTURAL: some query shapes cannot be fully solved by a single index regardless of data distribution (e.g. range filter + unrelated ORDER BY, OR across different columns). State this directly: "No single index can fully satisfy both — here is the best available approximation and why."
- DATA-DEPENDENT: the index is structurally correct but its benefit depends on cardinality, selectivity, or query frequency. Label these with what the user should verify (e.g. "only beneficial if fewer than ~5% of rows match — verify with EXPLAIN ANALYZE before deploying").
Do not use "depends on data" as a hedge for a structural impossibility. These are different claims.

8. ADVISE ON VALIDATION — IN THE MESSAGE, NOT IN THE PLAN
Do not include EXPLAIN / EXPLAIN ANALYZE commands in the DDL plan steps — the Apply button should contain only the index creation DDL. Instead, in the accompanying answer message, tell the user what to run to verify each index after applying it (e.g. EXPLAIN ANALYZE for PostgreSQL, SET STATISTICS IO ON for SQL Server, explain("executionStats") for MongoDB). Keep the advisory concise — one line per index is enough.

9. WRITE-COST AND ROLLOUT ORDER
When proposing more than 5 new indexes on one table, note the cumulative write overhead on INSERT/UPDATE/DELETE and suggest deploying in order of query frequency — highest-impact first. If the table is large and live, recommend building one at a time and monitoring write latency between each.
=== END INDEX GUIDANCE ==="#
}


struct DbInstructions {
    indexes:      &'static str,
    json:         Option<&'static str>,
    live_changes: &'static str,
    extra:        Option<&'static str>,
    general:      &'static str,
}

impl DbInstructions {
    fn format(&self, db_name: &str) -> String {
        let mut out = format!("=== {}-SPECIFIC INSTRUCTIONS ===\n", db_name.to_uppercase());

        out.push_str("\nINDEXES:\n");
        out.push_str(self.indexes);

        if let Some(json) = self.json {
            out.push_str("\n\nJSON:\n");
            out.push_str(json);
        }

        out.push_str("\n\nSCHEMA CHANGES ON LIVE TABLES:\n");
        out.push_str(self.live_changes);

        if let Some(extra) = self.extra {
            out.push_str("\n\n");
            out.push_str(extra);
        }

        out.push_str("\n\nGENERAL:\n");
        out.push_str(self.general);

        out.push_str(&format!("\n=== END {}-SPECIFIC INSTRUCTIONS ===", db_name.to_uppercase()));
        out
    }
}

fn postgresql_instructions() -> DbInstructions {
    DbInstructions {
        indexes: "- B-tree: equality and range on scalar values (=, <, >, BETWEEN, ORDER BY). The default.
- Hash: equality-only (=). Ask the user if they only need equality — no range support.
- GIN: full-text search (tsvector/tsquery), JSONB containment (@>, ?), array overlap (&&). Use when searching inside documents or arrays.
- GiST: geometric/spatial data (PostGIS), full-text with ranking, range types, nearest-neighbour.
- BRIN: very large append-only tables where values correlate with physical row order (timestamps, sequential IDs). Tiny footprint, fast range scans.
- SP-GiST: non-balanced structures — IP ranges, geometric points.
- Partial indexes: always consider WHERE clauses (e.g. WHERE deleted_at IS NULL).
- Covering indexes: use INCLUDE (...) to add non-key columns for index-only scans.
- CREATE INDEX CONCURRENTLY: always recommend this over a plain CREATE INDEX on any table with live traffic — avoids locking the table.",
        json: Some("- Prefer jsonb over json — jsonb is binary, indexable with GIN, and supports operators. Plain json only makes sense when preserving exact input text matters.
- Index specific JSONB paths with expression indexes: CREATE INDEX ON t ((data->>'field'))." ),
        live_changes: "- ALTER TABLE on large tables acquires an ACCESS EXCLUSIVE lock. Mention this risk and suggest doing it during low-traffic windows or using CONCURRENTLY where applicable.
- After large bulk inserts or deletes, recommend VACUUM ANALYZE to update planner statistics.",
        extra: Some("PARTITIONING:
- For very large tables, consider RANGE, LIST, or HASH partitioning. Ask the user about data volume and access patterns before suggesting a strategy."),
        general: "- Always schema-qualify table names (e.g. public.users, not just users).
- Use $1, $2 placeholders for parameterised queries, never string interpolation.",
    }
}

fn mysql_instructions() -> DbInstructions {
    DbInstructions {
        indexes: "- B-tree: equality, range, ORDER BY. The default for InnoDB.
- FULLTEXT: full-text search on CHAR/VARCHAR/TEXT with MATCH ... AGAINST. Never suggest LIKE '%...%' for text search — always suggest FULLTEXT instead.
- SPATIAL: geometry columns (Point, LineString, Polygon) on InnoDB (MySQL 8+).
- Hash: MEMORY storage engine only — not suitable for disk-based tables.
- Composite indexes: column order matters — follow the leftmost prefix rule. Equality columns first, then sort, then range.
- Prefix indexes: for long VARCHAR/TEXT columns index only the first N characters: INDEX (col(n)).
- Invisible indexes (MySQL 8+): use to test removing an index without dropping it.
- Descending indexes (MySQL 8+): useful when ORDER BY mixes ASC and DESC.",
        json: Some("- Use the JSON column type (MySQL 5.7.8+) for semi-structured data. Index specific JSON paths via generated columns: ALTER TABLE t ADD COLUMN field VARCHAR(255) GENERATED ALWAYS AS (data->>'$.field'), then index the generated column."),
        live_changes: "- Large ALTER TABLE operations can lock the table. For tables with live traffic, mention pt-online-schema-change or gh-ost as safer alternatives.
- Always confirm the MySQL version — syntax and features differ significantly between 5.7, 8.0, and 8.4.",
        extra: Some("PRIMARY KEY IMPACT (InnoDB):
- InnoDB clusters data by primary key — every secondary index stores the PK value. A wide or random PK (e.g. UUID) causes page fragmentation and bloat. Ask the user about their PK strategy when designing new tables. Prefer AUTO_INCREMENT or ordered UUIDs (UUID_TO_BIN with swap_flag=1) for large tables."),
        general: "- Use backticks for identifiers that conflict with reserved words.
- Transactions: InnoDB supports transactions; MyISAM does not — always confirm the engine.",
    }
}

fn mssql_instructions() -> DbInstructions {
    DbInstructions {
        indexes: "- Clustered: determines physical row storage order. One per table, defaults to PRIMARY KEY. The clustered key is stored in every non-clustered index — a wide or frequently updated clustered key has cascading costs. Think carefully before suggesting a change.
- Non-clustered: separate structure with row pointers. Up to 999 per table.
- Columnstore (clustered or non-clustered): columnar storage for analytics and aggregation. If the table is used for reporting or large GROUP BY queries, ask the user if they want columnstore before defaulting to B-tree.
- Filtered: non-clustered with a WHERE clause — ideal for sparse columns and soft-delete patterns (WHERE IsDeleted = 0).
- Full-text: linguistic searches with CONTAINS / FREETEXT. Requires a Full-Text Catalog — ask the user before suggesting.
- Covering index with INCLUDE: add non-key columns to avoid key lookups.
- XML index: for xml typed columns — a primary XML index must exist before adding secondary ones.
- Spatial index: for geometry / geography columns.",
        json: None,
        live_changes: "- Large table modifications acquire schema locks. Mention this risk for tables with live traffic.
- Use SET NOCOUNT ON in stored procedures to suppress row-count messages.",
        extra: Some("QUERY HINTS AND RISKS:
- WITH (NOLOCK): commonly requested but dangerous — causes dirty reads and phantom rows. Always warn the user and suggest READ COMMITTED SNAPSHOT ISOLATION (RCSI) as a safer alternative.
- MERGE statement: prone to deadlocks and race conditions. Warn the user and suggest INSERT ... WHERE NOT EXISTS / UPDATE patterns instead.

DATA TYPES:
- Prefer DATETIME2 over DATETIME — higher precision, wider range, ANSI compliant.
- Use DATETIMEOFFSET when storing timezone-aware timestamps.
- Avoid NVARCHAR(MAX) / VARCHAR(MAX) unless necessary — they cannot be indexed directly."),
        general: "- Always use T-SQL syntax.
- Use square brackets for identifiers that conflict with reserved words: [order], [user].
- Wrap multi-statement changes in explicit transactions with TRY/CATCH.",
    }
}

fn sqlite_instructions() -> DbInstructions {
    DbInstructions {
        indexes: "- Partial indexes: CREATE INDEX ... WHERE ... — always consider these for filtered queries. Smaller and faster than full-table indexes.
- Expression indexes: index the result of an expression — e.g. CREATE INDEX idx ON t(lower(email)). Use when queries filter on expressions.
- Covering indexes: list all queried columns in the index so SQLite can do index-only scans without touching the table.
- FTS5 virtual tables: for full-text search — CREATE VIRTUAL TABLE ... USING fts5(...). Always suggest FTS5 over LIKE '%...%' for text search.",
        json: None,
        live_changes: "- No concurrent writers — SQLite only allows one write at a time. Not suitable for high-concurrency write workloads.",
        extra: Some("PRAGMAS — recommend these when relevant:
- PRAGMA journal_mode = WAL; — enables Write-Ahead Logging, much better for concurrent reads and writes. Recommend for any app with multiple connections.
- PRAGMA foreign_keys = ON; — foreign key enforcement is OFF by default. Always remind the user to enable this.
- PRAGMA cache_size = N; — increase for read-heavy workloads.
- PRAGMA synchronous = NORMAL; — safe performance improvement when using WAL mode.

SCHEMA DESIGN:
- WITHOUT ROWID tables: when the table has a natural non-integer PK and is frequently looked up by it — eliminates the hidden rowid and reduces storage.
- STRICT tables (SQLite 3.37+): enforce actual column type checking. Suggest for new tables where type safety matters.
- Generated columns (SQLite 3.31+): GENERATED ALWAYS AS — useful for indexing computed values.

LIMITATIONS TO MENTION WHEN RELEVANT:
- No ALTER COLUMN — changing a column type requires recreating the table.
- No DROP COLUMN before SQLite 3.35.
- No RIGHT JOIN or FULL OUTER JOIN before SQLite 3.39."),
        general: "- SQLite only has B-tree indexes — no other index types exist.
- Prefer typed affinity columns and STRICT mode for new schemas.",
    }
}

fn mongodb_instructions() -> DbInstructions {
    DbInstructions {
        indexes: "- Single field: basic equality and range on one field.
- Compound: multiple fields — always follow the ESR rule: Equality fields first, then Sort fields, then Range fields.
- Multikey: automatically used when indexing an array field — one index entry per array element. Cannot create a compound multikey index on two array fields simultaneously.
- Text: full-text search with $text. Only one text index per collection. Ask the user if they need language-aware stemming, multiple languages, or Atlas Search for richer features.
- 2dsphere: GeoJSON queries ($near, $geoWithin, $geoIntersects). Use for all modern geospatial work.
- 2d: legacy flat coordinate pairs only — prefer 2dsphere for new work.
- Hashed: hash-based sharding on a field. Equality only — no range queries.
- Wildcard: indexes all fields or a sub-document path. Useful for unpredictable schemas but expensive — always ask the user before suggesting.
- TTL: auto-deletes documents after a time period on a Date field. Use for sessions, logs, caches, or any expiry pattern.
- Partial: indexes only documents matching a filter expression. Greatly reduces index size — always consider for optional or sparse fields.
- Sparse: indexes only documents that contain the indexed field. Use for optional fields to avoid indexing missing values.",
        json: None,
        live_changes: "- Index builds on large collections are resource-intensive. Recommend building indexes during low-traffic windows or using the rolling index build procedure on replica sets.",
        extra: Some("AGGREGATION:
- Always put $match and $limit as early as possible in a pipeline to reduce the working set.
- Use $project early to drop unused fields.
- For large aggregations, suggest allowDiskUse: true if memory limits may be hit.
- Prefer $lookup with a pipeline over client-side joins — but warn that $lookup on large collections is expensive and an index on the foreign field is essential.

SCHEMA DESIGN:
- Embed documents when the child data is always accessed with the parent and has bounded size.
- Use references ($lookup) when the child is large, updated independently, or shared across multiple parents.
- Suggest $jsonSchema validation rules when the user is designing a new collection — MongoDB does not enforce types by default.
- Warn about unbounded arrays — arrays that grow indefinitely cause documents to exceed the 16MB BSON limit and degrade index performance.

TRANSACTIONS:
- Multi-document transactions are available on replica sets and sharded clusters (MongoDB 4.0+). Suggest them when the user needs atomicity across multiple collections.
- Warn that transactions have a 60-second timeout and carry performance overhead — use only when truly needed."),
        general: "- Always use JSON command documents, never SQL.
- Suggest explain(\"executionStats\") when the user reports a slow query.",
    }
}

fn db_specific_instructions(db_type: &str) -> String {
    match db_type {
        "postgresql"        => postgresql_instructions().format("PostgreSQL"),
        "mysql" => mysql_instructions().format("MySQL"),
        "mssql"             => mssql_instructions().format("SQL Server"),
        "sqlite"            => sqlite_instructions().format("SQLite"),
        "mongodb"           => mongodb_instructions().format("MongoDB"),
        _                   => String::new(),
    }
}


fn chat_system_prompt(schema_text: &str, db_type: &str) -> String {
    let is_mongo = db_type == "mongodb";

    let db_description = match db_type {
        "postgresql" => "PostgreSQL (relational — use standard SQL; always qualify table names with their schema, e.g. public.airlines not just airlines)",
        "mysql"      => "MySQL (relational — use standard SQL with InnoDB engine conventions)",
        "mssql"      => "Microsoft SQL Server (relational — use T-SQL syntax)",
        "sqlite"     => "SQLite (relational — use standard SQL with SQLite limitations)",
        "mongodb"    => "MongoDB (document database — NO SQL, use JSON command documents only)",
        _            => db_type,
    };

    let mode2 = if is_mongo {
        r#"--- MODE 2: PLAN ---
Use this when the user wants to query or modify data or collections.
Each step "command" must be a valid MongoDB JSON command document — never SQL. Examples:
  {"find":"users","filter":{"active":true}}
  {"insert":"orders","documents":[{"item":"widget","qty":10}]}
  {"createCollection":"logs"}
  {"drop":"temp"}
  {"createIndexes":"users","indexes":[{"key":{"email":1},"name":"email_idx","unique":true}]}
{
  "response_type": "plan",
  "message": "Brief explanation of what the plan does.",
  "plan": {
    "summary": "Short summary.",
    "risk_score": 1,
    "steps": [{ "type": "SQL", "command": "{\"find\":\"collection\",\"filter\":{}}", "description": "What this step does" }]
  }
}
Risk guide: find/count/aggregate = 1; insert/update = 2-4; delete/drop = 7-10."#
    } else {
        r#"--- MODE 2: PLAN ---
Use this when the user wants to run a query or change the schema (SELECT, INSERT, UPDATE, DELETE, DDL).
{
  "response_type": "plan",
  "message": "Brief explanation of what the plan does.",
  "plan": {
    "summary": "Short summary.",
    "risk_score": 1,
    "steps": [{ "type": "SQL", "command": "SELECT ...;", "description": "What this step does" }]
  }
}
Risk guide: SELECT = 1; DDL = 2-10 based on impact. Never use plan for a message with no SQL.
IMPORTANT: Always schema-qualify table names in SQL (e.g. public.airlines, not just airlines)."#
    };

    let empty_help = if is_mongo {
        "- Write createCollection commands to set up new collections\n\
         - Generate insert commands with sample documents\n\
         - Answer general MongoDB questions\n\
         - Propose a collection/document design from scratch"
    } else {
        "- Write CREATE TABLE statements to set up a new schema\n\
         - Generate INSERT statements with sample/test data\n\
         - Answer general SQL and database questions\n\
         - Propose a schema design from scratch"
    };

    format!(
        "You are an expert database assistant.\n\n\
         === CONNECTED DATABASE ===\n\
         Type: {db_description}\n\
         === END DATABASE INFO ===\n\n\
         === LIVE DATABASE SCHEMA ===\n\
         {schema_text}\n\
         === END SCHEMA ===\n\n\
         {index_guidance}\n\n\
         {db_specific}\n\n\
         If the schema is empty, you can still help:\n\
         {empty_help}\n\n\
         CRITICAL: Always respond with a raw JSON object — no markdown, no prose outside the JSON.\n\n\
         --- MODE 1: ANSWER ---\n\
         For greetings, explanations, or anything that does not require running a command.\n\
         IMPORTANT: The LIVE DATABASE SCHEMA above already includes each table/collection's\n\
         columns, indexes, and constraints. If the user asks about existing structure —\n\
         e.g. \"what indexes do I have\", \"what columns does X have\", \"is there a unique\n\
         constraint on Y\" — answer directly from the schema text in MODE 1. Do NOT propose\n\
         a plan just to run a command that would only re-fetch information already shown above.\n\
         Only use MODE 2 (PLAN) when you actually need to read or write data/documents, or\n\
         when the user asks about something not covered by the schema summary above.\n\
         {{\"response_type\":\"answer\",\"message\":\"Your reply here.\"}}\n\n\
         {mode2}\n\n\
         Note: conversation history is limited to {max_history} messages.",
        db_description = db_description,
        schema_text    = schema_text,
        index_guidance = index_guidance(),
        db_specific    = db_specific_instructions(db_type),
        empty_help     = empty_help,
        mode2          = mode2,
        max_history    = MAX_HISTORY,
    )
}

#[tauri::command]
pub async fn ai_chat(
    args:  ChatArgs,
    state: State<'_, AppState>,
) -> Result<ChatResponse, String> {
    let provider_id = args.provider_id.as_deref().unwrap_or("gemini");
    let provider    = get_provider(provider_id)
        .ok_or_else(|| format!("Unknown AI provider: {provider_id}"))?;

    let (schema_text, db_type) = {
        let guard = state.adapter.lock().await;
        match &*guard {
            Some(a) => match a.get_schema_metadata().await {
                Ok(meta) => {
                    let text = if meta.schema_text.trim().is_empty() {
                        "Schema was fetched but appears empty — the database may have no tables yet.".into()
                    } else {
                        meta.schema_text
                    };
                    (text, meta.db_type)
                }
                Err(e) => (
                    format!("Schema fetch failed: {e}. Tell the user the schema could not be loaded."),
                    a.driver_name().to_string(),
                ),
            },
            None => (
                "No database is currently connected. Tell the user to select a database profile in the sidebar.".into(),
                "unknown".into(),
            ),
        }
    };

    // Keep only the last MAX_HISTORY messages to stay within context limits.
    let trimmed_history: Vec<_> = args.history
        .iter()
        .rev()
        .take(MAX_HISTORY)
        .rev()
        .collect();

    let history: Vec<ChatMessage> = trimmed_history.iter().map(|m| ChatMessage {
        role: match m.role.as_str() {
            "assistant" => ChatRole::Assistant,
            "system"    => ChatRole::System,
            _           => ChatRole::User,
        },
        content: m.content.clone(),
    }).collect();

    let credentials = ProviderCredentials {
        api_key:  Some(Zeroizing::new(args.api_key)),
        base_url: None,
        model:    args.model,
    };

    let system = chat_system_prompt(&schema_text, &db_type);

    // Get raw reply from provider
    let raw = provider
        .chat(&credentials, &history, &system, &args.message)
        .await
        .map_err(|e| e.to_string())?;

    let clean = parse_response_raw(&raw);
    let mut response = serde_json::from_str::<ChatResponse>(clean)
        .map_err(|e| format!("Failed to parse AI response as JSON: {e}\nRaw: {clean}"))?;

    // Drop commentary steps with null/empty command or non-executable type.
    if let Some(plan) = response.plan.as_mut() {
        plan.steps.retain(|s| {
            matches!(s.step_type.to_uppercase().as_str(), "SQL" | "CLI")
                && s.command.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false)
        });
    }

    // If message is empty but we have a plan, use the summary as the message
    if response.message.trim().is_empty() {
        if let Some(ref plan) = response.plan {
            response.message = plan.summary.clone();
        } else {
            response.message = "Here is my response.".to_string();
        }
    }

    Ok(response)
}

// ── Streaming chat ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn ai_chat_stream(
    args:   ChatArgs,
    state:  State<'_, AppState>,
    window: tauri::Window,
) -> Result<ChatResponse, String> {
    let provider_id = args.provider_id.as_deref().unwrap_or("gemini");

    let provider = crate::ai::get_provider(provider_id)
        .ok_or_else(|| format!("Unknown AI provider: {provider_id}"))?;

    let (schema_text, db_type) = {
        let guard = state.adapter.lock().await;
        match &*guard {
            Some(a) => match a.get_schema_metadata().await {
                Ok(meta) => {
                    let text = if meta.schema_text.trim().is_empty() {
                        "Schema was fetched but appears empty.".into()
                    } else {
                        meta.schema_text
                    };
                    (text, meta.db_type)
                }
                Err(e) => (
                    format!("Schema fetch failed: {e}."),
                    a.driver_name().to_string(),
                ),
            },
            None => ("No database is currently connected.".into(), "unknown".into()),
        }
    };

    const MAX_HISTORY: usize = 20;
    let trimmed: Vec<_> = args.history.iter().rev().take(MAX_HISTORY).rev().collect();
    let history: Vec<crate::ai::ChatMessage> = trimmed.iter().map(|m| crate::ai::ChatMessage {
        role: match m.role.as_str() {
            "assistant" => crate::ai::ChatRole::Assistant,
            "system"    => crate::ai::ChatRole::System,
            _           => crate::ai::ChatRole::User,
        },
        content: m.content.clone(),
    }).collect();

    let credentials = crate::ai::ProviderCredentials {
        api_key:  Some(zeroize::Zeroizing::new(args.api_key.clone())),
        base_url: None,
        model:    args.model.clone(),
    };

    let system = chat_system_prompt(&schema_text, &db_type);

    let win     = window.clone();
    let log_win = window.clone();

    let (raw, req_body) = provider
        .chat_stream(&credentials, &history, &system, &args.message, Box::new(move |chunk| {
            let _ = win.emit("ai_chunk", &chunk);
        }))
        .await
        .map_err(|e| {
            crate::commands::logger::log_error(&log_win, format!("AI chat stream error: {e}"), None);
            e.to_string()
        })?;

    let model_label = args.model.as_deref().unwrap_or("default");
    crate::commands::logger::log_request(
        &window,
        format!("{provider_id} request → {model_label}"),
        if req_body.is_empty() { None } else { Some(req_body) },
    );

    let raw_owned = raw.clone();
    let raw = parse_response_raw(&raw_owned);
    let mut response = serde_json::from_str::<ChatResponse>(raw)
        .map_err(|e| format!("Failed to parse AI response: {e}\nRaw: {raw}"))?;

    // Drop any steps the AI emitted with null/empty command or a non-executable type
    // (e.g. commentary steps with "type": "answer" and "command": null).
    if let Some(plan) = response.plan.as_mut() {
        plan.steps.retain(|s| {
            matches!(s.step_type.to_uppercase().as_str(), "SQL" | "CLI")
                && s.command.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false)
        });
    }

    if response.message.trim().is_empty() {
        response.message = response.plan.as_ref()
            .map(|p| p.summary.clone())
            .unwrap_or_else(|| "Here is my response.".into());
    }

    crate::commands::logger::log_response(
        &window,
        format!("{provider_id} response → {model_label} → type={}", response.response_type),
        Some(raw_owned),
    );

    Ok(response)
}

/// Strip markdown code fences that some providers wrap JSON in.
fn parse_response_raw(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}


// ── Plan changes (direct, bypasses chat) ────────────────────────────────────

#[derive(Deserialize)]
pub struct PlanChangesArgs {
    pub user_intent:  String,
    pub api_key:      String,
    pub provider_id:  Option<String>,
    pub model:        Option<String>,
}

#[tauri::command]
pub async fn plan_changes(
    args:  PlanChangesArgs,
    state: State<'_, AppState>,
) -> Result<ChangePlan, String> {
    let provider_id = args.provider_id.as_deref().unwrap_or("gemini");
    let provider    = get_provider(provider_id)
        .ok_or_else(|| format!("Unknown AI provider: {provider_id}"))?;

    let schema = {
        let guard = state.adapter.lock().await;
        match &*guard {
            Some(a) => a.get_schema_metadata().await.map_err(|e| e.to_string())?,
            None    => return Err("No database connected".into()),
        }
    };

    let credentials = ProviderCredentials {
        api_key:  Some(Zeroizing::new(args.api_key)),
        base_url: None,
        model:    args.model,
    };

    // For Gemini, use post() directly so we can log exact bodies
    if provider_id == "gemini" {
        // plan_changes goes through the trait
        let result = provider
            .plan_changes(&credentials, &args.user_intent, &schema.schema_text)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result)
    } else {
        provider
            .plan_changes(&credentials, &args.user_intent, &schema.schema_text)
            .await
            .map_err(|e| e.to_string())
    }
}

// ── Execute changes ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExecuteChangesArgs {
    pub commands: Vec<String>,
    pub dry_run:  bool,
}

#[derive(Serialize)]
pub struct ExecuteResult {
    pub result: TransactionResult,
}

#[tauri::command]
pub async fn execute_changes(
    args:   ExecuteChangesArgs,
    state:  State<'_, AppState>,
    window: tauri::Window,
) -> Result<ExecuteResult, String> {
    let _mode = if args.dry_run { "dry run" } else { "execute" };
    crate::commands::logger::log_request(&window,
        format!("{} → {} statements", if args.dry_run { "Dry run" } else { "Execute" }, args.commands.len()),
        Some(serde_json::to_string(&args.commands).unwrap_or_default()),
    );
    let guard = state.adapter.lock().await;
    match &*guard {
        Some(a) => {
            // Prepend SET search_path for PostgreSQL so unqualified table names resolve correctly.
            // pg_dump files include SET search_path = '' which clears it, and AI-generated SQL
            // may not always schema-qualify table names.
            let commands = if a.driver_name() == "postgresql" {
                let mut cmds = vec!["SET search_path = public".to_string()];
                cmds.extend(args.commands);
                cmds
            } else {
                args.commands
            };
            let result = a.execute_transaction(commands, args.dry_run)
                .await
                .map_err(|e| {
                    crate::commands::logger::log_error(&window, format!("Change failed: {e}"), None);
                    e.to_string()
                })?;
            if result.success {
                crate::commands::logger::log_response(&window, format!("Execute succeeded · {} rows affected", result.rows_affected), None);
            } else {
                crate::commands::logger::log_error(&window, format!("Execute failed: {}", result.error.as_deref().unwrap_or("unknown")), None);
            }
            Ok(ExecuteResult { result })
        }
        None => Err("No database connected".into()),
    }
}

// ── List providers ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_ai_providers() -> Vec<crate::ai::ProviderInfo> {
    crate::ai::registered_providers()
}
