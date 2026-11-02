//! OpenAI-compatible chat completions client.
//!
//! Works with OpenAI, OpenRouter, Groq, Together, vLLM, LM Studio, Ollama's
//! `/v1` endpoint, and anything else that speaks `POST /chat/completions`.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct Client {
    url: String,
    api_key: Option<String>,
    model: String,
    timeout: Duration,
    extra_headers: Vec<(String, String)>,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<&'a ChatMessage>,
    temperature: f32,
    stream: bool,
}

/// One turn of a conversation. Owned so the interactive feedback loop can
/// keep the whole history between requests.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MsgOwned,
}

#[derive(Deserialize)]
struct MsgOwned {
    #[serde(default)]
    content: Option<String>,
}

impl Client {
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        model: &str,
        timeout_secs: u64,
        extra_headers: Vec<(String, String)>,
    ) -> Self {
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        };
        Self {
            url,
            api_key,
            model: model.to_string(),
            timeout: Duration::from_secs(timeout_secs.max(1)),
            extra_headers,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Send an existing conversation (system + user + assistant + user …).
    pub fn chat(&self, history: &[ChatMessage]) -> Result<String> {
        let body = ChatRequest {
            model: &self.model,
            messages: history.iter().collect(),
            temperature: 0.3,
            stream: false,
        };

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout(self.timeout)
            .build();

        let mut req = agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json");
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        for (k, v) in &self.extra_headers {
            req = req.set(k, v);
        }

        let resp = match req.send_json(json!(body)) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                bail!("API returned HTTP {}: {}", code, summarize_error(&text));
            }
            Err(ureq::Error::Transport(t)) => {
                bail!("request to {} failed: {}", self.url, t);
            }
        };

        let parsed: ChatResponse = resp.into_json().context(
            "API response was not valid JSON (is the base URL an OpenAI-compatible endpoint?)",
        )?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .context("API returned no choices")?;

        let content = content.trim().to_string();
        if content.is_empty() {
            bail!("API returned an empty message");
        }
        Ok(content)
    }
}

/// Pull the useful part out of an error body without echoing the whole thing.
fn summarize_error(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(m) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return m.to_string();
        }
        if let Some(m) = v.get("error").and_then(|e| e.as_str()) {
            return m.to_string();
        }
    }
    let t = body.trim();
    if t.len() > 300 {
        format!("{}…", &t[..300])
    } else if t.is_empty() {
        "(empty response body)".into()
    } else {
        t.to_string()
    }
}

/// Split a multi-alternative response into individual messages.
pub fn split_alternatives(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in text.split("\n---\n") {
        let trimmed = chunk.trim();
        if !trimmed.is_empty() && trimmed != "---" {
            out.push(trimmed.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_normalised() {
        let c = Client::new("https://x/v1/", None, "m", 1, vec![]);
        assert_eq!(c.url(), "https://x/v1/chat/completions");
        let c = Client::new("https://x/v1/chat/completions", None, "m", 1, vec![]);
        assert_eq!(c.url(), "https://x/v1/chat/completions");
    }

    #[test]
    fn alternatives_split_on_separator() {
        let got = split_alternatives("feat: a\n\n---\n\nfix: b\n");
        assert_eq!(got, vec!["feat: a", "fix: b"]);
    }

    #[test]
    fn single_message_is_untouched() {
        assert_eq!(split_alternatives("feat: a"), vec!["feat: a"]);
    }

    #[test]
    fn error_summary_prefers_json_message() {
        let body = r#"{"error":{"message":"bad key"}}"#;
        assert_eq!(summarize_error(body), "bad key");
    }
}
