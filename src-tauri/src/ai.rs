//! ai.rs — AI provider adapter pattern
//!
//! Adding a new provider = new file + impl IAiProvider. Nothing else changes.

pub mod gemini;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

// ── Shared types ──────────────────────────────────────────────────────────────

/// A single message in a multi-turn chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role:    ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

/// A single step in a change plan — provider-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeStep {
    #[serde(rename = "type")]
    pub step_type:   String, // "SQL" | "CLI"
    pub command:     Option<String>, // AI occasionally returns null for commentary steps
    pub description: String,
}

/// The canonical change plan all providers must return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePlan {
    pub summary:    String,
    pub risk_score: u8, // 1–10
    pub steps:      Vec<ChangeStep>,
}

/// Provider identity — serialized to frontend for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id:          String, // "gemini", "openai", "anthropic", "ollama"
    pub display_name: String,
    pub models:      Vec<String>,
    pub requires_key: bool,
    pub requires_url: bool, // for Ollama-style local providers
}

/// Credentials passed to a provider — generic enough for all providers.
/// Only the fields relevant to the provider will be populated.
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub api_key:  Option<Zeroizing<String>>,
    pub base_url: Option<String>, // for Ollama or custom OpenAI-compat endpoints
    pub model:    Option<String>, // override default model
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Every AI provider adapter must implement this trait.
///
/// Implementing a new provider:
///   1. Create `src/ai/{provider}.rs`
///   2. Define a struct and `impl IAiProvider for MyProvider`
///   3. Register it in `commands/changes.rs` dispatch match
///   4. Add the provider ID to the frontend `AI_PROVIDERS` list
#[async_trait]
pub trait IAiProvider: Send + Sync {
    /// Static metadata about this provider.
    fn info(&self) -> ProviderInfo;

    /// Generate a structured change plan from a natural-language intent
    /// and a schema description string.
    async fn plan_changes(
        &self,
        credentials: &ProviderCredentials,
        intent:      &str,
        schema_text: &str,
    ) -> Result<ChangePlan>;

    /// Send a conversational message and return the assistant's reply.
    /// `history` contains prior turns; `schema_text` gives the live schema
    /// as context so the model can reference table/column names.
    /// Send a chat message and return the raw JSON string response.
    /// The caller (changes.rs) builds the system prompt and passes it in.
    async fn chat(
        &self,
        credentials: &ProviderCredentials,
        history:     &[ChatMessage],
        system:      &str,
        message:     &str,
    ) -> Result<String>;

    /// Streaming variant — emits text chunks via `on_chunk` as they arrive,
    /// then returns (full_text, request_json) for logging.
    ///
    /// Default implementation calls `chat()` and emits the full response as
    /// a single chunk, so providers that don't support streaming work
    /// automatically without any extra code.
    async fn chat_stream(
        &self,
        credentials: &ProviderCredentials,
        history:     &[ChatMessage],
        system:      &str,
        message:     &str,
        on_chunk:    Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<(String, String)> {
        let text = self.chat(credentials, history, system, message).await?;
        on_chunk(text.clone());
        Ok((text, String::new()))
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Return the list of all registered providers for the frontend to display.
/// Add new providers here as they are implemented.
pub fn registered_providers() -> Vec<ProviderInfo> {
    vec![
        gemini::GeminiProvider.info(),
        // openai::OpenAiProvider.info(),      ← uncomment when implemented
        // anthropic::AnthropicProvider.info(), ← uncomment when implemented
        // ollama::OllamaProvider.info(),        ← uncomment when implemented
    ]
}

/// Instantiate the correct provider by ID.
/// Returns None if the provider ID is not recognised.
pub fn get_provider(provider_id: &str) -> Option<Box<dyn IAiProvider>> {
    match provider_id {
        "gemini"    => Some(Box::new(gemini::GeminiProvider)),
        // "openai"    => Some(Box::new(openai::OpenAiProvider)),
        // "anthropic" => Some(Box::new(anthropic::AnthropicProvider)),
        // "ollama"    => Some(Box::new(ollama::OllamaProvider)),
        _           => None,
    }
}
