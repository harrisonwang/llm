use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};
use llm_core::attachments::{build_user_message_content, read_image_attachments};
use llm_core::chat::{ChatRequest, Message, MessageContent, StreamOptions};
use llm_core::client::{LlmClient, complete_chat_with_output, stream_chat_with_output};
use llm_core::config::{
    ConfigUpdate, apply_config_update, read_config, selected_model_config, write_config,
};
use llm_core::models::write_models;
use llm_core::render::should_render_for;
use llm_core::search::{self, SearchProvider};
use std::env;
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
  llm \"解释 TCP 三次握手\"
  llm -m gpt-4.1-mini \"总结这段内容\"
  llm -p local \"快速起草一封邮件\"
  llm models
  llm models -p talkweb
  llm -a screenshot.png \"这个界面哪里有问题？\"
  llm --no-render \"写一段 Markdown\"
  cat report.md | llm \"总结风险和行动项\"
  EXA_API_KEY=... llm --search \"Rust 2026 edition 有哪些变化\"{after-help}";

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
pub struct Cli {
    /// 用户 prompt；如果 stdin 有输入，则 stdin 作为上下文，prompt 作为指令。
    #[arg(value_name = "prompt")]
    pub prompt: Vec<String>,

    /// 使用的 model；覆盖配置文件和 env: LLM_MODEL。
    #[arg(short, long, env = "LLM_MODEL", hide_env = true, value_name = "model")]
    model: Option<String>,

    /// 使用命名 profile；不传则使用默认配置。
    #[arg(short = 'p', long, value_name = "profile")]
    pub profile: Option<String>,

    /// provider API base URL；env: LLM_BASE_URL。
    #[arg(long, env = "LLM_BASE_URL", hide_env = true, value_name = "base-url")]
    base_url: Option<String>,

    /// API key；覆盖配置文件和 env: LLM_API_KEY。
    #[arg(long, env = "LLM_API_KEY", hide_env = true, value_name = "api-key")]
    api_key: Option<String>,

    /// system prompt。
    #[arg(short, long, value_name = "system-prompt")]
    system: Option<String>,

    /// 附加图片文件；PDF 请先用 pith 转文本后通过 stdin 传入。
    #[arg(short = 'a', long = "attach", value_name = "path")]
    pub attachments: Vec<PathBuf>,

    /// 使用搜索 API 获取搜索上下文；只使用命令行 prompt 作为搜索 query。
    #[arg(long)]
    pub search: bool,

    /// 搜索 provider；可选值: exa, brave；env: SEARCH_PROVIDER。
    #[arg(
        long,
        env = "SEARCH_PROVIDER",
        hide_env = true,
        value_enum,
        value_name = "provider"
    )]
    pub search_provider: Option<SearchProvider>,

    /// Exa API key；覆盖配置文件和 env: EXA_API_KEY。
    #[arg(long, env = "EXA_API_KEY", hide_env = true, value_name = "api-key")]
    pub exa_api_key: Option<String>,

    /// Brave Search API key；覆盖配置文件和 env: BRAVE_SEARCH_API_KEY。
    #[arg(
        long,
        env = "BRAVE_SEARCH_API_KEY",
        hide_env = true,
        value_name = "api-key"
    )]
    pub brave_api_key: Option<String>,

    /// streaming 输出；默认开启。
    #[arg(long, conflicts_with = "no_stream")]
    stream: bool,

    /// 关闭 streaming，等待完整响应后输出。
    #[arg(long)]
    no_stream: bool,

    /// 关闭 TTY Markdown 渲染，始终输出原始文本。
    #[arg(long)]
    pub no_render: bool,

    /// 显示帮助。
    #[arg(short = 'h', long = "help", action = ArgAction::Help, global = true)]
    help: Option<bool>,

    /// 显示版本。
    #[arg(short = 'V', long = "version", action = ArgAction::Version)]
    version: Option<bool>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
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

pub async fn run_cli() -> Result<()> {
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

    let has_attachments = !cli.attachments.is_empty();
    let prompt = if let Some((query, instruction, credentials)) = search_options {
        let mut stderr = io::stderr();
        write_search_provider_notice(credentials.provider, &mut stderr)?;
        let search_context =
            search::fetch_search_context(credentials.provider, &credentials.api_key, &query)
                .await?;
        search::build_prompt_with_search(stdin, &search_context, &instruction)
    } else {
        build_prompt(stdin, prompt_arg, has_attachments)?
    };

    let mut messages = Vec::new();
    if let Some(system) = cli.system {
        messages.push(Message {
            role: "system".to_string(),
            content: MessageContent::Text(system),
        });
    }
    let attachments = read_image_attachments(&cli.attachments)?;
    messages.push(Message {
        role: "user".to_string(),
        content: build_user_message_content(prompt, attachments),
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
        stream_chat_with_output(&base_url, &api_key, &request, false, pricing).await
    } else {
        complete_chat_with_output(&base_url, &api_key, &request, render, pricing).await
    }
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
    let models = LlmClient::new(base_url, api_key)?.models().await?;
    let mut stdout = io::stdout();
    write_models(&models, &mut stdout)
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

pub fn build_prompt(
    stdin: Option<String>,
    prompt_arg: String,
    has_attachments: bool,
) -> Result<String> {
    let prompt_arg = prompt_arg.trim().to_string();
    match (stdin, prompt_arg.is_empty()) {
        (Some(context), false) => Ok(format!(
            "<context>\n{}\n</context>\n\nInstruction:\n{}",
            context.trim_end(),
            prompt_arg
        )),
        (Some(context), true) => Ok(context),
        (None, false) => Ok(prompt_arg),
        (None, true) if has_attachments => Ok("Describe the attached image.".to_string()),
        (None, true) => Err(anyhow!(
            "missing prompt; try `llm \"hello\"` or pipe text into `llm`"
        )),
    }
}

pub fn write_search_provider_notice<W: Write>(provider: SearchProvider, out: &mut W) -> Result<()> {
    writeln!(out, "search provider: {provider}").context("failed to write search provider notice")
}

fn should_render(no_render: bool) -> bool {
    should_render_for(no_render, io::stdout().is_terminal())
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
