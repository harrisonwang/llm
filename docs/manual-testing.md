# llm 手动测试命令

这份清单用于在本机已配置好 `base_url`、`model`、`api_key` 和搜索 key 后，手动验证 `llm` CLI 的主要用户路径。

## 基础帮助

```bash
cargo run -p llm-cli --locked -- -h
cargo run -p llm-cli --locked -- config -h
cargo run -p llm-cli --locked -- models -h
```

## 普通对话

```bash
cargo run -p llm-cli --locked -- "用一句话解释 TCP 三次握手"
cargo run -p llm-cli --locked -- --no-stream --no-render "只输出：pong"
cargo run -p llm-cli --locked -- --no-render "写一段 Markdown，主题是 Rust 所有权"
```

## stdin 上下文

```bash
printf '项目代号：ORANGE-17\n风险：发布时间紧张\n' | cargo run -p llm-cli --locked -- --no-stream --no-render "只输出项目代号和风险"
cat README.md | cargo run -p llm-cli --locked -- --no-stream --no-render "总结这个项目的用途"
```

## 模型列表

```bash
cargo run -p llm-cli --locked -- models
cargo run -p llm-cli --locked -- models -p local
```

## 配置写入

以下命令会修改 `~/.llm/config.toml`。如需避免影响当前配置，先设置临时 HOME：

```bash
LLM_TEST_HOME="$(mktemp -d)"
HOME="$LLM_TEST_HOME" cargo run -p llm-cli --locked -- config --base-url http://localhost:11434/v1 --model llama3.2 --api-key local
HOME="$LLM_TEST_HOME" cargo run -p llm-cli --locked -- config --profile local --base-url http://localhost:11434/v1 --model llama3.2 --api-key local
HOME="$LLM_TEST_HOME" cargo run -p llm-cli --locked -- config --search-provider exa
HOME="$LLM_TEST_HOME" cargo run -p llm-cli --locked -- config --exa-api-key "$EXA_API_KEY"
HOME="$LLM_TEST_HOME" cargo run -p llm-cli --locked -- config --brave-api-key "$BRAVE_SEARCH_API_KEY"
cat "$LLM_TEST_HOME/.llm/config.toml"
```

## 搜索

```bash
cargo run -p llm-cli --locked -- --search --no-stream --no-render "Rust 当前 stable 版本是什么？用一句话回答并附来源"
cargo run -p llm-cli --locked -- --search --search-provider exa --no-stream --no-render "Claude Code 最新版本有什么变化？"
cargo run -p llm-cli --locked -- --search --search-provider brave --no-stream --no-render "Rust 2026 edition 有哪些变化？"
printf '关注点：只要和 CLI 相关的信息\n' | cargo run -p llm-cli --locked -- --search --no-stream --no-render "Claude Code CLI 有哪些最新能力？"
```

## 图片附件

将 `screenshot.png` 替换成本机图片路径：

```bash
cargo run -p llm-cli --locked -- -a screenshot.png --no-stream --no-render "这个界面哪里有问题？"
cargo run -p llm-cli --locked -- -a screenshot.png "描述这张图片"
```

## 已自动化的真实命令测试

以下测试会调用真实 provider，默认被 `#[ignore]` 跳过：

```bash
cargo test -p llm-cli --test cli_commands --locked
cargo test -p llm-cli --test cli_commands --locked -- --ignored --test-threads=1
```

第一条只编译测试，不调用真实 API；第二条会执行真实 `llm` 命令路径。
