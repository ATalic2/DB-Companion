// src/components/query/LogPanel.tsx
import { useState, useRef, useEffect, useCallback } from "react";
import { useAppLog, clearLog } from "../../hooks/useAppLog";
import type { LogEntry, LogLevel } from "../../hooks/useAppLog";

const DETAIL_MIN_H = 60;
const DETAIL_MAX_H = 600;
const DETAIL_DEFAULT_H = 200;

const LEVEL_STYLE: Record<LogLevel, { color: string; bg: string; label: string }> = {
  info:     { color: "#6b7280", bg: "transparent",  label: "INFO"  },
  debug:    { color: "#4b5563", bg: "transparent",  label: "DEBUG" },
  warn:     { color: "#f59e0b", bg: "transparent",  label: "WARN"  },
  error:    { color: "#ef4444", bg: "#450a0a22",    label: "ERROR" },
  request:  { color: "#4f8ef7", bg: "#0d1a3522",    label: "REQ"   },
  response: { color: "#22c55e", bg: "#052e1622",    label: "RES"   },
};

function DetailPane({ detail }: { detail: string }) {
  const [height, setHeight] = useState(DETAIL_DEFAULT_H);
  const dragging = useRef(false);
  const startY   = useRef(0);
  const startH   = useRef(0);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    startY.current   = e.clientY;
    startH.current   = height;

    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      const delta  = ev.clientY - startY.current;
      const newH   = Math.min(DETAIL_MAX_H, Math.max(DETAIL_MIN_H, startH.current + delta));
      setHeight(newH);
    };
    const onUp = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup",   onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
  }, [height]);

  const formatted = (() => {
    try { return JSON.stringify(JSON.parse(detail), null, 2); }
    catch { return detail; }
  })();

  return (
    <div style={{ padding: "0 10px 6px 10px" }}>
      <pre style={{
        background: "#080a0e", border: "1px solid #1a1f2e",
        borderRadius: 6, padding: "8px 10px", margin: 0,
        fontFamily: "monospace", fontSize: 10, color: "#a8c7fa",
        overflowX: "auto", whiteSpace: "pre-wrap", wordBreak: "break-all",
        height, overflowY: "auto",
        // keep the bottom resize handle inside the rounded border
        borderBottomLeftRadius: 0, borderBottomRightRadius: 0,
        borderBottom: "none",
      }}>
        {formatted}
      </pre>
      {/* Drag handle */}
      <div
        onMouseDown={onMouseDown}
        title="Drag to resize"
        style={{
          height: 6,
          background: "#1a1f2e",
          border: "1px solid #1a1f2e",
          borderTop: "2px solid #2a2f3d",
          borderBottomLeftRadius: 6,
          borderBottomRightRadius: 6,
          cursor: "ns-resize",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div style={{
          width: 28, height: 2, borderRadius: 1,
          background: "#374151",
          pointerEvents: "none",
        }} />
      </div>
    </div>
  );
}

function LogLine({ entry }: { entry: LogEntry }) {
  const [open, setOpen]   = useState(false);
  const style = LEVEL_STYLE[entry.level];
  const hasDetail = !!entry.detail;

  return (
    <div style={{ background: style.bg, borderBottom: "1px solid #0d0f14" }}>
      <div
        onClick={() => hasDetail && setOpen(v => !v)}
        style={{
          display: "flex", alignItems: "baseline", gap: 8,
          padding: "3px 10px", cursor: hasDetail ? "pointer" : "default",
          fontFamily: "monospace", fontSize: 11,
        }}
        onMouseEnter={e => { if (hasDetail) e.currentTarget.style.background = "#191c23"; }}
        onMouseLeave={e => { e.currentTarget.style.background = "transparent"; }}
      >
        <span style={{ color: "#374151", flexShrink: 0 }}>{entry.timestamp}</span>
        <span style={{
          color: style.color, fontSize: 9, fontWeight: 700,
          padding: "1px 4px", borderRadius: 3,
          border: `1px solid ${style.color}40`,
          flexShrink: 0, minWidth: 36, textAlign: "center",
        }}>
          {style.label}
        </span>
        <span style={{ color: entry.level === "error" ? "#f87171" : entry.level === "warn" ? "#fbbf24" : "#9ca3af", flex: 1, wordBreak: "break-all" }}>
          {entry.message}
        </span>
        {hasDetail && (
          <span style={{ color: "#4b5563", fontSize: 10, flexShrink: 0 }}>
            {open ? "▲" : "▼"}
          </span>
        )}
      </div>

      {open && entry.detail && (
        <DetailPane detail={entry.detail} />
      )}
    </div>
  );
}

export function LogPanel() {
  const { log } = useAppLog();
  const [filter, setFilter] = useState<LogLevel | "all">("all");
  const bottomRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    if (autoScroll) bottomRef.current?.scrollIntoView();
  }, [log, autoScroll]);

  const filtered = filter === "all" ? log : log.filter(e => e.level === filter);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "#0a0b0d" }}>
      {/* Toolbar */}
      <div style={{
        display: "flex", alignItems: "center", gap: 6,
        padding: "4px 10px", borderBottom: "1px solid #2a2f3d",
        background: "#111318", flexShrink: 0,
      }}>
        <span style={{ fontSize: 10, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1 }}>
          Console
        </span>
        <span style={{ fontSize: 10, color: "#374151" }}>{log.length} entries</span>

        {/* Filter pills */}
        <div style={{ display: "flex", gap: 4, marginLeft: 8 }}>
          {(["all", "request", "response", "error", "warn", "info", "debug"] as const).map(lvl => (
            <button
              key={lvl}
              onClick={() => setFilter(lvl)}
              style={{
                background: filter === lvl ? "#191c23" : "transparent",
                border: `1px solid ${filter === lvl ? "#2a2f3d" : "transparent"}`,
                borderRadius: 4, padding: "1px 7px", fontSize: 10,
                color: filter === lvl ? "#e8eaf0" : "#4b5563",
                cursor: "pointer", fontFamily: "monospace",
              }}
            >
              {lvl}
            </button>
          ))}
        </div>

        <div style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          <label style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 10, color: "#6b7280", cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={e => setAutoScroll(e.target.checked)}
              style={{ accentColor: "#4f8ef7" }}
            />
            Auto-scroll
          </label>
          <button
            onClick={clearLog}
            style={{ background: "transparent", border: "1px solid #2a2f3d", borderRadius: 4, padding: "2px 8px", color: "#6b7280", fontSize: 10, fontFamily: "inherit", cursor: "pointer" }}
          >
            Clear
          </button>
        </div>
      </div>

      {/* Log lines */}
      <div style={{ flex: 1, overflowY: "auto" }}>
        {filtered.length === 0 && (
          <div style={{ padding: "16px", fontSize: 11, color: "#374151", textAlign: "center", fontFamily: "monospace" }}>
            No log entries yet. Actions will appear here.
          </div>
        )}
        {filtered.map(entry => <LogLine key={entry.id} entry={entry} />)}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
