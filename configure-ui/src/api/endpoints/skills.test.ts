import assert from "node:assert/strict";
import test from "node:test";
import { listSkills } from "./skills.ts";

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

test("listSkills normalizes partial skill list responses", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    jsonResponse({
      skills: [
        { name: "core", enabled: true },
        { name: "disabled" },
        { enabled: true },
        null,
      ],
      order: ["disabled", 42, "core"],
    })) as typeof fetch;

  try {
    const result = await listSkills("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, {
      skills: [
        { name: "core", enabled: true },
        { name: "disabled", enabled: false },
      ],
      order: ["disabled", "core"],
    });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("listSkills treats malformed wrappers as an empty list", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    jsonResponse({
      skills: {},
      order: "core",
    })) as typeof fetch;

  try {
    const result = await listSkills("http://device", "123456");
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, {
      skills: [],
      order: [],
    });
  } finally {
    globalThis.fetch = originalFetch;
  }
});
