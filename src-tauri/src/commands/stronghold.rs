//! commands/stronghold.rs — Encrypted secret storage
//!
//! Stores secrets in an AES-256-GCM encrypted JSON file in the app data dir.
//! The encryption key is derived from username + hostname using SHA-256.
//! The vault file is deleted when the user removes their app data directory.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};

const VAULT_FILE: &str = "dbcompanion_vault.json";

#[derive(Serialize, Deserialize, Default)]
struct Vault {
    /// Each entry: base64(nonce) -> base64(ciphertext)
    entries: HashMap<String, (String, String)>,
}

fn derive_key() -> Key<Aes256Gcm> {
    let username = whoami::username();
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown".into());
    let mut hasher = Sha256::new();
    hasher.update(format!("dbcompanion-vault:{}:{}", username, hostname).as_bytes());
    let hash = hasher.finalize();
    *Key::<Aes256Gcm>::from_slice(&hash)
}

fn vault_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(VAULT_FILE))
}

fn load_vault(app: &AppHandle) -> Result<Vault, String> {
    let path = vault_path(app)?;
    if !path.exists() {
        return Ok(Vault::default());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

fn save_vault(app: &AppHandle, vault: &Vault) -> Result<(), String> {
    let path = vault_path(app)?;
    let data = serde_json::to_string(vault).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stronghold_save(
    app:   AppHandle,
    key:   String,
    value: String,
) -> Result<(), String> {
    let cipher = Aes256Gcm::new(&derive_key());
    let nonce  = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, value.as_bytes())
        .map_err(|e| format!("Encryption failed: {e}"))?;

    let mut vault = load_vault(&app)?;
    vault.entries.insert(
        key,
        (B64.encode(nonce), B64.encode(ciphertext)),
    );
    save_vault(&app, &vault)?;
    Ok(())
}

#[tauri::command]
pub async fn stronghold_get(
    app: AppHandle,
    key: String,
) -> Result<Option<String>, String> {
    let vault = load_vault(&app)?;

    let (nonce_b64, ct_b64) = match vault.entries.get(&key) {
        Some(v) => v,
        None    => return Ok(None),
    };

    let nonce      = B64.decode(nonce_b64).map_err(|e| e.to_string())?;
    let ciphertext = B64.decode(ct_b64).map_err(|e| e.to_string())?;

    let cipher    = Aes256Gcm::new(&derive_key());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {e}"))?;

    Ok(Some(String::from_utf8(plaintext).map_err(|e| e.to_string())?))
}

#[tauri::command]
pub async fn stronghold_delete(
    app: AppHandle,
    key: String,
) -> Result<(), String> {
    let mut vault = load_vault(&app)?;
    vault.entries.remove(&key);
    save_vault(&app, &vault)?;
    Ok(())
}
