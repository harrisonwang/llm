use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

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
  cat report.md | llm \"Summarize risks and action items\"
  BRAVE_SEARCH_API_KEY=... llm --search \"Rust 2026 edition changes\"{after-help}";

const COMMAND_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
用法 (Usage):
  {usage}

选项 (Options):
{options}

示例 (Examples):
  llm config --base-url https://api.openai.com/v1 --model gpt-4.1-mini
  llm config --model deepseek-v4
  llm config --api-key \"$OPENAI_API_KEY\"
  llm config --brave-api-key \"$BRAVE_SEARCH_API_KEY\"{after-help}";

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

    /// 使用 Brave Search 获取搜索上下文；只使用命令行 prompt 作为搜索 query。
    #[arg(long)]
    search: bool,

    /// Brave Search API key；覆盖配置文件和 env: BRAVE_SEARCH_API_KEY。
    #[arg(
        long,
        env = "BRAVE_SEARCH_API_KEY",
        hide_env = true,
        value_name = "api-key"
    )]
    brave_api_key: Option<String>,

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
        override_usage = "llm config [OPTIONS]"
    )]
    Config {
        /// provider API base URL。
        #[arg(long, value_name = "base-url")]
        base_url: Option<String>,

        /// 默认 model。
        #[arg(long, value_name = "model")]
        model: Option<String>,

        /// API key；本地 LLM server 可使用任意非空值。
        #[arg(long, value_name = "api-key")]
        api_key: Option<String>,

        /// Brave Search API key。
        #[arg(long, value_name = "api-key")]
        brave_api_key: Option<String>,
    },
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    brave_api_key: Option<String>,
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

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    #[serde(default)]
    web: BraveWebResults,
}

#[derive(Debug, Default, Deserialize)]
struct BraveWebResults {
    #[serde(default)]
    results: Vec<BraveWebResult>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    #[serde(default)]
    extra_snippets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BraveErrorResponse {
    error: Option<BraveError>,
}

#[derive(Debug, Deserialize)]
struct BraveError {
    code: Option<String>,
    detail: Option<String>,
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
                brave_api_key,
            } => update_config(base_url, model, api_key, brave_api_key),
        };
    }

    let config = read_config()?;
    let prompt_arg = cli.prompt.join(" ");
    let stdin = read_stdin_if_piped()?;
    let search_options = if cli.search {
        let instruction = search_instruction_from_prompt_arg(&prompt_arg)?;
        let query = build_search_query(stdin.as_deref(), &instruction);
        let brave_api_key = resolve_brave_api_key(cli.brave_api_key, config.brave_api_key.clone())?;
        Some((query, instruction, brave_api_key))
    } else {
        None
    };

    let base_url = first(cli.base_url, config.base_url, "LLM_BASE_URL", None).ok_or_else(|| {
        anyhow!("missing base URL; run `llm config --base-url URL` or set LLM_BASE_URL")
    })?;
    let model = first(cli.model, config.model, "LLM_MODEL", None)
        .ok_or_else(|| anyhow!("missing model; run `llm config --model MODEL` or set LLM_MODEL"))?;
    let api_key = first(
        cli.api_key,
        config.api_key,
        "LLM_API_KEY",
        Some("EMPTY".to_string()),
    )
    .unwrap();

    let prompt = if let Some((query, instruction, brave_api_key)) = search_options {
        let search_response = fetch_search_context(&brave_api_key, &query).await?;
        let search_context = format_search_context(&search_response)?;
        build_prompt_with_search(stdin, &search_context, &instruction)
    } else {
        build_prompt(stdin, prompt_arg)?
    };

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
    first_value(cli, env::var(env_key).ok(), config, fallback)
}

