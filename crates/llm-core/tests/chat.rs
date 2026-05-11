use llm_core::chat::{ChatRequest, StreamOptions};

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
