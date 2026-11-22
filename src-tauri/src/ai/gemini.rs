//! ai/gemini.rs — Gemini provider implementing IAiProvider

use super::{ChangePlan, ChatMessage, ChatRole, IAiProvider, ProviderCredentials, ProviderInfo};
use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_MODEL: &str = "gemini-3.5-flash-lite";
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ── Internal Gemini API types ─────────────────────────────────────────────────

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    generation_config: GeminiGenerationConfig,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    thinking_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_schema: Option<serde_json::Value>,
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

#[derive(Default)]
struct GeminiSseDecoder {
    pending: Vec<u8>,
}

impl GeminiSseDecoder {
    fn push(&mut self, bytes: &[u8], finish: bool) -> Result<Vec<String>> {
        self.pending.extend_from_slice(bytes);
        let mut text_parts = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            Self::read_line(&line, &mut text_parts)?;
        }
        if finish && !self.pending.is_empty() {
            Self::read_line(&std::mem::take(&mut self.pending), &mut text_parts)?;
        }
        Ok(text_parts)
    }

    fn read_line(line: &[u8], text_parts: &mut Vec<String>) -> Result<()> {
        let line = std::str::from_utf8(line)
            .map_err(|error| anyhow::anyhow!("Invalid Gemini stream text: {error}"))?
            .trim_end_matches(['\r', '\n']);
        let Some(data) = line.strip_prefix("data:") else { return Ok(()); };
        let data = data.trim_start();
        if data == "[DONE]" { return Ok(()); }
        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|error| anyhow::anyhow!("Invalid Gemini stream event: {error}"))?;
        if let Some(error) = value.pointer("/error/message").and_then(|v| v.as_str()) {
            bail!("Gemini stream error: {error}");
        }
        if let Some(parts) = value.pointer("/candidates/0/content/parts").and_then(|v| v.as_array()) {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
        }
        Ok(())
    }
}

impl GeminiProvider {
    fn thinking_config(credentials: &ProviderCredentials) -> Option<serde_json::Value> {
        let model = Self::model(credentials);
        let allowed: &[&str] = match model.as_str() {
            "gemini-3.7-flash" | "gemini-3.8-flash" => &["low", "medium", "high"],
            "gemini-3.6-flash" | "gemini-3.5-flash" | "gemini-3.5-flash-lite" => &["minimal", "low", "medium", "high"],
            _ => &[],
        };
        credentials.reasoning_effort.as_deref()
            .filter(|level| allowed.contains(level))
            .map(|level| serde_json::json!({"thinkingLevel": level}))
    }

    fn temperature(model: &str, value: f32) -> Option<f32> {
        // Gemini 3.8 Flash requires the default sampling configuration.
        (model != "gemini-3.8-flash").then_some(value)
    }

    fn client() -> Client {
        Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client")
    }

    fn model(creds: &ProviderCredentials) -> String {
        creds
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string())
    }

    fn api_key(creds: &ProviderCredentials) -> Result<String> {
        creds
            .api_key
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
        let mut contents: Vec<GeminiContent> = history
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| GeminiContent {
                role: if m.role == ChatRole::Assistant {
                    "model".into()
                } else {
                    "user".into()
                },
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();
        contents.push(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart {
                text: message.into(),
            }],
        });
        contents
    }

    /// POST to Gemini and return (response, request_json, response_json).
    async fn post_logged(
        &self,
        endpoint: &str,
        api_key: &str,
        body: &GeminiRequest,
    ) -> Result<(GeminiResponse, String, String)> {
        let request_json = serde_json::to_string_pretty(body).unwrap_or_default();
        let url = format!("{}?key={}", endpoint, api_key);
        let resp = Self::client().post(&url).json(body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Gemini API error {status}: {text}");
        }

        let response_text = resp.text().await?;
        let parsed = serde_json::from_str::<GeminiResponse>(&response_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse Gemini response: {e}"))?;
        Ok((parsed, request_json, response_text))
    }

    /// Chat with exact request/response bodies returned for logging.
    pub async fn chat_with_log(
        &self,
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
    ) -> Result<(String, String, String)> {
        let api_key = Self::api_key(credentials)?;
        let model = Self::model(credentials);
        let url = format!("{}/{}:generateContent", BASE_URL, model);

        let body = GeminiRequest {
            contents: Self::build_contents(history, message),
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: system.to_string(),
                }],
            }),
            generation_config: GeminiGenerationConfig {
                thinking_config: Self::thinking_config(credentials),
                temperature: Self::temperature(&model, 0.4),
                max_output_tokens: None,
                response_mime_type: Some("application/json".into()),
                response_schema: None,
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
            id: "gemini".into(),
            display_name: "Google Gemini".into(),
            models: vec![
                "gemini-3.5-flash-lite".into(),
                "gemini-3.7-flash".into(),
                "gemini-3.8-flash".into(),
                "gemini-3.6-flash".into(),
                "gemini-3.5-flash".into(),
            ],
            requires_key: true,
            requires_url: false,
        }
    }

    fn additional_system_instructions(&self) -> &'static str {
        r#"=== GEMINI-SPECIFIC INSTRUCTIONS ===
