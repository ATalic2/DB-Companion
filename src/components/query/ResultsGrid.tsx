// src/components/query/ResultsGrid.tsx
import { useState } from "react";
import { invoke }   from "@tauri-apps/api/core";

interface Props {
  columns:      string[];
  rows:         any[][];
  rowsAffected: number;
  executionMs:  number;
  error:        string | null;
  capped:       boolean;
}

type ExportFmt = "CSV" | "JSON";

export function ResultsGrid({ columns, rows, rowsAffected, executionMs, error, capped }: Props) {
  const [showExport, setShowExport] = useState(false);
  const [fmt,        setFmt]        = useState<ExportFmt>("CSV");

  const buildContent = (f: ExportFmt): string => {
    if (f === "CSV") {
      const header = columns.join(",");
      const body   = rows.map(r =>
        r.map(c => `"${String(c ?? "").replace(/"/g, '""')}"`).join(",")
      ).join("\n");
      return header + "\n" + body;
    }
    return JSON.stringify(
      rows.map(r => Object.fromEntries(columns.map((c, i) => [c, r[i] ?? null]))),
      null, 2
    );
  };

  const handleExport = async () => {
    setShowExport(false);
    const ext = fmt.toLowerCase();
    await invoke("save_export_file", {
      args: {
        default_name: `results.${ext}`,
        extension:    ext,
        content:      buildContent(fmt),
      }
    });
  };

  if (error) return (
    <div style={{ padding: 16, background: "#450a0a", border: "1px solid #7f1d1d", borderRadius: 8, fontFamily: "monospace", fontSize: 12, color: "#f87171", margin: 12 }}>
      ✗ {error}
    </div>
  );

  // Only show DML banner when there are genuinely no columns AND no rows
  // (INSERT / UPDATE / DELETE). A SELECT always returns both together.
  if (columns.length === 0 && rows.length === 0 && rowsAffected > 0) return (
    <div style={{ padding: "12px 16px", fontSize: 12, color: "#22c55e", fontFamily: "monospace" }}>
      ✓ Query executed successfully · {rowsAffected} row{rowsAffected !== 1 ? "s" : ""} affected · {executionMs}ms
    </div>
  );

  if (columns.length === 0) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", borderBottom: "1px solid #2a2f3d", flexShrink: 0 }}>
        <span style={{ fontSize: 11, color: "#6b7280", fontFamily: "monospace" }}>
          {rows.length} row{rows.length !== 1 ? "s" : ""} · {executionMs}ms
        </span>
        {capped && (
          <span style={{
            fontSize: 10, padding: "2px 8px", borderRadius: 4,
            background: "#1c1400", color: "#f59e0b",
            border: "1px solid #854d0e", fontFamily: "monospace",
          }}>
            ⚠ capped at 1 000 rows — add LIMIT to see more
          </span>
        )}

        {/* Export controls */}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6, position: "relative" }}>
          {/* Format picker */}
          <div style={{ position: "relative" }}>
            <button
              onClick={() => setShowExport(v => !v)}
              style={{
                background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4,
                padding: "3px 10px", color: "#9ca3af", fontSize: 11,
                fontFamily: "inherit", cursor: "pointer",
                display: "flex", alignItems: "center", gap: 4,
              }}
            >
              {fmt} ▾
            </button>
            {showExport && (
              <>
                <div
                  style={{ position: "fixed", inset: 0, zIndex: 49 }}
                  onClick={() => setShowExport(false)}
                />
                <div style={{
                  position: "absolute", right: 0, top: "calc(100% + 4px)",
                  background: "#111318", border: "1px solid #2a2f3d",
                  borderRadius: 6, overflow: "hidden", zIndex: 50,
                  minWidth: 110, boxShadow: "0 4px 16px rgba(0,0,0,.5)",
                }}>
                  {(["CSV", "JSON"] as ExportFmt[]).map(f => (
                    <button
                      key={f}
                      onClick={() => { setFmt(f); setShowExport(false); }}
                      style={{
                        width: "100%", textAlign: "left", background: "none",
                        border: "none", color: "#e8eaf0", fontSize: 12,
                        fontFamily: "inherit", padding: "8px 14px",
                        cursor: "pointer", display: "flex", alignItems: "center", gap: 8,
                      }}
                      onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
                      onMouseLeave={e => (e.currentTarget.style.background = "none")}
                    >
                      <span style={{ width: 12, color: "#4f8ef7", fontSize: 11 }}>
                        {fmt === f ? "✓" : ""}
                      </span>
                      {f}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>

          {/* Export action button */}
          <button
            onClick={handleExport}
            style={{
              background: "#1e3a5f", border: "1px solid #2a4a7f", borderRadius: 4,
              padding: "3px 10px", color: "#93c5fd", fontSize: 11,
              fontFamily: "inherit", cursor: "pointer",
            }}
            onMouseEnter={e => { (e.currentTarget as HTMLButtonElement).style.background = "#254a7a"; }}
            onMouseLeave={e => { (e.currentTarget as HTMLButtonElement).style.background = "#1e3a5f"; }}
          >
            Export
          </button>
        </div>
      </div>

      {/* Grid */}
      <div style={{ flex: 1, overflow: "auto" }}>
        <table style={{ width: "100%", borderCollapse: "collapse", fontFamily: "monospace", fontSize: 12 }}>
          <thead>
            <tr style={{ position: "sticky", top: 0, background: "#111318", zIndex: 1 }}>
              {columns.map(col => (
                <th key={col} style={{ padding: "6px 12px", textAlign: "left", borderBottom: "1px solid #2a2f3d", color: "#6b7280", fontWeight: 600, fontSize: 11, whiteSpace: "nowrap" }}>
                  {col}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, ri) => (
              <tr key={ri} style={{ borderBottom: "1px solid #1a1f2e" }}
                onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
                onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
              >
                {row.map((cell, ci) => (
                  <td key={ci} style={{ padding: "5px 12px", color: cell === null ? "#4b5563" : "#e8eaf0", whiteSpace: "nowrap", maxWidth: 300, overflow: "hidden", textOverflow: "ellipsis" }}>
                    {cell === null ? "NULL" : String(cell)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
