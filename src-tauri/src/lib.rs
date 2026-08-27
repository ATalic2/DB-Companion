//! lib.rs — DbCompanion Tauri application

pub mod ai;
pub mod commands;
pub mod db;

use commands::{
    ai_chat, ai_chat_stream, cancel_query, connect_db, disconnect, execute_changes, get_schema,
    list_ai_providers, open_sql_file, plan_changes, run_query, run_script, save_export_file,
    stronghold_delete, stronghold_get, stronghold_save, test_connection,
};
use std::sync::Arc;
use tokio::sync::Mutex;

pub type AdapterHandle = Arc<dyn db::IDatabaseAdapter>;

pub struct ActiveQuery {
    pub id: uuid::Uuid,
    pub cancellation: tokio_util::sync::CancellationToken,
}

pub struct AppState {
    // Commands clone the Arc while holding this mutex, then release the mutex
    // before doing any database I/O. This keeps disconnect/reconnect responsive.
    pub adapter: Arc<Mutex<Option<AdapterHandle>>>,
    pub schema: Arc<Mutex<Option<db::SchemaMetadata>>>,
    pub active_query: Arc<Mutex<Option<ActiveQuery>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            adapter: Arc::new(Mutex::new(None)),
            schema: Arc::new(Mutex::new(None)),
            active_query: Arc::new(Mutex::new(None)),
        }
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
            cancel_query,
            save_export_file,
        ])
        .run(tauri::generate_context!())
        .expect("error running DbCompanion");
}
