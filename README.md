# llm

一个极简 Rust LLM CLI。默认命令就是 prompt：

```bash
llm "解释一下 Rust ownership"
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

配置文件位置：

```text
$XDG_CONFIG_HOME/llm/config.toml
~/.config/llm/config.toml
```

也可以用环境变量覆盖：

```bash
LLM_BASE_URL=https://api.openai.com/v1 \
LLM_MODEL=gpt-4.1-mini \
LLM_API_KEY="$OPENAI_API_KEY" \
llm "hello"
```

测试或临时配置可以指定：

```bash
LLM_CONFIG_DIR=/tmp/llm-config llm config --base-url http://localhost:11434/v1 --model llama3.2
```

## 使用

```bash
llm "写三条产品发布文案"
llm -m gpt-4.1-mini "解释 TCP 三次握手"
llm -s "你是严谨的代码审查员" "审查这段代码"
```

stdin 会作为上下文，命令参数作为指令：

```bash
gist report.pdf | llm "总结这份报告，列出风险和行动项"
```

只从 stdin 读取 prompt：

```bash
echo "解释一下 transformer attention" | llm
```

默认使用 streaming。需要等待完整响应时：

```bash
llm --no-stream "hello"
```

## 当前边界

这是最小实现，暂时只支持 OpenAI-compatible `/chat/completions`：

- 无 chat history
- 无 logs
- 无 template
- 无 tools
- 无 schema mode
- 无 provider 插件

后续可以在不影响 `gist` 的前提下继续扩展。
