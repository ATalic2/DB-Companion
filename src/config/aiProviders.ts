import { invoke } from "@tauri-apps/api/core";

export const AI_PROVIDERS = [
  { id: "gemini",    label: "Google Gemini",    available: true,  keyLabel: "Gemini API Key",    keyPlaceholder: "Paste your Gemini API key here", keyHint: "aistudio.google.com", keyHintUrl: "https://aistudio.google.com/apikey" },
  { id: "openai",    label: "OpenAI",           available: true,  keyLabel: "OpenAI API Key",    keyPlaceholder: "sk-...",                         keyHint: "platform.openai.com", keyHintUrl: "https://platform.openai.com/api-keys" },
  { id: "anthropic", label: "Anthropic Claude", available: false, keyLabel: "Anthropic API Key", keyPlaceholder: "sk-ant-...",                    keyHint: "console.anthropic.com", keyHintUrl: "https://console.anthropic.com/settings/keys" },
  { id: "ollama",    label: "Ollama (local)",   available: false, keyLabel: "Base URL",          keyPlaceholder: "http://localhost:11434",         keyHint: "ollama.com", keyHintUrl: "https://ollama.com" },
] as const;

export interface RegisteredAiProvider {
  id: string;
  display_name: string;
  models: string[];
  requires_key: boolean;
  requires_url: boolean;
}

let registeredProvidersRequest: Promise<RegisteredAiProvider[]> | null = null;

export function loadRegisteredAiProviders(): Promise<RegisteredAiProvider[]> {
  if (!registeredProvidersRequest) {
    registeredProvidersRequest = invoke<RegisteredAiProvider[]>("list_ai_providers")
      .catch(error => {
        registeredProvidersRequest = null;
        throw error;
      });
  }
  return registeredProvidersRequest;
}
