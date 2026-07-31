//! commands/query.rs — Execute arbitrary SQL queries and return results

use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

pub const ROW_CAP: usize = 1_000;

#[derive(Serialize)]
pub struct QueryResult {
    pub columns:       Vec<String>,
    pub rows:          Vec<Vec<serde_json::Value>>,
    pub rows_affected: u64,
    pub execution_ms:  u128,
    pub error:         Option<String>,
    /// True when results were truncated at ROW_CAP.
    pub capped:        bool,
}

#[derive(Deserialize)]
pub struct RunQueryArgs {
    pub sql: String,
}

#[tauri::command]
pub async fn run_query(
    args:   RunQueryArgs,
    state:  State<'_, AppState>,
    window: tauri::Window,
) -> Result<QueryResult, String> {
    use std::time::Instant;

    let preview = args.sql.trim().chars().take(80).collect::<String>();
    crate::commands::logger::log_info(&window, format!("Query → {preview}…"), Some(args.sql.clone()));

    let guard = state.adapter.lock().await;
    let adapter = match &*guard {
        Some(a) => a,
        None    => return Err("No database connected".into()),
    };

    let start = Instant::now();
    let result = adapter.run_query(&args.sql).await;
    let ms = start.elapsed().as_millis();

    match result {
        Ok(mut qr) => {
            qr.execution_ms = ms;
            let capped = qr.rows.len() >= ROW_CAP;
            if capped { qr.rows.truncate(ROW_CAP); }
            qr.capped = capped;
            crate::commands::logger::log_info(&window,
                format!("Query OK — {} rows in {}ms{}", qr.rows.len(), ms,
                    if capped { " (capped)" } else { "" }), None);
            Ok(qr)
        }
        Err(e) => {
            crate::commands::logger::log_error(&window, format!("Query error: {e}"), None);
            Ok(QueryResult {
                columns: vec![], rows: vec![], rows_affected: 0,
                execution_ms: ms, error: Some(e.to_string()), capped: false,
            })
        }
    }
}

// ── Multi-statement script execution (currently MongoDB only) ──────────────

#[derive(Deserialize)]
pub struct RunScriptArgs {
    pub script: String,
}

#[tauri::command]
pub async fn run_script(
    args:   RunScriptArgs,
    state:  State<'_, AppState>,
    window: tauri::Window,
) -> Result<QueryResult, String> {
    use std::time::Instant;

    let preview = args.script.trim().chars().take(80).collect::<String>();
    crate::commands::logger::log_info(&window, format!("Script → {preview}…"), Some(args.script.clone()));

    let guard = state.adapter.lock().await;
    let adapter = match &*guard {
        Some(a) => a,
        None    => return Err("No database connected".into()),
    };

    let start = Instant::now();
    let result = adapter.run_script(&args.script).await;
    let ms = start.elapsed().as_millis();

    match result {
        Ok(output) => {
            crate::commands::logger::log_info(&window, format!("Script OK — {ms}ms"), None);
            Ok(QueryResult {
                columns: vec!["output".into()],
                rows: vec![vec![serde_json::Value::String(output)]],
                rows_affected: 0,
                execution_ms: ms,
                error: None,
                capped: false,
            })
        }
        Err(e) => {
            crate::commands::logger::log_error(&window, format!("Script error: {e}"), None);
            Ok(QueryResult {
                columns: vec![], rows: vec![], rows_affected: 0,
                execution_ms: ms, error: Some(e.to_string()), capped: false,
            })
        }
    }
}

// ── Save-file dialog ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct SaveFileArgs {
    pub default_name: String,
    pub extension:    String,
    pub content:      String,
}

#[tauri::command]
pub fn save_export_file(
    args: SaveFileArgs,
    app:  tauri::AppHandle,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;

    let label = format!("{} files", args.extension.to_uppercase());
    let ext   = args.extension.clone();

    let path = app
        .dialog()
        .file()
        .set_file_name(&args.default_name)
        .add_filter(&label, &[&ext])
        .blocking_save_file();

    match path {
        None    => Ok(false),
        Some(p) => {
            std::fs::write(p.as_path().unwrap(), &args.content)
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
    }
}
