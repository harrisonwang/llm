use clap::Parser;
use llm_cli::cli::{Cli, Command, build_prompt, write_search_provider_notice};
use llm_core::search::SearchProvider;
use std::path::PathBuf;

#[test]
fn build_prompt_wraps_pith_context_with_instruction() {
    let prompt = build_prompt(
        Some("# Report\n\nA finding from pith.\n".to_string()),
        "总结风险和行动项".to_string(),
        false,
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
fn attachment_flag_parses() {
    let cli = Cli::try_parse_from(["llm", "-a", "screenshot.png", "检查 UI"]).unwrap();

    assert_eq!(cli.attachments, [PathBuf::from("screenshot.png")]);
    assert_eq!(cli.prompt, ["检查 UI"]);
}

#[test]
fn repeated_attachment_flags_parse() {
    let cli = Cli::try_parse_from([
        "llm",
        "--attach",
        "one.png",
        "-a",
        "two.jpg",
        "比较这两张图",
    ])
    .unwrap();

    assert_eq!(
        cli.attachments,
        [PathBuf::from("one.png"), PathBuf::from("two.jpg")]
    );
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
fn build_prompt_defaults_when_only_attachment_is_present() {
    let prompt = build_prompt(None, String::new(), true).unwrap();

    assert_eq!(prompt, "Describe the attached image.");
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
    let cli = Cli::try_parse_from(["llm", "--search-provider", "exa", "--search", "Rust"]).unwrap();

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
