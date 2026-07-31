// src/hooks/useChangeAgent.ts
import { invoke } from "@tauri-apps/api/core";
import type { SchemaMetadata, TransactionResult } from "../types/changes";
import type { ConnectArgs } from "./useCredentialStore";

export interface ConnectResult {
  success: boolean;
  db_name: string;
  db_type: string;
  error:   string | null;
}

export function useChangeAgent() {
  const connectDb = (args: ConnectArgs) =>
    invoke<ConnectResult>("connect_db", { args });

  const disconnect = () => invoke<void>("disconnect");

  const getSchema = () => invoke<SchemaMetadata>("get_schema");

  const executeChanges = (commands: string[], dryRun: boolean) =>
    invoke<{ result: TransactionResult }>("execute_changes", {
      args: { commands, dry_run: dryRun },
    });

  return {
    connectDb,
    disconnect,
    getSchema,
    executeChanges,
  };
}
