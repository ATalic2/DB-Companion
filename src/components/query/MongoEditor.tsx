// src/components/query/MongoEditor.tsx
import { useRef, useState, useEffect } from "react";

interface Props {
  value:    string;
  onChange: (v: string) => void;
  onRun:    () => void;
  running:  boolean;
  scriptMode:         boolean;
  onToggleScriptMode: () => void;
}

const EXAMPLES = [
  { label: "find all",    cmd: '{"find":"collection","filter":{}}' },
  { label: "find where",  cmd: '{"find":"collection","filter":{"field":"value"}}' },
  { label: "insert",      cmd: '{"insert":"collection","documents":[{"field":"value"}]}' },
  { label: "update",      cmd: '{"update":"collection","updates":[{"q":{"field":"value"},"u":{"$set":{"field":"new"}}}]}' },
  { label: "delete",      cmd: '{"delete":"collection","deletes":[{"q":{"field":"value"},"limit":1}]}' },
  { label: "count",       cmd: '{"count":"collection","query":{}}' },
  { label: "createColl",  cmd: '{"create":"collection_name"}' },
  { label: "dropColl",    cmd: '{"drop":"collection_name"}' },
];

export function MongoEditor({ value, onChange, onRun, running, scriptMode, onToggleScriptMode }: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [jsonError, setJsonError] = useState<string | null>(null);

  const validate = (v: string) => {
    if (scriptMode) { setJsonError(null); return; } // script mode: plain mongosh text, not JSON
    const trimmed = v.trim();
    if (!trimmed) { setJsonError(null); return; }
    try { JSON.parse(trimmed); setJsonError(null); }
    catch (e: any) { setJsonError(e.message); }
  };

  const handleChange = (v: string) => {
    onChange(v);
    validate(v);
  };

  useEffect(() => { validate(value); }, [scriptMode]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      onRun();
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const ta    = textareaRef.current!;
      const start = ta.selectionStart;
      const end   = ta.selectionEnd;
      onChange(value.substring(0, start) + "  " + value.substring(end));
      setTimeout(() => { ta.selectionStart = ta.selectionEnd = start + 2; }, 0);
    }
  };

  const formatJson = () => {
    try {
      const parsed = JSON.parse(value.trim());
      onChange(JSON.stringify(parsed, null, 2));
      setJsonError(null);
    } catch {}
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "#0a0b0d" }}>

      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "6px 10px", borderBottom: "1px solid #2a2f3d", flexShrink: 0 }}>
        {/* Example snippets */}
        <div style={{ display: "flex", gap: 4, flexWrap: "wrap", flex: 1, overflow: "hidden" }}>
          {EXAMPLES.map(ex => (
            <button
              key={ex.label}
              onClick={() => { onChange(JSON.stringify(JSON.parse(ex.cmd), null, 2)); setJsonError(null); }}
              style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 3, padding: "2px 7px", color: "#6b7280", fontSize: 10, fontFamily: "monospace", cursor: "pointer" }}
              onMouseEnter={e => (e.currentTarget.style.color = "#e8eaf0")}
              onMouseLeave={e => (e.currentTarget.style.color = "#6b7280")}
            >
              {ex.label}
            </button>
          ))}
        </div>

        <div style={{ display: "flex", gap: 2, background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, padding: 2 }}>
          <button
            onClick={() => { if (scriptMode) onToggleScriptMode(); }}
            title={'JSON mode: run a single command document, e.g. { "find": ... }'}
            style={{
              background: scriptMode ? "transparent" : "#1e3a5f",
              border: "none", borderRadius: 3, padding: "3px 9px",
              color: scriptMode ? "#9ca3af" : "#4f8ef7",
              fontSize: 11, fontFamily: "inherit", cursor: "pointer",
            }}
          >
            JSON mode
          </button>
          <button
            onClick={() => { if (!scriptMode) onToggleScriptMode(); }}
            title={"Script mode: run a full multi-statement mongosh script (createCollection, createIndex, etc.)"}
            style={{
              background: scriptMode ? "#1e3a5f" : "transparent",
              border: "none", borderRadius: 3, padding: "3px 9px",
              color: scriptMode ? "#4f8ef7" : "#9ca3af",
              fontSize: 11, fontFamily: "inherit", cursor: "pointer",
            }}
          >
            Script mode
          </button>
        </div>

        <button
          onClick={formatJson}
          disabled={scriptMode}
          style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, padding: "3px 9px", color: scriptMode ? "#4b5563" : "#9ca3af", fontSize: 11, fontFamily: "inherit", cursor: scriptMode ? "not-allowed" : "pointer" }}
        >
          Format
        </button>

        <button
          onClick={onRun}
          disabled={running || !!jsonError}
          style={{ background: running || jsonError ? "#0d1a35" : "#1e3a5f", border: "1px solid #4f8ef7", borderRadius: 4, padding: "3px 12px", color: running || jsonError ? "#4f8ef7aa" : "#4f8ef7", fontSize: 11, fontFamily: "inherit", cursor: running || jsonError ? "not-allowed" : "pointer" }}
        >
          {running ? "Running…" : "Run  ⌘↵"}
        </button>
      </div>

      {/* Editor */}
      <div style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        <textarea
          ref={textareaRef}
          value={value}
          onChange={e => handleChange(e.target.value)}
          onKeyDown={handleKeyDown}
          spellCheck={false}
          placeholder={scriptMode
            ? 'db.createCollection("collection_name");\ndb.collection_name.createIndex({ "field": 1 }, { unique: true });\n\n// Paste a full mongosh setup script — runs as a session.\n// A leading "use dbname;" is fine, it\'s ignored (db is set by the connection profile).'
            : '{\n  "find": "users",\n  "filter": { "active": true }\n}'}
          style={{
            width: "100%", height: "100%",
            background: "#0a0b0d", color: "#e8eaf0",
            border: "none", outline: "none", resize: "none",
            fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
            fontSize: 13, lineHeight: 1.6,
            padding: "12px 14px",
            boxSizing: "border-box",
            caretColor: "#4f8ef7",
          }}
        />
      </div>

      {/* JSON validation */}
      {jsonError && (
        <div style={{ padding: "5px 12px", background: "#450a0a", borderTop: "1px solid #7f1d1d", fontSize: 11, color: "#f87171", fontFamily: "monospace", flexShrink: 0 }}>
          ✗ {jsonError}
        </div>
      )}
    </div>
  );
}
