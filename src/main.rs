use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use search::SearchProvider;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

mod search;

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
  llm -p local \"Draft quickly\"
  llm models
  llm models -p talkweb
  llm --no-render \"Write markdown\"
  cat report.md | llm \"Summarize risks and action items\"
  EXA_API_KEY=... llm --search \"Rust 2026 edition changes\"{after-help}";

const COMMAND_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
用法 (Usage):
  {usage}

选项 (Options):
{options}

示例 (Examples):
  llm config --base-url https://api.openai.com/v1 --model gpt-4.1-mini
  llm config --profile local --base-url http://localhost:11434/v1 --model llama3.2 --api-key local
  llm config --model deepseek-v4
  llm config --api-key \"$OPENAI_API_KEY\"
  llm config --search-provider exa
  llm config --exa-api-key \"$EXA_API_KEY\"
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

    /// 使用命名 profile；不传则使用默认配置。
    #[arg(short = 'p', long, value_name = "profile")]
    profile: Option<String>,

    /// provider API base URL；env: LLM_BASE_URL。
    #[arg(long, env = "LLM_BASE_URL", hide_env = true, value_name = "base-url")]
    base_url: Option<String>,

    /// API key；覆盖配置文件和 env: LLM_API_KEY。
    #[arg(long, env = "LLM_API_KEY", hide_env = true, value_name = "api-key")]
    api_key: Option<String>,

    /// system prompt。
    #[arg(short, long, value_name = "system-prompt")]
    system: Option<String>,

    /// 使用搜索 API 获取搜索上下文；只使用命令行 prompt 作为搜索 query。
    #[arg(long)]
    search: bool,

    /// 搜索 provider；可选值: exa, brave；env: SEARCH_PROVIDER。
    #[arg(
        long,
        env = "SEARCH_PROVIDER",
        hide_env = true,
        value_enum,
        value_name = "provider"
    )]
    search_provider: Option<SearchProvider>,

    /// Exa API key；覆盖配置文件和 env: EXA_API_KEY。
    #[arg(long, env = "EXA_API_KEY", hide_env = true, value_name = "api-key")]
    exa_api_key: Option<String>,

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

    /// 关闭 TTY Markdown 渲染，始终输出原始文本。
    #[arg(long)]
    no_render: bool,

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
        /// 写入命名 profile；不传则写入默认配置。
        #[arg(long, value_name = "profile")]
        profile: Option<String>,

        /// provider API base URL。
        #[arg(long, value_name = "base-url")]
        base_url: Option<String>,

        /// 默认 model。
        #[arg(long, value_name = "model")]
        model: Option<String>,

        /// API key；本地 LLM server 可使用任意非空值。
        #[arg(long, value_name = "api-key")]
        api_key: Option<String>,

        /// 搜索 provider；可选值: exa, brave。
        #[arg(long, value_enum, value_name = "provider")]
        search_provider: Option<SearchProvider>,

        /// Exa API key。
        #[arg(long, value_name = "api-key")]
        exa_api_key: Option<String>,

        /// Brave Search API key。
        #[arg(long, value_name = "api-key")]
        brave_api_key: Option<String>,
    },

    /// 列出 /models 返回的模型。
    #[command(
        help_template = COMMAND_HELP_TEMPLATE,
        override_usage = "llm models [OPTIONS]"
    )]
    Models {
        /// 使用命名 profile；不传则使用默认配置。
        #[arg(short = 'p', long, value_name = "profile")]
        profile: Option<String>,

        /// provider API base URL。
        #[arg(long, env = "LLM_BASE_URL", hide_env = true, value_name = "base-url")]
        base_url: Option<String>,

        /// API key；覆盖配置文件和 env: LLM_API_KEY。
        #[arg(long, env = "LLM_API_KEY", hide_env = true, value_name = "api-key")]
        api_key: Option<String>,
    },
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    search_provider: Option<SearchProvider>,
    exa_api_key: Option<String>,
    brave_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profiles: BTreeMap<String, ProfileConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pricing: BTreeMap<String, ModelPricing>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProfileConfig {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct ModelPricing {
    input_per_1m: Option<f64>,
    output_per_1m: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Debug)]
