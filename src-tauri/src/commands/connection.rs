//! commands/connection.rs — DB connection Tauri commands

use crate::{
    db::{
        postgres::PostgresAdapter,
        sqlite::SqliteAdapter,
        mysql::MySqlAdapter,
        mssql::MssqlAdapter,
        mongodb::MongoAdapter,
        IDatabaseAdapter, SchemaMetadata,
    },
    AppState,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use zeroize::Zeroizing;

#[derive(Deserialize)]
pub struct ConnectArgs {
    pub db_type:     String,
    pub host:        Option<String>,
    pub port:        Option<String>,
    pub user:        Option<String>,
    pub password:    Option<String>,
    pub dbname:      Option<String>,
    pub filepath:    Option<String>,
    pub dsn:         Option<String>,
    pub instance:    Option<String>,
    pub auth_source: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectResult {
    pub success: bool,
    pub db_name: String,
    pub db_type: String,
    pub error:   Option<String>,
}

#[tauri::command]
pub async fn connect_db(
    args:   ConnectArgs,
    state:  State<'_, AppState>,
    window: tauri::Window,
) -> Result<ConnectResult, String> {

    let result: Result<Box<dyn IDatabaseAdapter>, String> = match args.db_type.as_str() {

        "postgresql" => {
            let dsn = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or("")).into_owned();
                let pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    user, pass,
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("5432"),
                    args.dbname.as_deref().unwrap_or(""),
                )
            });
            PostgresAdapter::connect(dsn).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "sqlite" => {
            let path = args.filepath.as_deref().unwrap_or("").to_string();
            SqliteAdapter::open(&path)
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "mysql" => {
            let dsn = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                MySqlAdapter::dsn(
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("3306"),
                    args.user.as_deref().unwrap_or(""),
                    args.password.as_deref().unwrap_or(""),
                    args.dbname.as_deref().unwrap_or(""),
                )
            });
            MySqlAdapter::connect(dsn).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "mssql" => {
            let password = Zeroizing::new(args.password.clone().unwrap_or_default());
            MssqlAdapter::new(
                args.host.as_deref().unwrap_or("localhost"),
                args.port.as_deref().unwrap_or("1433"),
                args.user.as_deref().unwrap_or(""),
                password,
                args.dbname.as_deref().unwrap_or(""),
                args.instance.as_deref(),
            )
            .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
            .map_err(|e| e.to_string())
        }

        "mongodb" => {
            let uri = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or("")).into_owned();
                let pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                let host = args.host.as_deref().unwrap_or("localhost");
                let port = args.port.as_deref().unwrap_or("27017");
                let auth_source = args.auth_source.as_deref().unwrap_or("admin");
                let db = args.dbname.as_deref().unwrap_or("admin");
                if user.is_empty() {
                    format!("mongodb://{host}:{port}/{db}?authSource={auth_source}")
                } else {
                    format!("mongodb://{user}:{pass}@{host}:{port}/{db}?authSource={auth_source}")
                }
            });
            let redacted = {
                let raw_pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                if raw_pass.is_empty() { uri.to_string() }
                else { uri.replace(&raw_pass, "REDACTED") }
            };
            MongoAdapter::connect(uri, args.dbname.as_deref().unwrap_or("test")).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| format!("{e} [uri: {redacted}]"))
        }

        other => Err(format!("Unsupported db_type: {other}")),
    };

    match result {
        Ok(adapter) => {
            let db_type = args.db_type.clone();
            let db_name = adapter.driver_name().to_string();
            crate::commands::logger::log_info(&window, format!("Connected to {} ({})", db_name, db_type), None);
            *state.adapter.lock().await = Some(adapter);
            Ok(ConnectResult { success: true, db_name, db_type, error: None })
        }
        Err(e) => {
            crate::commands::logger::log_error(&window, format!("Connection failed: {e}"), None);
            Ok(ConnectResult {
                success: false,
                db_name: String::new(),
                db_type: args.db_type,
                error:   Some(e),
            })
        }
    }
}

