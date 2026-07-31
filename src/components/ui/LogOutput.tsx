// src/components/ui/LogOutput.tsx

import type { TransactionResult } from "../../types/changes";

interface Props {
  result: TransactionResult;
}

export function LogOutput({ result }: Props) {
  const ts = new Date().toLocaleTimeString("en-GB");
  const color = result.success ? "#22c55e" : "#ef4444";
  const bg    = result.success ? "#052e16"  : "#450a0a";
  const border = result.success ? "#166534" : "#7f1d1d";

  return (
    <div style={{
      marginTop: 12, padding: 14, background: bg,
      border: `1px solid ${border}`, borderRadius: 8,
      fontFamily: "monospace", fontSize: 11, color, lineHeight: 1.8,
    }}>
      {result.statements.map((s, i) => (
        <div key={i}>
          [{ts}] {s.sql.substring(0, 60)}… — {s.ok ? "OK" : `FAILED: ${s.error}`}
        </div>
      ))}
      {result.dry_run ? (
        <div style={{ color: "#f59e0b", marginTop: 4 }}>
          ⟳ Dry run complete — transaction rolled back cleanly
        </div>
      ) : (
        <div style={{ fontWeight: 500, marginTop: 4 }}>
          {result.success
            ? `● ${result.statements.length} statements · 0 errors · ACID-compliant`
            : `✗ Change failed — full rollback applied`}
        </div>
      )}
    </div>
  );
}
