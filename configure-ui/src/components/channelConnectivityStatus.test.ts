import test from "node:test";
import assert from "node:assert/strict";
import type { ChannelConnectivityItem } from "../api/endpoints/system.ts";
import {
  channelConnectivityMessageKey,
  isChannelOperational,
} from "./channelConnectivityStatus.ts";

test("isChannelOperational returns false for disabled channels", () => {
  const item: ChannelConnectivityItem = {
    id: "webhook",
    configured: false,
    ok: false,
    message_key: "network.connectivity_not_configured",
    runtime_status: "disabled",
  };

  assert.equal(isChannelOperational(item), false);
  assert.equal(
    channelConnectivityMessageKey(item),
    "network.connectivity_not_configured",
  );
});

test("isChannelOperational keeps explicit successful probe as online", () => {
  const item: ChannelConnectivityItem = {
    id: "qq_channel",
    configured: true,
    ok: true,
    message_key: null,
    runtime_status: "connected",
  };

  assert.equal(isChannelOperational(item), true);
  assert.equal(channelConnectivityMessageKey(item), null);
});

test("connected runtime status overrides passive stale probe unavailable message", () => {
  const item: ChannelConnectivityItem = {
    id: "qq_channel",
    configured: true,
    ok: false,
    message_key: "network.channel_connectivity_unavailable",
    runtime_status: "connected",
  };

  assert.equal(isChannelOperational(item), true);
  assert.equal(channelConnectivityMessageKey(item), null);
});

test("non-connected runtime status preserves endpoint failure reason", () => {
  const item: ChannelConnectivityItem = {
    id: "qq_channel",
    configured: true,
    ok: false,
    message_key: "network.channel_connectivity_unavailable",
    runtime_status: "connecting",
  };

  assert.equal(isChannelOperational(item), false);
  assert.equal(
    channelConnectivityMessageKey(item),
    "network.channel_connectivity_unavailable",
  );
});
