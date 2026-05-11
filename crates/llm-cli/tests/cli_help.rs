use std::process::Command;

fn llm(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_llm"))
        .args(args)
        .output()
        .expect("run llm");
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn top_level_help_matches_cli_contract() {
    let help = llm(&["-h"]);

    assert!(help.contains("极简 LLM CLI"));
    assert!(help.contains("用法 (Usage):"));
    assert!(help.contains("llm [OPTIONS] [prompt]... [COMMAND]"));
    assert!(help.contains("命令 (Commands):"));
    assert!(help.contains("config  写入配置文件。"));
    assert!(help.contains("models  列出 /models 返回的模型。"));
    assert!(help.contains("参数 (Arguments):"));
    assert!(help.contains("选项 (Options):"));
    assert!(help.contains("示例 (Examples):"));
    assert!(help.contains("llm --no-render \"写一段 Markdown\""));
}

#[test]
fn config_help_matches_cli_contract() {
    let help = llm(&["config", "-h"]);

    assert!(help.contains("写入配置文件。"));
    assert!(help.contains("llm config [OPTIONS]"));
    assert!(help.contains("--profile <profile>"));
    assert!(help.contains("--base-url <base-url>"));
    assert!(help.contains("--model <model>"));
    assert!(help.contains("--api-key <api-key>"));
    assert!(help.contains("--search-provider <provider>"));
    assert!(help.contains("示例 (Examples):"));
}

#[test]
fn models_help_matches_cli_contract() {
    let help = llm(&["models", "-h"]);

    assert!(help.contains("列出 /models 返回的模型。"));
    assert!(help.contains("llm models [OPTIONS]"));
    assert!(help.contains("--profile <profile>"));
    assert!(help.contains("--base-url <base-url>"));
    assert!(help.contains("--api-key <api-key>"));
    assert!(help.contains("显示帮助。"));
}
