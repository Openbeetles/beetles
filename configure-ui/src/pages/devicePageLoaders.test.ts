import test from "node:test";
import assert from "node:assert/strict";
import type { ApiResult } from "../api/client.ts";
import type {
  ChannelConnectivityResponse,
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
} from "../api/endpoints/system.ts";
import {
  loadDeviceChannelConnectivity,
  loadDeviceHealthBundle,
} from "./devicePageLoaders.ts";

test("loadDeviceHealthBundle returns the full bundle when all three endpoints succeed", async () => {
  const health: HealthData = { wifi: "connected" };
  const resource: ResourceSnapshotData = { pressure: "Cautious" };
  const metrics: MetricsSnapshotData = { llm_calls: 12 };

  const result = await loadDeviceHealthBundle({
    health: async () => ({ ok: true, data: health }),
    resource: async () => ({ ok: true, data: resource }),
    metrics: async () => ({ ok: true, data: metrics }),
  });

  assert.deepEqual(result, {
    ok: true,
    data: {
      health,
      resource,
      metrics,
    },
  });
});

test("loadDeviceHealthBundle keeps the first endpoint error as the shared failure reason", async () => {
  const result = await loadDeviceHealthBundle({
    health: async () => ({ ok: false, error: "health unavailable" }),
    resource: async () =>
      ({ ok: false, error: "resource unavailable" }) as ApiResult<ResourceSnapshotData>,
    metrics: async () =>
      ({ ok: false, error: "metrics unavailable" }) as ApiResult<MetricsSnapshotData>,
  });

  assert.deepEqual(result, {
    ok: false,
    error: "health unavailable",
  });
});

test("loadDeviceHealthBundle normalizes thrown transport errors to config.errorNetwork", async () => {
  const result = await loadDeviceHealthBundle({
    health: async () => {
      throw new Error("socket hang up");
    },
    resource: async () => ({ ok: true, data: {} }),
    metrics: async () => ({ ok: true, data: {} }),
  });

  assert.deepEqual(result, {
    ok: false,
    error: "config.errorNetwork",
  });
});

test("loadDeviceChannelConnectivity returns the channel list when the endpoint succeeds", async () => {
  const channels: ChannelConnectivityResponse["channels"] = [
    { id: "telegram", configured: true, ok: true, message_key: null },
  ];

  const result = await loadDeviceChannelConnectivity(async () => ({
    ok: true,
    data: { channels },
  }));

  assert.deepEqual(result, {
    ok: true,
    data: channels,
  });
});

test("loadDeviceChannelConnectivity preserves the endpoint error when payload is incomplete", async () => {
  const result = await loadDeviceChannelConnectivity(async () => ({
    ok: true,
    data: {} as ChannelConnectivityResponse,
    error: "upstream unavailable",
  }));

  assert.deepEqual(result, {
    ok: false,
    error: "upstream unavailable",
  });
});

test("loadDeviceChannelConnectivity falls back to a default error when payload is incomplete and the endpoint is silent", async () => {
  const result = await loadDeviceChannelConnectivity(async () => ({
    ok: true,
    data: {} as ChannelConnectivityResponse,
  }));

  assert.deepEqual(result, {
    ok: false,
    error: "network.channel_connectivity_unavailable",
  });
});
