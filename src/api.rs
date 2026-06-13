use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest client")
}

fn models_url(base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{}/models", b)
    } else {
        format!("{}/v1/models", b)
    }
}

fn chat_url(base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{}/chat/completions", b)
    } else {
        format!("{}/v1/chat/completions", b)
    }
}

pub async fn test_connection(base_url: &str, api_key: &str) -> Result<String> {
    let url = models_url(base_url);
    let resp = client()
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .context("send request")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {} — {}", status, body.chars().take(200).collect::<String>()));
    }
    Ok(format!("OK · {} {}", status.as_u16(), url))
}

pub async fn list_models(base_url: &str, api_key: &str) -> Result<Vec<Model>> {
    let url = models_url(base_url);
    let resp = client()
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .context("send request")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {} — {}", status, body.chars().take(200).collect::<String>()));
    }
    let parsed: ModelsResponse = resp.json().await.context("parse models response")?;
    Ok(parsed.data)
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

/// Stream chat completion tokens. Returns a tokio mpsc receiver of deltas
/// plus a "done" sentinel. On error the receiver gets a single Err string.
pub async fn stream_chat(
    base_url: String,
    api_key: String,
    model: String,
    history: Vec<(String, String)>,
) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
    let url = chat_url(&base_url);
    let messages: Vec<ChatMessage> = history
        .into_iter()
        .map(|(role, content)| ChatMessage { role, content })
        .collect();

    let resp = client()
        .post(&url)
        .bearer_auth(&api_key)
        .json(&ChatRequest {
            model,
            messages,
            stream: true,
        })
        .send()
        .await
        .context("send chat request")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {} — {}", status, body.chars().take(200).collect::<String>()));
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx
                        .send(StreamEvent::Error(format!("stream error: {}", e)))
                        .await;
                    return;
                }
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(idx) = buf.find("\n\n") {
                let event = buf[..idx].to_string();
                buf = buf[idx + 2..].to_string();
                for line in event.lines() {
                    if let Some(rest) = line.strip_prefix("data:") {
                        let payload = rest.trim();
                        if payload == "[DONE]" {
                            let _ = tx.send(StreamEvent::Done).await;
                            return;
                        }
                        if payload.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<StreamChunk>(payload) {
                            Ok(c) => {
                                for choice in c.choices {
                                    if let Some(content) = choice.delta.content {
                                        if !content.is_empty() {
                                            let _ = tx.send(StreamEvent::Delta(content)).await;
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                // skip non-JSON keepalives
                            }
                        }
                    }
                }
            }
        }
        let _ = tx.send(StreamEvent::Done).await;
    });

    Ok(rx)
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    Done,
    Error(String),
}
