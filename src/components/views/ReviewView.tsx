// src/components/views/ReviewView.tsx — Step 4

import { useState } from "react";
import type { ChangePlan, TransactionResult } from "../../types/changes";
import { DiffView }  from "../ui/DiffView";
import { LogOutput } from "../ui/LogOutput";
import { useChangeAgent } from "../../hooks/useChangeAgent";

interface Props {
  plan:   ChangePlan;
  onBack: () => void;
}

export function ReviewView({ plan, onBack }: Props) {
  const { executeChanges } = useChangeAgent();
  const [dryResult,  setDryResult]  = useState<TransactionResult | null>(null);
  const [execResult, setExecResult] = useState<TransactionResult | null>(null);
  const [dryLoading,  setDryLoading]  = useState(false);
  const [execLoading, setExecLoading] = useState(false);

  const commands = plan.steps
    .filter(s => s.type === "SQL")
    .map(s => s.command);

  const handleDryRun = async () => {
    setDryLoading(true);
    try {
      const res = await executeChanges(commands, true);
      setDryResult(res.result);
    } catch (e) {
      console.error(e);
    } finally {
      setDryLoading(false);
    }
  };

  const handleExecute = async () => {
    setExecLoading(true);
    try {
      const res = await executeChanges(commands, false);
      setExecResult(res.result);
    } catch (e) {
      console.error(e);
    } finally {
      setExecLoading(false);
    }
  };

  const executed = !!execResult?.success;

  return (
    <div style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 10, overflow: "hidden" }}>
      <div style={{ padding: "12px 16px", borderBottom: "1px solid #2a2f3d", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: ".8px", textTransform: "uppercase", color: "#6b7280" }}>
          Diff View
        </span>
        <span style={{ fontSize: 11, color: "#6b7280" }}>{plan.steps.length} statements</span>
      </div>

      <div style={{ padding: 16 }}>
        <DiffView steps={plan.steps} />

        {dryResult  && <LogOutput result={dryResult} />}
        {execResult && <LogOutput result={execResult} />}

        <div style={{ display: "flex", gap: 10, marginTop: 16, paddingTop: 16, borderTop: "1px solid #2a2f3d" }}>
          <button
            onClick={handleDryRun}
            disabled={dryLoading || executed}
            style={{
              background: "transparent", color: "#e8eaf0",
              border: "1px solid #2a2f3d", borderRadius: 6,
              padding: "8px 16px", fontSize: 13, fontFamily: "inherit",
              cursor: dryLoading || executed ? "not-allowed" : "pointer",
            }}
          >
            {dryLoading ? "Running…" : "⟳ Dry Run"}
          </button>

          <div style={{ flex: 1 }} />

          <button onClick={onBack} style={{ background: "transparent", color: "#e8eaf0", border: "1px solid #2a2f3d", borderRadius: 6, padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer" }}>
            ← Back
          </button>

          <button
            onClick={handleExecute}
            disabled={execLoading || executed}
            style={{
              background: executed ? "#052e16" : "#166534",
              color: "#22c55e",
              border: "1px solid #166534",
              borderRadius: 6, padding: "8px 16px",
              fontSize: 13, fontFamily: "inherit",
              cursor: execLoading || executed ? "not-allowed" : "pointer",
              fontWeight: 500,
            }}
          >
            {executed ? "✓ Executed" : execLoading ? "Executing…" : "✓ Execute Changes"}
          </button>
        </div>
      </div>
    </div>
  );
}
