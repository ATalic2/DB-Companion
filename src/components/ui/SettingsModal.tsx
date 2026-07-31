// src/components/ui/SettingsModal.tsx
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { DbProfile, AiProfile, DbType } from "../../hooks/useCredentialStore";

export type SettingsTab = "databases" | "ai";


const S = {
  input: {
    width: "100%", background: "#0a0b0d", border: "1px solid #2a2f3d",
    borderRadius: 6, padding: "8px 10px", color: "#e8eaf0",
    fontSize: 13, fontFamily: "inherit", outline: "none",
  } as React.CSSProperties,
  label: {
    fontSize: 11, color: "#6b7280", fontWeight: 700,
    letterSpacing: ".5px", textTransform: "uppercase" as const,
    display: "block", marginBottom: 4,
  },
  btnPrimary: {
    background: "#4f8ef7", color: "#fff", border: "none",
    borderRadius: 6, padding: "8px 16px", fontSize: 13,
    fontFamily: "inherit", cursor: "pointer", fontWeight: 500,
  } as React.CSSProperties,
  btnGhost: {
    background: "transparent", color: "#e8eaf0",
    border: "1px solid #2a2f3d", borderRadius: 6,
    padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer",
  } as React.CSSProperties,
  btnDanger: {
    background: "transparent", color: "#ef4444",
    border: "1px solid #7f1d1d", borderRadius: 4,
    padding: "4px 10px", fontSize: 11, fontFamily: "inherit", cursor: "pointer",
  } as React.CSSProperties,
};

const DB_TYPES: { value: DbType; label: string; defaultPort: string }[] = [
  { value: "postgresql", label: "PostgreSQL",  defaultPort: "5432"  },
  { value: "mysql",      label: "MySQL",       defaultPort: "3306"  },
  { value: "mssql",      label: "SQL Server",  defaultPort: "1433"  },
  { value: "mongodb",    label: "MongoDB",     defaultPort: "27017" },
  { value: "sqlite",     label: "SQLite",      defaultPort: ""      },
];

const USER_PLACEHOLDERS: Record<DbType, string> = {
  postgresql: "postgres",
  mysql:      "root",
  mssql:      "sa",
  mongodb:    "admin",
  sqlite:     "",
};

function PasswordInput({ value, onChange, placeholder = "••••••••" }: {
  value: string; onChange: (v: string) => void; placeholder?: string;
}) {
  const [show, setShow] = useState(false);
  return (
    <div style={{ position: "relative" }}>
      <input
        style={{ ...S.input, paddingRight: 52 }}
        type={show ? "text" : "password"}
        value={value}
        onChange={e => onChange(e.target.value)}
        placeholder={placeholder}
      />
      <button onClick={() => setShow(v => !v)} style={{ position: "absolute", right: 8, top: "50%", transform: "translateY(-50%)", background: "none", border: "none", color: "#6b7280", cursor: "pointer", fontSize: 11 }}>
        {show ? "hide" : "show"}
      </button>
    </div>
  );
}

