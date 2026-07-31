// src/App.tsx
import { useState, useRef, useEffect, useCallback } from "react";
import type { SchemaMetadata } from "./types/changes";
import { invoke } from "@tauri-apps/api/core";
import { useCredentialStore, buildConnectArgs } from "./hooks/useCredentialStore";
import type { DbProfile, AiProfile } from "./hooks/useCredentialStore";
import { useChangeAgent } from "./hooks/useChangeAgent";
import { QueryPage }       from "./components/query/QueryPage";
import { DbProfilesModal, AiKeysModal, AboutModal } from "./components/ui/SettingsModal";
import { SchemaBrowser }   from "./components/query/SchemaBrowser";
import type { TableMeta }  from "./types/changes";
import "./index.css";

type Modal = "db" | "ai" | "about" | null;

const SELECT: React.CSSProperties = {
  background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 6,
  color: "#e8eaf0", fontSize: 12, padding: "6px 10px",
  fontFamily: "inherit", cursor: "pointer", outline: "none", width: "100%",
};

const LAST_DB_KEY     = "dbcompanion_last_db_id";
const LAST_AI_KEY = "dbcompanion_last_gemini_id";
const SIDEBAR_W_KEY   = "dbcompanion_sidebar_w";
const CHAT_W_KEY      = "dbcompanion_chat_w";

// ── Vertical label shown when a panel is collapsed ────────────────────────────
function CollapsedLabel({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <div
      onClick={onClick}
      title={`Expand ${label}`}
      style={{
        height: "100%", width: 22, display: "flex", alignItems: "center",
        justifyContent: "center", cursor: "pointer", background: "#111318",
        borderRight: "1px solid #2a2f3d",
      }}
      onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
      onMouseLeave={e => (e.currentTarget.style.background = "#111318")}
    >
      <span style={{
        fontSize: 11, fontWeight: 700, color: "#6b7280",
        textTransform: "uppercase", letterSpacing: 2,
        writingMode: "vertical-rl", transform: "rotate(180deg)",
        userSelect: "none",
      }}>
        {label}
      </span>
    </div>
  );
}

// ── Drag handle ───────────────────────────────────────────────────────────────
function DragHandle({ onMouseDown, collapsed, onToggle, label }: {
  onMouseDown: (e: React.MouseEvent) => void;
  collapsed:   boolean;
  onToggle:    () => void;
  label:       string;
}) {
  const [hover, setHover] = useState(false);
  return (
    <div
      style={{ position: "relative", display: "flex", flexDirection: "column", alignItems: "center", zIndex: 20 }}
    >
      <div
        onMouseDown={!collapsed ? onMouseDown : undefined}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        style={{
          width: 5, height: "100%", cursor: collapsed ? "default" : "col-resize",
          background: hover && !collapsed ? "#2a2f3d" : "#1a1f2e",
          transition: "background .15s",
        }}
      />
      {/* Toggle button centred on the handle */}
      <button
        onClick={onToggle}
        title={collapsed ? `Expand ${label}` : `Collapse ${label}`}
        style={{
          position: "absolute", top: "50%", transform: "translateY(-50%)",
          background: "#191c23", border: "1px solid #2a2f3d",
          borderRadius: 4, width: 18, height: 18,
          display: "flex", alignItems: "center", justifyContent: "center",
          color: "#6b7280", fontSize: 11, cursor: "pointer", padding: 0,
          lineHeight: 1,
        }}
      >
        {collapsed ? "›" : "‹"}
      </button>
    </div>
  );
}

// ── Help menu ─────────────────────────────────────────────────────────────────
function HelpMenu({ onAbout }: { onAbout: () => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);
  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button onClick={() => setOpen(v => !v)} style={{ background: open ? "#191c23" : "transparent", border: "1px solid " + (open ? "#2a2f3d" : "transparent"), borderRadius: 6, padding: "5px 12px", color: "#e8eaf0", fontSize: 12, fontFamily: "inherit", cursor: "pointer" }}>
        Help
      </button>
      {open && (
        <div style={{ position: "absolute", top: "calc(100% + 6px)", right: 0, background: "#111318", border: "1px solid #2a2f3d", borderRadius: 8, minWidth: 160, zIndex: 50, overflow: "hidden", boxShadow: "0 8px 24px rgba(0,0,0,.5)" }}>
          <button
            onClick={() => { setOpen(false); onAbout(); }}
            style={{ width: "100%", textAlign: "left", background: "none", border: "none", color: "#e8eaf0", fontSize: 13, fontFamily: "inherit", padding: "10px 14px", cursor: "pointer" }}
            onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
            onMouseLeave={e => (e.currentTarget.style.background = "none")}
          >
            ℹ About DbCompanion
          </button>
        </div>
      )}
    </div>
  );
}