struct UsageSummary {
    usage: Usage,
    cost: Option<f64>,
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
                profile,
                base_url,
                model,
                api_key,
                search_provider,
                exa_api_key,
                brave_api_key,
            } => update_config(
                profile,
                base_url,
                model,
                api_key,
                search_provider,
                exa_api_key,
                brave_api_key,
            ),
            Command::Models {
                profile,
                base_url,
                api_key,
            } => list_models_command(profile, base_url, api_key).await,
        };
    }

    let config = read_config()?;
    let prompt_arg = cli.prompt.join(" ");
    let stdin = read_stdin_if_piped()?;
    let search_options = if cli.search {
        let instruction = search::search_instruction_from_prompt_arg(&prompt_arg)?;
        let query = search::build_search_query(stdin.as_deref(), &instruction);
        let credentials = search::resolve_credentials(
            cli.search_provider.or(config.search_provider),
            cli.brave_api_key,
            cli.exa_api_key,
            config.brave_api_key.clone(),
            config.exa_api_key.clone(),
        )?;
        Some((query, instruction, credentials))
    } else {
        None
    };

    let selected_config = selected_model_config(&config, cli.profile)?;
    let base_url =
        first(cli.base_url, selected_config.base_url, "LLM_BASE_URL", None).ok_or_else(|| {
            anyhow!("missing base URL; run `llm config --base-url URL` or set LLM_BASE_URL")
        })?;
    let model = first(cli.model, selected_config.model, "LLM_MODEL", None)
        .ok_or_else(|| anyhow!("missing model; run `llm config --model MODEL` or set LLM_MODEL"))?;
    let api_key = first(
        cli.api_key,
        selected_config.api_key,
        "LLM_API_KEY",
        Some("EMPTY".to_string()),
    )
    .unwrap();

    let prompt = if let Some((query, instruction, credentials)) = search_options {
        let mut stderr = io::stderr();
        write_search_provider_notice(credentials.provider, &mut stderr)?;
        let search_context =
            search::fetch_search_context(credentials.provider, &credentials.api_key, &query)
                .await?;
        search::build_prompt_with_search(stdin, &search_context, &instruction)
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

    let pricing = config.pricing.get(&model).copied();
    let stream = cli.stream || !cli.no_stream;
    let request = ChatRequest {
        model,
        messages,
        stream,
        stream_options: stream.then_some(StreamOptions {
            include_usage: true,
        }),
    };

    let render = should_render(cli.no_render);
    if request.stream {
        stream_chat(&base_url, &api_key, &request, render, pricing).await
    } else {
        complete_chat(&base_url, &api_key, &request, render, pricing).await
    }
}

fn selected_model_config(config: &Config, profile: Option<String>) -> Result<ProfileConfig> {
    if let Some(profile) = profile {
        let profile = validate_profile_name(&profile)?;
        return config
            .profiles
            .get(&profile)
            .cloned()
            .ok_or_else(|| anyhow!("profile '{profile}' not found; run `llm config --profile {profile} --base-url URL --model MODEL`"));
    }

    Ok(ProfileConfig {
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
    })
}

