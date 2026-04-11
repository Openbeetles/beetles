# 大模型服务商说明

[English](../en-us/llm-providers.md) | **中文** | [文档索引](../README.md)

这页讲的是 `config/llm.json` 应该怎么填。

主要说明三件事：

1. `provider` 可以填哪些值？
2. 哪些服务商的 `api_url` 可以留空？
3. 配了多个来源后，主用和备用来源怎么选？

## 基本规则

- Beetle 支持配置多个大模型来源
- 配置页和配置文件都可以改
- 部分服务商走 OpenAI 兼容接口
- 你可以配主用来源，也可以额外配备用来源

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

## 多来源怎么选

配置多个来源后，Beetle 会先用你指定的优先来源；当前一个来源不可用时，再尝试后面的来源。

实际可以按这个理解：

1. 先试你设成最高优先级的来源
2. 如果你还指定了第二优先来源，就接着试它
3. 其余可用来源再按列表顺序继续尝试

你不需要在聊天时手动切换。
只要把来源顺序配好即可。

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

厂商模型名会持续变化，所以这里的模型名只用来举例，不代表程序会长期固定这些名字。
