// src/components/query/AiChat.tsx — Single smart chat, AI decides answer vs plan
import React, { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { appendLog } from "../../hooks/useAppLog";
import { AI_PROVIDERS } from "../ui/SettingsModal";
import type { SchemaMetadata } from "../../types/changes";

interface ChangeStep {
  type:        string;
  command:     string | null;
  description: string;
}

interface ChangePlan {
  summary:    string;
  risk_score: number;
  steps:      ChangeStep[];
}

interface ChatResponse {
  response_type: "answer" | "plan";
  message:       string;
  plan?:         ChangePlan;
}

type PlanStatus = "pending" | "dry_run_ok" | "dry_run_fail" | "applied" | "failed";

interface Message {
  role:          "user" | "assistant";
  response_type: "answer" | "plan";
  content:       string;
  plan?:         ChangePlan;
  planStatus?:   PlanStatus;
  resultText?:   string;
  streaming?:    boolean;
  failed?:       boolean; // user message that got no response due to an error
}

interface HistoryEntry {
  role:    string;
  content: string;
}

interface Props {
  apiKey:           string;
  providerId?:      string;
  schemaMetadata:   SchemaMetadata | null;
  onInsertSql:      (sql: string) => void;
  onRunQuery:       (sql: string) => Promise<{ rows: any[][]; error: string | null } | null>;
  onRefreshSchema:  () => void;
}

function getModelsForProvider(providerId: string): string[] {
  return AI_PROVIDERS.find(p => p.id === providerId)?.models ?? [];
}

function RiskBadge({ score }: { score: number }) {
  const color = score <= 3 ? "#22c55e" : score <= 6 ? "#f59e0b" : "#ef4444";
  const label = score <= 3 ? "Low" : score <= 6 ? "Medium" : "High";
  return (
    <span style={{ fontSize: 10, padding: "2px 7px", borderRadius: 3, background: `${color}15`, color, border: `1px solid ${color}35`, fontFamily: "monospace" }}>
      {label} · {score}/10
    </span>
  );
}

function InputArea({ apiKey, providerId, input, setInput, loading, onSend }: {
  apiKey:    string;
  providerId: string;
  input:    string;
  setInput: (v: string) => void;
  loading:  boolean;
  onSend:   () => void;
}) {
  const [height,   setHeight]   = useState(80);
  const [dragging, setDragging] = useState(false);
  const startY   = useRef(0);
  const startH   = useRef(0);

  const onMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    startY.current = e.clientY;
    startH.current = height;
    setDragging(true);
  };

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const delta = startY.current - e.clientY; 
      setHeight(Math.min(300, Math.max(48, startH.current + delta)));
    };
    const onUp = () => setDragging(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup",   onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup",   onUp);
    };
  }, [dragging]);

  const disabled = loading || !input.trim() || !apiKey;

  return (
    <div style={{ borderTop: "1px solid #2a2f3d", background: "#111318", flexShrink: 0 }}>
      <div
        onMouseDown={onMouseDown}
        style={{
          height: 5, cursor: "row-resize",
          background: dragging ? "#4f8ef7" : "#1a1f2e",
          transition: dragging ? "none" : "background .15s",
        }}
      />
      <div style={{ padding: "8px 10px" }}>
        {!apiKey && (
          <div style={{ fontSize: 11, color: "#f59e0b", marginBottom: 6, textAlign: "center" }}>
            Add an AI provider key in the sidebar to get started.
          </div>
        )}
        <textarea
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSend(); } }}
          placeholder={apiKey ? "Ask a question or describe a change…" : "No AI key selected…"}
          disabled={!apiKey}
          style={{
            width: "100%", height: height,
            background: "#0a0b0d", border: "1px solid #2a2f3d",
            borderRadius: 6, padding: "7px 10px", color: "#e8eaf0",
            fontSize: 12, fontFamily: "inherit", resize: "none", outline: "none",
            opacity: apiKey ? 1 : 0.4, display: "block",
          }}
        />
        <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 6, marginTop: 6 }}>
          <ModelSelect providerId={providerId} />
          <button
            onClick={onSend}
            disabled={disabled}
            style={{
              background: disabled ? "#1e3a5f" : "#4f8ef7",
              border: "none", borderRadius: 6, padding: "6px 20px",
              color: disabled ? "#4a6fa5" : "#fff",
              fontSize: 12, cursor: disabled ? "not-allowed" : "pointer",
              flexShrink: 0,
            }}
          >
            {loading ? "Thinking…" : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}

