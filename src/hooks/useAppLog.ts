// src/hooks/useAppLog.ts
// Global log store — module-level so it persists across renders
// without needing React context

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export type LogLevel = "info" | "warn" | "error" | "debug" | "request" | "response";

export interface LogEntry {
  id:        number;
  level:     LogLevel;
  message:   string;
  detail?:   string; // collapsible JSON body
  timestamp: string;
}

// Module-level state — shared across all hook instances
let entries:   LogEntry[]              = [];
let nextId     = 0;
let listeners: Array<() => void>       = [];

function notify() { listeners.forEach(fn => fn()); }

export function appendLog(level: LogLevel, message: string, detail?: string) {
  const entry: LogEntry = {
    id:   nextId++,
    level,
    message,
    detail,
    timestamp: new Date().toLocaleTimeString("en-GB", { hour12: false }),
  };
  entries = [...entries.slice(-99), entry]; // keep last 100
  notify();
}

export function clearLog() { entries = []; notify(); }

// Hook — subscribes a component to log updates
export function useAppLog() {
  const [log, setLog] = useState<LogEntry[]>(entries);

  useEffect(() => {
    const fn = () => setLog([...entries]);
    listeners.push(fn);

    // Listen for log events emitted from Rust backend
    const unlisten = listen<{ level: string; message: string; detail?: string }>(
      "app_log",
      event => {
        appendLog(
          event.payload.level as LogLevel,
          event.payload.message,
          event.payload.detail,
        );
      }
    );

    return () => {
      listeners = listeners.filter(l => l !== fn);
      unlisten.then(fn => fn());
    };
  }, []);

  return { log, appendLog, clearLog };
}
