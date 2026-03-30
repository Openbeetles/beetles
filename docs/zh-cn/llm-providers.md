# LLM 提供商配置

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

在网页配置界面或通过 SPIFFS 上的 `config/llm.json`（亦可通过 [配置 API](config-api.md) 写入）管理多源列表。

## 实现如何选客户端（与厂商文档分开）

固件在 [`build_llm_clients`](../../src/llm/mod.rs) 中按 `provider` 分流：

- **`anthropic`**：走 `AnthropicClient`（Claude Messages API）。
- **`openai`、`openai_compatible`、`gemini`、`glm`、`qwen`、`deepseek`、`moonshot`、`ollama`**：走 **`OpenAiCompatibleClient`**（OpenAI 风格 chat/completions 协议；各厂商 base URL 与鉴权头由该客户端按 provider 处理）。

写入配置时，字段长度校验见 `config` 模块。**能否进入回退链**还受 `build_llm_clients` 过滤影响：`api_url` **可为空**的 provider 为上述 OpenAI 兼容族；**`anthropic` 及任何不在该列表中的标识**在 `api_url` 为空时**不会**加入客户端列表。`AnthropicClient` 对非空 `api_url` 的语义为「完整 Messages 请求 URL」（与代码中默认 `https://api.anthropic.com/v1/messages` 同级），见 [`anthropic.rs`](../../src/llm/anthropic.rs)。

**多源回退**（[`FallbackLlmClient`](../../src/llm/fallback.rs)）：worker 内按 [`llm_fallback_source_indices`](../../src/llm/mod.rs) 算出的顺序依次调用，**首次成功即返回**；全部失败则返回**最后一次**错误。若配置中设置了有效的 **`llm_router_source_index`**，则该源**最先**尝试；若还设置了有效的 **`llm_worker_source_index`**（且与主用不同），则第二个尝试该源；其余**校验通过**的源按列表顺序接在后面。均未设置时，顺序即为 `llm_sources` 中可用源的原有顺序。运行时仍为**单段 worker 内的多源回退链**。

各小节中的模型名为**示例**，请以服务商现行文档为准。

---

## 支持的提供商标识

### OpenAI
- **标识**: `openai`
- **模型示例**: `gpt-4o`, `gpt-4`, `gpt-3.5-turbo`
- **密钥**: [platform.openai.com](https://platform.openai.com)

### Anthropic (Claude)
- **标识**: `anthropic`
- **模型示例**: 以 [Anthropic 文档](https://docs.anthropic.com) 为准
- **密钥**: [console.anthropic.com](https://console.anthropic.com)

### Google Gemini（OpenAI 兼容客户端路径）
- **标识**: `gemini`
- **模型示例**: 以 [Google AI](https://ai.google.dev) 为准
- **密钥**: Google AI Studio

### 智谱 GLM
- **标识**: `glm`

### 通义千问
- **标识**: `qwen`

### DeepSeek
- **标识**: `deepseek`

### Moonshot
- **标识**: `moonshot`

### Ollama（本地）
- **标识**: `ollama`
- **api_url**: 一般为 `http://<主机>:11434/v1`
- **api_key**: 可填占位非空字符串（本地常不校验）

---

## 配置示例

### 单个提供商
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

### 多个提供商（顺序回退）
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

行为摘要：

- 尝试顺序由配置中的 **`llm_router_source_index` / `llm_worker_source_index`**（可选）与列表中可用源共同决定；**第一个成功的响应**立即返回。
- 若全部失败，返回**最后一次**错误。

### 离线优先（示例）

将 Ollama 放在列表末尾，主线路不可用时再使用本地模型：

```json
{
  "llm_sources": [
    {
      "provider": "qwen",
      "api_key": "sk-...",
      "model": "qwen-turbo",
      "api_url": ""
    },
    {
      "provider": "ollama",
      "api_key": "ollama",
      "model": "llama3",
      "api_url": "http://192.168.1.100:11434/v1"
    }
  ]
}
```