function DbForm({ onSave, onCancel }: {
  onSave: (profile: Omit<DbProfile, "id">, password: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [saving,   setSaving]   = useState(false);
  const [dbType,   setDbType]   = useState<DbType>("postgresql");
  const [label,    setLabel]    = useState("");
  const [host,     setHost]     = useState("localhost");
  const [port,     setPort]     = useState("5432");
  const [user,     setUser]     = useState("");
  const [password, setPassword] = useState("");
  const [dbname,   setDbname]   = useState("");
  const [instance,   setInstance]   = useState("");
  const [filepath,   setFilepath]   = useState("");
  const [dsn,        setDsn]        = useState("");
  const [useDsn,     setUseDsn]     = useState(false);
  const [authSource, setAuthSource] = useState("admin");

  const handleTypeChange = (t: DbType) => {
    setDbType(t);
    const cfg = DB_TYPES.find(d => d.value === t);
    if (cfg?.defaultPort) setPort(cfg.defaultPort);
  };

  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing,    setTesting]   = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);

  const buildArgs = () => {
    if (useDsn)               return { db_type: dbType, dsn };
    if (dbType === "sqlite")  return { db_type: dbType, filepath };
    if (dbType === "mongodb") return { db_type: dbType, host, port, user, password, dbname, auth_source: authSource || "admin" };
    return { db_type: dbType, host, port, user, password, dbname, instance: instance || undefined };
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await invoke<{ success: boolean; message: string }>("test_connection", { args: buildArgs() });
      setTestResult(res);
    } catch (e: any) {
      setTestResult({ success: false, message: e?.message ?? String(e) });
    } finally { setTesting(false); }
  };

  const handleSave = async () => {
    if (!label.trim()) return;
    setSaving(true);
    setSaveError(null);
    try {
      const base = { label: label.trim(), db_type: dbType };
      if (useDsn)              await onSave({ ...base, dsn }, "");
      else if (dbType === "sqlite") await onSave({ ...base, filepath }, "");
      else                     await onSave({ ...base, host, port, user, dbname, instance: instance || undefined }, password);
    } catch (e: any) {
      setSaveError(e?.message ?? String(e));
    } finally { setSaving(false); }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <div>
        <label style={S.label}>Database Type</label>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {DB_TYPES.map(t => (
            <button key={t.value} onClick={() => handleTypeChange(t.value)} style={{
              ...S.btnGhost, padding: "5px 12px", fontSize: 12,
              borderColor: dbType === t.value ? "#4f8ef7" : "#2a2f3d",
              color:       dbType === t.value ? "#4f8ef7" : "#6b7280",
              background:  dbType === t.value ? "#0d1a35" : "transparent",
            }}>{t.label}</button>
          ))}
        </div>
      </div>

      <div>
        <label style={S.label}>Profile Name</label>
        <input style={S.input} value={label} onChange={e => setLabel(e.target.value)} placeholder="e.g. prod-db, local-dev" />
      </div>

      {dbType === "sqlite" && (
        <div>
          <label style={S.label}>Database File</label>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              style={{ ...S.input, flex: 1 }}
              value={filepath}
              onChange={e => setFilepath(e.target.value)}
              placeholder="Select a .sqlite or .db file…"
              readOnly
            />
            <button
              onClick={async () => {
                const selected = await invoke<string | null>("plugin:dialog|open", {
                  options: {
                    multiple: false,
                    filters: [{ name: "SQLite Database", extensions: ["sqlite", "db", "sqlite3", "db3"] }],
                  }
                });
                if (selected) setFilepath(selected);
              }}
              style={{
                background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 6,
                padding: "0 14px", color: "#e8eaf0", fontSize: 12,
                fontFamily: "inherit", cursor: "pointer", whiteSpace: "nowrap" as const,
                flexShrink: 0,
              }}
            >
              Browse…
            </button>
          </div>
          {filepath && (
            <div style={{ fontSize: 11, color: "#22c55e", marginTop: 4, fontFamily: "monospace", wordBreak: "break-all" as const }}>
              ✓ {filepath}
            </div>
          )}
        </div>
      )}

      {dbType !== "sqlite" && (
        <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, cursor: "pointer" }}>
          <input type="checkbox" checked={useDsn} onChange={e => setUseDsn(e.target.checked)} style={{ accentColor: "#4f8ef7" }} />
          Use raw connection string instead
        </label>
      )}

      {dbType !== "sqlite" && useDsn && (
        <div>
          <label style={S.label}>Connection String</label>
          <input style={S.input} value={dsn} onChange={e => setDsn(e.target.value)}
            placeholder={
              dbType === "postgresql" ? "postgres://user:pass@host:5432/dbname" :
              dbType === "mssql"      ? "sqlserver://user:pass@host:1433?database=mydb" :
              "mysql://user:pass@host:3306/dbname"
            }
          />
        </div>
      )}

      {dbType !== "sqlite" && !useDsn && (
        <>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 110px", gap: 8 }}>
            <div>
              <label style={S.label}>Host</label>
              <input style={S.input} value={host} onChange={e => setHost(e.target.value)} placeholder="localhost" />
            </div>
            <div>
              <label style={S.label}>Port</label>
              <input style={S.input} value={port} onChange={e => setPort(e.target.value)} />
            </div>
          </div>
          {dbType === "mssql" && (
            <div>
              <label style={S.label}>Instance Name <span style={{ color: "#4b5563", fontWeight: 400 }}>(optional)</span></label>
              <input style={S.input} value={instance} onChange={e => setInstance(e.target.value)} placeholder="SQLEXPRESS" />
            </div>
          )}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
            <div>
              <label style={S.label}>Username</label>
              <input style={S.input} value={user} onChange={e => setUser(e.target.value)} placeholder={USER_PLACEHOLDERS[dbType]} />
            </div>
            <div>
              <label style={S.label}>Password</label>
              <PasswordInput value={password} onChange={setPassword} />
            </div>
          </div>
          <div>
            <label style={S.label}>Database Name</label>
            <input style={S.input} value={dbname} onChange={e => setDbname(e.target.value)} placeholder="mydb" />
          </div>
          {dbType === "mongodb" && (
            <div>
              <label style={S.label}>Auth Source <span style={{ color: "#4b5563", fontWeight: 400 }}>(database where user is defined)</span></label>
              <input style={S.input} value={authSource} onChange={e => setAuthSource(e.target.value)} placeholder="admin" />
            </div>
          )}
        </>
      )}

      {saveError && (
        <div style={{ padding: "8px 10px", background: "#450a0a", border: "1px solid #7f1d1d", borderRadius: 6, fontSize: 11, color: "#f87171" }}>
          ✗ {saveError}
        </div>
      )}
      {testResult && (
        <div style={{
          padding: "8px 10px", borderRadius: 6, fontSize: 11,
          background: testResult.success ? "#052e16" : "#450a0a",
          border: `1px solid ${testResult.success ? "#166534" : "#7f1d1d"}`,
          color: testResult.success ? "#4ade80" : "#f87171",
        }}>
          {testResult.success ? "✓" : "✗"} {testResult.message}
        </div>
      )}
      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", paddingTop: 4 }}>
        <button style={S.btnGhost} onClick={onCancel}>Cancel</button>
        <button
          onClick={handleTest}
          disabled={testing}
          style={{ ...S.btnGhost, color: testing ? "#4b5563" : "#4f8ef7", borderColor: "#1e3a5f" }}
        >
          {testing ? "Testing…" : "Test Connection"}
        </button>
        <button style={{ ...S.btnPrimary, opacity: label.trim() && !saving ? 1 : 0.5 }} onClick={handleSave} disabled={!label.trim() || saving}>
          {saving ? "Saving…" : "Save Profile"}
        </button>
      </div>
    </div>
  );
}

