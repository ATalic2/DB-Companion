// src/components/query/DocumentView.tsx
// Renders MongoDB query results as collapsible JSON documents
import { useState } from "react";

interface Props {
  columns:      string[];
  rows:         any[][];
  rowsAffected: number;
  executionMs:  number;
  error:        string | null;
}

// Recursively renders a JSON value with collapsible objects/arrays
function JsonNode({ value, depth = 0 }: { value: any; depth?: number }) {
  const [collapsed, setCollapsed] = useState(depth > 1);

  if (value === null) return <span style={{ color: "#6b7280" }}>null</span>;
  if (typeof value === "boolean") return <span style={{ color: "#f59e0b" }}>{String(value)}</span>;
  if (typeof value === "number") return <span style={{ color: "#34d399" }}>{value}</span>;
  if (typeof value === "string") return <span style={{ color: "#e5c07b" }}>"{value}"</span>;

  if (Array.isArray(value)) {
    if (value.length === 0) return <span style={{ color: "#6b7280" }}>[]</span>;
    return (
      <span>
        <button onClick={() => setCollapsed(v => !v)} style={BTN}>
          {collapsed ? `[ ${value.length} items ]` : "["}
        </button>
        {!collapsed && (
          <div style={{ paddingLeft: 16 }}>
            {value.map((item, i) => (
              <div key={i}>
                <span style={{ color: "#4b5563", fontSize: 10 }}>{i}: </span>
                <JsonNode value={item} depth={depth + 1} />
                {i < value.length - 1 && <span style={{ color: "#4b5563" }}>,</span>}
              </div>
            ))}
            <span style={{ color: "#6b7280" }}>]</span>
          </div>
        )}
      </span>
    );
  }

  if (typeof value === "object") {
    const entries = Object.entries(value);
    if (entries.length === 0) return <span style={{ color: "#6b7280" }}>{"{}"}</span>;
    return (
      <span>
        <button onClick={() => setCollapsed(v => !v)} style={BTN}>
          {collapsed ? `{ ${entries.length} fields }` : "{"}
        </button>
        {!collapsed && (
          <div style={{ paddingLeft: 16 }}>
            {entries.map(([k, v], i) => (
              <div key={k}>
                <span style={{ color: "#4f8ef7" }}>{k}</span>
                <span style={{ color: "#6b7280" }}>: </span>
                <JsonNode value={v} depth={depth + 1} />
                {i < entries.length - 1 && <span style={{ color: "#4b5563" }}>,</span>}
              </div>
            ))}
            <span style={{ color: "#6b7280" }}>{"}"}</span>
          </div>
        )}
      </span>
    );
  }

  return <span style={{ color: "#e8eaf0" }}>{String(value)}</span>;
}

const BTN: React.CSSProperties = {
  background: "none", border: "none", color: "#9ca3af",
  fontFamily: "monospace", fontSize: 12, cursor: "pointer",
  padding: 0, textDecoration: "underline dotted",
};

// Convert flat rows+columns back into documents for display
function rowsToDocuments(columns: string[], rows: any[][]): any[] {
  return rows.map(row =>
    Object.fromEntries(columns.map((col, i) => {
      let val = row[i];
      // If a value is a JSON string (nested object), parse it
      if (typeof val === "string") {
        try { val = JSON.parse(val); } catch {}
      }
      return [col, val ?? null];
    }))
  );
}

export function DocumentView({ columns, rows, rowsAffected, executionMs, error }: Props) {
  const [expandAll, setExpandAll] = useState(false);

  if (error) return (
    <div style={{ padding: 16, background: "#450a0a", border: "1px solid #7f1d1d", borderRadius: 8, fontFamily: "monospace", fontSize: 12, color: "#f87171", margin: 12 }}>
      ✗ {error}
    </div>
  );

  if (columns.length === 0 && rowsAffected > 0) return (
    <div style={{ padding: "12px 16px", fontSize: 12, color: "#22c55e", fontFamily: "monospace" }}>
      ✓ Command executed · {rowsAffected} affected · {executionMs}ms
    </div>
  );

  if (columns.length === 0) return null;

  const docs = rowsToDocuments(columns, rows);

  const exportJson = () => {
    const blob = new Blob([JSON.stringify(docs, null, 2)], { type: "application/json" });
    const url  = URL.createObjectURL(blob);
    const a    = document.createElement("a");
    a.href = url; a.download = "results.json"; a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", borderBottom: "1px solid #2a2f3d", flexShrink: 0 }}>
        <span style={{ fontSize: 11, color: "#6b7280", fontFamily: "monospace" }}>
          {docs.length} document{docs.length !== 1 ? "s" : ""} · {executionMs}ms
        </span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            onClick={() => setExpandAll(v => !v)}
            style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, padding: "3px 9px", color: "#9ca3af", fontSize: 11, fontFamily: "inherit", cursor: "pointer" }}
          >
            {expandAll ? "Collapse all" : "Expand all"}
          </button>
          <button
            onClick={exportJson}
            style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, padding: "3px 9px", color: "#9ca3af", fontSize: 11, fontFamily: "inherit", cursor: "pointer" }}
          >
            Export JSON
          </button>
        </div>
      </div>

      {/* Document list */}
      <div style={{ flex: 1, overflowY: "auto", padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        {docs.map((doc, i) => (
          <div key={expandAll ? `e${i}` : `c${i}`} style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 6, padding: "10px 12px", fontFamily: "monospace", fontSize: 12, lineHeight: 1.7 }}>
            <div style={{ fontSize: 10, color: "#374151", marginBottom: 6, fontFamily: "inherit" }}>
              Document {i + 1}
            </div>
            <JsonNode value={doc} depth={expandAll ? 99 : 0} />
          </div>
        ))}
      </div>
    </div>
  );
}
