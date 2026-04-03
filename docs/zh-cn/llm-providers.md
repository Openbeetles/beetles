# 大模型服务商说明

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

这页讲的是 `config/llm.json` 应该怎么填。

主要说明三件事：

1. `provider` 可以填哪些值？
2. 哪些服务商的 `api_url` 可以留空？
3. 配了多个来源后，到底按什么顺序尝试？

## 基本规则

- Beetle 支持配置多个大模型来源
- 配置入口是 `config/llm.json`
- 部分服务商走 OpenAI 兼容接口
- 配置多个来源时，会按顺序依次尝试

## 支持的服务商 ID

| 服务商 ID | 接口类型 | 说明 |
|-------------|------------------|------|
| `openai` | OpenAI 兼容接口 | 标准 OpenAI 路径 |
| `openai_compatible` | OpenAI 兼容接口 | 通用兼容接口 |
| `gemini` | OpenAI 兼容接口 | 内部会做厂商适配 |
| `glm` | OpenAI 兼容接口 | 智谱 GLM |
| `qwen` | OpenAI 兼容接口 | 通义千问 |
| `deepseek` | OpenAI 兼容接口 | DeepSeek |
| `moonshot` | OpenAI 兼容接口 | Moonshot |
| `ollama` | OpenAI 兼容接口 | 本地或自托管 |
| `anthropic` | Anthropic 接口 | Claude Messages API |

## `api_url` 的关键规则

### 哪些可以留空

下面这些服务商的 `api_url` 可以留空：

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
- 任何未知的 `provider` 值

对 `anthropic` 来说，非空 `api_url` 会被当作完整接口地址。

## 多来源切换顺序

配置多个来源后，Beetle 会按顺序依次尝试。

顺序如下：

1. `llm_router_source_index`，如果已配置且有效
2. `llm_worker_source_index`，如果已配置、有效，且和前一个来源不同
3. 其余所有可用源，按列表顺序追加

切换规则：

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

### 多个来源切换

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

## 这些规则来自代码实现

实现参考：

- 客户端构建逻辑：[`src/llm/mod.rs`](../../src/llm/mod.rs)
- 切换顺序逻辑：[`src/llm/fallback.rs`](../../src/llm/fallback.rs)
- Anthropic 客户端行为：[`src/llm/anthropic.rs`](../../src/llm/anthropic.rs)

厂商模型名会持续变化，所以这里的模型名只用来举例，不代表程序会长期固定这些名字。
