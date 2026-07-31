//! lib.rs — DbCompanion Tauri application

pub mod ai;
pub mod commands;
pub mod db;

use commands::{
    connect_db, disconnect, get_schema, test_connection, open_sql_file,
    plan_changes, execute_changes, ai_chat, ai_chat_stream, list_ai_providers,
    stronghold_save, stronghold_get, stronghold_delete,
    run_query, run_script, save_export_file,
};
use db::IDatabaseAdapter;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppState {
    pub adapter: Arc<Mutex<Option<Box<dyn IDatabaseAdapter>>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self { adapter: Arc::new(Mutex::new(None)) }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            // DB
            connect_db,
            disconnect,
            get_schema,
            test_connection,
            open_sql_file,
            // AI
            plan_changes,
            execute_changes,
            ai_chat,
            ai_chat_stream,
            list_ai_providers,
            // Vault
            stronghold_save,
            stronghold_get,
            stronghold_delete,
            // Query
            run_query,
            run_script,
            save_export_file,
        ])
        .run(tauri::generate_context!())
        .expect("error running DbCompanion");
}
