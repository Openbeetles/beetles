import test from "node:test";
import assert from "node:assert/strict";
import { saveWifi } from "./config.ts";

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

test("saveWifi requires both device URL and pairing code", async () => {
  const missingUrl = await saveWifi("", "123456", {
    wifi_ssid: "Beetle",
    wifi_pass: "secret",
  });
  assert.equal(missingUrl.ok, false);
  assert.equal(missingUrl.error, "NO_BASE_URL");

  const missingPairing = await saveWifi("http://device", "", {
    wifi_ssid: "Beetle",
    wifi_pass: "secret",
  });
  assert.equal(missingPairing.ok, false);
  assert.equal(missingPairing.error, "auth.pairing_required");
});

test("saveWifi POSTs WiFi credentials to the narrow firmware endpoint", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; method: string; body: string | null; pairing: string | null }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const headers = new Headers(init?.headers);
    calls.push({
      url: String(input),
      method: init?.method ?? "GET",
      body: typeof init?.body === "string" ? init.body : null,
      pairing: headers.get("x-pairing-code"),
    });
    return jsonResponse({ ok: true, restart_required: true });
  }) as typeof fetch;

  try {
    const result = await saveWifi("http://device", "123456", {
      wifi_ssid: "Beetle",
      wifi_pass: "secret",
    });
    assert.equal(result.ok, true);
    assert.deepEqual(calls, [
      {
        url: "http://device/api/config/wifi",
        method: "POST",
        body: JSON.stringify({ wifi_ssid: "Beetle", wifi_pass: "secret" }),
        pairing: "123456",
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
