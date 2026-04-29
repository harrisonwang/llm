use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

const HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
用法 (Usage):
  {usage}

命令 (Commands):
{subcommands}

参数 (Arguments):
{positionals}

选项 (Options):
{options}

示例 (Examples):
  llm \"Explain TCP three-way handshake\"
  llm -m gpt-4.1-mini \"Summarize this\"
  cat report.md | llm \"Summarize risks and action items\"{after-help}";

const COMMAND_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
用法 (Usage):
  {usage}

选项 (Options):
{options}

示例 (Examples):
  llm config --base-url https://api.openai.com/v1 --model gpt-4.1-mini
  llm config --base-url http://localhost:11434/v1 --model llama3.2 --api-key local{after-help}";

#[derive(Parser, Debug)]
#[command(
    name = "llm",
    version,
    about = "极简 LLM CLI",
    help_template = HELP_TEMPLATE,
    override_usage = "llm [OPTIONS] [prompt]... [COMMAND]",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// 用户 prompt；如果 stdin 有输入，则 stdin 作为上下文，prompt 作为指令。
    #[arg(value_name = "prompt")]
    prompt: Vec<String>,

    /// 使用的 model；覆盖配置文件和 env: LLM_MODEL。
    #[arg(short, long, env = "LLM_MODEL", hide_env = true, value_name = "model")]
    model: Option<String>,

    /// provider API base URL；env: LLM_BASE_URL。
    #[arg(long, env = "LLM_BASE_URL", hide_env = true, value_name = "base-url")]
    base_url: Option<String>,

    /// API key；覆盖配置文件和 env: LLM_API_KEY。
    #[arg(long, env = "LLM_API_KEY", hide_env = true, value_name = "api-key")]
    api_key: Option<String>,

    /// system prompt。
    #[arg(short, long, value_name = "system-prompt")]
    system: Option<String>,

    /// streaming 输出；默认开启。
    #[arg(long, conflicts_with = "no_stream")]
    stream: bool,

    /// 关闭 streaming，等待完整响应后输出。
    #[arg(long)]
    no_stream: bool,

    /// 显示帮助。
    #[arg(short = 'h', long = "help", action = ArgAction::Help, global = true)]
    help: Option<bool>,

    /// 显示版本。
    #[arg(short = 'V', long = "version", action = ArgAction::Version)]
    version: Option<bool>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 写入配置文件。
    #[command(
        help_template = COMMAND_HELP_TEMPLATE,
        override_usage = "llm config --base-url <base-url> --model <model> [OPTIONS]"
    )]
    Config {
        /// provider API base URL。
        #[arg(long, value_name = "base-url")]
        base_url: String,

        /// 默认 model。
        #[arg(long, value_name = "model")]
        model: String,

        /// API key；本地 LLM server 可使用任意非空值。
        #[arg(long, value_name = "api-key")]
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

    let stream = cli.stream || !cli.no_stream;
    let request = ChatRequest {
        model,
        messages,
        stream,
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

    let mut stdout = io::stdout();
    let mut pending = Vec::new();
    while let Some(chunk) = response.chunk().await.context("failed to read stream")? {
        if write_stream_bytes(&mut pending, &chunk, &mut stdout)? {
            return Ok(());
        }
    }
    finish_stream(&mut pending, &mut stdout)?;
    Ok(())
}

fn write_stream_bytes<W: Write>(pending: &mut Vec<u8>, bytes: &[u8], out: &mut W) -> Result<bool> {
    pending.extend_from_slice(bytes);

    while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
        let mut line = pending.drain(..=pos).collect::<Vec<_>>();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if write_stream_line(&line, out)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn finish_stream<W: Write>(pending: &mut Vec<u8>, out: &mut W) -> Result<()> {
    if !pending.is_empty() && write_stream_line(pending, out)? {
        pending.clear();
        return Ok(());
    }
    pending.clear();
    writeln!(out).context("failed to write final newline")?;
    Ok(())
}

fn write_stream_line<W: Write>(line: &[u8], out: &mut W) -> Result<bool> {
    match parse_stream_event(line) {
        StreamEvent::Text(text) => {
            out.write_all(text.as_bytes())
                .context("failed to write streamed output")?;
            out.flush().context("failed to flush streamed output")?;
            Ok(false)
        }
        StreamEvent::Done => {
            writeln!(out).context("failed to write final newline")?;
            Ok(true)
        }
        StreamEvent::Ignore => Ok(false),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StreamEvent {
    Text(String),
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

    if text.is_empty() {
        StreamEvent::Ignore
    } else {
        StreamEvent::Text(text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_wraps_gist_context_with_instruction() {
        let prompt = build_prompt(
            Some("# Report\n\nA finding from gist.\n".to_string()),
            "总结风险和行动项".to_string(),
        )
        .unwrap();

        assert_eq!(
            prompt,
            "<context>\n# Report\n\nA finding from gist.\n</context>\n\nInstruction:\n总结风险和行动项"
        );
    }

    #[test]
    fn explicit_stream_flag_conflicts_with_no_stream() {
        assert!(Cli::try_parse_from(["llm", "--stream", "--no-stream", "hello"]).is_err());
    }

    #[test]
    fn prompt_without_flags_still_parses() {
        let cli = Cli::try_parse_from(["llm", "hi"]).unwrap();

        assert_eq!(cli.prompt, ["hi"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn top_level_help_is_chinese() {
        let err = Cli::try_parse_from(["llm", "-h"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = err.to_string();
        assert!(help.contains("极简 LLM CLI"));
        assert!(help.contains("用法 (Usage):"));
        assert!(help.contains("llm [OPTIONS] [prompt]... [COMMAND]"));
        assert!(help.contains("命令 (Commands):"));
        assert!(help.contains("参数 (Arguments):"));
        assert!(help.contains("选项 (Options):"));
        assert!(help.contains("--model <model>"));
        assert!(help.contains("env: LLM_MODEL"));
        assert!(help.contains("--base-url <base-url>"));
        assert!(help.contains("provider API base URL"));
        assert!(help.contains("--api-key <api-key>"));
        assert!(help.contains("--system <system-prompt>"));
        assert!(help.contains("示例 (Examples):"));
        assert!(help.contains("cat report.md | llm \"Summarize risks and action items\""));
        assert!(help.contains("显示帮助。"));
        assert!(!help.contains("提示词"));
        assert!(!help.contains("<模型>"));
        assert!(!help.contains("OpenAI-compatible"));
        assert!(!help.contains("Usage:"));
        assert!(!help.contains("Options:"));
    }

    #[test]
    fn config_help_is_chinese() {
        let err = Cli::try_parse_from(["llm", "config", "-h"]).unwrap_err();

        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = err.to_string();
        assert!(help.contains("写入配置文件。"));
        assert!(help.contains("用法 (Usage):"));
        assert!(help.contains("llm config --base-url <base-url> --model <model> [OPTIONS]"));
        assert!(help.contains("--base-url <base-url>"));
        assert!(help.contains("provider API base URL"));
        assert!(help.contains("--model <model>"));
        assert!(help.contains("--api-key <api-key>"));
        assert!(help.contains("默认 model。"));
        assert!(help.contains("本地 LLM server"));
        assert!(help.contains("示例 (Examples):"));
        assert!(help.contains(
            "llm config --base-url http://localhost:11434/v1 --model llama3.2 --api-key local"
        ));
        assert!(help.contains("显示帮助。"));
        assert!(!help.contains("<地址>"));
        assert!(!help.contains("<模型>"));
        assert!(!help.contains("Usage:"));
        assert!(!help.contains("Options:"));
    }

    #[test]
    fn stream_bytes_preserve_utf8_split_across_chunks() {
        let token = "世界";
        let event = format!(
            "data: {}\n",
            serde_json::json!({ "choices": [{ "delta": { "content": token } }] })
        );
        let bytes = event.into_bytes();
        let token_start = bytes
            .windows("世".as_bytes().len())
            .position(|window| window == "世".as_bytes())
            .unwrap();
        let split_inside_token = token_start + 1;

        let mut pending = Vec::new();
        let mut out = Vec::new();

        assert!(!write_stream_bytes(&mut pending, &bytes[..split_inside_token], &mut out).unwrap());
        assert!(out.is_empty());
        assert!(!write_stream_bytes(&mut pending, &bytes[split_inside_token..], &mut out).unwrap());

        assert_eq!(String::from_utf8(out).unwrap(), token);
    }

    #[test]
    fn stream_done_writes_trailing_newline() {
        let event = format!(
            "data: {}\n",
            serde_json::json!({ "choices": [{ "delta": { "content": "hello" } }] })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();

        assert!(!write_stream_bytes(&mut pending, event.as_bytes(), &mut out).unwrap());
        assert!(write_stream_bytes(&mut pending, b"data: [DONE]\n", &mut out).unwrap());

        assert_eq!(String::from_utf8(out).unwrap(), "hello\n");
    }

    #[test]
    fn finish_stream_processes_last_line_without_newline() {
        let event = format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "content": "last" } }] })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();

        assert!(!write_stream_bytes(&mut pending, event.as_bytes(), &mut out).unwrap());
        finish_stream(&mut pending, &mut out).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "last\n");
    }
}
