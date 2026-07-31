//! commands/logger.rs — Structured logging via Tauri events
//!
//! Call log_info/log_error/log_request/log_response anywhere in Rust.
//! The frontend listens for "app_log" events and displays them in the console panel.

use serde::Serialize;
use tauri::{Emitter, Window};

#[derive(Serialize, Clone)]
pub struct LogEvent {
    pub level:   String,
    pub message: String,
    pub detail:  Option<String>,
}

fn emit(window: &Window, level: &str, message: impl Into<String>, detail: Option<String>) {
    let _ = window.emit("app_log", LogEvent {
        level:   level.into(),
        message: message.into(),
        detail,
    });
}

pub fn log_info(w: &Window, msg: impl Into<String>, detail: Option<String>) {
    emit(w, "info", msg, detail);
}

pub fn log_debug(w: &Window, msg: impl Into<String>, detail: Option<String>) {
    emit(w, "debug", msg, detail);
}

pub fn log_warn(w: &Window, msg: impl Into<String>) {
    emit(w, "warn", msg, None);
}

pub fn log_error(w: &Window, msg: impl Into<String>, detail: Option<String>) {
    emit(w, "error", msg, detail);
}

pub fn log_request(w: &Window, msg: impl Into<String>, body: Option<String>) {
    emit(w, "request", msg, body);
}

pub fn log_response(w: &Window, msg: impl Into<String>, body: Option<String>) {
    emit(w, "response", msg, body);
}
