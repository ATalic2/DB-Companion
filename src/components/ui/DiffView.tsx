// src/components/ui/DiffView.tsx

import type { ChangeStep } from "../../types/changes";

interface Props {
  steps: ChangeStep[];
}

export function DiffView({ steps }: Props) {
  return (
    <div style={{ fontFamily: "monospace", fontSize: 11, lineHeight: 1.9 }}>
      {steps.map((step, i) => (
        <div key={i}>
          <span style={{
            display: "block", padding: "2px 12px",
            background: "transparent", color: "#6b7280",
            borderLeft: "3px solid transparent",
          }}>
            -- Step {i + 1}: {step.description}
          </span>
          {step.command.split("\n").map((line, j) => (
            <span key={j} style={{
              display: "block", padding: "2px 12px",
              background: "rgba(5,46,22,0.12)", color: "#4ade80",
              borderLeft: "3px solid #166534",
            }}>
              + {line}
            </span>
          ))}
          <span style={{ display: "block", padding: "4px 0" }} />
        </div>
      ))}
    </div>
  );
}
