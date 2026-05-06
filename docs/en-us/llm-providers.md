# LLM Providers

[中文](../zh-cn/llm-providers.md) | **English** | [Doc index](../README.md)

Connect Beetle OS to one model provider first.
For a first working setup, keep it simple: one provider, one model, one valid API key.

## Fastest Working Path

1. choose one provider
2. fill `api_key`
3. fill `model`
4. leave `api_url` empty unless you use a custom endpoint
5. save the config and test one chat reply

You can add multiple sources and routing preferences later.

## Supported `provider` Values

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

## How To Fill `api_url`

If you use the provider's normal default endpoint, these usually work with an empty `api_url`:

- `openai`
- `gemini`
- `glm`
- `qwen`
- `deepseek`
- `moonshot`
- `ollama`
- `anthropic`

Set `api_url` only when:

- you use a custom endpoint
- you are going through a proxy or compatible gateway
- your provider requires a non-default base URL

For `openai_compatible`, you will usually want to set the actual compatible endpoint you use.

## How Beetle OS Chooses Between Multiple Sources

Beetle OS can keep more than one LLM source at the same time.

Beetle OS tries sources in the order they appear in `llm_sources`. Configure UI changes that priority with drag-and-drop sorting.

That means you do not need to switch providers manually during normal chat use.

## Minimal Examples

### One Source

```json
{
  "llm_sources": [
    {
      "id": "deepseek-main",
      "provider": "deepseek",
      "api_key": "sk-...",
      "model": "deepseek-chat",
      "api_url": "",
      "model_kind": "text",
      "custom_headers": []
    }
  ]
}
```

### Multiple Sources

```json
{
  "llm_sources": [
    {
      "id": "gemini-vision",
      "provider": "gemini",
      "api_key": "AIza...",
      "model": "gemini-1.5-flash",
      "api_url": "",
      "model_kind": "multimodal",
      "custom_headers": []
    },
    {
      "id": "qwen-text",
      "provider": "qwen",
      "api_key": "...",
      "model": "qwen-plus",
      "api_url": "",
      "model_kind": "text",
      "custom_headers": []
    },
    {
      "id": "ollama-local",
      "provider": "ollama",
      "api_key": "local",
      "model": "qwen2.5",
      "api_url": "http://192.168.1.100:11434/v1",
      "model_kind": "text",
      "custom_headers": []
    }
  ]
}
```

## Direct Takeaways

- `llm_sources` must not be empty
- every source needs at least `id`, `provider`, `api_key`, `model`, `model_kind`, and `custom_headers`
- vision tools only use sources with `model_kind = "multimodal"`
- add extra HTTP headers required by third-party routers in that source's `custom_headers`
- for local Ollama, a common endpoint is `http://<host>:11434/v1`
- example model names are examples only; Beetle OS does not require those exact names

## Read Next

- To complete browser setup: [configuration.md](configuration.md)
- To see what Beetle OS can do after setup: [capabilities.md](capabilities.md)
- To write config through the API: [config-api.md](config-api.md)
