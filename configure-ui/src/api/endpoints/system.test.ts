import test from "node:test";
import assert from "node:assert/strict";
import {
  getChannelConnectivity,
  getHealth,
  getMetrics,
  getResource,
  getSystemInfo,
} from "./system.ts";

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

test("default device status endpoints stay on the lightweight first-screen allowlist", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ path: string; search: string; method: string }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input));
    calls.push({
      path: url.pathname,
      search: url.search,
      method: init?.method ?? "GET",
    });
    return jsonResponse({});
  }) as typeof fetch;

  try {
    await getHealth("http://device");
    await getResource("http://device");
    await getMetrics("http://device");
    await getSystemInfo("http://device", "123456");

    assert.deepEqual(calls, [
      { path: "/api/health", search: "", method: "GET" },
      { path: "/api/resource", search: "", method: "GET" },
      { path: "/api/metrics", search: "", method: "GET" },
      { path: "/api/system_info", search: "", method: "GET" },
    ]);
    for (const denied of [
      "/api/wifi/scan",
      "/api/diagnose",
      "/api/hardware/discovery",
      "/api/channel_connectivity",
      "/api/config",
    ]) {
      assert.equal(
        calls.some((call) => call.path === denied),
        false,
        `${denied} must not be part of the default device status endpoints`,
      );
    }
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("health endpoint exposes current channel as lightweight state", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    jsonResponse({
      status: "ok",
      current_channel: { id: "qq_channel" },
    })) as typeof fetch;

  try {
    const result = await getHealth("http://device");

    assert.equal(result.ok, true);
    assert.equal(result.data?.current_channel?.id, "qq_channel");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("channel connectivity endpoint requires an explicit channel query", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ path: string; search: string; method: string }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input));
    calls.push({
      path: url.pathname,
      search: url.search,
      method: init?.method ?? "GET",
    });
    return jsonResponse({
      channel: {
        id: "qq_channel",
        configured: true,
        ok: true,
        message_key: null,
        runtime_status: "connected",
      },
      checked_at_unix_secs: 1,
    });
  }) as typeof fetch;

  try {
    const result = await getChannelConnectivity(
      "http://device",
      "qq_channel",
      "123456",
    );

    assert.equal(result.ok, true);
    assert.equal(result.data?.channel.id, "qq_channel");
    assert.deepEqual(calls, [
      {
        path: "/api/channel_connectivity",
        search: "?channel=qq_channel",
        method: "GET",
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("channel connectivity rejects empty channel before hitting fetch", async () => {
  const originalFetch = globalThis.fetch;
  let fetchCalled = false;
  globalThis.fetch = (async () => {
    fetchCalled = true;
    return jsonResponse({});
  }) as typeof fetch;

  try {
    const result = await getChannelConnectivity("http://device", " ", "123456");

    assert.equal(result.ok, false);
    assert.equal(result.status, 400);
    assert.equal(result.errorKey, "common.missing_query_param");
    assert.equal(fetchCalled, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