// ── AI provider definitions ───────────────────────────────────────────────────
export const AI_PROVIDERS = [
  { id: "gemini",    label: "Google Gemini",    available: true,  models: ["gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.5-pro", "gemini-3.5-flash", "gemini-3-flash-preview", "gemini-3.1-flash-lite"], keyLabel: "Gemini API Key",    keyPlaceholder: "Paste your Gemini API key here", keyHint: "aistudio.google.com",       keyHintUrl: "https://aistudio.google.com/apikey" },
  { id: "openai",    label: "OpenAI",           available: false, models: ["gpt-4o", "gpt-4o-mini", "o3-mini"],                   keyLabel: "OpenAI API Key",    keyPlaceholder: "sk-...",                         keyHint: "platform.openai.com",       keyHintUrl: "https://platform.openai.com/api-keys" },
  { id: "anthropic", label: "Anthropic Claude", available: false, models: ["claude-opus-4-6", "claude-sonnet-4-6", "claude-haiku-4-5"], keyLabel: "Anthropic API Key", keyPlaceholder: "sk-ant-...",                    keyHint: "console.anthropic.com",     keyHintUrl: "https://console.anthropic.com/settings/keys" },
  { id: "ollama",    label: "Ollama (local)",   available: false, models: ["llama3.3", "mistral", "codellama", "qwen2.5-coder"],                          keyLabel: "Base URL",          keyPlaceholder: "http://localhost:11434",         keyHint: "ollama.com",                keyHintUrl: "https://ollama.com" },
];