async fn list_models_command(
    profile: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<()> {
    let config = read_config()?;
    let selected_config = selected_model_config(&config, profile)?;
    let base_url =
        first(base_url, selected_config.base_url, "LLM_BASE_URL", None).ok_or_else(|| {
            anyhow!("missing base URL; run `llm config --base-url URL` or set LLM_BASE_URL")
        })?;
    let api_key = first(
        api_key,
        selected_config.api_key,
        "LLM_API_KEY",
        Some("EMPTY".to_string()),
    )
    .unwrap();
    let models = fetch_models(&base_url, &api_key).await?;
    let mut stdout = io::stdout();
    write_models(&models, &mut stdout)
}

async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<ModelInfo>> {
    let response: ModelsResponse = client(api_key)
        .get(models_url(base_url))
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

fn write_models<W: Write>(models: &[ModelInfo], out: &mut W) -> Result<()> {
    for model in models {
        writeln!(out, "{}", model.id).context("failed to write model list")?;
    }
    Ok(())
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

fn build_usage_summary(usage: Usage, pricing: Option<ModelPricing>) -> UsageSummary {
    UsageSummary {
        usage,
        cost: pricing.and_then(|pricing| calculate_cost(usage, pricing)),
    }
}

fn calculate_cost(usage: Usage, pricing: ModelPricing) -> Option<f64> {
    let input_cost = pricing.input_per_1m? * usage.prompt_tokens as f64 / 1_000_000.0;
    let output_cost = pricing.output_per_1m? * usage.completion_tokens as f64 / 1_000_000.0;
    Some(input_cost + output_cost)
}

fn write_usage_summary<W: Write>(summary: &UsageSummary, out: &mut W) -> Result<()> {
    write!(
        out,
        "tokens: {} in / {} out",
        summary.usage.prompt_tokens, summary.usage.completion_tokens
    )
    .context("failed to write usage summary")?;
    if let Some(cost) = summary.cost {
        write!(out, ", ~${cost:.3}").context("failed to write usage summary")?;
    }
    writeln!(out).context("failed to write usage summary")
}

fn write_search_provider_notice<W: Write>(provider: SearchProvider, out: &mut W) -> Result<()> {
    writeln!(out, "search provider: {provider}").context("failed to write search provider notice")
}

fn should_render(no_render: bool) -> bool {
    should_render_for(no_render, io::stdout().is_terminal())
}

fn should_render_for(no_render: bool, stdout_is_tty: bool) -> bool {
    !no_render && stdout_is_tty
}

fn write_markdown_output<W: Write>(text: &str, render: bool, out: &mut W) -> Result<()> {
    if render {
        termimad::MadSkin::default()
            .write_text_on(out, text)
            .context("failed to render markdown output")?;
        writeln!(out).context("failed to write final newline")?;
    } else {
        writeln!(out, "{text}").context("failed to write output")?;
    }
    Ok(())
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

async fn complete_chat(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    render: bool,
    pricing: Option<ModelPricing>,
) -> Result<()> {
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
    let mut stdout = io::stdout();
    write_markdown_output(text, render, &mut stdout)?;
    if let Some(usage) = response.usage {
        let mut stderr = io::stderr();
        write_usage_summary(&build_usage_summary(usage, pricing), &mut stderr)?;
    }
    Ok(())
}

async fn stream_chat(
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    render: bool,
    pricing: Option<ModelPricing>,
) -> Result<()> {
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
    let mut output = StreamOutput::new(render);
    let mut usage = None;
    while let Some(chunk) = response.chunk().await.context("failed to read stream")? {
        if write_stream_bytes(&mut pending, &chunk, &mut stdout, &mut output, &mut usage)? {
            output.finish(&mut stdout)?;
            if let Some(usage) = usage {
                let mut stderr = io::stderr();
                write_usage_summary(&build_usage_summary(usage, pricing), &mut stderr)?;
            }
            return Ok(());
        }
    }
    finish_stream(&mut pending, &mut stdout, &mut output, &mut usage)?;
    if let Some(usage) = usage {
        let mut stderr = io::stderr();
        write_usage_summary(&build_usage_summary(usage, pricing), &mut stderr)?;
    }
    Ok(())
}

enum StreamOutput {
    Raw,
    Rendered(String),
}

impl StreamOutput {
    fn new(render: bool) -> Self {
        if render {
            Self::Rendered(String::new())
        } else {
            Self::Raw
        }
    }

    fn write_text<W: Write>(&mut self, text: &str, out: &mut W) -> Result<()> {
        match self {
            Self::Raw => {
                out.write_all(text.as_bytes())
                    .context("failed to write streamed output")?;
                out.flush().context("failed to flush streamed output")?;
            }
            Self::Rendered(buffer) => buffer.push_str(text),
        }
        Ok(())
    }

    fn finish<W: Write>(&mut self, out: &mut W) -> Result<()> {
        match self {
            Self::Raw => writeln!(out).context("failed to write final newline")?,
            Self::Rendered(buffer) => write_markdown_output(buffer, true, out)?,
        }
        Ok(())
    }
}

fn write_stream_bytes<W: Write>(
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

fn finish_stream<W: Write>(
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

fn models_url(base_url: &str) -> String {
    format!("{}/models", base_url.trim_end_matches('/'))
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

#[derive(Default)]
struct ConfigUpdate {
    profile: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    search_provider: Option<SearchProvider>,
    exa_api_key: Option<String>,
    brave_api_key: Option<String>,
}

fn update_config(
    profile: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    search_provider: Option<SearchProvider>,
    exa_api_key: Option<String>,
    brave_api_key: Option<String>,
) -> Result<()> {
    let mut config = read_config()?;
    apply_config_update(
        &mut config,
        ConfigUpdate {
            profile,
            base_url,
            model,
            api_key,
            search_provider,
            exa_api_key,
            brave_api_key,
        },
    )?;
    write_config(config)
}

fn apply_config_update(config: &mut Config, update: ConfigUpdate) -> Result<()> {
    let ConfigUpdate {
        profile,
        base_url,
        model,
        api_key,
        search_provider,
        exa_api_key,
        brave_api_key,
    } = update;
    if base_url.is_none()
        && model.is_none()
        && api_key.is_none()
        && search_provider.is_none()
        && exa_api_key.is_none()
        && brave_api_key.is_none()
    {
        return Err(anyhow!(
            "nothing to configure; pass at least one of --base-url, --model, --api-key, --search-provider, --exa-api-key, --brave-api-key"
        ));
    }

    let updates_model_config = base_url.is_some() || model.is_some() || api_key.is_some();
    if let Some(profile) = profile {
        if search_provider.is_some() || exa_api_key.is_some() || brave_api_key.is_some() {
            return Err(anyhow!(
                "--profile cannot be combined with search config; configure search provider globally"
            ));
        }
        if !updates_model_config {
            return Err(anyhow!(
                "nothing to configure; pass at least one of --base-url, --model, --api-key"
            ));
        }
        let profile = validate_profile_name(&profile)?;
        let profile_config = config.profiles.entry(profile).or_default();
        set_config_value(&mut profile_config.base_url, base_url, "--base-url")?;
        set_config_value(&mut profile_config.model, model, "--model")?;
        set_config_value(&mut profile_config.api_key, api_key, "--api-key")?;
        ensure_profile_config_complete(profile_config)?;
        return Ok(());
    }

    set_config_value(&mut config.base_url, base_url, "--base-url")?;
    set_config_value(&mut config.model, model, "--model")?;
    set_config_value(&mut config.api_key, api_key, "--api-key")?;
    if let Some(search_provider) = search_provider {
        config.search_provider = Some(search_provider);
    }
    set_config_value(&mut config.exa_api_key, exa_api_key, "--exa-api-key")?;
    set_config_value(&mut config.brave_api_key, brave_api_key, "--brave-api-key")?;

    if updates_model_config {
        ensure_model_config_complete(config)?;
    }

    Ok(())
}

fn validate_profile_name(profile: &str) -> Result<String> {
    let profile = profile.trim();
    if profile.is_empty()
        || !profile
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(anyhow!(
            "invalid profile name; use letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(profile.to_string())
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
    ensure_complete_model_fields(config.base_url.as_deref(), config.model.as_deref())
}

fn ensure_profile_config_complete(config: &ProfileConfig) -> Result<()> {
    ensure_complete_model_fields(config.base_url.as_deref(), config.model.as_deref())
}

fn ensure_complete_model_fields(base_url: Option<&str>, model: Option<&str>) -> Result<()> {
    let missing_base_url = config_value_is_empty(base_url);
    let missing_model = config_value_is_empty(model);
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
    fn profile_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "-p", "local", "hi"]).unwrap();

        assert_eq!(cli.profile.as_deref(), Some("local"));
        assert_eq!(cli.prompt, ["hi"]);
    }

    #[test]
    fn config_profile_flag_parses() {
        let cli = Cli::try_parse_from([
            "llm",
            "config",
            "--profile",
            "local",
            "--base-url",
            "http://localhost:11434/v1",
            "--model",
            "llama3.2",
            "--api-key",
            "local",
        ])
        .unwrap();

        let Some(Command::Config {
            profile,
            base_url,
            model,
            api_key,
            ..
        }) = cli.command
        else {
            panic!("expected config command");
        };
        assert_eq!(profile.as_deref(), Some("local"));
        assert_eq!(base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(model.as_deref(), Some("llama3.2"));
        assert_eq!(api_key.as_deref(), Some("local"));
    }

    #[test]
    fn no_render_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "--no-render", "hi"]).unwrap();

        assert!(cli.no_render);
        assert_eq!(cli.prompt, ["hi"]);
    }

    #[test]
    fn should_render_only_for_tty_without_override() {
        assert!(should_render_for(false, true));
        assert!(!should_render_for(false, false));
        assert!(!should_render_for(true, true));
        assert!(!should_render_for(true, false));
    }

    #[test]
    fn raw_markdown_output_preserves_text() {
        let mut out = Vec::new();

        write_markdown_output("## Title\n\n**bold**", false, &mut out).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "## Title\n\n**bold**\n");
    }

    #[test]
    fn rendered_markdown_output_writes_terminal_text() {
        let mut out = Vec::new();

        write_markdown_output("## Title", true, &mut out).unwrap();

        assert!(!out.is_empty());
        assert!(String::from_utf8(out).unwrap().contains("Title"));
    }

    #[test]
    fn usage_summary_writes_tokens_and_cost() {
        let summary = build_usage_summary(
            Usage {
                prompt_tokens: 1_000,
                completion_tokens: 500,
            },
            Some(ModelPricing {
                input_per_1m: Some(1.0),
                output_per_1m: Some(2.0),
            }),
        );
        let mut out = Vec::new();

        write_usage_summary(&summary, &mut out).unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "tokens: 1000 in / 500 out, ~$0.002\n"
        );
    }

    #[test]
    fn usage_summary_omits_cost_without_complete_pricing() {
        let summary = build_usage_summary(
            Usage {
                prompt_tokens: 1_000,
                completion_tokens: 500,
            },
            Some(ModelPricing {
                input_per_1m: Some(1.0),
                output_per_1m: None,
            }),
        );
        let mut out = Vec::new();

        write_usage_summary(&summary, &mut out).unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "tokens: 1000 in / 500 out\n"
        );
    }

    #[test]
    fn streaming_request_includes_usage_options() {
        let request = ChatRequest {
            model: "gpt-4.1-mini".to_string(),
            messages: Vec::new(),
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(value["stream_options"]["include_usage"], true);
    }

    #[test]
    fn non_streaming_request_omits_usage_options() {
        let request = ChatRequest {
            model: "gpt-4.1-mini".to_string(),
            messages: Vec::new(),
            stream: false,
            stream_options: None,
        };

        let value = serde_json::to_value(&request).unwrap();

        assert!(value.get("stream_options").is_none());
    }

    #[test]
    fn config_serializes_pricing_table() {
        let mut config = Config::default();
        config.pricing.insert(
            "gpt-4.1-mini".to_string(),
            ModelPricing {
                input_per_1m: Some(0.4),
                output_per_1m: Some(1.6),
            },
        );

        let text = toml::to_string_pretty(&config).unwrap();

        assert!(text.contains("[pricing.\"gpt-4.1-mini\"]"));
        assert!(text.contains("input_per_1m = 0.4"));
        assert!(text.contains("output_per_1m = 1.6"));
    }

    #[test]
    fn config_reads_pricing_table() {
        let config: Config = toml::from_str(
            r#"
[pricing."gpt-4.1-mini"]
input_per_1m = 0.4
output_per_1m = 1.6
"#,
        )
        .unwrap();

        let pricing = config.pricing.get("gpt-4.1-mini").unwrap();
        assert_eq!(pricing.input_per_1m, Some(0.4));
        assert_eq!(pricing.output_per_1m, Some(1.6));
    }

    #[test]
    fn models_url_appends_models_endpoint() {
        assert_eq!(
            models_url("http://localhost:11434/v1/"),
            "http://localhost:11434/v1/models"
        );
    }

    #[test]
    fn write_models_outputs_one_model_id_per_line() {
        let models = vec![
            ModelInfo {
                id: "gpt-5.5".to_string(),
            },
            ModelInfo {
                id: "tw/gpu/qwen2.5-vl-32b-instruct".to_string(),
            },
        ];
        let mut out = Vec::new();

        write_models(&models, &mut out).unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "gpt-5.5\ntw/gpu/qwen2.5-vl-32b-instruct\n"
        );
    }

    #[test]
    fn models_response_reads_openai_compatible_shape() {
        let response: ModelsResponse = serde_json::from_str(
            r#"
{
  "object": "list",
  "data": [
    {
      "id": "gpt-5.5",
      "object": "model",
      "created": 1626777600,
      "owned_by": "custom",
      "supported_endpoint_types": ["openai"]
    }
  ]
}
"#,
        )
        .unwrap();

        assert_eq!(response.data[0].id, "gpt-5.5");
    }

    #[test]
    fn search_provider_notice_writes_to_given_output() {
        let mut out = Vec::new();

        write_search_provider_notice(SearchProvider::Exa, &mut out).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "search provider: exa\n");
    }

    #[test]
    fn search_flag_parses() {
        let cli = Cli::try_parse_from(["llm", "--search", "Rust", "edition"]).unwrap();

        assert!(cli.search);
        assert_eq!(cli.prompt, ["Rust", "edition"]);
    }

    #[test]
    fn search_provider_flag_parses() {
        let cli =
            Cli::try_parse_from(["llm", "--search-provider", "exa", "--search", "Rust"]).unwrap();

        assert_eq!(cli.search_provider, Some(SearchProvider::Exa));
    }

    #[test]
    fn search_api_key_flags_parse() {
        let cli = Cli::try_parse_from([
            "llm",
            "--exa-api-key",
            "exa-key",
            "--brave-api-key",
            "brave-key",
            "--search",
            "Rust",
        ])
        .unwrap();

        assert_eq!(cli.exa_api_key.as_deref(), Some("exa-key"));
        assert_eq!(cli.brave_api_key.as_deref(), Some("brave-key"));
    }

    #[test]
    fn config_search_flags_parse() {
        let cli = Cli::try_parse_from([
            "llm",
            "config",
            "--search-provider",
            "brave",
            "--exa-api-key",
            "exa-key",
            "--brave-api-key",
            "brave-key",
        ])
        .unwrap();

        let Some(Command::Config {
            search_provider,
            exa_api_key,
            brave_api_key,
            ..
        }) = cli.command
        else {
            panic!("expected config command");
        };
        assert_eq!(search_provider, Some(SearchProvider::Brave));
        assert_eq!(exa_api_key.as_deref(), Some("exa-key"));
        assert_eq!(brave_api_key.as_deref(), Some("brave-key"));
    }

    #[test]
    fn config_update_rejects_no_options() {
        let mut config = Config::default();
        let err = apply_config_update(&mut config, ConfigUpdate::default())
            .unwrap_err()
            .to_string();

        assert!(err.contains("nothing to configure"));
    }

    #[test]
    fn config_update_allows_brave_key_without_model_config() {
        let mut config = Config::default();

        apply_config_update(
            &mut config,
            ConfigUpdate {
                brave_api_key: Some(" brave-key ".to_string()),
                ..ConfigUpdate::default()
            },
        )
        .unwrap();

        assert_eq!(config.brave_api_key.as_deref(), Some("brave-key"));
        assert!(config.base_url.is_none());
        assert!(config.model.is_none());
    }

    #[test]
    fn config_update_allows_exa_search_config_without_model_config() {
        let mut config = Config::default();

        apply_config_update(
            &mut config,
            ConfigUpdate {
                search_provider: Some(SearchProvider::Exa),
                exa_api_key: Some(" exa-key ".to_string()),
                ..ConfigUpdate::default()
            },
        )
        .unwrap();

        assert_eq!(config.search_provider, Some(SearchProvider::Exa));
        assert_eq!(config.exa_api_key.as_deref(), Some("exa-key"));
        assert!(config.base_url.is_none());
        assert!(config.model.is_none());
    }

    #[test]
    fn config_update_requires_complete_initial_model_config() {
        let mut config = Config::default();
        let err = apply_config_update(
            &mut config,
            ConfigUpdate {
                model: Some("deepseek-v4".to_string()),
                ..ConfigUpdate::default()
            },
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
            ConfigUpdate {
                base_url: Some(" https://api.deepseek.com/v1 ".to_string()),
                model: Some(" deepseek-v4 ".to_string()),
                ..ConfigUpdate::default()
            },
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
            search_provider: None,
            exa_api_key: None,
            brave_api_key: None,
            profiles: BTreeMap::new(),
            pricing: BTreeMap::new(),
        };

        apply_config_update(
            &mut config,
            ConfigUpdate {
                model: Some("deepseek-v4".to_string()),
                ..ConfigUpdate::default()
            },
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
    fn profile_update_creates_complete_profile() {
        let mut config = Config::default();

        apply_config_update(
            &mut config,
            ConfigUpdate {
                profile: Some(" local ".to_string()),
                base_url: Some(" http://localhost:11434/v1 ".to_string()),
                model: Some(" llama3.2 ".to_string()),
                api_key: Some(" local ".to_string()),
                ..ConfigUpdate::default()
            },
        )
        .unwrap();

        let profile = config.profiles.get("local").unwrap();
        assert_eq!(
            profile.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(profile.model.as_deref(), Some("llama3.2"));
        assert_eq!(profile.api_key.as_deref(), Some("local"));
    }

    #[test]
    fn profile_update_rejects_incomplete_profile() {
        let mut config = Config::default();
        let err = apply_config_update(
            &mut config,
            ConfigUpdate {
                profile: Some("local".to_string()),
                model: Some("llama3.2".to_string()),
                ..ConfigUpdate::default()
            },
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("requires --base-url"));
    }

    #[test]
    fn profile_update_allows_single_model_change_after_complete_profile() {
        let mut config = Config::default();
        config.profiles.insert(
            "local".to_string(),
            ProfileConfig {
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("llama3.2".to_string()),
                api_key: Some("local".to_string()),
            },
        );

        apply_config_update(
            &mut config,
            ConfigUpdate {
                profile: Some("local".to_string()),
                model: Some("llama3.3".to_string()),
                ..ConfigUpdate::default()
            },
        )
        .unwrap();

        let profile = config.profiles.get("local").unwrap();
        assert_eq!(
            profile.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(profile.model.as_deref(), Some("llama3.3"));
        assert_eq!(profile.api_key.as_deref(), Some("local"));
    }

    #[test]
    fn profile_update_rejects_search_config() {
        let mut config = Config::default();
        let err = apply_config_update(
            &mut config,
            ConfigUpdate {
                profile: Some("local".to_string()),
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("llama3.2".to_string()),
                search_provider: Some(SearchProvider::Exa),
                ..ConfigUpdate::default()
            },
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("--profile cannot be combined with search config"));
    }

    #[test]
    fn selected_profile_uses_named_config() {
        let mut config = Config {
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            api_key: Some("openai-key".to_string()),
            search_provider: None,
            exa_api_key: None,
            brave_api_key: None,
            profiles: BTreeMap::new(),
            pricing: BTreeMap::new(),
        };
        config.profiles.insert(
            "local".to_string(),
            ProfileConfig {
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("llama3.2".to_string()),
                api_key: Some("local".to_string()),
            },
        );

        let selected = selected_model_config(&config, Some("local".to_string())).unwrap();

        assert_eq!(
            selected.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(selected.model.as_deref(), Some("llama3.2"));
        assert_eq!(selected.api_key.as_deref(), Some("local"));
    }

    #[test]
    fn selected_profile_rejects_missing_profile() {
        let config = Config::default();
        let err = selected_model_config(&config, Some("missing".to_string()))
            .unwrap_err()
            .to_string();

        assert!(err.contains("profile 'missing' not found"));
    }

    #[test]
    fn selected_profile_rejects_invalid_profile_name() {
        let config = Config::default();
        let err = selected_model_config(&config, Some("bad/name".to_string()))
            .unwrap_err()
            .to_string();

        assert!(err.contains("invalid profile name"));
    }

    #[test]
    fn config_serializes_profiles_table() {
        let mut config = Config::default();
        config.profiles.insert(
            "local".to_string(),
            ProfileConfig {
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("llama3.2".to_string()),
                api_key: Some("local".to_string()),
            },
        );

        let text = toml::to_string_pretty(&config).unwrap();

        assert!(text.contains("[profiles.local]"));
        assert!(text.contains("base_url = \"http://localhost:11434/v1\""));
        assert!(text.contains("model = \"llama3.2\""));
        assert!(text.contains("api_key = \"local\""));
    }

    #[test]
    fn config_reads_profiles_table() {
        let config: Config = toml::from_str(
            r#"
[profiles.local]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
api_key = "local"
"#,
        )
        .unwrap();

        let profile = config.profiles.get("local").unwrap();
        assert_eq!(
            profile.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(profile.model.as_deref(), Some("llama3.2"));
        assert_eq!(profile.api_key.as_deref(), Some("local"));
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
            search_provider: Some(SearchProvider::Exa),
            exa_api_key: Some("exa-key".to_string()),
            brave_api_key: Some("brave-key".to_string()),
            profiles: BTreeMap::new(),
            pricing: BTreeMap::new(),
        })
        .unwrap();

        assert!(text.contains("base_url = \"https://api.openai.com/v1\""));
        assert!(text.contains("model = \"gpt-4.1-mini\""));
        assert!(text.contains("search_provider = \"exa\""));
        assert!(text.contains("exa_api_key = \"exa-key\""));
        assert!(text.contains("brave_api_key = \"brave-key\""));
        assert!(!text.lines().any(|line| line.starts_with("api_key = ")));
    }

    #[test]
    fn config_reads_brave_api_key_field() {
        let config: Config = toml::from_str(
            r#"
search_provider = "exa"
exa_api_key = "exa-key"
brave_api_key = "brave-key"
"#,
        )
        .unwrap();

        assert_eq!(config.search_provider, Some(SearchProvider::Exa));
        assert_eq!(config.exa_api_key.as_deref(), Some("exa-key"));
        assert_eq!(config.brave_api_key.as_deref(), Some("brave-key"));
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
        assert!(help.contains("--profile <profile>"));
        assert!(help.contains("env: LLM_MODEL"));
        assert!(help.contains("--base-url <base-url>"));
        assert!(help.contains("provider API base URL"));
        assert!(help.contains("--api-key <api-key>"));
        assert!(help.contains("--search-provider <provider>"));
        assert!(help.contains("--exa-api-key <api-key>"));
        assert!(help.contains("--no-render"));
        assert!(help.contains("--system <system-prompt>"));
        assert!(help.contains("示例 (Examples):"));
        assert!(help.contains("llm -p local \"Draft quickly\""));
        assert!(help.contains("llm --no-render \"Write markdown\""));
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
        assert!(help.contains("--profile <profile>"));
        assert!(help.contains("--base-url <base-url>"));
        assert!(help.contains("provider API base URL"));
        assert!(help.contains("--model <model>"));
        assert!(help.contains("--api-key <api-key>"));
        assert!(help.contains("--search-provider <provider>"));
        assert!(help.contains("--exa-api-key <api-key>"));
        assert!(help.contains("--brave-api-key <api-key>"));
        assert!(help.contains("默认 model。"));
        assert!(help.contains("本地 LLM server"));
        assert!(help.contains("示例 (Examples):"));
        assert!(
            help.contains("llm config --base-url https://api.openai.com/v1 --model gpt-4.1-mini")
        );
        assert!(help.contains("llm config --profile local --base-url http://localhost:11434/v1 --model llama3.2 --api-key local"));
        assert!(help.contains("llm config --model deepseek-v4"));
        assert!(help.contains("llm config --search-provider exa"));
        assert!(help.contains("llm config --exa-api-key \"$EXA_API_KEY\""));
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
        let mut output = StreamOutput::new(false);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                &bytes[..split_inside_token],
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        assert!(out.is_empty());
        assert!(
            !write_stream_bytes(
                &mut pending,
                &bytes[split_inside_token..],
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );

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
        let mut output = StreamOutput::new(false);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                event.as_bytes(),
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        assert!(
            write_stream_bytes(
                &mut pending,
                b"data: [DONE]\n",
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        output.finish(&mut out).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "hello\n");
    }

    #[test]
    fn rendered_stream_buffers_until_done() {
        let event = format!(
            "data: {}\n",
            serde_json::json!({ "choices": [{ "delta": { "content": "## Title" } }] })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();
        let mut output = StreamOutput::new(true);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                event.as_bytes(),
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        assert!(out.is_empty());
        assert!(
            write_stream_bytes(
                &mut pending,
                b"data: [DONE]\n",
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        output.finish(&mut out).unwrap();

        assert!(String::from_utf8(out).unwrap().contains("Title"));
    }

    #[test]
    fn stream_usage_chunk_updates_usage_without_stdout() {
        let event = format!(
            "data: {}\n",
            serde_json::json!({
                "choices": [],
                "usage": {
                    "prompt_tokens": 1234,
                    "completion_tokens": 567,
                }
            })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();
        let mut output = StreamOutput::new(false);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                event.as_bytes(),
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );

        assert!(out.is_empty());
        assert_eq!(
            usage,
            Some(Usage {
                prompt_tokens: 1234,
                completion_tokens: 567,
            })
        );
    }

    #[test]
    fn stream_chunk_can_include_text_and_usage() {
        let event = format!(
            "data: {}\n",
            serde_json::json!({
                "choices": [{ "delta": { "content": "hello" } }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 2,
                }
            })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();
        let mut output = StreamOutput::new(false);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                event.as_bytes(),
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );

        assert_eq!(String::from_utf8(out).unwrap(), "hello");
        assert_eq!(
            usage,
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
            })
        );
    }

    #[test]
    fn finish_stream_processes_last_line_without_newline() {
        let event = format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "content": "last" } }] })
        );
        let mut pending = Vec::new();
        let mut out = Vec::new();
        let mut output = StreamOutput::new(false);
        let mut usage = None;

        assert!(
            !write_stream_bytes(
                &mut pending,
                event.as_bytes(),
                &mut out,
                &mut output,
                &mut usage,
            )
            .unwrap()
        );
        finish_stream(&mut pending, &mut out, &mut output, &mut usage).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "last\n");
    }
}
