# LLM Setup

[中文](../zh-cn/llm-providers.md) | **English** | [Doc index](../README.md)

This page answers two practical questions:

1. which `provider` values Beetle supports now
2. how Beetle chooses between multiple sources

## Supported `provider` values

Current code supports:

- `openai`
- `openai_compatible`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

## How to fill `api_url`

If you want the built-in default endpoint, these can usually leave `api_url` empty:

- `openai`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

If you use your own endpoint, proxy, or hosted service, set `api_url` to that address.

For `openai_compatible`, you will usually want to fill in the actual compatible endpoint you are using.

## How Beetle picks between sources

Beetle can keep more than one LLM source at the same time.

By default, it tries them in the order they appear in `llm_sources`.

If you set these fields:

- `llm_router_source_index`
- `llm_worker_source_index`

then the order becomes:

1. try `llm_router_source_index` first
2. try `llm_worker_source_index` next
3. then continue through the rest of the usable list

You do not need to switch sources manually in chat.

## Minimal examples

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

### Multiple sources

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

## Direct takeaways

- `llm_sources` must not be empty
- every source needs at least `provider`, `api_key`, and `model`
- for local Ollama, the common address is `http://<host>:11434/v1`
- model names in examples are only examples
