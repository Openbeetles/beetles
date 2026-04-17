# 大模型配置

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

这页只讲两件事：

1. `provider` 现在支持哪些值
2. 多个来源时，Beetle 按什么顺序使用

## 支持的 `provider`

当前代码支持这些值：

- `openai`
- `openai_compatible`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

## `api_url` 怎么填

如果你用官方默认地址，下面这些通常可以留空：

- `openai`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

如果你用的是自建地址、代理地址或第三方兼容服务，就把 `api_url` 填成你自己的地址。

对 `openai_compatible` 来说，通常应该填写你实际使用的兼容服务地址。

## 多个来源怎么选

Beetle 支持同时配置多个来源。

默认情况下，会按 `llm_sources` 里的顺序依次尝试。

如果你设置了下面两个字段，顺序会变成：

- `llm_router_source_index`
- `llm_worker_source_index`

实际理解起来很简单：

1. 先试 `llm_router_source_index`
2. 再试 `llm_worker_source_index`
3. 还不行，再按列表里的其余可用来源继续试

你不需要在聊天里手动切换。

## 最小示例

### 单个来源

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

### 多个来源

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
      "provider": "qwen",
      "api_key": "...",
      "model": "qwen-plus",
      "api_url": ""
    },
    {
      "provider": "ollama",
      "api_key": "local",
      "model": "qwen2.5",
      "api_url": "http://192.168.1.100:11434/v1"
    }
  ],
  "llm_router_source_index": 0,
  "llm_worker_source_index": 1
}
```

## 几个直接结论

- `llm_sources` 不能为空
- 每个来源都至少要有 `provider`、`api_key`、`model`
- 如果你用的是本地 Ollama，常见地址是 `http://<主机>:11434/v1`
- 示例里的模型名只是示例，不代表 Beetle 固定要求这些名字
