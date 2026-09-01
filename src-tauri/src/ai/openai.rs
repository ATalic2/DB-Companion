//! OpenAI provider using the Responses API.

use super::{ChangePlan, ChatMessage, ChatRole, IAiProvider, ProviderCredentials, ProviderInfo};
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};

const DEFAULT_MODEL: &str = "gpt-5.4-mini";
const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

#[derive(Serialize)]
struct OpenAiInput {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    instructions: String,
    input: Vec<OpenAiInput>,
    text: Value,
    store: bool,
    stream: bool,
}

pub struct OpenAiProvider;

impl OpenAiProvider {
    fn client() -> Client {
        Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .expect("reqwest client")
    }

    fn model(credentials: &ProviderCredentials) -> String {
        credentials
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string())
    }

    fn api_key(credentials: &ProviderCredentials) -> Result<&str> {
        credentials
            .api_key
            .as_deref()
            .map(|key| key.as_str())
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| anyhow!("No API key provided for OpenAI"))
    }

    fn build_input(history: &[ChatMessage], message: &str) -> Vec<OpenAiInput> {
        let mut input = history
            .iter()
            .filter_map(|item| match item.role {
                ChatRole::User => Some(OpenAiInput {
                    role: "user",
                    content: item.content.clone(),
                }),
                ChatRole::Assistant => Some(OpenAiInput {
                    role: "assistant",
                    content: item.content.clone(),
                }),
                ChatRole::System => None,
            })
            .collect::<Vec<_>>();
        input.push(OpenAiInput {
            role: "user",
            content: message.to_string(),
        });
        input
    }

    fn structured_text(name: &str, schema: Value) -> Value {
        json!({
            "format": {
                "type": "json_schema",
                "name": name,
                "strict": true,
                "schema": schema
            }
        })
    }

    fn extract_text(response: &Value) -> Result<String> {
        if let Some(error) = response.get("error").filter(|value| !value.is_null()) {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown OpenAI API error");
            bail!("OpenAI API error: {message}");
        }

        response
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .find_map(|content| {
                (content.get("type").and_then(Value::as_str) == Some("output_text"))
                    .then(|| content.get("text").and_then(Value::as_str))
                    .flatten()
            })
            .map(str::to_string)
            .ok_or_else(|| anyhow!("OpenAI returned no text output"))
    }

    async fn send(&self, api_key: &str, body: &OpenAiRequest) -> Result<Value> {
        let response = Self::client()
            .post(RESPONSES_URL)
            .bearer_auth(api_key)
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&response_text)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or(response_text);
            bail!("OpenAI API error {status}: {detail}");
        }

        serde_json::from_str(&response_text)
            .map_err(|error| anyhow!("Failed to parse OpenAI response: {error}"))
    }

    fn chat_request(
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
        stream: bool,
    ) -> OpenAiRequest {
        OpenAiRequest {
            model: Self::model(credentials),
            instructions: system.to_string(),
            input: Self::build_input(history, message),
            text: Self::structured_text("db_companion_chat_response", chat_response_schema()),
            store: false,
            stream,
        }
    }
}

#[async_trait]
impl IAiProvider for OpenAiProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "openai".into(),
            display_name: "OpenAI".into(),
            models: vec![
                "gpt-5.4-mini".into(),
                "gpt-5.4".into(),
                "gpt-5.4-nano".into(),
            ],
            requires_key: true,
            requires_url: false,
        }
    }

    fn additional_system_instructions(&self) -> &'static str {
        r#"=== OPENAI-SPECIFIC INSTRUCTIONS ===
