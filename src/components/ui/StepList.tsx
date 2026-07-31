// src/components/ui/StepList.tsx

import { useState } from "react";
import type { ChangeStep } from "../../types/changes";

interface Props {
  steps: ChangeStep[];
}

export function StepList({ steps }: Props) {
  const [expanded, setExpanded] = useState<number | null>(0);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {steps.map((step, i) => (
        <div
          key={i}
          onClick={() => setExpanded(expanded === i ? null : i)}
          style={{
            border: "1px solid #2a2f3d", borderRadius: 8,
            overflow: "hidden", cursor: "pointer",
          }}
        >
          <div style={{
            display: "flex", alignItems: "center", gap: 10,
            padding: "10px 14px", background: "#191c23",
          }}>
            <span style={{
              fontSize: 10, padding: "2px 7px", borderRadius: 3,
              fontFamily: "monospace", fontWeight: 500,
              ...(step.type === "SQL"
                ? { background: "#0d1a35", color: "#4f8ef7", border: "1px solid #1e3a5f" }
                : { background: "#1c1102", color: "#f59e0b", border: "1px solid #854d0e" }),
            }}>
              {step.type}
            </span>
            <span style={{ fontSize: 12, fontWeight: 500 }}>{step.description}</span>
            <span style={{ marginLeft: "auto", fontSize: 11, color: "#6b7280" }}>
              {expanded === i ? "▲" : "▼"}
            </span>
          </div>
          {expanded === i && (
            <div style={{
              padding: "10px 14px", background: "#080a0e",
              borderTop: "1px solid #2a2f3d",
              fontFamily: "monospace", fontSize: 11, color: "#a8c7fa", lineHeight: 1.8,
              whiteSpace: "pre-wrap",
            }}>
              {step.command}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
