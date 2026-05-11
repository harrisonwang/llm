use llm_core::models::{ModelInfo, models_url, write_models};

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
