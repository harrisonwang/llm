use llm_core::config::{
    Config, ConfigUpdate, ProfileConfig, apply_config_update, default_config_path,
    selected_model_config,
};
use llm_core::search::SearchProvider;
use llm_core::usage::ModelPricing;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

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

    assert_eq!(default_config_path().unwrap(), expected);
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
