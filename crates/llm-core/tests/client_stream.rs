use llm_core::client::{finish_stream, write_stream_bytes};
use llm_core::render::StreamOutput;
use llm_core::usage::Usage;

#[test]
fn stream_bytes_preserve_utf8_split_across_chunks() {
    let token = "世界";
    let event = format!(
        "data: {}\n",
        serde_json::json!({ "choices": [{ "delta": { "content": token } }] })
    );
    let bytes = event.into_bytes();
    let token_start = bytes
        .windows("世".len())
        .position(|window| window == "世".as_bytes())
        .unwrap();
    let split_inside_token = token_start + 1;

    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(false);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            &bytes[..split_inside_token],
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    assert!(out.is_empty());
    assert!(
        !write_stream_bytes(
            &mut pending,
            &bytes[split_inside_token..],
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );

    assert_eq!(String::from_utf8(out).unwrap(), token);
}

#[test]
fn stream_done_writes_trailing_newline() {
    let event = format!(
        "data: {}\n",
        serde_json::json!({ "choices": [{ "delta": { "content": "hello" } }] })
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(false);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            event.as_bytes(),
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    assert!(
        write_stream_bytes(
            &mut pending,
            b"data: [DONE]\n",
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    output.finish(&mut out).unwrap();

    assert_eq!(String::from_utf8(out).unwrap(), "hello\n");
}

#[test]
fn rendered_stream_buffers_until_done() {
    let event = format!(
        "data: {}\n",
        serde_json::json!({ "choices": [{ "delta": { "content": "## Title" } }] })
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(true);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            event.as_bytes(),
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    assert!(out.is_empty());
    assert!(
        write_stream_bytes(
            &mut pending,
            b"data: [DONE]\n",
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    output.finish(&mut out).unwrap();

    assert!(String::from_utf8(out).unwrap().contains("Title"));
}

#[test]
fn stream_usage_chunk_updates_usage_without_stdout() {
    let event = format!(
        "data: {}\n",
        serde_json::json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 1234,
                "completion_tokens": 567,
            }
        })
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(false);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            event.as_bytes(),
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );

    assert!(out.is_empty());
    assert_eq!(
        usage,
        Some(Usage {
            prompt_tokens: 1234,
            completion_tokens: 567,
        })
    );
}

#[test]
fn stream_chunk_can_include_text_and_usage() {
    let event = format!(
        "data: {}\n",
        serde_json::json!({
            "choices": [{ "delta": { "content": "hello" } }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 2,
            }
        })
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(false);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            event.as_bytes(),
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );

    assert_eq!(String::from_utf8(out).unwrap(), "hello");
    assert_eq!(
        usage,
        Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 2,
        })
    );
}

#[test]
fn finish_stream_processes_last_line_without_newline() {
    let event = format!(
        "data: {}",
        serde_json::json!({ "choices": [{ "delta": { "content": "last" } }] })
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut output = StreamOutput::new(false);
    let mut usage = None;

    assert!(
        !write_stream_bytes(
            &mut pending,
            event.as_bytes(),
            &mut out,
            &mut output,
            &mut usage,
        )
        .unwrap()
    );
    finish_stream(&mut pending, &mut out, &mut output, &mut usage).unwrap();

    assert_eq!(String::from_utf8(out).unwrap(), "last\n");
}
