use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const DEFAULT_PROVIDER_PRIORITY: [SearchProvider; 2] = [SearchProvider::Exa, SearchProvider::Brave];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SearchProvider {
    Exa,
    Brave,
}

impl fmt::Display for SearchProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchProvider::Exa => f.write_str("exa"),
            SearchProvider::Brave => f.write_str("brave"),
        }
    }
}

#[derive(Debug)]
pub struct SearchCredentials {
    pub provider: SearchProvider,
    pub api_key: String,
}

pub fn resolve_credentials(
    requested_provider: Option<SearchProvider>,
    override_brave_api_key: Option<String>,
    override_exa_api_key: Option<String>,
    config_brave_api_key: Option<String>,
    config_exa_api_key: Option<String>,
) -> Result<SearchCredentials> {
    let override_brave_api_key = normalize_key(override_brave_api_key);
    let override_exa_api_key = normalize_key(override_exa_api_key);
    let config_brave_api_key = normalize_key(config_brave_api_key);
    let config_exa_api_key = normalize_key(config_exa_api_key);

    if let Some(provider) = requested_provider {
        return api_key_for(
            provider,
            override_brave_api_key.or(config_brave_api_key),
            override_exa_api_key.or(config_exa_api_key),
        );
    }

    if let Some(credentials) = choose_by_priority(override_brave_api_key, override_exa_api_key) {
        return Ok(credentials);
    }
    if let Some(credentials) = choose_by_priority(config_brave_api_key, config_exa_api_key) {
        return Ok(credentials);
    }

    Err(anyhow!(
        "missing search API key; configure EXA_API_KEY or BRAVE_SEARCH_API_KEY, or run `llm config --exa-api-key KEY` / `llm config --brave-api-key KEY`"
    ))
}

fn choose_by_priority(
    brave_api_key: Option<String>,
    exa_api_key: Option<String>,
) -> Option<SearchCredentials> {
    for provider in DEFAULT_PROVIDER_PRIORITY {
        let credentials = match provider {
            SearchProvider::Exa => exa_api_key
                .clone()
                .map(|api_key| SearchCredentials { provider, api_key }),
            SearchProvider::Brave => brave_api_key
                .clone()
                .map(|api_key| SearchCredentials { provider, api_key }),
        };
        if credentials.is_some() {
            return credentials;
        }
    }
    None
}

fn api_key_for(
    provider: SearchProvider,
    brave_api_key: Option<String>,
    exa_api_key: Option<String>,
) -> Result<SearchCredentials> {
    let api_key = match provider {
        SearchProvider::Exa => exa_api_key.ok_or_else(|| {
            anyhow!(
                "missing Exa API key; pass `--exa-api-key`, set EXA_API_KEY, or run `llm config --exa-api-key KEY`"
            )
        })?,
        SearchProvider::Brave => brave_api_key.ok_or_else(|| {
            anyhow!(
                "missing Brave Search API key; pass `--brave-api-key`, set BRAVE_SEARCH_API_KEY, or run `llm config --brave-api-key KEY`"
            )
        })?,
    };

    Ok(SearchCredentials { provider, api_key })
}

