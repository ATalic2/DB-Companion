// src/components/query/QueryPage.tsx
import { useState, useRef, useCallback, useEffect } from "react";
import { invoke }        from "@tauri-apps/api/core";
import { SqlEditor }     from "./SqlEditor";
import { MongoEditor }   from "./MongoEditor";
import { ResultsGrid }   from "./ResultsGrid";
import { DocumentView }  from "./DocumentView";
import { LogPanel }      from "./LogPanel";
import { AiChat }        from "./AiChat";
import { appendLog }     from "../../hooks/useAppLog";
import type { SchemaMetadata } from "../../types/changes";
import type { AiProfile }      from "../../hooks/useCredentialStore";

interface QueryResult {
  columns:       string[];
  rows:          any[][];
  rows_affected: number;
  execution_ms:  number;
  error:         string | null;
  capped:        boolean;
}

interface Props {
  schema:          SchemaMetadata | null;
  schemaLoading:   boolean;
  onRefreshSchema: () => void;
  activeAi:        AiProfile | undefined;
  getAiKey:        (id: string) => Promise<string | null>;
  chatWidth:       number;
  chatCollapsed:   boolean;
  onChatDragStart: (e: React.MouseEvent) => void;
  onChatToggle:    () => void;
}

type BottomTab = "results" | "console";

