use std::io::Write;
use std::process::{Command, Stdio};

fn llm(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_llm"))
        .args(args)
        .output()
        .expect("run llm")
}

fn assert_success_with_stdout(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.trim().is_empty());
    stdout
}

#[test]
#[ignore = "uses the configured real LLM provider"]
fn real_prompt_streams_by_default() {
    let output = llm(&["用一句话解释 TCP 三次握手"]);

    let stdout = assert_success_with_stdout(output);
    assert!(stdout.contains("TCP") || stdout.contains("握手"));
}

#[test]
#[ignore = "uses the configured real LLM provider"]
fn real_prompt_supports_no_stream() {
    let output = llm(&["--no-stream", "--no-render", "只输出：pong"]);

    let stdout = assert_success_with_stdout(output);
    assert!(stdout.to_lowercase().contains("pong"));
}

#[test]
#[ignore = "uses the configured real LLM provider"]
fn real_prompt_wraps_piped_stdin_as_context() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_llm"))
        .args(["--no-stream", "--no-render", "只输出上下文中的项目代号"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn llm");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all("项目代号：ORANGE-17\n".as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();

    let stdout = assert_success_with_stdout(output);
    assert!(stdout.contains("ORANGE-17"));
}

#[test]
#[ignore = "uses the configured real LLM provider"]
fn real_models_lists_available_models() {
    let output = llm(&["models"]);

    assert_success_with_stdout(output);
}

#[test]
#[ignore = "uses configured real search and LLM providers"]
fn real_search_answers_with_sources() {
    let output = llm(&[
        "--search",
        "--no-stream",
        "--no-render",
        "Rust 当前 stable 版本是什么？用一句话回答",
    ]);

    let stdout = assert_success_with_stdout(output);
    assert!(stdout.contains("Source") || stdout.contains("来源") || stdout.contains("http"));
}
