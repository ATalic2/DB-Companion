// src/components/ui/RiskMeter.tsx

interface Props {
  score: number; // 1–10
}

function riskColor(score: number) {
  if (score <= 3) return { bar: "#22c55e", text: "#22c55e", label: "Low risk · non-destructive" };
  if (score <= 6) return { bar: "#f59e0b", text: "#f59e0b", label: "Medium risk · review carefully" };
  return { bar: "#ef4444", text: "#ef4444", label: "High risk · destructive changes" };
}

export function RiskMeter({ score }: Props) {
  const { bar, text, label } = riskColor(score);
  const pct = (score / 10) * 100;

  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 12,
      padding: "12px 16px", background: "#191c23",
      borderRadius: 8, marginBottom: 12,
    }}>
      <span style={{ fontSize: 11, color: "#6b7280", whiteSpace: "nowrap" }}>Risk Score</span>
      <div style={{ flex: 1, height: 6, background: "#1f2537", borderRadius: 3, overflow: "hidden" }}>
        <div style={{ width: `${pct}%`, height: "100%", background: bar, borderRadius: 3, transition: "width .5s" }} />
      </div>
      <span style={{ fontFamily: "monospace", fontSize: 13, color: text, fontWeight: 500 }}>
        {score}/10
      </span>
      <span style={{ fontSize: 11, color: text }}>{label}</span>
    </div>
  );
}
