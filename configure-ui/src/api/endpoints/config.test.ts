import test from "node:test";
import assert from "node:assert/strict";
import {
  getAccount,
  getAccounts,
  getCapabilities,
  getLlm,
  getProviders,
  saveLlm,
  getSystem,
} from "./config.ts";

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

test("getProviders normalizes partial provider catalog rows", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({
      items: [
        {
          provider_kind: "mailgun",
          account_fields: [{ key: "identity_class", required: true }],
        },
      ],
    });
  }) as typeof fetch;

  try {
    const result = await getProviders("http://device", "123456");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data.items, [
      {
        provider_kind: "mailgun",
        display_name_key: "",
        capabilities: [],
        account_fields: [
          {
            key: "identity_class",
            label_key: "",
            description_key: "",
            label: undefined,
            description: undefined,
            value_kind: "text",
            required: true,
            secret: false,
            multiple: false,
            default_value: undefined,
            default_values: [],
            options: [],
          },
        ],
        config_fields: [],
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getAccounts normalizes partial account summary rows", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({ items: [{ account_key: "acct-1" }] });
  }) as typeof fetch;

  try {
    const result = await getAccounts("http://device", "123456");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data.items, [
      {
        account_key: "acct-1",
        provider_kind: "",
        display_name_key: "",
        account_label: "",
        identity_class: "other",
        enabled_capabilities: [],
        selected_for_capabilities: [],
        readiness: "needs_configuration",
        next_action: "configure_account",
        missing_fields_count: 0,
        has_runtime_error: false,
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getAccount normalizes partial detail payloads", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({
      account: { account_key: "acct-1", provider_kind: "mailgun" },
      assessment: { missing_fields: ["api_key"] },
    });
  }) as typeof fetch;

  try {
    const result = await getAccount("http://device", "123456", "acct-1");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data.fields, []);
    assert.deepEqual(result.data.account, {
      account_key: "acct-1",
      provider_kind: "mailgun",
      display_name_key: "",
      external_account_id: "",
      account_label: "",
      identity_class: "other",
      enabled_capabilities: [],
      selected_for_capabilities: [],
      credential_status: undefined,
      runtime_status: undefined,
    });
    assert.deepEqual(result.data.assessment, {
      account_key: "acct-1",
      provider_kind: "mailgun",
      enabled_capabilities: [],
      credential_present: false,
      credential_configured: false,
      probe_supported: false,
      missing_fields: ["api_key"],
      missing_field_details: [],
      readiness: "needs_configuration",
      next_action: "configure_account",
      runtime_status: undefined,
    });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getAccount preserves explicit empty assessment capabilities", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({
      account: {
        account_key: "acct-1",
        provider_kind: "mailgun",
        enabled_capabilities: ["mail"],
      },
      assessment: { enabled_capabilities: [] },
    });
  }) as typeof fetch;

  try {
    const result = await getAccount("http://device", "123456", "acct-1");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data.account.enabled_capabilities, ["mail"]);
    assert.deepEqual(result.data.assessment.enabled_capabilities, []);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getCapabilities drops malformed capability rows", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({
      items: [
        {},
        { capability: "unknown" },
        { capability: "calendar", accounts: [{ account_key: "acct-1" }] },
      ],
    });
  }) as typeof fetch;

  try {
    const result = await getCapabilities("http://device", "123456");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data.items, [
      {
        capability: "calendar",
        default_account_key: undefined,
        selection_status: "missing",
        selected_account_key: undefined,
        ready: false,
        next_action: "none",
        accounts: [
          {
            account_key: "acct-1",
            provider_kind: "",
            display_name_key: "",
            account_label: "",
            identity_class: "other",
            enabled_capabilities: [],
            selected_for_capabilities: [],
            readiness: "needs_configuration",
            next_action: "configure_account",
            missing_fields_count: 0,
            has_runtime_error: false,
          },
        ],
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getLlm normalizes partial source responses", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => {
    return jsonResponse({ llm_sources: [{ provider: "openai" }, null] });
  }) as typeof fetch;

  try {
    const result = await getLlm("http://device", "123456");
    assert.equal(result.ok, true);
    assert.ok(result.data);
    assert.deepEqual(result.data, {
      llm_sources: [
        {
          id: "",
          provider: "openai",
          api_key: "",
          model: "",
          api_url: "",
          max_tokens: null,
          model_kind: "text",
          custom_headers: [],
        },
      ],
    });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("saveLlm posts only llm_sources", async () => {
  const originalFetch = globalThis.fetch;
  const bodies: unknown[] = [];
  globalThis.fetch = (async (_input: string | URL | Request, init?: RequestInit) => {
    bodies.push(init?.body ? JSON.parse(String(init.body)) : null);
    return jsonResponse({});
  }) as typeof fetch;

  try {
    const result = await saveLlm("http://device", "123456", {
      llm_sources: [
        {
          id: "src-1",
          provider: "openai",
          api_key: "key",
          model: "gpt-4o",
          api_url: "https://api.openai.com/v1",
          max_tokens: 4096,
          model_kind: "multimodal",
          custom_headers: [{ name: "X-Beetle", value: "alpha" }],
        },
      ],
    });

    assert.equal(result.ok, true);
    assert.deepEqual(bodies, [
      {
        llm_sources: [
          {
            id: "src-1",
            provider: "openai",
            api_key: "key",
            model: "gpt-4o",
            api_url: "https://api.openai.com/v1",
            max_tokens: 4096,
            model_kind: "multimodal",
            custom_headers: [{ name: "X-Beetle", value: "alpha" }],
          },
        ],
      },
    ]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