EXECUTABLE OUTPUT CLASSIFICATION — HARD RULE:
- If your response contains, proposes, writes, shows, generates, or demonstrates any runnable SQL, MongoDB command, or CLI command, you MUST use response_type "plan" and put the command in plan.steps.
- Treat requests to "give", "write", "show", "generate", "make", or provide an "example" or "test" query as intent to run that query. The user does not need to explicitly say "run" or ask for a clickable command.
- SELECT and other read-only queries are executable commands and MUST be plans with risk_score 1.
- Never put a runnable command only in an "answer" message. Use "answer" only when no executable command is included or proposed.
- When response_type is "answer", plan MUST be null. When response_type is "plan", plan MUST contain at least one executable step.
=== END OPENAI-SPECIFIC INSTRUCTIONS ==="#
    }

    async fn plan_changes(
        &self,
        credentials: &ProviderCredentials,
        intent: &str,
        schema_text: &str,
    ) -> Result<ChangePlan> {
        let prompt = format!(
            r#"Developer intent:
{intent}

Current database schema:
```
{schema_text}
```

Prefer non-destructive operations and never drop data without explicit developer instruction.
Use risk_score 1-3 for additive changes, 4-6 for renames/indexes, and 7-10 for destructive or type-changing operations.
Every step must be safe to run in sequence inside one transaction."#
        );
        let body = OpenAiRequest {
            model: Self::model(credentials),
            instructions: "You are a senior database engineer and schema change safety expert. Produce the requested database change plan.".into(),
            input: vec![OpenAiInput {
                role: "user",
                content: prompt,
            }],
            text: Self::structured_text("db_companion_change_plan", change_plan_schema()),
            store: false,
            stream: false,
        };
        let response = self.send(Self::api_key(credentials)?, &body).await?;
        let text = Self::extract_text(&response)?;
        serde_json::from_str(&text).map_err(|error| anyhow!("JSON parse error: {error}"))
    }

    async fn chat(
        &self,
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
    ) -> Result<String> {
        let body = Self::chat_request(credentials, history, system, message, false);
        let response = self.send(Self::api_key(credentials)?, &body).await?;
        Self::extract_text(&response)
    }

    async fn chat_stream(
        &self,
        credentials: &ProviderCredentials,
        history: &[ChatMessage],
        system: &str,
        message: &str,
        on_chunk: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<(String, String)> {
        let body = Self::chat_request(credentials, history, system, message, true);
        let request_json = serde_json::to_string_pretty(&body).unwrap_or_default();
        let mut response = Self::client()
            .post(RESPONSES_URL)
            .bearer_auth(Self::api_key(credentials)?)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/error/message")?
                        .as_str()
                        .map(str::to_string)
                })
                .unwrap_or(text);
            bail!("OpenAI API error {status}: {detail}");
        }

        let mut pending = String::new();
        let mut full = String::new();
        while let Some(chunk) = response.chunk().await? {
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = pending.find('\n') {
                let line = pending[..end].trim_end_matches('\r').to_string();
                pending.drain(..=end);
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    continue;
                }
                let event: Value = serde_json::from_str(data)
                    .map_err(|error| anyhow!("Failed to parse OpenAI stream event: {error}"))?;
                match event.get("type").and_then(Value::as_str) {
                    Some("response.output_text.delta") => {
                        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                            full.push_str(delta);
                            on_chunk(delta.to_string());
                        }
                    }
                    Some("error") => {
                        let message = event
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown streaming error");
                        bail!("OpenAI stream error: {message}");
                    }
                    Some("response.failed") => {
                        let message = event
                            .pointer("/response/error/message")
                            .and_then(Value::as_str)
                            .unwrap_or("Response generation failed");
                        bail!("OpenAI stream error: {message}");
                    }
                    _ => {}
                }
            }
        }

        if full.is_empty() {
            bail!("OpenAI returned no text output");
        }
        Ok((full, request_json))
    }
}

fn change_step_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "type": { "type": "string", "enum": ["SQL", "CLI"] },
            "command": { "type": ["string", "null"] },
            "description": { "type": "string" }
        },
        "required": ["type", "command", "description"]
    })
}

fn change_plan_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "summary": { "type": "string" },
            "risk_score": { "type": "integer", "minimum": 1, "maximum": 10 },
            "steps": { "type": "array", "items": change_step_schema() }
        },
        "required": ["summary", "risk_score", "steps"]
    })
}

fn chat_response_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "response_type": { "type": "string", "enum": ["answer", "plan"] },
            "message": { "type": "string" },
            "plan": {
                "anyOf": [change_plan_schema(), { "type": "null" }]
            }
        },
        "required": ["response_type", "message", "plan"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_output_text_without_assuming_first_output_item() {
        let response = json!({
            "output": [
                { "type": "reasoning" },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "{\"response_type\":\"answer\"}" }
                ]}
            ]
        });
        assert_eq!(
            OpenAiProvider::extract_text(&response).unwrap(),
            "{\"response_type\":\"answer\"}"
        );
    }

    #[test]
    fn system_history_is_not_duplicated_in_input() {
        let history = vec![
            ChatMessage {
                role: ChatRole::System,
                content: "old".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "hello".into(),
            },
        ];
        let input = OpenAiProvider::build_input(&history, "next");
        assert_eq!(input.len(), 2);
        assert_eq!(input[0].role, "user");
        assert_eq!(input[1].content, "next");
    }
}
