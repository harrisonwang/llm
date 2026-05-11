use llm_core::search::{
    SearchProvider, build_prompt_with_search, build_search_query, resolve_credentials,
    search_instruction_from_prompt_arg,
};

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
fn credentials_prefer_exa_over_brave_when_provider_is_unspecified() {
    let credentials = resolve_credentials(
        None,
        Some("brave-key".to_string()),
        Some("exa-key".to_string()),
        None,
        None,
    )
    .unwrap();

    assert_eq!(credentials.provider, SearchProvider::Exa);
    assert_eq!(credentials.api_key, "exa-key");
}

#[test]
fn credentials_fall_back_to_brave_when_exa_key_is_missing() {
    let credentials =
        resolve_credentials(None, Some("brave-key".to_string()), None, None, None).unwrap();

    assert_eq!(credentials.provider, SearchProvider::Brave);
    assert_eq!(credentials.api_key, "brave-key");
}

#[test]
fn credentials_prefer_override_key_tier_over_config_priority() {
    let credentials = resolve_credentials(
        None,
        Some("override-brave-key".to_string()),
        None,
        None,
        Some("config-exa-key".to_string()),
    )
    .unwrap();

    assert_eq!(credentials.provider, SearchProvider::Brave);
    assert_eq!(credentials.api_key, "override-brave-key");
}

#[test]
fn credentials_honor_explicit_provider() {
    let credentials = resolve_credentials(
        Some(SearchProvider::Brave),
        Some("brave-key".to_string()),
        Some("exa-key".to_string()),
        None,
        None,
    )
    .unwrap();

    assert_eq!(credentials.provider, SearchProvider::Brave);
    assert_eq!(credentials.api_key, "brave-key");
}

#[test]
fn credentials_require_key_for_explicit_provider() {
    let err = resolve_credentials(
        Some(SearchProvider::Exa),
        Some("brave-key".to_string()),
        None,
        None,
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("missing Exa API key"));
}

#[test]
fn credentials_require_at_least_one_search_key() {
    let err = resolve_credentials(None, None, None, None, None)
        .unwrap_err()
        .to_string();

    assert!(err.contains("missing search API key"));
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
