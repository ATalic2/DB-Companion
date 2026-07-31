// src/components/views/SchemaView.tsx — Step 2

import type { SchemaMetadata } from "../../types/changes";

interface Props {
  schema:   SchemaMetadata | null;
  loading:  boolean;
  onBack:   () => void;
  onNext:   () => void;
}

export function SchemaView({ schema, loading, onBack, onNext }: Props) {
  if (loading) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: 200, color: "#6b7280" }}>
        Extracting schema…
      </div>
    );
  }

  return (
    <div style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 10, overflow: "hidden" }}>
      <div style={{ padding: "12px 16px", borderBottom: "1px solid #2a2f3d", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: ".8px", textTransform: "uppercase", color: "#6b7280" }}>
          Extracted Schema — {schema?.db_name}
        </span>
        <span style={{ fontFamily: "monospace", fontSize: 11, padding: "3px 8px", border: "1px solid #166534", borderRadius: 4, color: "#22c55e", background: "#052e16" }}>
          {schema?.tables.length ?? 0} tables
        </span>
      </div>

      <div style={{ padding: 16, display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
        {schema?.tables.map(table => (
          <div key={table.name} style={{
            background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 8, overflow: "hidden",
          }}>
            <div style={{ padding: "8px 12px", background: "#1a1f2e", borderBottom: "1px solid #2a2f3d", fontFamily: "monospace", fontSize: 12, color: "#4f8ef7" }}>
              ⬡ {table.name}
            </div>
            <div style={{ padding: "8px 0" }}>
              {table.columns.map(col => (
                <div key={col.name} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 12px", fontFamily: "monospace", fontSize: 11 }}>
                  {col.is_pk && <span style={{ fontSize: 9, padding: "2px 5px", background: "#0d1a35", color: "#4f8ef7", borderRadius: 3, border: "1px solid #1e3a5f" }}>PK</span>}
                  {col.is_fk && <span style={{ fontSize: 9, padding: "2px 5px", background: "#1c1102", color: "#f59e0b", borderRadius: 3, border: "1px solid #854d0e" }}>FK</span>}
                  <span>{col.name}</span>
                  <span style={{ marginLeft: "auto", color: "#6b7280" }}>{col.data_type}</span>
                </div>
              ))}
              {table.indexes.map(idx => (
                <div key={idx.name} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 12px", fontFamily: "monospace", fontSize: 11 }}>
                  <span style={{ fontSize: 9, padding: "2px 5px", background: "#1a0f2e", color: "#a78bfa", borderRadius: 3, border: "1px solid #4c1d95" }}>
                    {idx.unique ? "unique" : "idx"}
                  </span>
                  <span style={{ color: "#6b7280" }}>{idx.name}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      <div style={{ padding: "0 16px 16px", display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <button onClick={onBack} style={{ background: "transparent", color: "#e8eaf0", border: "1px solid #2a2f3d", borderRadius: 6, padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer" }}>
          ← Back
        </button>
        <button onClick={onNext} style={{ background: "#4f8ef7", color: "#fff", border: "none", borderRadius: 6, padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer", fontWeight: 500 }}>
          Generate Change Plan →
        </button>
      </div>
    </div>
  );
}
