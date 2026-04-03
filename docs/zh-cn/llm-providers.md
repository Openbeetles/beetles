# LLM 提供商说明

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

`config/llm.json` 的 LLM 配置规则如下。

内容包括：

1. `provider` 可以填什么？
2. 哪些 provider 的 `api_url` 可以留空？
3. 多个源时，回退顺序到底怎么走？

## 基本规则

- Beetle 支持配置多个 LLM 源
- 配置入口是 `config/llm.json`
- 部分 provider 走 OpenAI 兼容客户端
- 配多个源时，会按顺序组成一条回退链

## 支持的 Provider ID

| Provider ID | 走哪条客户端路径 | 说明 |
|-------------|------------------|------|
| `openai` | OpenAI 兼容客户端 | 标准 OpenAI 路径 |
| `openai_compatible` | OpenAI 兼容客户端 | 通用兼容接口 |
| `gemini` | OpenAI 兼容客户端 | 客户端内部做厂商适配 |
| `glm` | OpenAI 兼容客户端 | 智谱 GLM |
| `qwen` | OpenAI 兼容客户端 | 通义千问 |
| `deepseek` | OpenAI 兼容客户端 | DeepSeek |
| `moonshot` | OpenAI 兼容客户端 | Moonshot |
| `ollama` | OpenAI 兼容客户端 | 本地或自托管 |
| `anthropic` | Anthropic 客户端 | Claude Messages API |

## `api_url` 的关键规则

### 哪些可以留空

下面这些 provider 的 `api_url` 可以为空：

- `openai`
- `openai_compatible`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`

### 哪些不能留空

- `anthropic`
- 任何未知的 provider ID

对 `anthropic` 来说，非空 `api_url` 会被当作完整的 Messages 接口 URL。

## 多源回退顺序

配置多个源后，Beetle 会把它们串成一条有顺序的回退链。

顺序是：

1. `llm_router_source_index`，如果已配置且有效
2. `llm_worker_source_index`，如果已配置、有效，且和 router 源不同
3. 其余所有可用源，按列表顺序追加

行为规则：

- 第一个成功的响应直接返回
- 如果全部失败，就返回最后一次错误

## 最小示例

### 单个源

```json
{
  "llm_sources": [
    {
      "provider": "deepseek",
      "api_key": "sk-...",
      "model": "deepseek-chat",
      "api_url": ""
    }
  ]
}
```

### 多个源回退

```json
{
  "llm_sources": [
    {
      "provider": "gemini",
      "api_key": "AIza...",
      "model": "gemini-1.5-flash",
      "api_url": ""
    },
    {
      "provider": "glm",
      "api_key": "...",
      "model": "glm-4-flash",
      "api_url": ""
    },
    {
      "provider": "ollama",
      "api_key": "ollama",
      "model": "qwen2",
      "api_url": "http://192.168.1.100:11434/v1"
    }
  ]
}
```

## Ollama 说明

- `provider` 用 `ollama`
- `api_url` 通常写成 `http://<主机>:11434/v1`
- 如果服务端不校验密钥，`api_key` 只要是非空占位字符串即可

## 这些规则来自哪里

实现参考：

- 客户端构建逻辑：[`src/llm/mod.rs`](../../src/llm/mod.rs)
- 回退链逻辑：[`src/llm/fallback.rs`](../../src/llm/fallback.rs)
- Anthropic 客户端行为：[`src/llm/anthropic.rs`](../../src/llm/anthropic.rs)

厂商模型名会不断变化，所以这里的模型名只用来举例，不代表固件会长期固定这些名字。
