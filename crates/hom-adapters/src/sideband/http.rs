//! HTTP sideband channel — used by OpenCode's REST API.
//!
//! OpenCode exposes a local HTTP server (default localhost:4096) with these endpoints:
//!   - GET  /global/health             — health check
//!   - POST /session/:id/prompt_async  — send a prompt and get an async task ID back
//!   - GET  /global/event              — SSE stream for events
//!
//! The `prompt_async` endpoint expects a JSON body with a `parts` array:
//! ```json
//! { "parts": [{ "type": "text", "text": "..." }] }
//! ```
//!
//! See: <https://github.com/sst/opencode>

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::debug;

use hom_core::{HarnessEvent, HomError, HomResult, SidebandChannel};

pub struct HttpSideband {
    base_url: String,
    client: Client,
    session_id: Option<String>,
}

impl HttpSideband {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            session_id: None,
        }
    }

    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

#[async_trait]
impl SidebandChannel for HttpSideband {
    async fn send_prompt(&self, prompt: &str) -> HomResult<String> {
        // A session_id is required to target a specific OpenCode session.
        // If none is set, fall back to "default" — the caller should prefer
        // with_session() to target the correct session.
        let sid = self.session_id.as_deref().unwrap_or("default");

        let url = format!("{}/session/{}/prompt_async", self.base_url, sid);

        // OpenCode expects `parts` array matching its message format
        let body = json!({
            "parts": [{ "type": "text", "text": prompt }]
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| HomError::AdapterError(format!("HTTP send_prompt failed: {e}")))?;

        let status = resp.status();
        let resp_body = resp
            .text()
            .await
            .map_err(|e| HomError::AdapterError(format!("read response body: {e}")))?;

        if !status.is_success() {
            return Err(HomError::AdapterError(format!(
                "HTTP {status}: {resp_body}"
            )));
        }

        debug!(url, "prompt sent via HTTP sideband");
        Ok(resp_body)
    }

    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>> {
        let url = format!("{}/global/event", self.base_url);

        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!(url, error = %e, "SSE poll failed");
                return Ok(Vec::new());
            }
        };

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let body = resp.text().await.unwrap_or_default();
        let mut events = Vec::new();

        // Parse SSE format: lines starting with "data: " contain JSON
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ")
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(data)
                && let Some(event_type) = value.get("type").and_then(|t| t.as_str())
            {
                match event_type {
                    "token_usage" => {
                        let input = value.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
                        let output = value.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
                        events.push(HarnessEvent::TokenUsage { input, output });
                    }
                    "task_completed" => {
                        let summary = value
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        events.push(HarnessEvent::TaskCompleted { summary });
                    }
                    "error" => {
                        let message = value
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        events.push(HarnessEvent::Error { message });
                    }
                    _ => {
                        debug!(event_type, "unknown SSE event type");
                    }
                }
            }
        }

        Ok(events)
    }

    async fn health_check(&self) -> HomResult<bool> {
        let url = format!("{}/global/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
