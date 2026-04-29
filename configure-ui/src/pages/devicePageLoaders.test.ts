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
  loadDeviceStatusBundle,
} from "./devicePageLoaders.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function flushLoaderContinuation() {
  await Promise.resolve();
  await Promise.resolve();
}

test("loadDeviceStatusBundle returns the full bundle when all three endpoints succeed", async () => {
  const health: HealthData = { status: "ok", network_status: { sta_connected: true } };
  const resource: ResourceSnapshotData = { pressure: "Cautious" };
  const metrics: MetricsSnapshotData = { llm_calls: 12 };

  const result = await loadDeviceStatusBundle({
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

test("loadDeviceStatusBundle starts resource and metrics only after earlier endpoints settle", async () => {
  const health: HealthData = { status: "ok", network_status: { sta_connected: true } };
  const resource: ResourceSnapshotData = { pressure: "Normal" };
  const metrics: MetricsSnapshotData = { llm_calls: 1 };
  const healthDeferred = deferred<ApiResult<HealthData>>();
  const resourceDeferred = deferred<ApiResult<ResourceSnapshotData>>();
  const metricsDeferred = deferred<ApiResult<MetricsSnapshotData>>();
  const calls: string[] = [];

  const pending = loadDeviceStatusBundle({
    health: async () => {
      calls.push("health");
      return healthDeferred.promise;
    },
    resource: async () => {
      calls.push("resource");
      return resourceDeferred.promise;
    },
    metrics: async () => {
      calls.push("metrics");
      return metricsDeferred.promise;
    },
  });

  await Promise.resolve();
  assert.deepEqual(calls, ["health"]);

  healthDeferred.resolve({ ok: true, data: health });
  await flushLoaderContinuation();
  assert.deepEqual(calls, ["health", "resource"]);

  resourceDeferred.resolve({ ok: true, data: resource });
  await flushLoaderContinuation();
  assert.deepEqual(calls, ["health", "resource", "metrics"]);

  metricsDeferred.resolve({ ok: true, data: metrics });
  assert.deepEqual(await pending, {
    ok: true,
    data: { health, resource, metrics },
  });
});

test("loadDeviceStatusBundle stops after the first failed endpoint", async () => {
  const calls: string[] = [];

  const result = await loadDeviceStatusBundle({
    health: async () => {
      calls.push("health");
      return { ok: false, error: "health unavailable" };
    },
    resource: async () => {
      calls.push("resource");
      return { ok: true, data: { pressure: "Normal" } };
    },
    metrics: async () => {
      calls.push("metrics");
      return { ok: true, data: { llm_calls: 1 } };
    },
  });

  assert.deepEqual(calls, ["health"]);
  assert.deepEqual(result, {
    ok: false,
    error: "health unavailable",
  });
});

test("loadDeviceStatusBundle keeps the first endpoint error as the shared failure reason", async () => {
  const result = await loadDeviceStatusBundle({
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

test("loadDeviceStatusBundle normalizes thrown transport errors to config.errorNetwork", async () => {
  const result = await loadDeviceStatusBundle({
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