// ── Main App ──────────────────────────────────────────────────────────────────
export default function App() {
  const {
    dbProfiles, aiProfiles,
    addDbProfile, deleteDbProfile, getDbPassword,
    addAiProfile, deleteAiProfile, getAiKey,
  } = useCredentialStore();

  const { connectDb, getSchema } = useChangeAgent();

  const [modal,         setModal]         = useState<Modal>(null);
  const [schema,        setSchema]        = useState<SchemaMetadata | null>(null);
  const [schemaLoading, setSchemaLoading] = useState(false);
  const [connected,     setConnected]     = useState(false);
  const [error,         setError]         = useState<string | null>(null);

  // Restore last used selections from localStorage
  const [selectedDbId,     setSelectedDbId]     = useState(() => localStorage.getItem(LAST_DB_KEY)     ?? "");
  const [selectedGeminiId, setSelectedGeminiId] = useState(() => localStorage.getItem(LAST_AI_KEY) ?? "");

  // Persist selections on change
  useEffect(() => { localStorage.setItem(LAST_DB_KEY,     selectedDbId);     }, [selectedDbId]);
  useEffect(() => { localStorage.setItem(LAST_AI_KEY, selectedGeminiId); }, [selectedGeminiId]);

  // ── Panel sizing ──────────────────────────────────────────────────────────
  const [sidebarW,         setSidebarW]         = useState(() => Number(localStorage.getItem(SIDEBAR_W_KEY)) || 240);
  const [chatW,            setChatW]            = useState(() => Number(localStorage.getItem(CHAT_W_KEY))    || 300);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [chatCollapsed,    setChatCollapsed]    = useState(false);

  const sidebarWRef = useRef(sidebarW);
  const chatWRef    = useRef(chatW);
  const draggingRef = useRef<"sidebar" | "chat" | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Persist panel sizes
  useEffect(() => { localStorage.setItem(SIDEBAR_W_KEY, String(sidebarW)); }, [sidebarW]);
  useEffect(() => { localStorage.setItem(CHAT_W_KEY,    String(chatW));    }, [chatW]);

  const onSidebarDragStart = useCallback((e: React.MouseEvent) => { e.preventDefault(); draggingRef.current = "sidebar"; }, []);
  const onChatDragStart    = useCallback((e: React.MouseEvent) => { e.preventDefault(); draggingRef.current = "chat";    }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!draggingRef.current || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      if (draggingRef.current === "sidebar") {
        const w = Math.min(420, Math.max(140, e.clientX - rect.left));
        setSidebarW(w); sidebarWRef.current = w;
      } else {
        const w = Math.min(520, Math.max(200, rect.right - e.clientX));
        setChatW(w); chatWRef.current = w;
      }
    };
    const onUp = () => { draggingRef.current = null; };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
    return () => { window.removeEventListener("mousemove", onMove); window.removeEventListener("mouseup", onUp); };
  }, []);

  const toggleSidebar = () => setSidebarCollapsed(v => !v);
  const toggleChat    = () => setChatCollapsed(v => !v);

  // ── DB connection ─────────────────────────────────────────────────────────
  const activeDb:     DbProfile     | undefined = dbProfiles.find(p => p.id === selectedDbId);
  const activeAi: AiProfile | undefined = aiProfiles.find(p => p.id === selectedGeminiId);

  const connectAndLoadSchema = useCallback(async (db: DbProfile) => {
    setSchemaLoading(true);
    setError(null);
    try {
      const password = await getDbPassword(db.id) ?? "";
      const result   = await connectDb(buildConnectArgs(db, password));
      if (!result.success) throw new Error(result.error ?? "Connection failed");
      setConnected(true);
      setSchema(await getSchema());
    } catch (e: any) {
      setError(`Connection failed: ${e?.message ?? String(e)}`);
      setConnected(false); setSchema(null);
    } finally {
      setSchemaLoading(false);
    }
  }, [getDbPassword, connectDb, getSchema]);

  useEffect(() => {
    if (activeDb) connectAndLoadSchema(activeDb);
    else { setConnected(false); setSchema(null); }
  }, [selectedDbId]);

  // Auto-select if profile list loads and nothing is selected
  useEffect(() => {
    if (!selectedDbId && dbProfiles.length > 0) {
      const saved = localStorage.getItem(LAST_DB_KEY);
      if (saved && dbProfiles.find(p => p.id === saved)) setSelectedDbId(saved);
    }
    if (!selectedGeminiId && aiProfiles.length > 0) {
      const saved = localStorage.getItem(LAST_AI_KEY);
      if (saved && aiProfiles.find(p => p.id === saved)) setSelectedGeminiId(saved);
    }
  }, [dbProfiles, aiProfiles]);

  const handleSelectTable = (_table: TableMeta) => {};

  const effectiveChatW = chatCollapsed ? 0 : chatW;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", background: "#0a0b0d", color: "#e8eaf0", fontFamily: "'Syne', sans-serif" }}>

      {/* ── Header ── */}
      <div style={{ display: "flex", alignItems: "center", borderBottom: "1px solid #2a2f3d", background: "#111318", height: 52, flexShrink: 0 }}>
        <div style={{ padding: "0 16px", fontSize: 15, fontWeight: 700, display: "flex", alignItems: "center", gap: 8, height: "100%", borderRight: "1px solid #2a2f3d" }}>
          <div style={{ width: 8, height: 8, background: "#4f8ef7", borderRadius: "50%", animation: "pulse 2s infinite" }} />
          DbCompanion
        </div>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8, paddingRight: 16 }}>
          <span style={{ fontFamily: "monospace", fontSize: 11, padding: "3px 8px", borderRadius: 4, ...(connected ? { border: "1px solid #1e3a5f", color: "#4f8ef7", background: "#0d1a35" } : { border: "1px solid #2a2f3d", color: "#6b7280" }) }}>
            {connected && activeDb ? `● ${activeDb.db_type.charAt(0).toUpperCase() + activeDb.db_type.slice(1)}` : "○ Not connected"}
          </span>
          <span style={{ fontFamily: "monospace", fontSize: 11, padding: "3px 8px", borderRadius: 4, ...(activeAi ? { border: "1px solid #166534", color: "#22c55e", background: "#052e16" } : { border: "1px solid #854d0e", color: "#f59e0b", background: "#1c1102" }) }}>
              {activeAi ? activeAi.provider_id.charAt(0).toUpperCase() + activeAi.provider_id.slice(1) : "⚠ No AI key"}
          </span>
          <HelpMenu onAbout={() => setModal("about")} />
        </div>
      </div>

      {/* ── Body ── */}
      <div ref={containerRef} style={{ flex: 1, display: "flex", overflow: "hidden" }}>

        {/* ── Sidebar ── */}
        {sidebarCollapsed ? (
          <CollapsedLabel label="Sidebar" onClick={toggleSidebar} />
        ) : (
          <div style={{ width: sidebarW, flexShrink: 0, background: "#111318", display: "flex", flexDirection: "column", overflow: "hidden" }}>

            {/* DB + AI key section */}
            <div style={{ padding: "12px 12px 0", flexShrink: 0 }}>

              {/* Database */}
              <div style={{ marginBottom: 14 }}>
                <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: 1.5, color: "#6b7280", textTransform: "uppercase", marginBottom: 7, display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  Database
                  <button onClick={() => setModal("db")} style={{ background: "none", border: "none", color: "#4f8ef7", fontSize: 10, cursor: "pointer", fontFamily: "inherit" }}>Manage</button>
                </div>
                {dbProfiles.length === 0 ? (
                  <div style={{ fontSize: 11, color: "#6b7280" }}>No profiles saved.</div>
                ) : (
                  <>
                    <select value={selectedDbId} onChange={e => setSelectedDbId(e.target.value)} style={SELECT}>
                      <option value="">— Select —</option>
                      {dbProfiles.map(p => <option key={p.id} value={p.id}>{p.label}</option>)}
                    </select>
                    {activeDb && (
                      <div style={{ marginTop: 5, padding: "5px 8px", background: "#191c23", border: `1px solid ${connected ? "#166534" : "#2a2f3d"}`, borderRadius: 6, fontSize: 10, fontFamily: "monospace", color: "#6b7280", wordBreak: "break-all" }}>
                        <span style={{ color: connected ? "#22c55e" : "#6b7280" }}>{connected ? "●" : "○"}</span>{" "}
                        <span style={{ color: connected ? "#22c55e" : "#9ca3af" }}>{connected ? "Connected" : "Not connected"}</span>
                        {(() => {
                          const summary = (() => {
                            if (activeDb.db_type === "sqlite") return activeDb.filepath ?? activeDb.dsn ?? "";
                            if (activeDb.dsn) {
                              try {
                                const u = new URL(activeDb.dsn.replace(/^postgresql/, 'http').replace(/^postgres/, 'http'));
                                return `${u.username}@${u.hostname}:${u.port}${u.pathname}`;
                              } catch { return activeDb.dsn; }
                            }
                            // Fallback for profiles saved via individual fields (no raw DSN stored)
                            if (activeDb.host) {
                              const target = `${activeDb.host}${activeDb.port ? ":" + activeDb.port : ""}${activeDb.dbname ? "/" + activeDb.dbname : ""}`;
                              return activeDb.user ? `${activeDb.user}@${target}` : target;
                            }
                            return "";
                          })();
                          return summary ? <> — {summary}</> : null;
                        })()}
                      </div>
                    )}
                    {schemaLoading && <div style={{ fontSize: 10, color: "#6b7280", fontFamily: "monospace", marginTop: 3 }}>Connecting…</div>}
                  </>
                )}
              </div>

              {/* DB connection error — shown directly under Database section */}
              {error && (
                <div style={{ padding: "7px 9px", background: "#450a0a", border: "1px solid #7f1d1d", borderRadius: 6, fontSize: 11, color: "#f87171", marginBottom: 14, boxSizing: "border-box", display: "flex", alignItems: "flex-start", gap: 6 }}>
                  <span style={{ flex: 1, wordBreak: "break-word" }}>{error}</span>
                  <button onClick={() => setError(null)} style={{ flexShrink: 0, background: "none", border: "none", color: "#f87171", cursor: "pointer", padding: 0, lineHeight: 1 }}>✕</button>
                </div>
              )}

              {/* AI key */}
              <div style={{ marginBottom: 14 }}>
                <div style={{ fontSize: 10, fontWeight: 700, letterSpacing: 1.5, color: "#6b7280", textTransform: "uppercase", marginBottom: 7, display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  AI Provider Key
                  <button onClick={() => setModal("ai")} style={{ background: "none", border: "none", color: "#4f8ef7", fontSize: 10, cursor: "pointer", fontFamily: "inherit" }}>Manage</button>
                </div>
                {aiProfiles.length === 0 ? (
                  <div style={{ fontSize: 11, color: "#6b7280" }}>No keys saved.</div>
                ) : (
                  <>
                    <select value={selectedGeminiId} onChange={e => setSelectedGeminiId(e.target.value)} style={SELECT}>
                      <option value="">— Select —</option>
                      {aiProfiles.map(p => <option key={p.id} value={p.id}>{p.label}</option>)}
                    </select>
                  </>
                )}
              </div>

              {/* Divider before schema */}
              <div style={{ borderTop: "1px solid #2a2f3d", marginBottom: 10 }} />
            </div>

            {/* Schema browser — fills remaining sidebar space */}
            <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
              <SchemaBrowser
                schema={schema}
                onSelectTable={handleSelectTable}
                onRefresh={() => activeDb && connectAndLoadSchema(activeDb)}
                loading={schemaLoading}
              />
            </div>
          </div>
        )}

        {/* Sidebar drag handle + collapse */}
        <DragHandle
          onMouseDown={onSidebarDragStart}
          collapsed={sidebarCollapsed}
          onToggle={toggleSidebar}
          label="Sidebar"
        />

        {/* ── Main (QueryPage — owns editor + chat split) ── */}
        <div style={{ flex: 1, overflow: "hidden", minWidth: 0 }}>
          <QueryPage
            schema={schema}
            schemaLoading={schemaLoading}
            onRefreshSchema={() => activeDb && connectAndLoadSchema(activeDb)}
            activeAi={activeAi}
            getAiKey={getAiKey}
            chatWidth={effectiveChatW}
            chatCollapsed={chatCollapsed}
            onChatDragStart={onChatDragStart}
            onChatToggle={toggleChat}
          />
        </div>
      </div>

      {/* ── Modals ── */}
      {modal === "db"     && <DbProfilesModal dbProfiles={dbProfiles} onAddDb={addDbProfile} onDeleteDb={deleteDbProfile} onTestDb={async (id) => {
        const db = dbProfiles.find(p => p.id === id)!;
        const password = await getDbPassword(db.id) ?? "";
        return invoke<{ success: boolean; message: string }>("test_connection", { args: buildConnectArgs(db, password) });
      }} onClose={() => setModal(null)} />}
      {modal === "ai" && <AiKeysModal aiProfiles={aiProfiles} onAddAi={(l, k, p) => addAiProfile(l, k, p)} onDeleteAi={deleteAiProfile} onClose={() => setModal(null)} />}
      {modal === "about"  && <AboutModal onClose={() => setModal(null)} />}
    </div>
  );
}