function CollapsedLabel({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <div onClick={onClick} title={`Expand ${label}`} style={{ height: "100%", width: 22, display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", background: "#0d0f14", borderLeft: "1px solid #2a2f3d" }}
      onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
      onMouseLeave={e => (e.currentTarget.style.background = "#0d0f14")}
    >
      <span style={{ fontSize: 11, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 2, writingMode: "vertical-rl", userSelect: "none" }}>{label}</span>
    </div>
  );
}

function DragHandle({ onMouseDown, collapsed, onToggle, label }: { onMouseDown: (e: React.MouseEvent) => void; collapsed: boolean; onToggle: () => void; label: string }) {
  const [hover, setHover] = useState(false);
  return (
    <div style={{ position: "relative", display: "flex", flexDirection: "column", alignItems: "center", zIndex: 20 }}>
      <div onMouseDown={!collapsed ? onMouseDown : undefined} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
        style={{ width: 5, height: "100%", cursor: collapsed ? "default" : "col-resize", background: hover && !collapsed ? "#2a2f3d" : "#1a1f2e", transition: "background .15s" }} />
      <button onClick={onToggle} title={collapsed ? `Expand ${label}` : `Collapse ${label}`}
        style={{ position: "absolute", top: "50%", transform: "translateY(-50%)", background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, width: 18, height: 18, display: "flex", alignItems: "center", justifyContent: "center", color: "#6b7280", fontSize: 11, cursor: "pointer", padding: 0, lineHeight: 1 }}>
        {collapsed ? "‹" : "›"}
      </button>
    </div>
  );
}

const SQL_STORAGE_KEY   = "dbcompanion_editor_sql";
const MONGO_STORAGE_KEY = "dbcompanion_editor_mongo";

export function QueryPage({
  schema, schemaLoading: _schemaLoading, onRefreshSchema,
  activeAi, getAiKey,
  chatWidth, chatCollapsed, onChatDragStart, onChatToggle,
}: Props) {
  const isMongo = schema?.db_type === "mongodb";

  const [sql, setSql] = useState<string>(() => {
    try { return localStorage.getItem(SQL_STORAGE_KEY) || "SELECT * FROM "; }
    catch { return "SELECT * FROM "; }
  });

  const [mongoCmd, setMongoCmd] = useState<string>(() => {
    try { return localStorage.getItem(MONGO_STORAGE_KEY) || '{\n  "find": "collection",\n  "filter": {}\n}'; }
    catch { return '{\n  "find": "collection",\n  "filter": {}\n}'; }
  });

  const handleSqlChange = (v: string) => {
    setSql(v);
    try { localStorage.setItem(SQL_STORAGE_KEY, v); } catch {}
  };

  const handleMongoCmdChange = (v: string) => {
    setMongoCmd(v);
    try { localStorage.setItem(MONGO_STORAGE_KEY, v); } catch {}
  };

  const [result,    setResult]    = useState<QueryResult | null>(null);
  const [running,   setRunning]   = useState(false);
  const [aiKey,     setAiKey]     = useState("");
  const [bottomTab, setBottomTab] = useState<BottomTab>("results");
  const [mongoScriptMode, setMongoScriptMode] = useState(false);

  // Reset result when switching db types so stale SQL results don't show in mongo view
  useEffect(() => { setResult(null); }, [isMongo]);

  const [editorPct,    setEditorPct]    = useState(55);
  const [draggingVert, setDraggingVert] = useState(false);
  const leftPaneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (activeAi) getAiKey(activeAi.id).then(k => setAiKey(k ?? ""));
    else setAiKey("");
  }, [activeAi?.id]);

  const onVertDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setDraggingVert(true);
  }, []);

  useEffect(() => {
    if (!draggingVert) return;
    const onMove = (e: MouseEvent) => {
      if (!leftPaneRef.current) return;
      const rect = leftPaneRef.current.getBoundingClientRect();
      setEditorPct(Math.min(85, Math.max(20, ((e.clientY - rect.top) / rect.height) * 100)));
    };
    const onUp = () => setDraggingVert(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
    return () => { window.removeEventListener("mousemove", onMove); window.removeEventListener("mouseup", onUp); };
  }, [draggingVert]);

  // Strip -- comments from a SQL string, respecting single-quoted strings.
  // Keeps the same line structure so cursor positions stay valid.
  const stripSqlComments = (sql: string): string =>
    sql.split("\n").map(line => {
      let inSingle = false;
      for (let i = 0; i < line.length; i++) {
        if (line[i] === "'") { inSingle = !inSingle; }
        else if (!inSingle && line[i] === "-" && line[i + 1] === "-") {
          return line.slice(0, i);
        }
      }
      return line;
    }).join("\n");

  const SQL_VERBS = /\b(SELECT|INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|TRUNCATE|WITH|SET|GRANT|REVOKE|BEGIN|COMMIT|ROLLBACK|VACUUM|ANALYZE|COMMENT|DO|CALL|COPY)\b/i;

  const getStatementAtCursor = (fullSql: string, cursorPos: number): string => {
    const stripped = stripSqlComments(fullSql);
    let start = 0;
    const stmts: { text: string; start: number; end: number }[] = [];
    for (let i = 0; i <= stripped.length; i++) {
      if (i === stripped.length || stripped[i] === ";") {
        const text = stripped.slice(start, i).trim();
        if (text && SQL_VERBS.test(text)) stmts.push({ text, start, end: i });
        start = i + 1;
      }
    }
    return stmts.find(s => cursorPos >= s.start && cursorPos <= s.end + 1)?.text ?? fullSql.trim();
  };

  const DDL_RE = /\b(CREATE|DROP|ALTER|RENAME)\b/i;

  const runQuery = useCallback(async (queryToRun: string): Promise<QueryResult | null> => {
    if (!queryToRun.trim() || running) return null;
    setRunning(true);
    setBottomTab("results");
    const useScript = isMongo && mongoScriptMode;
    appendLog("info", useScript ? `Running script` : `Running query`, queryToRun.trim());
    console.log("[runQuery]", queryToRun.trim());
    try {
      const res = useScript
        ? await invoke<QueryResult>("run_script", { args: { script: queryToRun } })
        : await invoke<QueryResult>("run_query", { args: { sql: queryToRun } });
      setResult(res);
      if (res.error) appendLog("error", `${useScript ? "Script" : "Query"} error: ${res.error}`);
      else if (useScript || DDL_RE.test(queryToRun)) onRefreshSchema();
      return res;
    } catch (e: any) {
      const msg = String(e);
      setResult({ columns: [], rows: [], rows_affected: 0, execution_ms: 0, error: msg, capped: false });
      appendLog("error", `${useScript ? "Script" : "Query"} failed: ${msg}`);
      return null;
    } finally {
      setRunning(false);
    }
  }, [running, isMongo, mongoScriptMode]);

  const handleInsertSql = (s: string) => {
    if (isMongo) setMongoCmd(s);
    else setSql(s);
  };

  const VertDragHandle = () => {
    const [hover, setHover] = useState(false);
    return (
      <div onMouseDown={onVertDragStart} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
        style={{ height: 5, cursor: "row-resize", background: hover ? "#2a2f3d" : "#1a1f2e", flexShrink: 0, transition: "background .15s" }} />
    );
  };

  const editorLabel  = isMongo ? "Document Editor" : "SQL Editor";
  const resultsLabel = isMongo ? "Documents"       : "Results";

  return (
    <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>

      {/* ── Editor + results/console ── */}
      <div ref={leftPaneRef} style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minWidth: 200 }}>

        {/* Top bar */}
        <div style={{ display: "flex", borderBottom: "1px solid #2a2f3d", background: "#111318", flexShrink: 0, alignItems: "center", padding: "0 12px 0 0" }}>
          <div style={{ padding: "10px 16px", fontSize: 12, fontWeight: 600, color: "#e8eaf0", borderBottom: "2px solid #4f8ef7" }}>
            {editorLabel}
          </div>
        </div>

        {/* Editor */}
        <div style={{ flex: `0 0 ${editorPct}%`, overflow: "hidden" }}>
          {isMongo ? (
            <MongoEditor
              value={mongoCmd}
              onChange={handleMongoCmdChange}
              onRun={() => runQuery(mongoCmd)}
              running={running}
              scriptMode={mongoScriptMode}
              onToggleScriptMode={() => setMongoScriptMode(m => !m)}
            />
          ) : (
            <SqlEditor
              value={sql}
              onChange={handleSqlChange}
              onRunAll={() => runQuery(sql)}
              onRunAtCursor={pos => runQuery(getStatementAtCursor(sql, pos))}
              running={running}
            />
          )}
        </div>

        <VertDragHandle />

        {/* Bottom panel */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", background: "#0a0b0d" }}>
          <div style={{ display: "flex", borderBottom: "1px solid #2a2f3d", background: "#111318", flexShrink: 0 }}>
            {(["results", "console"] as BottomTab[]).map(tab => (
              <button key={tab} onClick={() => setBottomTab(tab)} style={{
                background: "none", border: "none",
                borderBottom: `2px solid ${bottomTab === tab ? "#4f8ef7" : "transparent"}`,
                color: bottomTab === tab ? "#e8eaf0" : "#6b7280",
                padding: "7px 14px", fontSize: 11, fontFamily: "inherit",
                cursor: "pointer", fontWeight: bottomTab === tab ? 600 : 400,
              }}>
                {tab === "results" ? resultsLabel : "Console"}
              </button>
            ))}
          </div>

          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ height: "100%", display: bottomTab === "results" ? "block" : "none" }}>
              {result ? (
                isMongo ? (
                  <DocumentView
                    columns={result.columns} rows={result.rows}
                    rowsAffected={result.rows_affected} executionMs={result.execution_ms}
                    error={result.error}
                  />
                ) : (
                  <ResultsGrid
                    columns={result.columns} rows={result.rows}
                    rowsAffected={result.rows_affected} executionMs={result.execution_ms}
                    error={result.error} capped={result.capped}
                  />
                )
              ) : (
                <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", flexDirection: "column", gap: 6 }}>
                  <span style={{ color: "#4b5563", fontSize: 12, fontFamily: "monospace" }}>No results yet</span>
                  <span style={{ color: "#374151", fontSize: 11 }}>
                    {isMongo ? "⌘↵ · run command" : "Ctrl+Enter · run at cursor  |  Ctrl+Shift+Enter · run all"}
                  </span>
                </div>
              )}
            </div>
            <div style={{ height: "100%", display: bottomTab === "console" ? "block" : "none" }}>
              <LogPanel />
            </div>
          </div>
        </div>
      </div>

      {/* ── Chat drag handle + panel ── */}
      <DragHandle onMouseDown={onChatDragStart} collapsed={chatCollapsed} onToggle={onChatToggle} label="AI Chat" />

      {chatCollapsed ? (
        <CollapsedLabel label="AI Chat" onClick={onChatToggle} />
      ) : (
        <div style={{ width: chatWidth, flexShrink: 0, overflow: "hidden" }}>
          <AiChat apiKey={aiKey} providerId={activeAi?.provider_id ?? "gemini"} schemaMetadata={schema} onInsertSql={handleInsertSql} onRunQuery={runQuery} onRefreshSchema={onRefreshSchema} />
        </div>
      )}
    </div>
  );
}
