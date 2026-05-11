use crate::chat::{ChatCompletion, ChatRequest};
use crate::models::{ChatResponse, ModelInfo, ModelsResponse, chat_url, models_url};
use crate::render::{StreamOutput, write_markdown_output};
use crate::usage::{ModelPricing, Usage, build_usage_summary, write_usage_summary};
use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::io::Write;

pub struct LlmClient {
    base_url: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            base_url: base_url.into(),
            http: client(&api_key.into()),
        })
    }

    pub async fn complete_chat(&self, request: &ChatRequest) -> Result<ChatCompletion> {
        let response: ChatResponse = self
            .http
            .post(chat_url(&self.base_url))
            .json(request)
            .send()
            .await
            .context("request failed")?
            .error_for_status()
            .context("provider returned an error")?
            .json()
            .await
            .context("failed to parse response JSON")?;

        let text = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .unwrap_or("")
            .to_string();
        Ok(ChatCompletion {
            text,
            usage: response.usage,
        })
    }

    pub async fn stream_chat<W: Write>(
        &self,
        request: &ChatRequest,
        out: &mut W,
        output: &mut StreamOutput,
    ) -> Result<Option<Usage>> {
        let mut response = self
            .http
            .post(chat_url(&self.base_url))
            .json(request)
            .send()
            .await
            .context("request failed")?
            .error_for_status()
            .context("provider returned an error")?;

        let mut pending = Vec::new();
        let mut usage = None;
        while let Some(chunk) = response.chunk().await.context("failed to read stream")? {
            if write_stream_bytes(&mut pending, &chunk, out, output, &mut usage)? {
                output.finish(out)?;
                return Ok(usage);
            }
        }
        finish_stream(&mut pending, out, output, &mut usage)?;
        Ok(usage)
    }

    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        let response: ModelsResponse = self
            .http
            .get(models_url(&self.base_url))
            .send()
            .await
            .context("request failed")?
            .error_for_status()
            .context("provider returned an error")?
            .json()
            .await
            .context("failed to parse models JSON")?;

        Ok(response.data)
    }
}

pub async fn complete_chat_with_output(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    render: bool,
    pricing: Option<ModelPricing>,
) -> Result<()> {
    let response = LlmClient::new(base_url, api_key)?
        .complete_chat(request)
        .await?;
    let mut stdout = std::io::stdout();
    write_markdown_output(&response.text, render, &mut stdout)?;
    if let Some(usage) = response.usage {
        let mut stderr = std::io::stderr();
        write_usage_summary(&build_usage_summary(usage, pricing), &mut stderr)?;
    }
    Ok(())
}

pub async fn stream_chat_with_output(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    render: bool,
    pricing: Option<ModelPricing>,
) -> Result<()> {
    let mut stdout = std::io::stdout();
    let mut output = StreamOutput::new(render);
    let usage = LlmClient::new(base_url, api_key)?
        .stream_chat(request, &mut stdout, &mut output)
        .await?;
    if let Some(usage) = usage {
        let mut stderr = std::io::stderr();
        write_usage_summary(&build_usage_summary(usage, pricing), &mut stderr)?;
    }
    Ok(())
}

pub fn client(api_key: &str) -> reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if !api_key.is_empty() {
        let value = format!("Bearer {api_key}");
        if let Ok(header) = HeaderValue::from_str(&value) {
            headers.insert(AUTHORIZATION, header);
        }
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("client")
}

pub fn write_stream_bytes<W: Write>(
    pending: &mut Vec<u8>,
    bytes: &[u8],
    out: &mut W,
    output: &mut StreamOutput,
    usage: &mut Option<Usage>,
) -> Result<bool> {
    pending.extend_from_slice(bytes);

    while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
        let mut line = pending.drain(..=pos).collect::<Vec<_>>();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if write_stream_line(&line, out, output, usage)? {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn finish_stream<W: Write>(
    pending: &mut Vec<u8>,
    out: &mut W,
    output: &mut StreamOutput,
    usage: &mut Option<Usage>,
) -> Result<()> {
    if !pending.is_empty() && write_stream_line(pending, out, output, usage)? {
        pending.clear();
        return output.finish(out);
    }
    pending.clear();
    output.finish(out)
}

fn write_stream_line<W: Write>(
    line: &[u8],
    out: &mut W,
    output: &mut StreamOutput,
    usage: &mut Option<Usage>,
) -> Result<bool> {
    match parse_stream_event(line) {
        StreamEvent::Chunk {
            text,
            usage: chunk_usage,
        } => {
            if let Some(chunk_usage) = chunk_usage {
                *usage = Some(chunk_usage);
            }
            if !text.is_empty() {
                output.write_text(&text, out)?;
            }
            Ok(false)
        }
        StreamEvent::Done => Ok(true),
        StreamEvent::Ignore => Ok(false),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StreamEvent {
    Chunk { text: String, usage: Option<Usage> },
    Done,
    Ignore,
}

fn parse_stream_event(line: &[u8]) -> StreamEvent {
    let line = trim_ascii_whitespace(line);
    if line.is_empty() || line.starts_with(b":") || !line.starts_with(b"data:") {
        return StreamEvent::Ignore;
    }

    let data = trim_ascii_whitespace(&line[b"data:".len()..]);
    if data == b"[DONE]" {
        return StreamEvent::Done;
    }

    let parsed: StreamChunk = match serde_json::from_slice(data) {
        Ok(value) => value,
        Err(_) => return StreamEvent::Ignore,
    };

    let mut text = String::new();
    for choice in parsed.choices {
        if let Some(content) = choice.delta.content {
            text.push_str(&content);
        }
    }

    if text.is_empty() && parsed.usage.is_none() {
        StreamEvent::Ignore
    } else {
        StreamEvent::Chunk {
            text,
            usage: parsed.usage,
        }
    }
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|pos| pos + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}
