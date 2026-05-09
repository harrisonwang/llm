# llm

一个极简 Rust LLM CLI。默认命令就是 prompt：

```bash
llm "解释一下 Rust ownership"
```

## 安装

发布后可以通过 Homebrew tap 安装：

```bash
brew install harrisonwang/tap/llm
```

## 配置

先设置 OpenAI-compatible endpoint：

```bash
llm config \
  --base-url https://api.openai.com/v1 \
  --model gpt-4.1-mini \
  --api-key "$OPENAI_API_KEY"
```

本地 OpenAI-compatible 服务也可以：

```bash
llm config \
  --base-url http://localhost:11434/v1 \
  --model llama3.2 \
  --api-key local
```

也可以配置命名 profile，并在运行时用 `-p/--profile` 选择；不传 `-p` 时继续使用上面的默认配置：

```bash
llm config --profile deepseek --base-url https://api.deepseek.com/v1 --model deepseek-v4 --api-key "$DEEPSEEK_API_KEY"
llm config --profile local --base-url http://localhost:11434/v1 --model llama3.2 --api-key local
llm config --profile openai --base-url https://api.openai.com/v1 --model gpt-4.1-mini --api-key "$OPENAI_API_KEY"

llm -p local "草稿性问题，便宜又快"
llm -p deepseek "写代码"
llm "默认 profile"
```

命名 profile 会写入：

```toml
[profiles.local]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
api_key = "local"
```

配置文件位置：

```text
~/.llm/config.toml
```

首次配置模型调用至少需要 `--base-url` 和 `--model`。配置完整后，可以单独更新某一项；命名 profile 同样适用：

```bash
llm config --model deepseek-v4
llm config --base-url https://api.deepseek.com/v1
llm config --api-key "$DEEPSEEK_API_KEY"
llm config --profile local --model llama3.3
llm config --search-provider exa
llm config --exa-api-key "$EXA_API_KEY"
llm config --brave-api-key "$BRAVE_SEARCH_API_KEY"
```

`--search-provider`、`--exa-api-key`、`--brave-api-key` 是独立搜索配置，也可以在没有模型配置时单独写入。`search_provider` 可选值是 `exa` 和 `brave`；未设置时按 `exa`、`brave` 的顺序自动选择已配置 key 的 provider。

运行时取值优先级是命令行参数、环境变量、配置文件：

```bash
LLM_BASE_URL=https://api.openai.com/v1 \
LLM_MODEL=gpt-4.1-mini \
LLM_API_KEY="$OPENAI_API_KEY" \
llm "hello"
```

搜索模式需要 Exa 或 Brave Search API key：

```bash
llm config --exa-api-key "$EXA_API_KEY"
llm config --brave-api-key "$BRAVE_SEARCH_API_KEY"
```

也可以在单次调用中传入：

```bash
llm --exa-api-key "$EXA_API_KEY" --search "Rust 2026 edition 最新变化"
llm --brave-api-key "$BRAVE_SEARCH_API_KEY" --search "Rust 2026 edition 最新变化"
llm --search-provider brave --search "Rust 2026 edition 最新变化"
```

## 使用

```bash
llm "写三条产品发布文案"
llm -p local "草稿性问题，便宜又快"
llm -m gpt-4.1-mini "解释 TCP 三次握手"
llm -s "你是严谨的代码审查员" "审查这段代码"
llm --search "Rust 2026 edition 最新变化"
```

stdin 会作为上下文，命令参数作为指令：

```bash
pith report.pdf | llm "总结这份报告，列出风险和行动项"
pith report.pdf | llm --stream "总结这份报告，列出风险和行动项"
```

只从 stdin 读取 prompt：

```bash
echo "解释一下 transformer attention" | llm
```

网页内容抓取继续通过其他 CLI 组合：

```bash
pith -h https://example.com | llm "总结这页"
```

`--search` 会用命令行 prompt 作为问题；如果 stdin 有输入，也会把 stdin 的前部一起作为搜索查询，并把 stdin 原文作为模型上下文：

```bash
cargo -V | llm --search "这个版本的cargo有什么特性？"
```

启用 `--search` 时会在 stderr 输出实际使用的 provider，例如 `search provider: exa`；stdout 仍只输出模型回答，方便继续管道处理。

默认使用 streaming。需要等待完整响应时：

```bash
llm --stream "hello"
llm --no-stream "hello"
```

## 当前边界

这是最小实现，暂时只支持 OpenAI-compatible `/chat/completions`：

- 无 chat history
- 无 logs
- 无 template
- 无 agentic tools
- 无 schema mode
- 无 provider 插件

`--search` 当前是一次 Exa 或 Brave Search 预检索，不是模型自主调用工具。

后续可以在不影响 `pith` 的前提下继续扩展。