// Model selection context — shared between AiChat and InputArea without prop-drilling
const ModelContext = React.createContext<{ model: string; setModel: (m: string) => void }>({
  model: "", setModel: () => {},
});

function ModelSelect({ providerId }: { providerId: string }) {
  const { model, setModel } = React.useContext(ModelContext);
  const models = getModelsForProvider(providerId);
  if (models.length === 0) return null;
  return (
    <select
      value={model}
      onChange={e => setModel(e.target.value)}
      style={{
        background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 6,
        padding: "5px 8px", color: "#9ca3af", fontSize: 11,
        fontFamily: "inherit", cursor: "pointer", maxWidth: 160,
      }}
    >
      {models.map(m => <option key={m} value={m}>{m}</option>)}
    </select>
  );
}

export function AiChat({ apiKey, providerId = "gemini", schemaMetadata: _schemaMetadata, onInsertSql, onRunQuery, onRefreshSchema }: Props) {
  const defaultModel = getModelsForProvider(providerId)[0] ?? "";
  const [selectedModel, setSelectedModel] = useState(defaultModel);

  // Reset model when provider changes
  React.useEffect(() => {
    setSelectedModel(getModelsForProvider(providerId)[0] ?? "");
  }, [providerId]);

  const [messages, setMessages] = useState<Message[]>([
    {
      role: "assistant", response_type: "answer",
      content: "Hi! I can answer questions about your database or propose schema changes.\n\nAsk me anything, or describe a change you want to make — I'll generate a plan for you to review before anything runs.",
    }
  ]);
  const [input,   setInput]   = useState("");
  const [loading, setLoading] = useState(false);
  const [error,   setError]   = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);

  const buildHistory = (): HistoryEntry[] =>
    messages
      .filter(m => m.role !== "assistant" || messages.indexOf(m) > 0)
      .map(m => ({
        role:    m.role,
        content: m.role === "assistant"
          ? m.plan
            ? `[Change plan proposed]\n${m.content}\nSummary: ${m.plan.summary}`
            : m.content
          : m.content,
      }));

  const send = async () => {
    if (!input.trim() || loading || !apiKey) return;
    const userText = input.trim();
    setInput("");
    setError(null);

    const placeholderIdx = messages.length + 1;
    setMessages(m => [...m, 
      { role: "user", response_type: "answer", content: userText },
      { role: "assistant", response_type: "answer", content: "", streaming: true }
    ]);
    setLoading(true);

    let accumulated = "";
    let renderBuf   = "";
    let rafId: ReturnType<typeof setTimeout> | null = null;

    const flushRender = () => {
      if (renderBuf.length === 0) return;
      accumulated += renderBuf;
      renderBuf = "";
      setMessages(m => {
        const next = [...m];
        if (next[placeholderIdx]) {
          next[placeholderIdx] = { ...next[placeholderIdx], content: accumulated };
        }
        return next;
      });
    };

    const unlisten = await listen<string>("ai_chunk", event => {
      renderBuf += event.payload;
      if (!rafId) {
        rafId = setTimeout(() => {
          flushRender();
          rafId = null;
        }, 40);
      }
    });

    try {
      const res = await invoke<ChatResponse>("ai_chat_stream", {
        args: {
          message:     userText,
          history:     buildHistory(),
          api_key:     apiKey,
          provider_id: providerId,
          model:       selectedModel || undefined,
        }
      });

      unlisten();
      if (rafId) { clearTimeout(rafId); rafId = null; }
      flushRender();

      setMessages(m => {
        const next = [...m];
        next[placeholderIdx] = {
          role:          "assistant",
          response_type: res.response_type,
          content:       res.message,
          plan:          res.plan,
          planStatus:    res.plan ? "pending" : undefined,
          streaming:     false,
        };
        return next;
      });
    } catch (e: any) {
      unlisten();
      if (rafId) { clearTimeout(rafId); rafId = null; }
      // Remove the stuck assistant placeholder, mark the user message as failed.
      setMessages(m =>
        m
          .filter((_, i) => i !== placeholderIdx)
          .map((msg, i) =>
            i === placeholderIdx - 1
              ? { ...msg, failed: true }
              : msg.streaming ? { ...msg, streaming: false } : msg
          )
      );
      setError(e?.message ?? String(e));
    } finally {
      setLoading(false);
    }
  };

  const executePlan = async (plan: ChangePlan, msgIdx: number) => {
    const commands = plan.steps.filter(s => s.type === "SQL" && s.command).map(s => s.command!);
    if (!commands.length) { setError("No SQL steps in this plan."); return; }

    const isSelectOnly = commands.every(c => /^\s*select\b/i.test(c.trim()));
    if (isSelectOnly) {
      const res = await onRunQuery(commands.join("\n"));
      const label = res?.error
        ? `✗ Query error: ${res.error}`
        : `✓ ${res?.rows?.length ?? 0} row${(res?.rows?.length ?? 0) !== 1 ? "s" : ""} returned.`;
      setMessages(m => {
        const next = [...m];
        next[msgIdx] = { ...next[msgIdx], planStatus: res?.error ? "failed" : "applied", resultText: label };
        return next;
      });
      return;
    }
  
    // 1. Set UI to 'Applying' state
    setMessages(m => {
      const next = [...m];
      next[msgIdx] = { ...next[msgIdx], planStatus: "pending", resultText: "Applying changes..." };
      return next;
    });
  
    try {
      const res = await invoke<{ result: { success: boolean; error: string | null; rows_affected: number } }>(
        "execute_changes", { args: { commands, dry_run: false } }
      );
      const { success, error: execErr, rows_affected } = res.result;
  
      if (success) {
        appendLog(
          "info",
          `Changes applied — ${plan.summary ?? "Changes"} (${rows_affected} row${rows_affected === 1 ? "" : "s"} affected)`,
          JSON.stringify({ summary: plan.summary, steps: commands }, null, 2),
        );
        setMessages(m => {
          const next = [...m];
          next[msgIdx] = { 
            ...next[msgIdx], 
            planStatus: "applied", 
            resultText: `✓ Applied — ${rows_affected} rows affected.` 
          };
          return next;
        });
        onRefreshSchema();
        return;
      }
  
      // 2. Handle Failure: Ask AI for a correction, but DO NOT auto-execute
      appendLog(
        "error",
        `Change failed — ${plan.summary ?? "Changes"}: ${execErr}`,
        JSON.stringify({ summary: plan.summary, steps: commands, error: execErr }, null, 2),
      );
      setMessages(m => {
        const next = [...m];
        next[msgIdx] = { 
          ...next[msgIdx], 
          planStatus: "failed", 
          resultText: `✗ Failed: ${execErr}. Asking AI for a fix…` 
        };
        return next;
      });
  
      setLoading(true);
      try {
        // Build a targeted correction prompt based on the error type,
      // so the AI gets actionable guidance instead of guessing blindly.
      const buildCorrectionPrompt = (err: string, sqls: string[]): string => {
        const sql = sqls.join('\n');
        const base = `The following SQL failed with this error:\n${err}\n\nSQL that failed:\n${sql}\n\n`;

        // Generated/computed column — AI must omit it from INSERT
        const generatedCol = err.match(/cannot insert.*non-DEFAULT.*column ["\`]?([\w.]+)["\`]?/i)
                          ?? err.match(/column ["\`]?([\w.]+)["\`]? is a generated column/i);
        if (generatedCol) {
          return base + `IMPORTANT: The column "${generatedCol[1]}" is a GENERATED ALWAYS column — Postgres computes its value automatically. Remove it entirely from the INSERT column list and VALUES. Do not attempt to provide a value for it.`;
        }

        // Check constraint violation
        const checkViolation = err.match(/violates check constraint ["\`]?([\w.]+)["\`]?/i);
        if (checkViolation) {
          return base + `IMPORTANT: The value violates the check constraint "${checkViolation[1]}". Review the allowed values for this constraint from the schema and use only valid values.`;
        }

        // Unique/duplicate key
        if (/duplicate key|unique constraint/i.test(err)) {
          return base + `IMPORTANT: A duplicate key error occurred. Use different unique values, or add ON CONFLICT DO NOTHING if duplicates are acceptable.`;
        }

        // Foreign key violation
        if (/foreign key constraint/i.test(err)) {
          return base + `IMPORTANT: A foreign key constraint was violated. Make sure referenced rows exist before inserting dependent rows, or insert parent rows first.`;
        }

        // Not null violation
        const notNull = err.match(/null value in column ["\`]?([\w.]+)["\`]? .*not.null/i);
        if (notNull) {
          return base + `IMPORTANT: Column "${notNull[1]}" does not allow NULL. Provide a concrete non-null value for it.`;
        }

        // Generic fallback
        return base + `Please provide a corrected plan that avoids this error. Do not repeat the same SQL.`;
      };

      const correctionPrompt = buildCorrectionPrompt(execErr!, commands);
        
        const corrected = await invoke<ChatResponse>("ai_chat_stream", {
          args: { message: correctionPrompt, history: buildHistory(), api_key: apiKey, provider_id: providerId, model: selectedModel || undefined }
        });
  
        if (corrected.response_type === "plan" && corrected.plan) {
          // Just add the new message. The UI will render a new "Apply" button for this plan.
          setMessages(m => [...m, { 
            role: "assistant", 
            response_type: "plan", 
            content: corrected.message, 
            plan: corrected.plan, 
            planStatus: "pending" 
          }]);
        } else {
          // If the AI just sends an answer/explanation instead of a new plan
          setMessages(m => [...m, { 
            role: "assistant", 
            response_type: "answer", 
            content: corrected.message 
          }]);
        }
      } finally {
        setLoading(false);
      }
  
    } catch (e: any) {
      setError(e?.message ?? String(e));
      setMessages(m => {
        const next = [...m];
        next[msgIdx] = { ...next[msgIdx], planStatus: "failed", resultText: "Execution error." };
        return next;
      });
    }
  };

  return (
    <ModelContext.Provider value={{ model: selectedModel, setModel: setSelectedModel }}>
    <style>{`
      @keyframes ai-dot-pulse {
        0%, 80%, 100% { opacity: 0.15; transform: scale(0.8); }
        40%            { opacity: 1;    transform: scale(1.15); }
      }
    `}</style>
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "#0d0f14" }}>
      <div style={{ padding: "9px 14px", borderBottom: "1px solid #2a2f3d", background: "#111318", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: "#e8eaf0" }}>AI Assistant</span>
        <button
          onClick={() => { setMessages([]); setError(null); }}
          title="New chat"
          style={{ background: "none", border: "1px solid #2a2f3d", borderRadius: 4, padding: "2px 8px", color: "#6b7280", fontSize: 10, cursor: "pointer" }}
          onMouseEnter={e => (e.currentTarget.style.color = "#e8eaf0")}
          onMouseLeave={e => (e.currentTarget.style.color = "#6b7280")}
        >
          + New chat
        </button>
      </div>

      <div style={{ flex: 1, overflowY: "auto", padding: "12px 10px", display: "flex", flexDirection: "column", gap: 12 }}>
        {messages.map((msg, i) => (
          <div key={i}>
            {msg.role === "user" && (
              <div style={{ display: "flex", justifyContent: "flex-end" }}>
                <div style={{
                  maxWidth: "85%", padding: "8px 12px",
                  borderRadius: "12px 12px 2px 12px", fontSize: 12,
                  background: msg.failed ? "#1a1008" : "#0d1a35",
                  border: `1px solid ${msg.failed ? "#854d0e" : "#1e3a5f"}`,
                  color: msg.failed ? "#9ca3af" : "#e8eaf0",
                  whiteSpace: "pre-wrap",
                  opacity: msg.failed ? 0.7 : 1,
                }}>
                  {msg.content}
                  {msg.failed && (
                    <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 6 }}>
                      <button
                        onClick={() => {
                          setMessages(m => m.filter((_, mi) => mi !== i));
                          setInput(msg.content);
                          setError(null);
                        }}
                        title="Resend message"
                        style={{
                          background: "#2a1a08", border: "1px solid #854d0e",
                          borderRadius: 6, color: "#f59e0b",
                          fontSize: 10, padding: "2px 8px",
                          cursor: "pointer", lineHeight: 1.6,
                        }}
                      >↺ Resend</button>
                    </div>
                  )}
                </div>
              </div>
            )}

            {msg.role === "assistant" && msg.response_type === "answer" && (!msg.streaming || msg.content) && (
              <div style={{ display: "flex", justifyContent: "flex-start" }}>
                <div style={{ maxWidth: "90%", padding: "8px 12px", borderRadius: "12px 12px 12px 2px", fontSize: 12, background: "#191c23", border: "1px solid #2a2f3d", color: "#d1d5db" }}>
                  {msg.content.split(/(```sql[\s\S]*?```|```[\s\S]*?```)/g).map((part, pi) => {
                    if (part.startsWith("```")) {
                      const code = part.replace(/```sql\n?|```\n?/g, "").trim();
                      return (
                        <div key={pi} style={{ position: "relative", margin: "6px 0" }}>
                          <pre style={{ background: "#080a0e", border: "1px solid #2a2f3d", borderRadius: 6, padding: "8px", fontSize: 11, color: "#a8c7fa", whiteSpace: "pre-wrap", wordBreak: "break-all", overflowX: "auto" }}>{code}</pre>
                          <button onClick={() => onInsertSql(code)} style={{ position: "absolute", top: 4, right: 4, background: "#191c23", border: "1px solid #2a2f3d", borderRadius: 4, padding: "2px 7px", color: "#6b7280", fontSize: 10 }}>↗</button>
                        </div>
                      );
                    }
                    return <span key={pi} style={{ whiteSpace: "pre-wrap" }}>{part}</span>;
                  })}
                  {msg.streaming && msg.content && <span style={{ display: "inline-block", width: 2, height: "1em", background: "#4f8ef7", marginLeft: 2 }} />}
                </div>
              </div>
            )}

            {msg.role === "assistant" && msg.response_type === "plan" && msg.plan && (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {msg.content && (
                  <div style={{ maxWidth: "90%", padding: "8px 12px", borderRadius: "12px 12px 12px 2px", fontSize: 12, background: "#191c23", border: "1px solid #2a2f3d", color: "#d1d5db" }}>
                    {msg.content}
                  </div>
                )}
                <div style={{ background: "#111318", border: `1px solid ${msg.planStatus === "applied" ? "#166534" : msg.planStatus === "failed" ? "#7f1d1d" : "#2a2f3d"}`, borderRadius: 10 }}>
                  <div style={{ padding: "9px 14px", borderBottom: "1px solid #2a2f3d", display: "flex", gap: 8 }}>
                    <span style={{ fontSize: 10, fontWeight: 700, color: "#6b7280" }}>PLAN</span>
                    <RiskBadge score={msg.plan.risk_score} />
                    <span style={{ marginLeft: "auto", fontSize: 10 }}>{msg.planStatus}</span>
                  </div>
                  <div style={{ padding: "9px 14px", fontSize: 12, color: "#9ca3af" }}>{msg.plan.summary}</div>
                  <div style={{ padding: "8px 14px", display: "flex", flexDirection: "column", gap: 6 }}>
                    {msg.plan.steps.map((step, si) => (
                      <div key={si} style={{ background: "#0a0b0d", borderRadius: 6, border: "1px solid #1a1f2e" }}>
                        <div style={{ padding: "5px 10px", fontSize: 11, color: "#6b7280" }}>{step.description}</div>
                        <pre style={{ padding: "6px 10px", fontSize: 11, color: "#a8c7fa", whiteSpace: "pre-wrap", wordBreak: "break-all", overflowX: "auto" }}>{step.command}</pre>
                      </div>
                    ))}
                  </div>
                  {msg.resultText && <div style={{ padding: "8px 14px", fontSize: 11, color: "#22c55e", borderTop: "1px solid #2a2f3d" }}>{msg.resultText}</div>}
                  {msg.planStatus === "pending" && (
                    <div style={{ padding: "10px 14px", borderTop: "1px solid #2a2f3d" }}>
                      <button onClick={() => executePlan(msg.plan!, i)} style={{ width: "100%", background: "#166534", border: "none", borderRadius: 6, padding: "6px", color: "#fff", fontSize: 11 }}>✓ Apply</button>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        ))}
        {messages.some(m => m.streaming && !m.content) && (
          <div style={{ display: "flex", justifyContent: "flex-start", marginBottom: 8 }}>
            <div style={{ background: "#191c23", border: "1px solid #2a2f3d", borderRadius: "12px 12px 12px 2px", padding: "10px 14px", display: "flex", alignItems: "center", gap: 5 }}>
              {[0, 1, 2].map(i => (
                <span key={i} style={{
                  display: "inline-block", width: 6, height: 6, borderRadius: "50%",
                  background: "#4f8ef7", opacity: 0.3,
                  animation: "ai-dot-pulse 1.2s ease-in-out infinite",
                  animationDelay: `${i * 0.2}s`,
                }} />
              ))}
            </div>
          </div>
        )}
        {error && <div style={{ color: "#f87171", fontSize: 11 }}>✗ {error}</div>}
        <div ref={bottomRef} />
      </div>

      <InputArea apiKey={apiKey} providerId={providerId} input={input} setInput={setInput} loading={loading} onSend={send} />
    </div>
    </ModelContext.Provider>
  );
}