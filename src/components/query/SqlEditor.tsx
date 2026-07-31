// src/components/query/SqlEditor.tsx
import { useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  value:         string;
  onChange:      (v: string) => void;
  onRunAll:      () => void;
  onRunAtCursor: (cursorPos: number) => void;
  running:       boolean;
}

export function SqlEditor({ value, onChange, onRunAll, onRunAtCursor, running }: Props) {
  const textareaRef    = useRef<HTMLTextAreaElement>(null);
  const lineNumbersRef = useRef<HTMLDivElement>(null);

  const syncScroll = () => {
    if (lineNumbersRef.current && textareaRef.current) {
      lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop;
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "Enter") {
      e.preventDefault();
      onRunAll();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      onRunAtCursor(textareaRef.current?.selectionStart ?? 0);
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const ta    = textareaRef.current!;
      const start = ta.selectionStart;
      const end   = ta.selectionEnd;
      const next  = value.substring(0, start) + "  " + value.substring(end);
      onChange(next);
      setTimeout(() => { ta.selectionStart = ta.selectionEnd = start + 2; }, 0);
    }
  };

  const handleRunAtCursor = () => {
    onRunAtCursor(textareaRef.current?.selectionStart ?? 0);
  };

  const handleOpenFile = async () => {
    try {
      const content = await invoke<string | null>("open_sql_file");
      if (content !== null) onChange(content);
    } catch (e) {
      console.error("open_sql_file error:", e);
    }
  };

  const lineCount = value.split("\n").length;
  const disabled  = running || !value.trim();

  const btnBase: React.CSSProperties = {
    border: "none", borderRadius: 5, padding: "4px 12px",
    fontSize: 11, fontFamily: "inherit", cursor: disabled ? "not-allowed" : "pointer",
    fontWeight: 500, display: "flex", alignItems: "center", gap: 5,
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* Toolbar */}
      <div style={{
        display: "flex", alignItems: "center", gap: 6,
        padding: "5px 10px", borderBottom: "1px solid #2a2f3d",
        background: "#111318", flexShrink: 0,
      }}>
        <span style={{ fontSize: 10, color: "#374151", fontFamily: "monospace", marginRight: 4 }}>
          Ctrl+Enter · cursor &nbsp;·&nbsp; Ctrl+Shift+Enter · all
        </span>

        <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          {/* Open SQL file */}
          <button
            onClick={handleOpenFile}
            title="Open a .sql file into the editor"
            style={{
              ...btnBase,
              background: "#191c23",
              color: "#9ca3af",
              border: "1px solid #2a2f3d",
            }}
          >
            <span style={{ fontSize: 12 }}>📂</span>
            Open File
          </button>

          {/* Run at cursor */}
          <button
            onClick={handleRunAtCursor}
            disabled={disabled}
            title="Run statement at cursor (Ctrl+Enter)"
            style={{
              ...btnBase,
              background: disabled ? "#151a24" : "#191c23",
              color:      disabled ? "#374151" : "#9ca3af",
              border:     "1px solid #2a2f3d",
            }}
          >
            <span style={{ fontSize: 10 }}>▶</span>
            Run at Cursor
          </button>

          {/* Run all */}
          <button
            onClick={onRunAll}
            disabled={disabled}
            title="Run all statements (Ctrl+Shift+Enter)"
            style={{
              ...btnBase,
              background: disabled ? "#1e3a5f" : "#4f8ef7",
              color:      disabled ? "#4a6fa5" : "#fff",
            }}
          >
            <span style={{ fontSize: 10 }}>▶▶</span>
            Run All
          </button>
        </div>
      </div>

      {/* Editor area */}
      <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
        {/* Line numbers */}
        <div
          ref={lineNumbersRef}
          style={{
            width: 44, flexShrink: 0,
            background: "#0d0f14", borderRight: "1px solid #1a1f2e",
            padding: "12px 0", textAlign: "right",
            fontFamily: "monospace", fontSize: 12, color: "#374151",
            userSelect: "none", overflowY: "hidden", lineHeight: "1.6",
          }}
        >
          {Array.from({ length: lineCount }, (_, i) => (
            <div key={i} style={{ paddingRight: 8 }}>{i + 1}</div>
          ))}
        </div>

        {/* Textarea */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={e => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onScroll={syncScroll}
          spellCheck={false}
          style={{
            flex: 1,
            background: "#0a0b0d", border: "none", outline: "none",
            color: "#e8eaf0", fontFamily: "monospace", fontSize: 12,
            lineHeight: 1.6, padding: 12, resize: "none",
            whiteSpace: "pre", overflowX: "auto",
          }}
          placeholder="SELECT * FROM users LIMIT 100;"
        />
      </div>
    </div>
  );
}
