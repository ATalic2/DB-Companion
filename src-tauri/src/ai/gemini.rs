//! ai/gemini.rs — Gemini provider implementing IAiProvider

use super::{
    ChatMessage, ChatRole, IAiProvider, ChangePlan,
    ProviderCredentials, ProviderInfo,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const BASE_URL:      &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ── Internal Gemini API types ─────────────────────────────────────────────────

#[derive(Serialize)]
struct GeminiRequest {
    contents:           Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    generation_config:  GeminiGenerationConfig,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiContent {
    role:  String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    temperature:        f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens:  Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_schema:    Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: String,
}

// ── Provider ──────────────────────────────────────────────────────────────────

pub struct GeminiProvider;

impl GeminiProvider {
    fn client() -> Client {
        Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client")
    }

    fn model(creds: &ProviderCredentials) -> String {
        creds.model.clone().unwrap_or_else(|| DEFAULT_MODEL.to_string())
    }

    fn api_key(creds: &ProviderCredentials) -> Result<String> {
        creds.api_key
            .as_ref()
            .map(|k| k.as_str().to_string())
            .ok_or_else(|| anyhow::anyhow!("No API key provided for Gemini"))
    }

    fn extract_text(resp: GeminiResponse) -> String {
        resp.candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .unwrap_or_default()
    }

    fn build_contents(history: &[ChatMessage], message: &str) -> Vec<GeminiContent> {
        let mut contents: Vec<GeminiContent> = history.iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| GeminiContent {
                role:  if m.role == ChatRole::Assistant { "model".into() } else { "user".into() },
                parts: vec![GeminiPart { text: m.content.clone() }],
            })
            .collect();
        contents.push(GeminiContent {
            role:  "user".into(),
            parts: vec![GeminiPart { text: message.into() }],
        });
        contents
    }

    /// POST to Gemini and return (response, request_json, response_json).
    async fn post_logged(
        &self,
        endpoint: &str,
        api_key:  &str,
        body:     &GeminiRequest,
    ) -> Result<(GeminiResponse, String, String)> {
        let request_json  = serde_json::to_string_pretty(body).unwrap_or_default();
        let url           = format!("{}?key={}", endpoint, api_key);
        let resp          = Self::client().post(&url).json(body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text   = resp.text().await.unwrap_or_default();
            bail!("Gemini API error {status}: {text}");
        }

        let response_text = resp.text().await?;
        let parsed = serde_json::from_str::<GeminiResponse>(&response_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse Gemini response: {e}\nRaw: {response_text}"))?;
        Ok((parsed, request_json, response_text))
    }

    /// Chat with exact request/response bodies returned for logging.
    pub async fn chat_with_log(
        &self,
        credentials: &ProviderCredentials,
        history:     &[ChatMessage],
        system:      &str,
        message:     &str,
    ) -> Result<(String, String, String)> {
        let api_key = Self::api_key(credentials)?;
        let model   = Self::model(credentials);
        let url     = format!("{}/{}:generateContent", BASE_URL, model);

        let body = GeminiRequest {
            contents: Self::build_contents(history, message),
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart { text: system.to_string() }],
            }),
            generation_config: GeminiGenerationConfig {
                temperature:        0.4,
                max_output_tokens:  None,
                response_mime_type: Some("application/json".into()),
                response_schema:    None,
            },
        };

        let (resp, req_body, res_body) = self.post_logged(&url, &api_key, &body).await?;
        Ok((Self::extract_text(resp), req_body, res_body))
    }

}

// ── IAiProvider implementation ────────────────────────────────────────────────

