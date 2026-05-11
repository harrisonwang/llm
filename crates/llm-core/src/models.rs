use crate::usage::Usage;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ModelsResponse {
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
pub struct ModelInfo {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Choice {
    pub message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResponseMessage {
    pub content: Option<String>,
}

pub fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

pub fn models_url(base_url: &str) -> String {
    format!("{}/models", base_url.trim_end_matches('/'))
}

pub fn write_models<W: std::io::Write>(models: &[ModelInfo], out: &mut W) -> anyhow::Result<()> {
    use anyhow::Context;

    for model in models {
        writeln!(out, "{}", model.id).context("failed to write model list")?;
    }
    Ok(())
}
