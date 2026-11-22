//! commands/stronghold.rs — Encrypted secret storage
//!
//! Stores secrets in the app data dir. On Windows, DPAPI binds new secrets to
//! the current Windows user. Legacy AES entries are migrated on first read.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
#[cfg(not(windows))]
use aes_gcm::aead::{AeadCore, OsRng};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

const VAULT_FILE: &str = "dbcompanion_vault.json";
const DPAPI_MARKER: &str = "dpapi-user-v1";
static VAULT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn vault_lock() -> &'static Mutex<()> {
    VAULT_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(windows)]
mod windows_protection {
    use std::ffi::c_void;

    #[repr(C)]
    struct DataBlob { size: u32, data: *mut u8 }

    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(input: *mut DataBlob, description: *const u16,
            entropy: *mut DataBlob, reserved: *mut c_void, prompt: *mut c_void,
            flags: u32, output: *mut DataBlob) -> i32;
        fn CryptUnprotectData(input: *mut DataBlob, description: *mut *mut u16,
            entropy: *mut DataBlob, reserved: *mut c_void, prompt: *mut c_void,
            flags: u32, output: *mut DataBlob) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" { fn LocalFree(memory: *mut c_void) -> *mut c_void; }

    fn transform(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
        let size = u32::try_from(data.len()).map_err(|_| "Secret is too large".to_string())?;
        let mut input = DataBlob { size, data: data.as_ptr() as *mut u8 };
        let mut output = DataBlob { size: 0, data: std::ptr::null_mut() };
        // CRYPTPROTECT_UI_FORBIDDEN; DPAPI defaults to current-user scope.
        let ok = unsafe {
            if protect {
                CryptProtectData(&mut input, std::ptr::null(), std::ptr::null_mut(),
                    std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output)
            } else {
                CryptUnprotectData(&mut input, std::ptr::null_mut(), std::ptr::null_mut(),
                    std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output)
            }
        };
        if ok == 0 { return Err(format!("Windows credential protection failed: {}", std::io::Error::last_os_error())); }
        let bytes = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
        unsafe { LocalFree(output.data.cast()); }
        Ok(bytes)
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> { transform(data, true) }
    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> { transform(data, false) }
}

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

fn encrypt_entry(value: &str) -> Result<(String, String), String> {
    #[cfg(windows)]
    { return Ok((DPAPI_MARKER.into(), B64.encode(windows_protection::protect(value.as_bytes())?))); }
    #[cfg(not(windows))]
    {
        let cipher = Aes256Gcm::new(&derive_key());
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, value.as_bytes()).map_err(|e| e.to_string())?;
        Ok((B64.encode(nonce), B64.encode(ciphertext)))
    }
}

fn decrypt_legacy(nonce_b64: &str, ct_b64: &str) -> Result<String, String> {
    let nonce = B64.decode(nonce_b64).map_err(|e| e.to_string())?;
    if nonce.len() != 12 { return Err("Invalid legacy vault nonce".into()); }
    let ciphertext = B64.decode(ct_b64).map_err(|e| e.to_string())?;
    let plaintext = Aes256Gcm::new(&derive_key())
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {e}"))?;
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stronghold_save(app: AppHandle, key: String, value: String) -> Result<(), String> {
    let entry = encrypt_entry(&value)?;
    let _guard = vault_lock().lock().await;
    let mut vault = load_vault(&app)?;
    #[cfg(windows)]
    {
        let mut upgraded = false;
        for (name, (nonce, ciphertext)) in vault.entries.clone() {
            if nonce == DPAPI_MARKER { continue; }
            if let Ok(value) = decrypt_legacy(&nonce, &ciphertext) {
                vault.entries.insert(name, encrypt_entry(&value)?);
                upgraded = true;
            }
        }
        if upgraded { save_vault(&app, &vault)?; }
    }
    vault.entries.insert(key, entry);
    save_vault(&app, &vault)?;
    Ok(())
}

#[tauri::command]
pub async fn stronghold_get(app: AppHandle, key: String) -> Result<Option<String>, String> {
    let _guard = vault_lock().lock().await;
    let mut vault = load_vault(&app)?;
    #[cfg(windows)]
    {
        let mut upgraded = false;
        for (name, (nonce, ciphertext)) in vault.entries.clone() {
            if nonce == DPAPI_MARKER { continue; }
            if let Ok(value) = decrypt_legacy(&nonce, &ciphertext) {
                vault.entries.insert(name, encrypt_entry(&value)?);
                upgraded = true;
            }
        }
        if upgraded { save_vault(&app, &vault)?; }
    }

    let (nonce_b64, ct_b64) = match vault.entries.get(&key) {
        Some(v) => v,
        None => return Ok(None),
    };

    let ciphertext = B64.decode(ct_b64).map_err(|e| e.to_string())?;
    if nonce_b64 == DPAPI_MARKER {
        #[cfg(windows)]
        { return String::from_utf8(windows_protection::unprotect(&ciphertext)?)
            .map(Some).map_err(|e| e.to_string()); }
        #[cfg(not(windows))]
        { return Err("Windows-protected secret cannot be opened here".into()); }
    }
    // Existing entries remain readable and are upgraded without losing their
    // old ciphertext if the Windows protected write fails.
    Ok(Some(decrypt_legacy(nonce_b64, ct_b64)?))
}

#[tauri::command]
pub async fn stronghold_delete(app: AppHandle, key: String) -> Result<(), String> {
    let _guard = vault_lock().lock().await;
    let mut vault = load_vault(&app)?;
    vault.entries.remove(&key);
    save_vault(&app, &vault)?;
    Ok(())
}
