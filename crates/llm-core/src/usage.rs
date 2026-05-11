use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct ModelPricing {
    pub input_per_1m: Option<f64>,
    pub output_per_1m: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug)]
pub struct UsageSummary {
    pub usage: Usage,
    pub cost: Option<f64>,
}

pub fn build_usage_summary(usage: Usage, pricing: Option<ModelPricing>) -> UsageSummary {
    UsageSummary {
        usage,
        cost: pricing.and_then(|pricing| calculate_cost(usage, pricing)),
    }
}

pub fn calculate_cost(usage: Usage, pricing: ModelPricing) -> Option<f64> {
    let input_cost = pricing.input_per_1m? * usage.prompt_tokens as f64 / 1_000_000.0;
    let output_cost = pricing.output_per_1m? * usage.completion_tokens as f64 / 1_000_000.0;
    Some(input_cost + output_cost)
}

pub fn write_usage_summary<W: Write>(summary: &UsageSummary, out: &mut W) -> Result<()> {
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
