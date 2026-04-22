import test from "node:test";
import assert from "node:assert/strict";
import { postPairingCode } from "./pairingCode.ts";

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

test("postPairingCode rejects missing device URLs", async () => {
  const result = await postPairingCode("", "123456");
  assert.equal(result.ok, false);
  assert.equal(result.error, "NO_BASE_URL");
});

test("postPairingCode POSTs the six-digit pairing code to the firmware endpoint", async () => {
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; method: string; body: string | null }> = [];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    calls.push({
      url: String(input),
      method: init?.method ?? "GET",
      body: typeof init?.body === "string" ? init.body : null,
    });
    return jsonResponse({ ok: true });
  }) as typeof fetch;

  try {
    const result = await postPairingCode("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(calls, [
      {
        url: "http://device/api/pairing_code",
        method: "POST",
        body: JSON.stringify({ code: "123456" }),
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