#[async_trait]
impl IAiProvider for GeminiProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id:           "gemini".into(),
            display_name: "Google Gemini".into(),
            models: vec![
                "gemini-2.5-flash".into(),
                "gemini-2.5-pro".into(),
                "gemini-2.0-flash".into(),
            ],
            requires_key: true,
            requires_url: false,
        }
    }

    async fn plan_changes(
        &self,
        credentials: &ProviderCredentials,
        intent:      &str,
        schema_text: &str,
    ) -> Result<ChangePlan> {
        let api_key = Self::api_key(credentials)?;
        let model   = Self::model(credentials);
        let url     = format!("{}/{}:generateContent", BASE_URL, model);

        let prompt = format!(
            r#"You are a senior database engineer and schema change safety expert.
A developer has described a database change they want to perform.
Analyse the schema, plan the change, and respond ONLY with valid JSON.

## Developer intent
{intent}

## Current database schema
```
{schema_text}
```

## Rules
- Prefer non-destructive operations (ADD COLUMN over DROP+ADD).
- Use CONCURRENTLY for PostgreSQL index creation when safe.
- Never drop data without explicit developer instruction.
- risk_score: 1-3 = low (additive), 4-6 = medium (rename/index), 7-10 = high (DROP/ALTER TYPE).
- Each step must be safe to run in sequence inside a single transaction.

Respond with JSON matching the schema exactly. No markdown, no explanation outside the JSON."#
        );

        let body = GeminiRequest {
            contents: vec![GeminiContent {
                role:  "user".into(),
                parts: vec![GeminiPart { text: prompt }],
            }],
            system_instruction: None,
            generation_config: GeminiGenerationConfig {
                temperature:        0.2,
                max_output_tokens:  None,
                response_mime_type: Some("application/json".into()),
                response_schema:    Some(change_plan_schema()),
            },
        };

        let (resp, _, _) = self.post_logged(&url, &api_key, &body).await?;
        let raw = Self::extract_text(resp);
        let clean = raw.trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        Ok(serde_json::from_str(clean)
            .map_err(|e| anyhow::anyhow!("JSON parse error: {e}\nRaw: {clean}"))?)
    }

    async fn chat(
        &self,
        credentials: &ProviderCredentials,
        history:     &[ChatMessage],
        system:      &str,
        message:     &str,
    ) -> Result<String> {
        let (text, _, _) = self.chat_with_log(credentials, history, system, message).await?;
        Ok(text)
    }

    async fn chat_stream(
        &self,
        credentials: &ProviderCredentials,
        history:     &[ChatMessage],
        system:      &str,
        message:     &str,
        on_chunk:    Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<(String, String)> {
        let api_key = Self::api_key(credentials)?;
        let model   = Self::model(credentials);
        let url     = format!("{}/{}:streamGenerateContent?alt=sse&key={}", BASE_URL, model, api_key);

        let body = GeminiRequest {
            contents: Self::build_contents(history, message),
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart { text: system.to_string() }],
            }),
            generation_config: GeminiGenerationConfig {
                temperature:        0.4,
                max_output_tokens:  None,
                response_mime_type: Some("application/json".into()),
                response_schema:    None,
            },
        };

        let request_json = serde_json::to_string_pretty(&body).unwrap_or_default();
        let mut resp = Self::client().post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text   = resp.text().await.unwrap_or_default();
            bail!("Gemini API error {status}: {text}");
        }

        let mut full = String::new();
        while let Some(chunk) = resp.chunk().await? {
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                let line = line.trim();
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str == "[DONE]" { break; }
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(t) = val
                            .pointer("/candidates/0/content/parts/0/text")
                            .and_then(|v| v.as_str())
                        {
                            full.push_str(t);
                            on_chunk(t.to_string());
                        }
                    }
                }
            }
        }

        Ok((full, request_json))
    }
}

// ── JSON schemas ──────────────────────────────────────────────────────────────

fn chat_response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "response_type": { "type": "string" },
            "message":       { "type": "string" },
            "plan": {
                "type": "object",
                "nullable": true,
                "properties": {
                    "summary":    { "type": "string" },
                    "risk_score": { "type": "integer" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type":        { "type": "string" },
                                "command":     { "type": "string" },
                                "description": { "type": "string" }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn change_plan_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary":    { "type": "string" },
            "risk_score": { "type": "integer" },
            "steps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "type":        { "type": "string" },
                        "command":     { "type": "string" },
                        "description": { "type": "string" }
                    }
                }
            }
        }
    })
}

// Suppress unused warning — chat_response_schema is available for future use
#[allow(dead_code)]
fn _use_chat_schema() -> serde_json::Value { chat_response_schema() }