#[tauri::command]
pub async fn get_schema(state: State<'_, AppState>, window: tauri::Window) -> Result<SchemaMetadata, String> {
    let guard = state.adapter.lock().await;
    match &*guard {
        Some(a) => {
            crate::commands::logger::log_info(&window, "Fetching schema…", None);
            let result = a.get_schema_metadata().await.map_err(|e| e.to_string());
            match &result {
                Ok(s)  => {
                    let index_summary: Vec<String> = s.tables.iter()
                        .map(|t| format!("{}: {} indexes", t.name, t.indexes.len()))
                        .collect();
                    crate::commands::logger::log_info(&window, format!(
                        "Schema loaded — {} tables | {}",
                        s.tables.len(),
                        index_summary.join(", ")
                    ), None);
                }
                Err(e) => crate::commands::logger::log_error(&window, format!("Schema error: {e}"), None),
            }
            result
        }
        None => Err("No database connected".into()),
    }
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    *state.adapter.lock().await = None;
    Ok(())
}

#[derive(Serialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

/// Build a throw-away adapter and immediately drop it — never stored in state.
/// Times out after 8 seconds so the UI doesn't hang on unreachable hosts.
#[tauri::command]
pub async fn test_connection(args: ConnectArgs) -> Result<TestResult, String> {
    let connect_future = async {
        let result: Result<Box<dyn IDatabaseAdapter>, String> = match args.db_type.as_str() {

        "postgresql" => {
            let dsn = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or("")).into_owned();
                let pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    user, pass,
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("5432"),
                    args.dbname.as_deref().unwrap_or(""),
                )
            });
            PostgresAdapter::connect(dsn).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "sqlite" => {
            let path = args.filepath.as_deref().unwrap_or("").to_string();
            SqliteAdapter::open(&path)
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "mysql" => {
            let dsn = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                MySqlAdapter::dsn(
                    args.host.as_deref().unwrap_or("localhost"),
                    args.port.as_deref().unwrap_or("3306"),
                    args.user.as_deref().unwrap_or(""),
                    args.password.as_deref().unwrap_or(""),
                    args.dbname.as_deref().unwrap_or(""),
                )
            });
            MySqlAdapter::connect(dsn).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| e.to_string())
        }

        "mssql" => {
            let password = Zeroizing::new(args.password.clone().unwrap_or_default());
            MssqlAdapter::new(
                args.host.as_deref().unwrap_or("localhost"),
                args.port.as_deref().unwrap_or("1433"),
                args.user.as_deref().unwrap_or(""),
                password,
                args.dbname.as_deref().unwrap_or(""),
                args.instance.as_deref(),
            )
            .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
            .map_err(|e| e.to_string())
        }

        "mongodb" => {
            let uri = Zeroizing::new(if let Some(d) = &args.dsn {
                d.clone()
            } else {
                let user = urlencoding::encode(args.user.as_deref().unwrap_or("")).into_owned();
                let pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                let host = args.host.as_deref().unwrap_or("localhost");
                let port = args.port.as_deref().unwrap_or("27017");
                let auth_source = args.auth_source.as_deref().unwrap_or("admin");
                let db = args.dbname.as_deref().unwrap_or("admin");
                if user.is_empty() {
                    format!("mongodb://{host}:{port}/{db}?authSource={auth_source}")
                } else {
                    format!("mongodb://{user}:{pass}@{host}:{port}/{db}?authSource={auth_source}")
                }
            });
            let redacted = {
                let raw_pass = urlencoding::encode(args.password.as_deref().unwrap_or("")).into_owned();
                if raw_pass.is_empty() { uri.to_string() }
                else { uri.replace(&raw_pass, "REDACTED") }
            };
            MongoAdapter::connect(uri, args.dbname.as_deref().unwrap_or("test")).await
                .map(|a| Box::new(a) as Box<dyn IDatabaseAdapter>)
                .map_err(|e| format!("{e} [uri: {redacted}]"))
        }

        other => Err(format!("Unsupported db_type: {other}")),
    };

    match result {
        Ok(adapter) => {
            let name = adapter.driver_name().to_string();
            // adapter is dropped here — connection closed
            Ok(TestResult {
                success: true,
                message: format!("Connected to {name} successfully."),
            })
        }
        Err(e) => Ok(TestResult { success: false, message: e }),
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(8), connect_future).await {
        Ok(inner) => inner,
        Err(_)    => Ok(TestResult {
            success: false,
            message: "Connection timed out after 8 seconds.".into(),
        }),
    }
}

/// Open a file-picker dialog and return the file's text content.
/// The Tauri dialog plugin handles the picker; std::fs reads the file.
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
        Some(p) => {
            let content = std::fs::read_to_string(p.into_path().map_err(|e| format!("Invalid path: {e}"))?)
                .map_err(|e| format!("Failed to read file: {e}"))?;
            Ok(Some(content))
        }
        None => Ok(None), // user cancelled
    }
}
