// src/components/views/PlanView.tsx — Step 3

import type { ChangePlan } from "../../types/changes";
import { RiskMeter } from "../ui/RiskMeter";
import { StepList }  from "../ui/StepList";

interface Props {
  plan:    ChangePlan | null;
  loading: boolean;
  onBack:  () => void;
  onNext:  () => void;
}

export function PlanView({ plan, loading, onBack, onNext }: Props) {
  if (loading) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: 200, color: "#6b7280" }}>
        Asking Gemini…
      </div>
    );
  }

  if (!plan) return null;

  return (
    <div style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 10, overflow: "hidden" }}>
      <div style={{ padding: "12px 16px", borderBottom: "1px solid #2a2f3d", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: ".8px", textTransform: "uppercase", color: "#6b7280" }}>
          AI Change Plan
        </span>
        <span style={{ fontSize: 11, color: "#6b7280", fontFamily: "monospace" }}>gemini-2.5-flash · structured output</span>
      </div>

      <div style={{ padding: 16 }}>
        <RiskMeter score={plan.risk_score} />

        <div style={{
          padding: "12px 16px", borderLeft: "3px solid #4f8ef7",
          background: "#0d1a35", borderRadius: "0 8px 8px 0",
          marginBottom: 16, fontSize: 13, lineHeight: 1.6, color: "#b8c5e0",
        }}>
          {plan.summary}
        </div>

        <div style={{ fontSize: 11, fontWeight: 700, color: "#6b7280", textTransform: "uppercase", letterSpacing: 1, marginBottom: 8 }}>
          Proposed Steps
        </div>

        <StepList steps={plan.steps} />
      </div>

      <div style={{ padding: "0 16px 16px", display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <button onClick={onBack} style={{ background: "transparent", color: "#e8eaf0", border: "1px solid #2a2f3d", borderRadius: 6, padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer" }}>
          ← Back
        </button>
        <button onClick={onNext} style={{ background: "#4f8ef7", color: "#fff", border: "none", borderRadius: 6, padding: "8px 16px", fontSize: 13, fontFamily: "inherit", cursor: "pointer", fontWeight: 500 }}>
          Review & Approve →
        </button>
      </div>
    </div>
  );
}
