// src/components/views/DescribeView.tsx — Step 1

import { useState } from "react";
import type { DbProfile, AiProfile } from "../../hooks/useCredentialStore";

interface Props {
  activeDb:       DbProfile | undefined;
  activeAi:   AiProfile | undefined;
  onNext:         (intent: string) => void;
  onOpenDbModal:  () => void;
  onOpenAiModal: () => void;
}

export function DescribeView({ activeDb, activeAi, onNext, onOpenDbModal, onOpenAiModal }: Props) {
  const [intent, setIntent] = useState("");
  const ready = !!activeDb && !!activeAi && intent.trim().length > 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>

      {/* Warning if missing credentials */}
      {(!activeDb || !activeAi) && (
        <div style={{ padding: "10px 16px", background: "#1c1102", border: "1px solid #854d0e", borderRadius: 8, fontSize: 12, color: "#f59e0b", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <span>
            {!activeDb && !activeAi ? "Select a database profile and AI key in the sidebar to continue."
              : !activeDb   ? "Select a database profile in the sidebar."
              : "Select a Gemini API key in the sidebar."}
          </span>
          <button
            onClick={!activeDb ? onOpenDbModal : onOpenAiModal}
            style={{ background: "#854d0e", border: "none", borderRadius: 4, padding: "4px 10px", color: "#fef3c7", fontSize: 11, fontFamily: "inherit", cursor: "pointer" }}
          >
            Add Now
          </button>
        </div>
      )}

      {/* Active selection summary */}
      {activeDb && activeAi && (
        <div style={{ display: "flex", gap: 8 }}>
          <div style={{ flex: 1, padding: "8px 12px", background: "#191c23", border: "1px solid #166534", borderRadius: 8, fontSize: 11, color: "#22c55e" }}>
            🗄 <strong>{activeDb.label}</strong> <span style={{ color: "#6b7280" }}>({activeDb.db_type})</span>
          </div>
          <div style={{ flex: 1, padding: "8px 12px", background: "#191c23", border: "1px solid #166534", borderRadius: 8, fontSize: 11, color: "#22c55e" }}>
            🤖 <strong>{activeAi.label}</strong>
          </div>
        </div>
      )}

      {/* Intent input */}
      <div style={{ background: "#111318", border: "1px solid #2a2f3d", borderRadius: 10, overflow: "hidden" }}>
        <div style={{ padding: "12px 16px", borderBottom: "1px solid #2a2f3d", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: ".8px", textTransform: "uppercase", color: "#6b7280" }}>Change intent</span>
          <span style={{ fontSize: 11, color: "#6b7280" }}>{activeDb?.label ?? "No database selected"}</span>
        </div>
        <textarea
          value={intent}
          onChange={e => setIntent(e.target.value)}
          disabled={!activeDb || !activeAi}
          placeholder={
            activeDb && activeAi
              ? "Describe what you want to do in plain English…\n\nExamples:\n• Add soft-delete to the users table with deleted_at timestamp\n• Rename 'email' to 'email_address' across all references\n• Add a composite index on (user_id, created_at) in the orders table\n• Normalize the address fields into a separate addresses table"
              : "Select a database profile and Gemini key first…"
          }
          style={{
            width: "100%", background: "transparent", border: "none", outline: "none",
            color: "#e8eaf0", fontSize: 14, fontFamily: "Syne, sans-serif",
            resize: "none", padding: 16, minHeight: 160, lineHeight: 1.6,
            opacity: activeDb && activeAi ? 1 : 0.4,
          }}
        />
        <div style={{ padding: "10px 16px", borderTop: "1px solid #2a2f3d", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <span style={{ fontSize: 11, color: "#6b7280" }}>AI will extract schema → propose plan → await your approval</span>
          <button
            disabled={!ready}
            onClick={() => onNext(intent)}
            style={{
              background: ready ? "#4f8ef7" : "#1e3a5f",
              color: ready ? "#fff" : "#4a6fa5",
              border: "none", borderRadius: 6, padding: "8px 16px",
              fontSize: 13, fontFamily: "inherit",
              cursor: ready ? "pointer" : "not-allowed", fontWeight: 500,
            }}
          >
            Analyse Schema →
          </button>
        </div>
      </div>
    </div>
  );
}
