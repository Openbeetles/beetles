import test from "node:test";
import assert from "node:assert/strict";
import { getAccounts, getProviders, getSystem } from "./config.ts";

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

test("getSystem GETs the dedicated system segment endpoint", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; method: string; pairing: string | null }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const headers = new Headers(init?.headers);
    calls.push({
      url: String(input),
      method: init?.method ?? "GET",
      pairing: headers.get("x-pairing-code"),
    });
    return jsonResponse({
      wifi_ssid: "BeetleNet",
      wifi_pass: "",
      proxy_url: "http://proxy.local:8080",
      session_max_messages: 32,
      tg_group_activation: "mention",
      locale: "zh",
    });
  }) as typeof fetch;

  try {
    const result = await getSystem("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, {
      wifi_ssid: "BeetleNet",
      wifi_pass: "",
      proxy_url: "http://proxy.local:8080",
      session_max_messages: 32,
      tg_group_activation: "mention",
      locale: "zh",
    });
    assert.deepEqual(calls, [
      {
        url: "http://device/api/config/system",
        method: "GET",
        pairing: "123456",
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getAccounts normalizes legacy account list wrappers", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({ accounts: [] });
  }) as typeof fetch;

  try {
    const result = await getAccounts("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, { count: 0, items: [] });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getProviders normalizes legacy provider catalog wrappers", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({ providers: [] });
  }) as typeof fetch;

  try {
    const result = await getProviders("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, { count: 0, items: [] });
  } finally {
    globalThis.fetch = originalFetch;
  }
});
