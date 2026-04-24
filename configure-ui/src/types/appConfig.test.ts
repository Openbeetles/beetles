import assert from "node:assert/strict";
import test from "node:test";
import { normalizeChannelsConfigFromDevice } from "./appConfig.ts";

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
  });

  assert.equal(config.tg_token, "token");
  assert.ok(config.available_channels.includes("telegram"));
  assert.ok(config.available_channels.includes("wecom"));
});