fn normalize_key(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn search_instruction_from_prompt_arg(prompt_arg: &str) -> Result<String> {
    let instruction = prompt_arg.trim();
    if instruction.is_empty() {
        return Err(anyhow!(
            "missing search instruction; use `llm --search \"question\"`"
        ));
    }
    Ok(instruction.to_string())
}

pub fn build_search_query(stdin: Option<&str>, instruction: &str) -> String {
    const MAX_QUERY_CHARS: usize = 400;
    let mut parts = Vec::new();
    if let Some(context) = stdin.map(compact_for_search_query)
        && !context.is_empty()
    {
        parts.push(context);
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

pub async fn fetch_search_context(
    provider: SearchProvider,
    api_key: &str,
    query: &str,
) -> Result<String> {
    match provider {
        SearchProvider::Exa => {
            let response = fetch_exa_search(api_key, query).await?;
            format_exa_search_context(&response)
        }
        SearchProvider::Brave => {
            let response = fetch_brave_search(api_key, query).await?;
            format_brave_search_context(&response)
        }
    }
}

async fn fetch_brave_search(api_key: &str, query: &str) -> Result<BraveSearchResponse> {
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
        return Err(provider_status_error(
            "Brave Search",
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

async fn fetch_exa_search(api_key: &str, query: &str) -> Result<ExaSearchResponse> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Exa client")?
        .post("https://api.exa.ai/search")
        .header("Accept", "application/json")
        .header("x-api-key", api_key)
        .json(&ExaSearchRequest {
            query,
            search_type: "auto",
            num_results: 10,
            contents: ExaContentsRequest { highlights: true },
        })
        .send()
        .await
        .context("Exa search request failed")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_status_error("Exa", status, None, &body));
    }

    response
        .json()
        .await
        .context("failed to parse Exa response JSON")
}

fn provider_status_error(
    provider: &str,
    status: StatusCode,
    rate_limit_reset: Option<&str>,
    body: &str,
) -> anyhow::Error {
    let mut message = format!("{provider} returned HTTP {status}");
    if status == StatusCode::TOO_MANY_REQUESTS
        && let Some(reset) = rate_limit_reset
    {
        message.push_str(&format!("; rate limit resets in {reset}s"));
    }

    let body = body.trim();
    if !body.is_empty() {
        message.push_str(": ");
        message.push_str(&format_error_body(body));
    }

    anyhow!(message)
}

fn format_error_body(body: &str) -> String {
    let parsed = serde_json::from_str::<ProviderErrorResponse>(body);
    if let Ok(error_response) = parsed {
        if let Some(error) = error_response.error {
            match error {
                serde_json::Value::String(message) if !message.trim().is_empty() => return message,
                serde_json::Value::Object(map) => {
                    let code = map.get("code").and_then(|value| value.as_str());
                    let detail = map
                        .get("detail")
                        .or_else(|| map.get("message"))
                        .and_then(|value| value.as_str());
                    match (code, detail) {
                        (Some(code), Some(detail)) => return format!("{code}: {detail}"),
                        (Some(code), None) => return code.to_string(),
                        (None, Some(detail)) => return detail.to_string(),
                        (None, None) => {}
                    }
                }
                _ => {}
            }
        }
        if let Some(message) = error_response
            .message
            .filter(|message| !message.trim().is_empty())
        {
            return message;
        }
        if let Some(detail) = error_response
            .detail
            .filter(|detail| !detail.trim().is_empty())
        {
            return detail;
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

fn format_brave_search_context(response: &BraveSearchResponse) -> Result<String> {
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

            let mut snippets = Vec::new();
            if let Some(description) = result.description.as_deref().map(str::trim)
                && !description.is_empty()
            {
                snippets.push(description.to_string());
            }
            snippets.extend(
                result
                    .extra_snippets
                    .iter()
                    .map(|snippet| snippet.trim())
                    .filter(|snippet| !snippet.is_empty())
                    .map(str::to_string),
            );

            Some(SearchResultContext {
                title: title.to_string(),
                url: url.to_string(),
                snippets,
            })
        })
        .collect::<Vec<_>>();

    format_search_results("Brave Search", results)
}

fn format_exa_search_context(response: &ExaSearchResponse) -> Result<String> {
    let results = response
        .results
        .iter()
        .filter_map(|result| {
            let title = result.title.as_deref()?.trim();
            let url = result.url.as_deref()?.trim();
            if title.is_empty() || url.is_empty() {
                return None;
            }

            let mut snippets = result
                .highlights
                .iter()
                .map(|snippet| snippet.trim())
                .filter(|snippet| !snippet.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if snippets.is_empty()
                && let Some(text) = result.text.as_deref().map(str::trim)
                && !text.is_empty()
            {
                snippets.push(truncate_chars(text, 500));
            }

            Some(SearchResultContext {
                title: title.to_string(),
                url: url.to_string(),
                snippets,
            })
        })
        .collect::<Vec<_>>();

    format_search_results("Exa", results)
}

fn format_search_results(provider: &str, results: Vec<SearchResultContext>) -> Result<String> {
    if results.is_empty() {
        return Err(anyhow!("{provider} returned no search results"));
    }

    let mut context = String::from("<search_context>\n");
    context.push_str(&format!("Provider: {provider}\n\n"));
    for (idx, result) in results.iter().enumerate() {
        context.push_str(&format!("Source {}:\n", idx + 1));
        context.push_str(&format!("Title: {}\n", result.title));
        context.push_str(&format!("URL: {}\n", result.url));
        context.push_str("Snippets:\n");

        if result.snippets.is_empty() {
            context.push_str("- [No snippet provided]\n");
        } else {
            for snippet in &result.snippets {
                context.push_str("- ");
                context.push_str(snippet);
                context.push('\n');
            }
        }
        context.push('\n');
    }
    context.push_str("</search_context>");
    Ok(context)
}

pub fn build_prompt_with_search(
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

struct SearchResultContext {
    title: String,
    url: String,
    snippets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderErrorResponse {
    error: Option<serde_json::Value>,
    message: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExaSearchRequest<'a> {
    query: &'a str,
    #[serde(rename = "type")]
    search_type: &'static str,
    #[serde(rename = "numResults")]
    num_results: u8,
    contents: ExaContentsRequest,
}

#[derive(Debug, Serialize)]
struct ExaContentsRequest {
    highlights: bool,
}

#[derive(Debug, Deserialize)]
struct ExaSearchResponse {
    #[serde(default)]
    results: Vec<ExaSearchResult>,
}

#[derive(Debug, Deserialize)]
struct ExaSearchResult {
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    highlights: Vec<String>,
    text: Option<String>,
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
