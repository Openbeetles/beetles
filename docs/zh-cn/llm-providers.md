# 大模型服务

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

本页说明 Beetls OS 的大模型提供方配置。
首次接入建议采用单一 provider、单一 model 和一组可用密钥完成验证。

## 基础配置流程

1. 选一个 provider
2. 填 `api_key`
3. 填 `model`
4. 除非你用的是自定义地址，否则 `api_url` 先留空
5. 保存后，通过已配置聊天通道执行一次联通验证

多来源配置和路由优先级建议在单源验证完成后再启用。

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

如果你用的是提供商默认地址，下面这些通常可以留空：

- `openai`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

下面这些情况再填写 `api_url`：

- 你走的是自定义地址
- 你走代理或兼容网关
- 提供商要求非默认 base URL

对 `openai_compatible` 来说，通常应该填你实际在用的兼容服务地址。

## 多来源选择规则

Beetls OS 支持同时配置多个来源。

默认情况下，会按 `llm_sources` 里的顺序依次尝试。

如果你设置了：

- `llm_router_source_index`
- `llm_worker_source_index`

Beetls OS 会优先按这个顺序尝试：

1. `llm_router_source_index`
2. `llm_worker_source_index`
3. 列表里其余可用来源

按该配置运行时会自动选择可用来源，无需在聊天过程中手动切换。

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

## 配置要点

- `llm_sources` 不能为空
- 每个来源至少要有 `provider`、`api_key`、`model`
- 如果你用的是本地 Ollama，常见地址是 `http://<主机>:11434/v1`
- 示例里的模型名只是示例，不代表 Beetls OS 固定要求这些名字

## 相关文档

- 浏览器配置范围：[configuration.md](configuration.md)
- 能力概览：[capabilities.md](capabilities.md)
- 配置接口参考：[config-api.md](config-api.md)
