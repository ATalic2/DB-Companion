//! commands/query.rs — Execute arbitrary SQL queries and return results

use crate::{ActiveQuery, AppState};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Duration};
use tauri::State;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const ROW_CAP: usize = 1_000;

#[derive(Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub rows_affected: u64,
    pub execution_ms: u128,
    pub error: Option<String>,
    /// True when results were truncated at ROW_CAP.
    pub capped: bool,
}

#[derive(Deserialize)]
pub struct RunQueryArgs {
    pub sql: String,
    pub timeout_ms: Option<u64>,
}

enum QueryControlError {
    Cancelled,
    TimedOut,
}

async fn run_controlled<T>(
    state: &AppState,
    timeout_ms: Option<u64>,
    operation: impl Future<Output = T>,
) -> Result<T, QueryControlError> {
    let id = Uuid::new_v4();
    let cancellation = CancellationToken::new();
    {
        let mut active = state.active_query.lock().await;
        if let Some(previous) = active.replace(ActiveQuery {
            id,
            cancellation: cancellation.clone(),
        }) {
            previous.cancellation.cancel();
        }
    }

    let timeout_ms = timeout_ms.unwrap_or(60_000);
    let result = if timeout_ms == 0 {
        tokio::select! {
            value = operation => Ok(value),
            _ = cancellation.cancelled() => Err(QueryControlError::Cancelled),
        }
    } else {
        tokio::select! {
            value = operation => Ok(value),
            _ = cancellation.cancelled() => Err(QueryControlError::Cancelled),
            _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => Err(QueryControlError::TimedOut),
        }
    };

    let mut active = state.active_query.lock().await;
    if active.as_ref().is_some_and(|query| query.id == id) {
        *active = None;
    }
    result
}

#[tauri::command]
pub async fn cancel_query(state: State<'_, AppState>) -> Result<bool, String> {
    let cancellation = state
        .active_query
        .lock()
        .await
        .as_ref()
        .map(|query| query.cancellation.clone());
    if let Some(cancellation) = cancellation {
        cancellation.cancel();
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub async fn run_query(
    args: RunQueryArgs,
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<QueryResult, String> {
    use std::time::Instant;

    crate::commands::logger::log_info(
        &window,
        format!("Running query ({} characters)", args.sql.chars().count()),
        None,
    );

    let adapter = match state.adapter.lock().await.clone() {
        Some(a) => a,
        None => return Err("No database connected".into()),
    };

    let start = Instant::now();
    let result = run_controlled(&state, args.timeout_ms, adapter.run_query(&args.sql)).await;
    let ms = start.elapsed().as_millis();

    match result {
        Ok(Ok(mut qr)) => {
            qr.execution_ms = ms;
            let capped = qr.capped || qr.rows.len() > ROW_CAP;
            if capped {
                qr.rows.truncate(ROW_CAP);
            }
            qr.capped = capped;
            crate::commands::logger::log_info(
                &window,
                format!(
                    "Query OK — {} rows in {}ms{}",
                    qr.rows.len(),
                    ms,
                    if capped { " (capped)" } else { "" }
                ),
                None,
            );
            Ok(qr)
        }
        Ok(Err(e)) => {
            crate::commands::logger::log_error(&window, format!("Query error: {e}"), None);
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                execution_ms: ms,
                error: Some(e.to_string()),
                capped: false,
            })
        }
        Err(QueryControlError::Cancelled) => Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_ms: ms,
            error: Some("Query cancelled".into()),
            capped: false,
        }),
        Err(QueryControlError::TimedOut) => Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_ms: ms,
            error: Some(format!(
                "Query timed out after {} seconds",
                args.timeout_ms.unwrap_or(60_000) / 1_000
            )),
            capped: false,
        }),
    }
}

// ── Multi-statement script execution (currently MongoDB only) ──────────────

#[derive(Deserialize)]
pub struct RunScriptArgs {
    pub script: String,
    pub timeout_ms: Option<u64>,
}

#[tauri::command]
pub async fn run_script(
    args: RunScriptArgs,
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<QueryResult, String> {
    use std::time::Instant;

    crate::commands::logger::log_info(
        &window,
        format!(
            "Running script ({} characters)",
            args.script.chars().count()
        ),
        None,
    );

    let adapter = match state.adapter.lock().await.clone() {
        Some(a) => a,
        None => return Err("No database connected".into()),
    };

    let start = Instant::now();
    let result = run_controlled(&state, args.timeout_ms, adapter.run_script(&args.script)).await;
    let ms = start.elapsed().as_millis();

    match result {
        Ok(Ok(output)) => {
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
        Ok(Err(e)) => {
            crate::commands::logger::log_error(&window, format!("Script error: {e}"), None);
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                execution_ms: ms,
                error: Some(e.to_string()),
                capped: false,
            })
        }
        Err(QueryControlError::Cancelled) => Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_ms: ms,
            error: Some("Script cancelled".into()),
            capped: false,
        }),
        Err(QueryControlError::TimedOut) => Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            execution_ms: ms,
            error: Some(format!(
                "Script timed out after {} seconds",
                args.timeout_ms.unwrap_or(60_000) / 1_000
            )),
            capped: false,
        }),
    }
}

// ── Save-file dialog ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct SaveFileArgs {
    pub default_name: String,
    pub extension: String,
    pub content: String,
}

#[tauri::command]
pub fn save_export_file(args: SaveFileArgs, app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;

    let label = format!("{} files", args.extension.to_uppercase());
    let ext = args.extension.clone();

    let path = app
        .dialog()
        .file()
        .set_file_name(&args.default_name)
        .add_filter(&label, &[&ext])
        .blocking_save_file();

    match path {
        None => Ok(false),
        Some(p) => {
            std::fs::write(p.as_path().unwrap(), &args.content).map_err(|e| e.to_string())?;
            Ok(true)
        }
    }
}
