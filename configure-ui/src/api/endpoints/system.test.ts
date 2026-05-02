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
  const calls: Array<{ path: string; method: string }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input));
    calls.push({ path: url.pathname, method: init?.method ?? "GET" });
    return jsonResponse({ channels: [] });
  }) as typeof fetch;

  try {
    await getHealth("http://device");
    await getResource("http://device");
    await getMetrics("http://device");
    await getSystemInfo("http://device", "123456");
    await getChannelConnectivity("http://device", "123456");

    assert.deepEqual(calls, [
      { path: "/api/health", method: "GET" },
      { path: "/api/resource", method: "GET" },
      { path: "/api/metrics", method: "GET" },
      { path: "/api/system_info", method: "GET" },
      { path: "/api/channel_connectivity", method: "GET" },
    ]);
    for (const denied of [
      "/api/wifi/scan",
      "/api/diagnose",
      "/api/hardware/discovery",
      "/api/channel_connectivity/refresh",
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