function AiProviderForm({ onSave, onCancel }: {
  onSave:   (label: string, key: string, providerId: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [providerId, setProviderId] = useState("gemini");
  const [label,      setLabel]      = useState("");
  const [key,        setKey]        = useState("");
  const [saving,     setSaving]     = useState(false);
  const [saveError,  setSaveError]  = useState<string | null>(null);

  const provider = AI_PROVIDERS.find(p => p.id === providerId)!;

  const handleSave = async () => {
    if (!label.trim() || !key.trim()) return;
    setSaving(true);
    setSaveError(null);
    try {
      await onSave(label.trim(), key.trim(), providerId);
    } catch (e: any) {
      setSaveError(e?.message ?? String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>

      {/* Provider pills */}
      <div>
        <label style={S.label}>AI Provider</label>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {AI_PROVIDERS.map(p => (
            <button key={p.id}
              onClick={() => { if (p.available) { setProviderId(p.id); setKey(""); } }}
              style={{
                ...S.btnGhost, padding: "5px 12px", fontSize: 12,
                borderColor: providerId === p.id ? "#4f8ef7" : "#2a2f3d",
                color:       !p.available ? "#374151" : providerId === p.id ? "#4f8ef7" : "#6b7280",
                background:  providerId === p.id ? "#0d1a35" : "transparent",
                cursor:      p.available ? "pointer" : "not-allowed",
              }}
            >
              {p.label}
              {!p.available && <span style={{ marginLeft: 4, fontSize: 9, color: "#4b5563" }}>soon</span>}
            </button>
          ))}
        </div>
      </div>

      {/* Profile label */}
      <div>
        <label style={S.label}>Profile Name</label>
        <input style={S.input} value={label} onChange={e => setLabel(e.target.value)} placeholder="e.g. personal, work" />
      </div>

      {/* API key / URL */}
      <div>
        <label style={S.label}>{provider.keyLabel}</label>
        <PasswordInput value={key} onChange={setKey} placeholder={provider.keyPlaceholder} />
        <div style={{ fontSize: 11, color: "#6b7280", marginTop: 6, display: "flex", alignItems: "center", gap: 4 }}>
          {provider.id === "ollama" ? "Run Ollama locally and enter its base URL above." : (
            <>Get a free key at{" "}
              <button onClick={() => openUrl(provider.keyHintUrl)}
                style={{ background: "none", border: "none", color: "#4f8ef7", cursor: "pointer", fontSize: 11, fontFamily: "inherit", padding: 0, textDecoration: "underline" }}>
                {provider.keyHint}
              </button>
            </>
          )}
          {" "}· Stored encrypted on your machine
        </div>
      </div>

      {saveError && (
        <div style={{ padding: "8px 10px", background: "#450a0a", border: "1px solid #7f1d1d", borderRadius: 6, fontSize: 11, color: "#f87171" }}>✗ {saveError}</div>
      )}

      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        <button style={S.btnGhost} onClick={onCancel}>Cancel</button>
        <button
          style={{ ...S.btnPrimary, opacity: label.trim() && key.trim() && !saving ? 1 : 0.5 }}
          onClick={handleSave} disabled={!label.trim() || !key.trim() || saving}
        >
          {saving ? "Saving…" : "Save Key"}
        </button>
      </div>
    </div>
  );
}
// ── Shared modal shell ────────────────────────────────────────────────────────
function ModalShell({ title, onClose, children }: {
  title: string; onClose: () => void; children: React.ReactNode;
}) {
  return (
    <div onClick={onClose} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.75)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
      <div onClick={e => e.stopPropagation()} style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 12, width: 560, maxHeight: "85vh", display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <div style={{ padding: "16px 20px", borderBottom: "1px solid #2a2f3d", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span style={{ fontWeight: 700, fontSize: 15 }}>{title}</span>
          <button onClick={onClose} style={{ background: "none", border: "none", color: "#6b7280", fontSize: 18, cursor: "pointer" }}>✕</button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: 20, display: "flex", flexDirection: "column", gap: 10 }}>
          {children}
        </div>
      </div>
    </div>
  );
}

// ── DB Profiles Modal ─────────────────────────────────────────────────────────
function ProfileRow({ p, dbTypeLabel, onDelete, onTest }: {
  p:           DbProfile;
  dbTypeLabel: (t: DbType) => string;
  onDelete:    () => void;
  onTest:      () => Promise<{ success: boolean; message: string }>;
}) {
  const [testing,   setTesting]   = useState(false);
  const [testState, setTestState] = useState<{ success: boolean; message: string } | null>(null);

  const runTest = async () => {
    setTesting(true);
    setTestState(null);
    try {
      const res = await onTest();
      setTestState(res);
    } catch (e: any) {
      setTestState({ success: false, message: e?.message ?? String(e) });
    } finally { setTesting(false); }
  };

  return (
    <div style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 8, padding: "10px 14px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 500 }}>{p.label}</div>
          <div style={{ fontSize: 11, color: "#6b7280", fontFamily: "monospace", marginTop: 2 }}>
            {p.db_type === "sqlite" ? p.filepath
              : p.dsn ? p.dsn.replace(/:([^@]+)@/, ":••••@")
              : `${p.user ?? ""}@${p.host ?? ""}:${p.port ?? ""}/${p.dbname ?? ""}`}
          </div>
        </div>
        <span style={{ fontSize: 10, padding: "2px 6px", borderRadius: 3, background: "#0d1a35", color: "#4f8ef7", border: "1px solid #1e3a5f" }}>
          {dbTypeLabel(p.db_type)}
        </span>
        <button
          onClick={runTest}
          disabled={testing}
          style={{ fontSize: 11, padding: "4px 10px", borderRadius: 5, border: "1px solid #1e3a5f", background: "none", color: testing ? "#4b5563" : "#4f8ef7", cursor: testing ? "not-allowed" : "pointer" }}
        >
          {testing ? "Testing…" : "Test"}
        </button>
        <button style={S.btnDanger} onClick={onDelete}>Delete</button>
      </div>
      {testState && (
        <div style={{
          marginTop: 8, padding: "6px 10px", borderRadius: 5, fontSize: 11,
          background: testState.success ? "#052e16" : "#450a0a",
          border: `1px solid ${testState.success ? "#166534" : "#7f1d1d"}`,
          color: testState.success ? "#4ade80" : "#f87171",
        }}>
          {testState.success ? "✓" : "✗"} {testState.message}
        </div>
      )}
    </div>
  );
}

export function DbProfilesModal({ dbProfiles, onAddDb, onDeleteDb, onTestDb, onClose }: {
  dbProfiles:   DbProfile[];
  onAddDb:      (profile: Omit<DbProfile, "id">, password: string) => Promise<unknown>;
  onDeleteDb:   (id: string) => Promise<unknown>;
  onTestDb:     (id: string) => Promise<{ success: boolean; message: string }>;
  onClose:      () => void;
}) {
  const [adding, setAdding] = useState(false);
  const dbTypeLabel = (t: DbType) => DB_TYPES.find(d => d.value === t)?.label ?? t;

  return (
    <ModalShell title="🗄 Database Profiles" onClose={onClose}>
      {dbProfiles.length === 0 && !adding && (
        <div style={{ textAlign: "center", color: "#6b7280", fontSize: 13, padding: "20px 0" }}>No database profiles yet.</div>
      )}
      {!adding && dbProfiles.map(p => (
        <ProfileRow
          key={p.id}
          p={p}
          dbTypeLabel={dbTypeLabel}
          onDelete={() => onDeleteDb(p.id)}
          onTest={() => onTestDb(p.id)}
        />
      ))}
      {adding ? (
        <div style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 8, padding: 16 }}>
          <div style={{ fontSize: 12, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 14 }}>New Database Profile</div>
          <DbForm
            onSave={async (p, pw) => { await onAddDb(p, pw); setAdding(false); }}
            onCancel={() => setAdding(false)}
          />
        </div>
      ) : (
        <button onClick={() => setAdding(true)} style={{ ...S.btnGhost, width: "100%", justifyContent: "center", display: "flex", borderStyle: "dashed" }}>
          + Add Database Profile
        </button>
      )}
    </ModalShell>
  );
}

// ── Gemini Keys Modal ─────────────────────────────────────────────────────────
export function AiKeysModal({ aiProfiles, onAddAi, onDeleteAi, onClose }: {
  aiProfiles: AiProfile[];
  onAddAi:    (label: string, apiKey: string, providerId?: string) => Promise<unknown>;
  onDeleteAi: (id: string) => Promise<unknown>;
  onClose:    () => void;
}) {
  const [adding, setAdding] = useState(false);

  return (
    <ModalShell title="🤖 AI Provider Keys" onClose={onClose}>
      {aiProfiles.length === 0 && !adding && (
        <div style={{ textAlign: "center", color: "#6b7280", fontSize: 13, padding: "20px 0" }}>No AI provider keys saved yet.</div>
      )}
      {!adding && aiProfiles.map(p => {
        const provider = AI_PROVIDERS.find(pr => pr.id === p.provider_id);
        return (
          <div key={p.id} style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 8, padding: "10px 14px", display: "flex", alignItems: "center", gap: 10 }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, fontWeight: 500 }}>{p.label}</div>
              {provider && <div style={{ fontSize: 11, color: "#6b7280", marginTop: 2, fontFamily: "monospace" }}>{provider.label}</div>}
            </div>
            {provider && (
              <span style={{ fontSize: 10, padding: "2px 7px", borderRadius: 4, background: "#0d1a35", color: "#4f8ef7", border: "1px solid #1e3a5f", fontFamily: "monospace", flexShrink: 0 }}>
                {provider.label}
              </span>
            )}
            <button style={S.btnDanger} onClick={() => onDeleteAi(p.id)}>Delete</button>
          </div>
        );
      })}
      {adding ? (
        <div style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 8, padding: 16 }}>
          <div style={{ fontSize: 12, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 14 }}>New AI Provider Key</div>
          <AiProviderForm
            onSave={async (label, key, providerId) => { await onAddAi(label, key, providerId); setAdding(false); }}
            onCancel={() => setAdding(false)}
          />
        </div>
      ) : (
        <button onClick={() => setAdding(true)} style={{ ...S.btnGhost, width: "100%", justifyContent: "center", display: "flex", borderStyle: "dashed" }}>
          + Add AI Provider Key
        </button>
      )}
    </ModalShell>
  );
}

