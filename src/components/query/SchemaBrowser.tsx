// src/components/query/SchemaBrowser.tsx
import { useState } from "react";
import type { SchemaMetadata, TableMeta, ColumnMeta, IndexMeta, TableConstraint } from "../../types/changes";

interface Props {
  schema:        SchemaMetadata | null;
  onSelectTable: (table: TableMeta) => void;
  onRefresh:     () => void;
  loading:       boolean;
}

const SYSTEM_PREFIXES: Record<string, string[]> = {
  sqlite: ["sqlite_"],
};

function isSystemTable(name: string, dbType: string): boolean {
  return (SYSTEM_PREFIXES[dbType] ?? []).some(p => name.toLowerCase().startsWith(p.toLowerCase()));
}

// ─── Tiny shared primitives ───────────────────────────────────────────────────

function normaliseType(type: string): string {
  return type
    .replace("timestamp without time zone", "timestamp")
    .replace("timestamp with time zone",    "timestamptz")
    .replace("character varying",           "varchar")
    .replace("double precision",            "float8");
}

function TypePill({ type }: { type: string }) {
  const normalised = normaliseType(type);
  const short = normalised.length > 14 ? normalised.slice(0, 12) + "…" : normalised;
  return (
    <span style={{
      fontSize: 9, padding: "1px 4px", borderRadius: 3,
      background: "#0d1117", color: "#4b5563",
      border: "1px solid #1a1f2e", fontFamily: "monospace", flexShrink: 0,
    }}>
      {short}
    </span>
  );
}


