import assert from "node:assert/strict";
import test from "node:test";
import {
  normalizeChannelsConfigFromDevice,
  normalizeLlmConfigFromDevice,
  normalizeSystemConfigFromDevice,
} from "./appConfig.ts";

test("normalizeChannelsConfigFromDevice fills missing channel string fields", () => {
  const config = normalizeChannelsConfigFromDevice({
    enabled_channel: "wecom",
    available_channels: ["wecom"],
    wecom_bot_id: "bot-id",
  });

  assert.equal(config.enabled_channel, "wecom");
  assert.equal(config.wecom_bot_id, "bot-id");
  assert.equal(config.wecom_ws_url, "");
  assert.equal(config.wecom_bot_secret, "");
  assert.equal(config.webhook_enabled, false);
  assert.deepEqual(config.available_channels, ["wecom"]);
});

test("normalizeChannelsConfigFromDevice keeps legacy responses usable without catalog", () => {
  const config = normalizeChannelsConfigFromDevice({
    enabled_channel: "telegram",
    tg_token: "token",
    tg_group_activation: "unexpected",
  });

  assert.equal(config.tg_token, "token");
  assert.equal(config.tg_group_activation, "mention");
  assert.ok(config.available_channels.includes("telegram"));
  assert.ok(config.available_channels.includes("wecom"));
});

test("normalizeLlmConfigFromDevice normalizes source capability fields", () => {
  const config = normalizeLlmConfigFromDevice({
    llm_sources: [
      {
        id: "src-openai",
        provider: "openai",
        model: "gpt-4o",
        max_tokens: 4096,
        model_kind: "multimodal",
        custom_headers: [{ name: "X-Beetle", value: "alpha" }],
      },
      null,
      {
        api_key: 42,
        api_url: "https://example.test/v1",
        model_kind: "unsupported",
        custom_headers: [{ name: 42, value: "beta" }, null],
      },
    ],
  });

  assert.deepEqual(config, {
    llm_sources: [
      {
        id: "src-openai",
        provider: "openai",
        api_key: "",
        model: "gpt-4o",
        api_url: "",
        max_tokens: 4096,
        model_kind: "multimodal",
        custom_headers: [{ name: "X-Beetle", value: "alpha" }],
      },
      {
        id: "",
        provider: "",
        api_key: "",
        model: "",
        api_url: "https://example.test/v1",
        max_tokens: null,
        model_kind: "text",
        custom_headers: [{ name: "", value: "beta" }],
      },
    ],
  });
});

test("normalizeSystemConfigFromDevice fills missing scalar fields", () => {
  const config = normalizeSystemConfigFromDevice({
    wifi_ssid: "Office",
  });

  assert.deepEqual(config, {
    wifi_ssid: "Office",
    wifi_pass: "",
    proxy_url: "",
    locale: null,
  });
});
