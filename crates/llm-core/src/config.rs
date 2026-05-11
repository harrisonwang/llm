use crate::search::SearchProvider;
use crate::usage::ModelPricing;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub search_provider: Option<SearchProvider>,
    pub exa_api_key: Option<String>,
    pub brave_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfileConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pricing: BTreeMap<String, ModelPricing>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProfileConfig {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Default)]
pub struct ConfigUpdate {
    pub profile: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub search_provider: Option<SearchProvider>,
    pub exa_api_key: Option<String>,
    pub brave_api_key: Option<String>,
}

pub fn read_config() -> Result<Config> {
    read_config_from(&default_config_path()?)
}

pub fn read_config_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read config: {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse config: {}", path.display()))
}

pub fn write_config(config: Config) -> Result<()> {
    let path = default_config_path()?;
    write_config_to(&path, &config)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

pub fn write_config_to(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(path, text).with_context(|| format!("failed to write config: {}", path.display()))
}

pub fn selected_model_config(config: &Config, profile: Option<String>) -> Result<ProfileConfig> {
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

pub fn apply_config_update(config: &mut Config, update: ConfigUpdate) -> Result<()> {
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

pub fn validate_profile_name(profile: &str) -> Result<String> {
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

pub fn default_config_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".llm").join("config.toml"))
}