Follow the response mode contract in the shared instructions exactly. For a plan, keep every runnable command in plan.steps and keep the accompanying message concise.
=== END GEMINI-SPECIFIC INSTRUCTIONS ==="#
    }

    async fn plan_changes(
        &self,
        credentials: &ProviderCredentials,
        intent: &str,
        schema_text: &str,
    ) -> Result<ChangePlan> {
        let api_key = Self::api_key(credentials)?;
        let model = Self::model(credentials);
        let url = format!("{}/{}:generateContent", BASE_URL, model);

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
- Never drop data without explicit developer instruction.
- risk_score: 1-3 = low (additive), 4-6 = medium (rename/index), 7-10 = high (DROP/ALTER TYPE).
- Each step must be safe to run in sequence inside a single transaction.

Respond with JSON matching the schema exactly. No markdown, no explanation outside the JSON."#
        );

        let body = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart { text: prompt }],
            }],
            system_instruction: None,
            generation_config: GeminiGenerationConfig {
                thinking_config: Self::thinking_config(credentials),
                temperature: Self::temperature(&model, 0.2),
                max_output_tokens: None,
                response_mime_type: Some("application/json".into()),
                response_schema: Some(change_plan_schema()),
            },
        };

        let (resp, _, _) = self.post_logged(&url, &api_key, &body).await?;
        let raw = Self::extract_text(resp);
        let clean = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        Ok(serde_json::from_str(clean).map_err(|e| anyhow::anyhow!("JSON parse error: {e}"))?)
    }

    async fn chat(
        &self,
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
    ) -> Result<String> {
        let (text, _, _) = self
            .chat_with_log(credentials, history, system, message)
            .await?;
        Ok(text)
    }

    async fn chat_stream(
        &self,
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
        on_chunk: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<(String, String)> {
        let api_key = Self::api_key(credentials)?;
        let model = Self::model(credentials);
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse&key={}",
            BASE_URL, model, api_key
        );

        let body = GeminiRequest {
            contents: Self::build_contents(history, message),
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: system.to_string(),
                }],
            }),
            generation_config: GeminiGenerationConfig {
                thinking_config: Self::thinking_config(credentials),
                temperature: Self::temperature(&model, 0.4),
                max_output_tokens: None,
                response_mime_type: Some("application/json".into()),
                response_schema: None,
            },
        };

        let request_json = serde_json::to_string_pretty(&body).unwrap_or_default();
        let mut resp = Self::client()
            .post(&url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Gemini API error {status}: {text}");
        }

        let mut full = String::new();
        let mut decoder = GeminiSseDecoder::default();
        while let Some(chunk) = resp.chunk().await? {
            for text in decoder.push(&chunk, false)? {
                full.push_str(&text);
                on_chunk(text);
            }
        }
        for text in decoder.push(&[], true)? {
            full.push_str(&text);
            on_chunk(text);
        }

        if full.is_empty() { bail!("Gemini returned no text output"); }

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
fn _use_chat_schema() -> serde_json::Value {
    chat_response_schema()
}

#[cfg(test)]
mod stream_tests {
    use super::{GeminiGenerationConfig, GeminiProvider, GeminiSseDecoder};
    use crate::ai::ProviderCredentials;

    #[test]
    fn thinking_level_is_model_specific_and_optional() {
        let mut credentials = ProviderCredentials {
            api_key: None, base_url: None, model: Some("gemini-3.8-flash".into()),
            reasoning_effort: None,
        };
        assert!(GeminiProvider::thinking_config(&credentials).is_none());
        credentials.reasoning_effort = Some("minimal".into());
        assert!(GeminiProvider::thinking_config(&credentials).is_none());
        credentials.reasoning_effort = Some("low".into());
        assert_eq!(GeminiProvider::thinking_config(&credentials), Some(serde_json::json!({"thinkingLevel": "low"})));
    }

    #[test]
    fn gemini_38_omits_unsupported_temperature() {
        let config = GeminiGenerationConfig {
            thinking_config: None,
            temperature: GeminiProvider::temperature("gemini-3.8-flash", 0.4),
            max_output_tokens: None,
            response_mime_type: None,
            response_schema: None,
        };
        assert!(serde_json::to_value(config).unwrap().get("temperature").is_none());
        assert_eq!(GeminiProvider::temperature("gemini-3.7-flash", 0.4), Some(0.4));
    }

    #[test]
    fn decodes_json_and_utf8_split_across_network_chunks() {
        let event = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"café\"}]}}]}\n\n";
        let bytes = event.as_bytes();
        let split = bytes.iter().position(|byte| *byte == 0xc3).unwrap() + 1;
        let mut decoder = GeminiSseDecoder::default();
        assert!(decoder.push(&bytes[..split], false).unwrap().is_empty());
        assert_eq!(decoder.push(&bytes[split..], false).unwrap(), vec!["café"]);
        assert!(decoder.push(b"data: [DONE]\n", true).unwrap().is_empty());
    }

    #[test]
    fn reports_malformed_events_instead_of_dropping_them() {
        let mut decoder = GeminiSseDecoder::default();
        assert!(decoder.push(b"data: {broken}\n", false).is_err());
    }
}