function SectionWrapper({
  icon, label, count, children, defaultOpen = false,
}: {
  icon: string; label: string; count: number;
  children: React.ReactNode; defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  if (count === 0) return null;
  return (
    <div style={{ marginBottom: 1 }}>
      <div
        onClick={() => setOpen(v => !v)}
        style={{
          display: "flex", alignItems: "center", gap: 5,
          padding: "4px 8px 4px 4px", cursor: "pointer", userSelect: "none",
          borderRadius: 3,
        }}
        onMouseEnter={e => (e.currentTarget.style.background = "#13161e")}
        onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
      >
        <span style={{ fontSize: 9, color: "#2d3340", width: 8, flexShrink: 0 }}>
          {open ? "▼" : "▶"}
        </span>
        <span style={{ fontSize: 11 }}>{icon}</span>
        <span style={{ fontSize: 11, color: "#6b7280", flex: 1, fontWeight: 500 }}>{label}</span>
        <span style={{ fontSize: 9, color: "#2d3340" }}>{count}</span>
      </div>
      {open && (
        <div style={{ paddingLeft: 16 }}>
          {children}
        </div>
      )}
    </div>
  );
}

// ─── Section: Columns ─────────────────────────────────────────────────────────

function ColumnsSection({ columns }: { columns: ColumnMeta[] }) {
  return (
    <SectionWrapper icon="▤" label="Columns" count={columns.length} defaultOpen={true}>
      <div style={{ padding: "2px 0 4px" }}>
        {columns.map(col => (
          <div
            key={col.name}
            style={{ display: "flex", alignItems: "center", gap: 5, padding: "3px 6px 3px 0" }}
            title={col.default_val ? `Default: ${col.default_val}` : undefined}
          >
            {/* Badge */}
            {col.is_pk ? (
              <span style={{
                fontSize: 8, padding: "1px 3px", borderRadius: 3,
                background: "#0d1a35", color: "#4f8ef7",
                border: "1px solid #1e3a5f", flexShrink: 0, lineHeight: 1.4,
              }}>PK</span>
            ) : col.is_fk ? (
              <span style={{
                fontSize: 8, padding: "1px 3px", borderRadius: 3,
                background: "#1c1102", color: "#f59e0b",
                border: "1px solid #854d0e", flexShrink: 0, lineHeight: 1.4,
              }}>FK</span>
            ) : (
              <span style={{ width: 16, flexShrink: 0 }} />
            )}

            {/* Name */}
            <span style={{
              fontSize: 11, fontFamily: "monospace",
              color: col.is_pk ? "#e8eaf0" : "#9ca3af",
              flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
            }}>
              {col.name}
            </span>

            {/* Type */}
            <TypePill type={col.data_type} />
          </div>
        ))}
      </div>
    </SectionWrapper>
  );
}

// ─── Section: Foreign Keys ────────────────────────────────────────────────────

function ForeignKeysSection({ columns }: { columns: ColumnMeta[] }) {
  const fkCols = columns.filter(c => c.is_fk && c.fk);
  return (
    <SectionWrapper icon="⇢" label="Foreign Keys" count={fkCols.length}>
      <div style={{ padding: "2px 0 4px" }}>
        {fkCols.map(col => {
          const fk = col.fk!;
          const hasActions =
            (fk.on_delete && fk.on_delete !== "NO ACTION" && fk.on_delete !== "NO_ACTION") ||
            (fk.on_update && fk.on_update !== "NO ACTION" && fk.on_update !== "NO_ACTION");
          return (
            <div key={col.name} style={{ padding: "4px 6px 4px 0", borderBottom: "1px solid #0e1016" }}>
              {/* constraint name */}
              <div style={{ fontSize: 9, color: "#374151", fontFamily: "monospace", marginBottom: 2 }}>
                {fk.constraint_name}
              </div>
              {/* from col → ref */}
              <div style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
                <span style={{ fontSize: 11, fontFamily: "monospace", color: "#f59e0b" }}>
                  {col.name}
                </span>
                <span style={{ fontSize: 10, color: "#374151" }}>→</span>
                <span style={{ fontSize: 11, fontFamily: "monospace", color: "#60a5fa" }}>
                  {fk.ref_table}
                  <span style={{ color: "#374151" }}>.</span>
                  {fk.ref_column}
                </span>
              </div>
              {/* actions */}
              {hasActions && (
                <div style={{ display: "flex", gap: 6, marginTop: 2 }}>
                  {fk.on_delete && fk.on_delete !== "NO ACTION" && fk.on_delete !== "NO_ACTION" && (
                    <span style={{ fontSize: 9, color: "#6b7280" }}>
                      DEL: <span style={{ color: actionColor(fk.on_delete) }}>{fk.on_delete}</span>
                    </span>
                  )}
                  {fk.on_update && fk.on_update !== "NO ACTION" && fk.on_update !== "NO_ACTION" && (
                    <span style={{ fontSize: 9, color: "#6b7280" }}>
                      UPD: <span style={{ color: actionColor(fk.on_update) }}>{fk.on_update}</span>
                    </span>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </SectionWrapper>
  );
}

function actionColor(action: string): string {
  const a = action.toUpperCase();
  if (a.includes("CASCADE"))   return "#f87171"; // red — destructive
  if (a.includes("SET NULL"))  return "#fb923c"; // orange
  if (a.includes("SET DEFAULT")) return "#facc15"; // yellow
  if (a.includes("RESTRICT"))  return "#a78bfa"; // purple
  return "#6b7280";
}

// ─── Section: Constraints ─────────────────────────────────────────────────────

function ConstraintsSection({ constraints }: { constraints: TableConstraint[] }) {
  return (
    <SectionWrapper icon="✦" label="Constraints" count={constraints.length}>
      <div style={{ padding: "2px 0 4px" }}>
        {constraints.map(con => (
          <div key={con.name} style={{ padding: "4px 6px 4px 0", borderBottom: "1px solid #0e1016" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 5, marginBottom: 2 }}>
              <span style={{
                fontSize: 8, padding: "1px 4px", borderRadius: 3,
                background: con.constraint_type === "CHECK" ? "#1a0f00" : "#0f1a0f",
                color:      con.constraint_type === "CHECK" ? "#f97316" : "#4ade80",
                border: `1px solid ${con.constraint_type === "CHECK" ? "#7c2d12" : "#14532d"}`,
                flexShrink: 0, lineHeight: 1.4,
              }}>
                {con.constraint_type}
              </span>
              <span style={{
                fontSize: 10, fontFamily: "monospace", color: "#9ca3af",
                overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
              }}>
                {con.name}
              </span>
            </div>
            <div style={{
              fontSize: 10, fontFamily: "monospace", color: "#4b5563",
              whiteSpace: "pre-wrap", wordBreak: "break-word", lineHeight: 1.5,
            }}>
              {con.definition}
            </div>
          </div>
        ))}
      </div>
    </SectionWrapper>
  );
}

// ─── Section: Indexes ─────────────────────────────────────────────────────────

function IndexesSection({ indexes }: { indexes: IndexMeta[] }) {
  return (
    <SectionWrapper icon="⚡" label="Indexes" count={indexes.length}>
      <div style={{ padding: "2px 0 4px" }}>
        {indexes.map(idx => (
          <div key={idx.name} style={{ padding: "4px 6px 4px 0", borderBottom: "1px solid #0e1016" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 5, marginBottom: 2 }}>
              {idx.unique && (
                <span style={{
                  fontSize: 8, padding: "1px 3px", borderRadius: 3,
                  background: "#1a0f2e", color: "#a78bfa",
                  border: "1px solid #4c1d95", flexShrink: 0, lineHeight: 1.4,
                }}>UNIQUE</span>
              )}
              <span style={{
                fontSize: 10, fontFamily: "monospace", color: "#9ca3af",
                overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1,
              }}>
                {idx.name}
              </span>
              {idx.index_type && idx.index_type !== "btree" && (
                <span style={{ fontSize: 9, color: "#374151", flexShrink: 0 }}>{idx.index_type}</span>
              )}
            </div>
            <div style={{ fontSize: 10, fontFamily: "monospace", color: "#4b5563" }}>
              ({idx.columns.join(", ")})
              {idx.predicate && (
                <span style={{ color: "#374151" }}> WHERE {idx.predicate}</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </SectionWrapper>
  );
}

// ─── Table node ───────────────────────────────────────────────────────────────

function TableNode({ table, onSelect }: { table: TableMeta; onSelect: (t: TableMeta) => void }) {
  const [open, setOpen] = useState(false);

  const fkCount         = table.columns.filter(c => c.is_fk).length;
  const constraintCount = table.constraints?.length ?? 0;
  const indexCount      = table.indexes.length;

  return (
    <div>
      {/* Table row */}
      <div
        style={{ display: "flex", alignItems: "center", gap: 5, padding: "4px 8px", cursor: "pointer", userSelect: "none" }}
        onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
        onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
        onClick={() => { setOpen(v => !v); onSelect(table); }}
      >
        <span style={{ color: "#4b5563", fontSize: 9, width: 10, flexShrink: 0 }}>
          {open ? "▼" : "▶"}
        </span>
        <span style={{ fontSize: 12, fontFamily: "monospace", color: "#4f8ef7", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {table.name}
        </span>
        <span style={{ fontSize: 10, color: "#4b5563" }}>{table.columns.length}</span>
      </div>

      {/* Expanded sections */}
      {open && (
        <div style={{ paddingLeft: 14, paddingBottom: 4 }}>
          <ColumnsSection     columns={table.columns} />
          {fkCount > 0 && (
            <ForeignKeysSection columns={table.columns} />
          )}
          {constraintCount > 0 && (
            <ConstraintsSection constraints={table.constraints} />
          )}
          {indexCount > 0 && (
            <IndexesSection indexes={table.indexes} />
          )}
        </div>
      )}
    </div>
  );
}

// ─── Table group (user / system) ──────────────────────────────────────────────

function TableGroup({ label, tables, onSelect, defaultOpen, dimmed }: {
  label: string; tables: TableMeta[]; onSelect: (t: TableMeta) => void;
  defaultOpen: boolean; dimmed?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  if (tables.length === 0) return null;
  return (
    <div>
      <div
        onClick={() => setOpen(v => !v)}
        style={{ display: "flex", alignItems: "center", gap: 5, padding: "5px 8px", cursor: "pointer", userSelect: "none", borderBottom: "1px solid #1a1f2e" }}
        onMouseEnter={e => (e.currentTarget.style.background = "#191c23")}
        onMouseLeave={e => (e.currentTarget.style.background = "transparent")}
      >
        <span style={{ fontSize: 9, color: "#4b5563" }}>{open ? "▼" : "▶"}</span>
        <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: 1, textTransform: "uppercase", color: dimmed ? "#4b5563" : "#6b7280", flex: 1 }}>
          {label}
        </span>
        <span style={{ fontSize: 10, color: "#4b5563" }}>{tables.length}</span>
      </div>
      {open && tables.map(t => (
        <TableNode key={t.name} table={t} onSelect={onSelect} />
      ))}
    </div>
  );
}

// ─── Root ─────────────────────────────────────────────────────────────────────

export function SchemaBrowser({ schema, onSelectTable, onRefresh, loading }: Props) {
  const [search, setSearch] = useState("");

  const dbType    = schema?.db_type ?? "";
  const allTables = schema?.tables ?? [];
  const filtered  = allTables.filter(t => t.name.toLowerCase().includes(search.toLowerCase()));

  const userTables   = filtered.filter(t => !isSystemTable(t.name, dbType));
  const systemTables = filtered.filter(t =>  isSystemTable(t.name, dbType));

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* Header */}
      <div style={{ padding: "8px 10px", borderBottom: "1px solid #2a2f3d", display: "flex", alignItems: "center", gap: 6, flexShrink: 0 }}>
        <span style={{ fontSize: 10, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, flex: 1 }}>
          {schema ? schema.db_name : "Schema"}
        </span>
        {schema && (
          <span style={{ fontSize: 10, color: "#4b5563" }}>{allTables.length} tables</span>
        )}
        <button
          onClick={onRefresh}
          disabled={loading}
          style={{ background: "none", border: "none", color: "#6b7280", cursor: "pointer", fontSize: 13, padding: 2, lineHeight: 1 }}
          title="Refresh schema"
        >
          {loading ? "…" : "↻"}
        </button>
      </div>

      {/* Search */}
      <div style={{ padding: "6px 8px", borderBottom: "1px solid #2a2f3d", flexShrink: 0 }}>
        <input
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder="Filter tables…"
          style={{ width: "100%", background: "#0a0b0d", border: "1px solid #2a2f3d", borderRadius: 5, padding: "5px 8px", color: "#e8eaf0", fontSize: 11, fontFamily: "monospace", outline: "none", boxSizing: "border-box" }}
        />
      </div>

      {/* Tree */}
      <div style={{ flex: 1, overflowY: "auto", padding: "4px 0" }}>
        {!schema && !loading && (
          <div style={{ padding: "16px 12px", fontSize: 11, color: "#4b5563", textAlign: "center" }}>
            Connect to a database to see the schema.
          </div>
        )}
        {loading && (
          <div style={{ padding: "16px 12px", fontSize: 11, color: "#6b7280", textAlign: "center" }}>
            Loading schema…
          </div>
        )}
        {schema && (
          <>
            <TableGroup label="Tables" tables={userTables} onSelect={onSelectTable} defaultOpen={true} />
            <TableGroup label="System Tables" tables={systemTables} onSelect={onSelectTable} defaultOpen={false} dimmed />
            {filtered.length === 0 && search && (
              <div style={{ padding: "12px", fontSize: 11, color: "#4b5563", textAlign: "center" }}>
                No tables match "{search}"
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
