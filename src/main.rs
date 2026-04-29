use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "llm", version, about = "Minimal OpenAI-compatible LLM CLI")]
struct Cli {
    /// Prompt. If stdin is piped too, stdin is treated as context and this is the instruction.
    prompt: Vec<String>,

    /// Model to use. Overrides config and LLM_MODEL.
    #[arg(short, long, env = "LLM_MODEL")]
    model: Option<String>,

    /// Base URL, e.g. https://api.openai.com/v1 or http://localhost:11434/v1.
    #[arg(long, env = "LLM_BASE_URL")]
    base_url: Option<String>,

    /// API key. Overrides config and LLM_API_KEY.
    #[arg(long, env = "LLM_API_KEY")]
    api_key: Option<String>,

    /// System prompt.
    #[arg(short, long)]
    system: Option<String>,

    /// Disable streaming; wait and print the complete response.
    #[arg(long)]
    no_stream: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Write ~/.config/llm/config.toml.
    Config {
        /// Base URL, e.g. https://api.openai.com/v1 or http://localhost:11434/v1.
        #[arg(long)]
        base_url: String,

        /// Default model.
        #[arg(long)]
        model: String,

        /// API key. For local OpenAI-compatible servers, use any non-empty value.
        #[arg(long)]
        api_key: Option<String>,
    },
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        return match command {
            Command::Config {
                base_url,
                model,
                api_key,
            } => write_config(Config {
                base_url: Some(base_url),
                model: Some(model),
                api_key,
            }),
        };
    }

    let config = read_config()?;
    let base_url = first(cli.base_url, config.base_url, "LLM_BASE_URL", None).ok_or_else(|| {
        anyhow!(
            "missing base URL; run `llm config --base-url URL --model MODEL` or set LLM_BASE_URL"
        )
    })?;
    let model = first(cli.model, config.model, "LLM_MODEL", None).ok_or_else(|| {
        anyhow!("missing model; run `llm config --base-url URL --model MODEL` or set LLM_MODEL")
    })?;
    let api_key = first(
        cli.api_key,
        config.api_key,
        "LLM_API_KEY",
        Some("EMPTY".to_string()),
    )
    .unwrap();

    let prompt_arg = cli.prompt.join(" ");
    let stdin = read_stdin_if_piped()?;
    let prompt = build_prompt(stdin, prompt_arg)?;

    let mut messages = Vec::new();
    if let Some(system) = cli.system {
        messages.push(Message {
            role: "system".to_string(),
            content: system,
        });
    }
    messages.push(Message {
        role: "user".to_string(),
        content: prompt,
    });

    let request = ChatRequest {
        model,
        messages,
        stream: !cli.no_stream,
    };

    if request.stream {
        stream_chat(&base_url, &api_key, &request).await
    } else {
        complete_chat(&base_url, &api_key, &request).await
    }
}

fn first(
    cli: Option<String>,
    config: Option<String>,
    env_key: &str,
    fallback: Option<String>,
) -> Option<String> {
    cli.or_else(|| env::var(env_key).ok())
        .or(config)
        .or(fallback)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn build_prompt(stdin: Option<String>, prompt_arg: String) -> Result<String> {
    let prompt_arg = prompt_arg.trim().to_string();
    match (stdin, prompt_arg.is_empty()) {
        (Some(context), false) => Ok(format!(
            "<context>\n{}\n</context>\n\nInstruction:\n{}",
            context.trim_end(),
            prompt_arg
        )),
        (Some(context), true) => Ok(context),
        (None, false) => Ok(prompt_arg),
        (None, true) => Err(anyhow!(
            "missing prompt; try `llm \"hello\"` or pipe text into `llm`"
        )),
    }
}

fn read_stdin_if_piped() -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read stdin")?;
    if input.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

async fn complete_chat(base_url: &str, api_key: &str, request: &ChatRequest) -> Result<()> {
    let response: ChatResponse = client(api_key)
        .post(chat_url(base_url))
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
        .unwrap_or("");
    println!("{text}");
    Ok(())
}

async fn stream_chat(base_url: &str, api_key: &str, request: &ChatRequest) -> Result<()> {
    let mut response = client(api_key)
        .post(chat_url(base_url))
        .json(request)
        .send()
        .await
        .context("request failed")?
        .error_for_status()
        .context("provider returned an error")?;

    let mut pending = String::new();
    while let Some(chunk) = response.chunk().await.context("failed to read stream")? {
        let text = String::from_utf8_lossy(&chunk);
        pending.push_str(&text);

        while let Some(pos) = pending.find('\n') {
            let line = pending[..pos].trim().to_string();
            pending = pending[pos + 1..].to_string();
            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                println!();
                return Ok(());
            }
            let parsed: StreamChunk = match serde_json::from_str(data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            for choice in parsed.choices {
                if let Some(content) = choice.delta.content {
                    print!("{content}");
                    io::stdout().flush().ok();
                }
            }
        }
    }
    println!();
    Ok(())
}

fn client(api_key: &str) -> reqwest::Client {
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

fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

fn read_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config: {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse config: {}", path.display()))
}

fn write_config(config: Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(&config).context("failed to serialize config")?;
    fs::write(&path, text)
        .with_context(|| format!("failed to write config: {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    if let Ok(dir) = env::var("LLM_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir).join("config.toml"));
        }
    }

    if let Ok(dir) = env::var("XDG_CONFIG_HOME") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir).join("llm").join("config.toml"));
        }
    }

    let home = env::var("HOME").context("HOME is not set; set LLM_CONFIG_DIR explicitly")?;
    let dir = PathBuf::from(home).join(".config");
    Ok(dir.join("llm").join("config.toml"))
}