// ── About Modal ───────────────────────────────────────────────────────────────
export function AboutModal({ onClose }: { onClose: () => void }) {
  return (
    <ModalShell title="ℹ About DbCompanion" onClose={onClose}>
      <div style={{ fontSize: 13, color: "#9ca3af", lineHeight: 1.8, display: "flex", flexDirection: "column", gap: 16 }}>

        <div>
          <div style={{ fontSize: 11, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 8 }}>About</div>
          <div>DbCompanion is an AI-powered database change agent built with Tauri v2 and Rust. Describe a schema change in plain English, review the AI-generated change plan, dry-run it, and execute it with a single click.</div>
          <div style={{ marginTop: 6, fontFamily: "monospace", fontSize: 11, color: "#4f8ef7" }}>v0.1.0 · Tauri 2 · Rust</div>
        </div>

        <div style={{ borderTop: "1px solid #2a2f3d", paddingTop: 16 }}>
          <div style={{ fontSize: 11, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 8 }}>Safety Defaults</div>
          {[
            ["Explicit approval required", "No change is ever executed without you reviewing and approving the full plan first. The AI proposes — you decide."],
            ["Dry run before commit", "Every change is automatically executed inside a transaction and rolled back before you commit, catching syntax errors and constraint violations safely."],
            ["ACID transaction with auto-rollback", "All SQL statements run inside a single atomic transaction. If any statement fails, the entire change is rolled back automatically — partial changes are architecturally impossible."],
          ].map(([title, desc]) => (
            <div key={title} style={{ marginBottom: 10 }}>
              <div style={{ color: "#22c55e", fontSize: 12, fontWeight: 600 }}>✓ {title}</div>
              <div style={{ fontSize: 12, color: "#6b7280", marginTop: 2 }}>{desc}</div>
            </div>
          ))}
        </div>

        <div style={{ borderTop: "1px solid #2a2f3d", paddingTop: 16 }}>
          <div style={{ fontSize: 11, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 8 }}>Zero-Knowledge Architecture</div>
          {[
            ["AI API calls proxied through Rust", "Your API key is never handled by the JavaScript frontend. All AI requests are made by the Rust backend, which zeroizes the key from memory immediately after the call completes."],
            ["Credentials encrypted at rest", "Database passwords and API keys are stored in a Stronghold vault — an AES-256-GCM encrypted file on your machine. Uninstalling the app and its data directory removes the vault entirely."],
            ["Nothing leaves your machine", "No telemetry, no cloud sync, no central server. Every credential lives on-device. The only outbound connections are directly from your machine to the AI provider APIs."],
          ].map(([title, desc]) => (
            <div key={title} style={{ marginBottom: 10 }}>
              <div style={{ color: "#4f8ef7", fontSize: 12, fontWeight: 600 }}>🔒 {title}</div>
              <div style={{ fontSize: 12, color: "#6b7280", marginTop: 2 }}>{desc}</div>
            </div>
          ))}
        </div>

      </div>
    </ModalShell>
  );
}
