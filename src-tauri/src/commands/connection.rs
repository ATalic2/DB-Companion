//! Database connection and schema commands.

use crate::{
    db::{
        mongodb::MongoAdapter, mssql::MssqlAdapter, mysql::MySqlAdapter, postgres::PostgresAdapter,
        sqlite::SqliteAdapter, SchemaMetadata,
    },
    AdapterHandle, AppState,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use zeroize::Zeroizing;

#[derive(Deserialize)]
pub struct ConnectArgs {
    pub db_type: String,
    pub host: Option<String>,
    pub port: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub dbname: Option<String>,
    pub filepath: Option<String>,
    pub dsn: Option<String>,
    pub instance: Option<String>,
    pub auth_source: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectResult {
    pub success: bool,
    pub db_name: String,
    pub db_type: String,
    pub error: Option<String>,
}

/// The sole adapter-construction path, shared by connect and connection test.
async fn create_adapter(args: &ConnectArgs) -> Result<AdapterHandle, String> {
    match args.db_type.as_str() {
        "postgresql" => {
            let dsn = Zeroizing::new(if let Some(dsn) = &args.dsn {
                dsn.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or(""));
                let password = urlencoding::encode(args.password.as_deref().unwrap_or(""));
                format!(
                    "postgres://{user}:{password}@{}:{}/{}",
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("5432"),
                    args.dbname.as_deref().unwrap_or("")
                )
            });
            PostgresAdapter::connect(dsn)
                .await
                .map(|adapter| Arc::new(adapter) as AdapterHandle)
                .map_err(|error| error.to_string())
        }
        "sqlite" => SqliteAdapter::open(args.filepath.as_deref().unwrap_or(""))
            .map(|adapter| Arc::new(adapter) as AdapterHandle)
            .map_err(|error| error.to_string()),
        "mysql" => {
            let dsn = Zeroizing::new(if let Some(dsn) = &args.dsn {
                dsn.clone()
            } else {
                MySqlAdapter::dsn(
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("3306"),
                    args.user.as_deref().unwrap_or(""),
                    args.password.as_deref().unwrap_or(""),
                    args.dbname.as_deref().unwrap_or(""),
                )
            });
            MySqlAdapter::connect(dsn)
                .await
                .map(|adapter| Arc::new(adapter) as AdapterHandle)
                .map_err(|error| error.to_string())
        }
        "mssql" => {
            let adapter = if let Some(dsn) = &args.dsn {
                MssqlAdapter::from_connection_string(dsn)
            } else {
                MssqlAdapter::new(
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("1433"),
                    args.user.as_deref().unwrap_or(""),
                    Zeroizing::new(args.password.clone().unwrap_or_default()),
                    args.dbname.as_deref().unwrap_or(""),
                    args.instance.as_deref(),
                )
            };
            adapter
                .map(|adapter| Arc::new(adapter) as AdapterHandle)
                .map_err(|error| error.to_string())
        }
        "mongodb" => {
            let uri = Zeroizing::new(if let Some(dsn) = &args.dsn {
                dsn.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or(""));
                let password = urlencoding::encode(args.password.as_deref().unwrap_or(""));
                let host = args.host.as_deref().unwrap_or("localhost");
                let port = args.port.as_deref().unwrap_or("27017");
                let database = args.dbname.as_deref().unwrap_or("admin");
                let auth_source = args.auth_source.as_deref().unwrap_or("admin");
                if user.is_empty() {
                    format!("mongodb://{host}:{port}/{database}?authSource={auth_source}")
                } else {
                    format!("mongodb://{user}:{password}@{host}:{port}/{database}?authSource={auth_source}")
                }
            });
            MongoAdapter::connect(uri, args.dbname.as_deref().unwrap_or("test"))
                .await
                .map(|adapter| Arc::new(adapter) as AdapterHandle)
                .map_err(|error| error.to_string())
        }
        other => Err(format!("Unsupported db_type: {other}")),
    }
}

#[tauri::command]
pub async fn connect_db(
    args: ConnectArgs,
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<ConnectResult, String> {
    match create_adapter(&args).await {
        Ok(adapter) => {
            let db_type = args.db_type;
            let db_name = adapter.driver_name().to_string();
            crate::commands::logger::log_info(
                &window,
                format!("Connected to {db_name} ({db_type})"),
                None,
            );
            *state.adapter.lock().await = Some(adapter);
            *state.schema.lock().await = None;
            Ok(ConnectResult {
                success: true,
                db_name,
                db_type,
                error: None,
            })
        }
        Err(error) => {
            // The UI receives the error, but logs do not persist connection
            // strings which a driver may include in its diagnostic text.
            crate::commands::logger::log_error(&window, "Connection failed", None);
            Ok(ConnectResult {
                success: false,
                db_name: String::new(),
                db_type: args.db_type,
                error: Some(error),
            })
        }
    }
}

#[tauri::command]
pub async fn get_schema(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<SchemaMetadata, String> {
    if let Some(schema) = state.schema.lock().await.clone() {
        return Ok(schema);
    }
    let adapter = state
        .adapter
        .lock()
        .await
        .clone()
        .ok_or_else(|| "No database connected".to_string())?;
    crate::commands::logger::log_info(&window, "Fetching schema…", None);
    let result = adapter
        .get_schema_metadata()
        .await
        .map_err(|error| error.to_string());
    match &result {
        Ok(schema) => {
            crate::commands::logger::log_info(
                &window,
                format!("Schema loaded — {} tables", schema.tables.len()),
                None,
            );
            *state.schema.lock().await = Some(schema.clone());
        }
        Err(_) => crate::commands::logger::log_error(&window, "Schema load failed", None),
    }
    result
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(active) = state.active_query.lock().await.take() {
        active.cancellation.cancel();
    }
    *state.adapter.lock().await = None;
    *state.schema.lock().await = None;
    Ok(())
}

#[derive(Serialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

#[tauri::command]
pub async fn test_connection(args: ConnectArgs) -> Result<TestResult, String> {
    match tokio::time::timeout(std::time::Duration::from_secs(8), create_adapter(&args)).await {
        Ok(Ok(adapter)) => Ok(TestResult {
            success: true,
            message: format!("Connected to {} successfully.", adapter.driver_name()),
        }),
        Ok(Err(error)) => Ok(TestResult {
            success: false,
            message: error,
        }),
        Err(_) => Ok(TestResult {
            success: false,
            message: "Connection timed out after 8 seconds.".into(),
        }),
    }
}

#[tauri::command]
pub async fn open_sql_file(window: tauri::Window) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = window
        .dialog()
        .file()
        .add_filter("SQL files", &["sql", "SQL"])
        .add_filter("Text files", &["txt"])
        .add_filter("All files", &["*"])
        .blocking_pick_file();
    match path {
        Some(path) => {
            let path = path
                .into_path()
                .map_err(|error| format!("Invalid path: {error}"))?;
            std::fs::read_to_string(path)
                .map(Some)
                .map_err(|error| format!("Failed to read file: {error}"))
        }
        None => Ok(None),
    }
}
