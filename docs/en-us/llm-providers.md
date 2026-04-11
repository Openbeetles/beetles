# LLM Providers

[中文](../zh-cn/llm-providers.md) | **English** | [Doc index](../README.md)

This page explains the LLM settings in `config/llm.json`.

It focuses on three questions:

1. Which `provider` values are supported?
2. When can `api_url` be empty?
3. How are primary and backup sources chosen?

## Quick Summary

- Beetle supports multiple LLM sources.
- You can edit them in the config UI or in `config/llm.json`.
- Some providers are handled as OpenAI-compatible clients.
- You can configure a primary source and one or more backups.

## Supported Provider IDs

| Provider ID | Client path | Notes |
|-------------|-------------|------|
| `openai` | OpenAI-compatible client | Standard OpenAI path |
| `openai_compatible` | OpenAI-compatible client | Generic compatible endpoint |
| `gemini` | OpenAI-compatible client | Vendor-specific handling inside the client |
| `glm` | OpenAI-compatible client | Zhipu GLM |
| `qwen` | OpenAI-compatible client | Qwen |
| `deepseek` | OpenAI-compatible client | DeepSeek |
| `moonshot` | OpenAI-compatible client | Moonshot |
| `ollama` | OpenAI-compatible client | Usually local/self-hosted |
| `anthropic` | Anthropic client | Uses Claude Messages API |

## Important `api_url` Rules

### When `api_url` may be empty

These provider IDs may use an empty `api_url`:

- `openai`
- `openai_compatible`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`

### When `api_url` must be present

- `anthropic`
- any unknown provider ID

For `anthropic`, a non-empty `api_url` is treated as the full Messages endpoint URL.

## How multiple sources are picked

When more than one source is configured, Beetle first tries your preferred source. If that source is unavailable, it moves on to the next one.

The practical rule is:

1. try the source you marked as highest priority
2. if you set a second preferred source, try that next
3. then continue through the remaining usable sources in list order

You do not need to switch sources manually during normal use.
Just set the order you want.

## Minimal Examples

### One source

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

### Multiple sources with fallback

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

## Ollama Notes

- Use `provider: "ollama"`
- `api_url` is usually `http://<host>:11434/v1`
- `api_key` can be any non-empty placeholder if the server ignores it

Vendor model names change over time. Treat model names in examples as examples, not guarantees.
