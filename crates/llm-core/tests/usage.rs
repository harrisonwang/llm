use llm_core::usage::{ModelPricing, Usage, build_usage_summary, write_usage_summary};

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
