// src/hooks/useCredentialStore.ts
//
// Credential storage strategy:
//   - Profile METADATA (label, host, port, db_type, etc.) → localStorage
//     These are not secret and need to be available synchronously for the UI.
//   - SECRETS (passwords, API keys) → Tauri Stronghold (AES-256-GCM encrypted vault)
//     Fetched async via Rust commands. Deleted with the app's data directory.

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export type DbType = "postgresql" | "sqlite" | "mysql" | "mssql" | "mongodb";

export interface DbProfile {
  id:           string;
  label:        string;
  db_type:      DbType;
  // PostgreSQL / MySQL / MSSQL
  host?:        string;
  port?:        string;
  user?:        string;
  dbname?:      string;
  // MSSQL only
  instance?:    string;
  // MongoDB only
  auth_source?: string;
  // SQLite only
  filepath?:    string;
  // Raw DSN fallback
  dsn?:         string;
  // NOTE: password is NOT stored here — it lives in Stronghold
}

export interface AiProfile {
  id:          string;
  label:       string;
  provider_id: string; // "gemini" | "openai" | "anthropic" | "ollama"
  // NOTE: key is NOT stored here — it lives in Stronghold
}

// ── Stronghold helpers ────────────────────────────────────────────────────────

async function shSave(key: string, value: string) {
  await invoke("stronghold_save", { key, value });
}

async function shGet(key: string): Promise<string | null> {
  return invoke<string | null>("stronghold_get", { key });
}

async function shDelete(key: string) {
  await invoke("stronghold_delete", { key });
}

const dbPasswordKey  = (id: string) => `db:profile:${id}:password`;
const aiKey      = (id: string) => `ai:profile:${id}:key`;

// ── localStorage metadata helpers ────────────────────────────────────────────

const META_KEY = "dbcompanion_meta_v2";

interface Meta {
  dbProfiles:     DbProfile[];
  aiProfiles: AiProfile[];
}

function loadMeta(): Meta {
  try {
    const raw = localStorage.getItem(META_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      // Convert old storage shape: geminiProfiles -> aiProfiles
      if (parsed.geminiProfiles && !parsed.aiProfiles) {
        parsed.aiProfiles = parsed.geminiProfiles;
        delete parsed.geminiProfiles;
      }
      return {
        dbProfiles: parsed.dbProfiles ?? [],
        aiProfiles: parsed.aiProfiles ?? [],
      };
    }
  } catch {}
  return { dbProfiles: [], aiProfiles: [] };
}

function saveMeta(meta: Meta) {
  localStorage.setItem(META_KEY, JSON.stringify(meta));
}

// ── Hook ─────────────────────────────────────────────────────────────────────

export function useCredentialStore() {
  const [meta, setMeta] = useState<Meta>(loadMeta);

  // Persist metadata on every change
  useEffect(() => { saveMeta(meta); }, [meta]);

  // ── DB Profiles ─────────────────────────────────────────────────────────────

  const addDbProfile = useCallback(async (
    profile: Omit<DbProfile, "id">,
    password: string,
  ) => {
    const id      = crypto.randomUUID();
    const newProf = { ...profile, id };

    // Save secret to Stronghold
    if (password) await shSave(dbPasswordKey(id), password);

    setMeta(m => ({ ...m, dbProfiles: [...m.dbProfiles, newProf] }));
    return newProf;
  }, []);

  const deleteDbProfile = useCallback(async (id: string) => {
    await shDelete(dbPasswordKey(id));
    setMeta(m => ({ ...m, dbProfiles: m.dbProfiles.filter(p => p.id !== id) }));
  }, []);

  /** Retrieve the password for a profile from Stronghold. */
  const getDbPassword = useCallback((id: string): Promise<string | null> => {
    return shGet(dbPasswordKey(id));
  }, []);

  // ── Gemini Profiles ─────────────────────────────────────────────────────────

  const addAiProfile = useCallback(async (
    label: string,
    apiKey: string,
    provider_id: string = "gemini",
  ) => {
    const id      = crypto.randomUUID();
    const newProf = { id, label, provider_id };

    await shSave(aiKey(id), apiKey);
    setMeta(m => ({ ...m, aiProfiles: [...m.aiProfiles, newProf] }));
    return newProf;
  }, []);

  const deleteAiProfile = useCallback(async (id: string) => {
    await shDelete(aiKey(id));
    setMeta(m => ({ ...m, aiProfiles: m.aiProfiles.filter(p => p.id !== id) }));
  }, []);

  /** Retrieve the API key for a Gemini profile from Stronghold. */
  const getAiKey = useCallback(async (id: string): Promise<string | null> => {
    // Try new key first, fall back to old gemini:profile: key for existing profiles
    const val = await shGet(aiKey(id));
    if (val) return val;
    return shGet(`gemini:profile:${id}:key`);
  }, []);

  return {
    dbProfiles:        meta.dbProfiles,
    aiProfiles:    meta.aiProfiles,
    addDbProfile,
    deleteDbProfile,
    getDbPassword,
    addAiProfile,
    deleteAiProfile,
    getAiKey,
  };
}

// ── Build ConnectArgs for the Rust backend ────────────────────────────────────

export interface ConnectArgs {
  db_type:      string;
  host?:        string;
  port?:        string;
  user?:        string;
  password?:    string;
  dbname?:      string;
  filepath?:    string;
  dsn?:         string;
  instance?:    string;
  auth_source?: string;
}

export function buildConnectArgs(profile: DbProfile, password: string): ConnectArgs {
  return {
    db_type:     profile.db_type,
    host:        profile.host,
    port:        profile.port,
    user:        profile.user,
    password,
    dbname:      profile.dbname,
    filepath:    profile.filepath,
    dsn:         profile.dsn,
    instance:    profile.instance,
    auth_source: profile.auth_source,
  };
}