fn first_value(
    cli: Option<String>,
    env_value: Option<String>,
    config: Option<String>,
    fallback: Option<String>,
) -> Option<String> {
    cli.or(env_value)
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

fn search_instruction_from_prompt_arg(prompt_arg: &str) -> Result<String> {
    let instruction = prompt_arg.trim();
    if instruction.is_empty() {
        return Err(anyhow!(
            "missing search instruction; use `llm --search \"question\"`"
        ));
    }
    Ok(instruction.to_string())
}

fn build_search_query(stdin: Option<&str>, instruction: &str) -> String {
    const MAX_QUERY_CHARS: usize = 400;
    let mut parts = Vec::new();
    if let Some(context) = stdin.map(compact_for_search_query) {
        if !context.is_empty() {
            parts.push(context);
        }
    }
    parts.push(compact_for_search_query(instruction));

    truncate_chars(&parts.join(" "), MAX_QUERY_CHARS)
}

fn compact_for_search_query(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn resolve_brave_api_key(cli_key: Option<String>, config_key: Option<String>) -> Result<String> {
    resolve_brave_api_key_from(cli_key, env::var("BRAVE_SEARCH_API_KEY").ok(), config_key)
}

fn resolve_brave_api_key_from(
    cli_key: Option<String>,
    env_key: Option<String>,
    config_key: Option<String>,
) -> Result<String> {
    first_value(cli_key, env_key, config_key, None).ok_or_else(|| {
        anyhow!(
            "missing Brave Search API key; pass `--brave-api-key`, set BRAVE_SEARCH_API_KEY, or run `llm config --brave-api-key KEY`"
        )
    })
}

async fn fetch_search_context(api_key: &str, query: &str) -> Result<BraveSearchResponse> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Brave Search client")?
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .query(&[
            ("q", query),
            ("count", "10"),
            ("country", "US"),
            ("search_lang", "en"),
            ("spellcheck", "1"),
        ])
        .send()
        .await
        .context("Brave Search request failed")?;

    let status = response.status();
    if !status.is_success() {
        let rate_limit_reset = response
            .headers()
            .get("X-RateLimit-Reset")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.unwrap_or_default();
        return Err(brave_status_error(
            status,
            rate_limit_reset.as_deref(),
            &body,
        ));
    }

    response
        .json()
        .await
        .context("failed to parse Brave Search response JSON")
}

fn brave_status_error(
    status: StatusCode,
    rate_limit_reset: Option<&str>,
    body: &str,
) -> anyhow::Error {
    let mut message = format!("Brave Search returned HTTP {status}");
    if status == StatusCode::TOO_MANY_REQUESTS {
        if let Some(reset) = rate_limit_reset {
            message.push_str(&format!("; rate limit resets in {reset}s"));
        }
    }

    let body = body.trim();
    if !body.is_empty() {
        message.push_str(": ");
        message.push_str(&format_brave_error_body(body));
    }

    anyhow!(message)
}

fn format_brave_error_body(body: &str) -> String {
    let parsed = serde_json::from_str::<BraveErrorResponse>(body);
    if let Ok(error_response) = parsed {
        if let Some(error) = error_response.error {
            match (error.code, error.detail) {
                (Some(code), Some(detail)) => return format!("{code}: {detail}"),
                (Some(code), None) => return code,
                (None, Some(detail)) => return detail,
                (None, None) => {}
            }
        }
    }

    truncate_error_body(body)
}

fn truncate_error_body(body: &str) -> String {
    const MAX_ERROR_BODY_CHARS: usize = 500;
    let mut output = String::new();
    for (idx, ch) in body.chars().enumerate() {
        if idx >= MAX_ERROR_BODY_CHARS {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}

fn format_search_context(response: &BraveSearchResponse) -> Result<String> {
    let results = response
        .web
        .results
        .iter()
        .filter_map(|result| {
            let title = result.title.as_deref()?.trim();
            let url = result.url.as_deref()?.trim();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            Some((title, url, result))
        })
        .collect::<Vec<_>>();

    if results.is_empty() {
        return Err(anyhow!("Brave Search returned no search results"));
    }

    let mut context = String::from("<search_context>\n");
    for (idx, (title, url, result)) in results.iter().enumerate() {
        context.push_str(&format!("Source {}:\n", idx + 1));
        context.push_str(&format!("Title: {title}\n"));
        context.push_str(&format!("URL: {url}\n"));
        context.push_str("Snippets:\n");

        let mut wrote_snippet = false;
        if let Some(description) = result.description.as_deref().map(str::trim) {
            if !description.is_empty() {
                wrote_snippet = true;
                context.push_str("- ");
                context.push_str(description);
                context.push('\n');
            }
        }
        for snippet in result
            .extra_snippets
            .iter()
            .map(|snippet| snippet.trim())
            .filter(|snippet| !snippet.is_empty())
        {
            wrote_snippet = true;
            context.push_str("- ");
            context.push_str(snippet);
            context.push('\n');
        }
        if !wrote_snippet {
            context.push_str("- [No snippet provided]\n");
        }
        context.push('\n');
    }
    context.push_str("</search_context>");
    Ok(context)
}

fn build_prompt_with_search(
    stdin: Option<String>,
    search_context: &str,
    instruction: &str,
) -> String {
    let mut prompt = String::new();
    if let Some(context) = stdin {
        prompt.push_str("<context>\n");
        prompt.push_str(context.trim_end());
        prompt.push_str("\n</context>\n\n");
    }
    prompt.push_str(search_context.trim_end());
    prompt.push_str("\n\nInstruction:\n");
    prompt.push_str(instruction.trim());
    prompt.push_str("\n\nSearch answer requirements:\n- Answer using the context and search_context above for current or searched facts.\n- Include a Sources section with Markdown links for every search source you rely on.\n- If the search_context is insufficient, say so explicitly and do not guess from prior knowledge.");
    prompt
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

fn update_config(
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    brave_api_key: Option<String>,
) -> Result<()> {
    let mut config = read_config()?;
    apply_config_update(&mut config, base_url, model, api_key, brave_api_key)?;
    write_config(config)
}

fn apply_config_update(
    config: &mut Config,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    brave_api_key: Option<String>,
) -> Result<()> {
    if base_url.is_none() && model.is_none() && api_key.is_none() && brave_api_key.is_none() {
        return Err(anyhow!(
            "nothing to configure; pass at least one of --base-url, --model, --api-key, --brave-api-key"
        ));
    }

    let updates_model_config = base_url.is_some() || model.is_some() || api_key.is_some();
    set_config_value(&mut config.base_url, base_url, "--base-url")?;
    set_config_value(&mut config.model, model, "--model")?;
    set_config_value(&mut config.api_key, api_key, "--api-key")?;
    set_config_value(&mut config.brave_api_key, brave_api_key, "--brave-api-key")?;

    if updates_model_config {
        ensure_model_config_complete(config)?;
    }

    Ok(())
}

fn set_config_value(
    target: &mut Option<String>,
    value: Option<String>,
    option_name: &str,
) -> Result<()> {
    if let Some(value) = value {
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(anyhow!("{option_name} cannot be empty"));
        }
        *target = Some(value);
    }
    Ok(())
}

fn ensure_model_config_complete(config: &Config) -> Result<()> {
    let missing_base_url = config_value_is_empty(config.base_url.as_deref());
    let missing_model = config_value_is_empty(config.model.as_deref());
    match (missing_base_url, missing_model) {
        (false, false) => Ok(()),
        (true, true) => Err(anyhow!(
            "model config requires --base-url and --model; run `llm config --base-url URL --model MODEL`"
        )),
        (true, false) => Err(anyhow!(
            "model config requires --base-url; run `llm config --base-url URL`"
        )),
        (false, true) => Err(anyhow!(
            "model config requires --model; run `llm config --model MODEL`"
        )),
    }
}

fn config_value_is_empty(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
}

fn config_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".llm").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_wraps_pith_context_with_instruction() {
        let prompt = build_prompt(
            Some("# Report\n\nA finding from pith.\n".to_string()),
            "总结风险和行动项".to_string(),
        )
        .unwrap();

        assert_eq!(
            prompt,
            "<context>\n# Report\n\nA finding from pith.\n</context>\n\nInstruction:\n总结风险和行动项"
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
    fn search_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "--search", "Rust", "edition"]).unwrap();

        assert!(cli.search);
        assert_eq!(cli.prompt, ["Rust", "edition"]);
    }

    #[test]
    fn brave_api_key_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "--brave-api-key", "brave-key", "--search", "Rust"])
            .unwrap();

        assert_eq!(cli.brave_api_key.as_deref(), Some("brave-key"));
    }

    #[test]
    fn config_brave_api_key_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "config", "--brave-api-key", "brave-key"]).unwrap();

        let Some(Command::Config { brave_api_key, .. }) = cli.command else {
            panic!("expected config command");
        };
        assert_eq!(brave_api_key.as_deref(), Some("brave-key"));
    }

    #[test]
    fn search_requires_command_line_prompt() {
        let err = search_instruction_from_prompt_arg("   ")
            .unwrap_err()
            .to_string();

        assert!(err.contains("missing search instruction"));
    }

    #[test]
    fn search_query_includes_piped_context() {
        let query = build_search_query(
            Some("cargo 1.90.0 (840b83a10 2025-07-30)\n"),
            "这个版本的cargo有什么特性？",
        );

        assert!(query.contains("cargo 1.90.0"));
        assert!(query.contains("这个版本的cargo有什么特性？"));
    }

    #[test]
    fn search_query_is_limited_to_api_max_length() {
        let query = build_search_query(Some(&"x ".repeat(300)), &"y ".repeat(300));

        assert_eq!(query.chars().count(), 400);
    }

    #[test]
    fn brave_api_key_requires_cli_or_env() {
        let err = resolve_brave_api_key_from(None, None, None)
            .unwrap_err()
            .to_string();

        assert!(err.contains("missing Brave Search API key"));
    }

    #[test]
    fn brave_api_key_prefers_cli_over_env() {
        let key = resolve_brave_api_key_from(
            Some(" cli-key ".to_string()),
            Some("env-key".to_string()),
            Some("config-key".to_string()),
        )
        .unwrap();

        assert_eq!(key, "cli-key");
    }

    #[test]
    fn brave_api_key_prefers_env_over_config() {
        let key = resolve_brave_api_key_from(
            None,
            Some(" env-key ".to_string()),
            Some("config-key".to_string()),
        )
        .unwrap();

        assert_eq!(key, "env-key");
    }

    #[test]
    fn brave_api_key_uses_config_when_cli_and_env_are_missing() {
        let key = resolve_brave_api_key_from(None, None, Some(" config-key ".to_string())).unwrap();

        assert_eq!(key, "config-key");
    }

    #[test]
    fn config_update_rejects_no_options() {
        let mut config = Config::default();
        let err = apply_config_update(&mut config, None, None, None, None)
            .unwrap_err()
            .to_string();

        assert!(err.contains("nothing to configure"));
    }

    #[test]
    fn config_update_allows_brave_key_without_model_config() {
        let mut config = Config::default();

        apply_config_update(
            &mut config,
            None,
            None,
            None,
            Some(" brave-key ".to_string()),
        )
        .unwrap();

        assert_eq!(config.brave_api_key.as_deref(), Some("brave-key"));
        assert!(config.base_url.is_none());
        assert!(config.model.is_none());
    }

    #[test]
    fn config_update_requires_complete_initial_model_config() {
        let mut config = Config::default();
        let err = apply_config_update(
            &mut config,
            None,
            Some("deepseek-v4".to_string()),
            None,
            None,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("requires --base-url"));
    }

    #[test]
    fn config_update_accepts_complete_initial_model_config() {
        let mut config = Config::default();

        apply_config_update(
            &mut config,
            Some(" https://api.deepseek.com/v1 ".to_string()),
            Some(" deepseek-v4 ".to_string()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            config.base_url.as_deref(),
            Some("https://api.deepseek.com/v1")
        );
        assert_eq!(config.model.as_deref(), Some("deepseek-v4"));
    }

    #[test]
    fn config_update_allows_single_model_change_after_complete_config() {
        let mut config = Config {
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            api_key: Some("old-key".to_string()),
            brave_api_key: None,
        };

        apply_config_update(
            &mut config,
            None,
            Some("deepseek-v4".to_string()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            config.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(config.model.as_deref(), Some("deepseek-v4"));
        assert_eq!(config.api_key.as_deref(), Some("old-key"));
    }

    #[test]
    fn config_path_uses_dot_llm_under_home() {
        let home = env::var("HOME").unwrap();
        let expected = PathBuf::from(home).join(".llm").join("config.toml");

        assert_eq!(config_path().unwrap(), expected);
    }

    #[test]
    fn config_serializes_configured_fields() {
        let text = toml::to_string_pretty(&Config {
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            api_key: None,
            brave_api_key: Some("brave-key".to_string()),
        })
        .unwrap();

        assert!(text.contains("base_url = \"https://api.openai.com/v1\""));
        assert!(text.contains("model = \"gpt-4.1-mini\""));
        assert!(text.contains("brave_api_key = \"brave-key\""));
        assert!(!text.lines().any(|line| line.starts_with("api_key = ")));
    }

    #[test]
    fn config_reads_brave_api_key_field() {
        let config: Config = toml::from_str(r#"brave_api_key = "brave-key""#).unwrap();

        assert_eq!(config.brave_api_key.as_deref(), Some("brave-key"));
    }

    #[test]
    fn format_search_context_includes_brave_results() {
        let response: BraveSearchResponse = serde_json::from_value(serde_json::json!({
            "web": {
                "results": [
                    {
                        "title": "Rust Blog",
                        "url": "https://blog.rust-lang.org/",
                        "description": "Rust release notes.",
                        "extra_snippets": ["Rust edition updates."]
                    }
                ]
            }
        }))
        .unwrap();

        let context = format_search_context(&response).unwrap();

        assert!(context.contains("<search_context>"));
        assert!(context.contains("Title: Rust Blog"));
        assert!(context.contains("URL: https://blog.rust-lang.org/"));
        assert!(context.contains("- Rust release notes."));
        assert!(context.contains("- Rust edition updates."));
        assert!(context.contains("</search_context>"));
    }

    #[test]
    fn format_search_context_rejects_empty_results() {
        let response: BraveSearchResponse = serde_json::from_value(serde_json::json!({
            "web": {
                "results": []
            }
        }))
        .unwrap();

        let err = format_search_context(&response).unwrap_err().to_string();

        assert!(err.contains("no search results"));
    }

    #[test]
    fn search_prompt_includes_stdin_context_and_instruction() {
        let prompt = build_prompt_with_search(
            Some("cargo 1.90.0 (840b83a10 2025-07-30)\n".to_string()),
            "<search_context>\nSource 1:\nTitle: Cargo\nURL: https://doc.rust-lang.org/cargo/\nSnippets:\n- Cargo docs.\n\n</search_context>",
            "这个版本的cargo有什么特性？",
        );

        assert!(prompt.contains("<context>"));
        assert!(prompt.contains("cargo 1.90.0"));
        assert!(prompt.contains("<search_context>"));
        assert!(prompt.contains("这个版本的cargo有什么特性？"));
    }

    #[test]
    fn brave_error_body_formats_json_error() {
        let body = r#"{"error":{"code":"BAD_REQUEST","detail":"bad query","status":400},"type":"ErrorResponse"}"#;

        assert_eq!(format_brave_error_body(body), "BAD_REQUEST: bad query");
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
        assert!(help.contains("llm config [OPTIONS]"));
        assert!(help.contains("--base-url <base-url>"));
        assert!(help.contains("provider API base URL"));
        assert!(help.contains("--model <model>"));
        assert!(help.contains("--api-key <api-key>"));
        assert!(help.contains("--brave-api-key <api-key>"));
        assert!(help.contains("默认 model。"));
        assert!(help.contains("本地 LLM server"));
        assert!(help.contains("示例 (Examples):"));
        assert!(
            help.contains("llm config --base-url https://api.openai.com/v1 --model gpt-4.1-mini")
        );
        assert!(help.contains("llm config --model deepseek-v4"));
        assert!(help.contains("llm config --brave-api-key \"$BRAVE_SEARCH_API_KEY\""));
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
