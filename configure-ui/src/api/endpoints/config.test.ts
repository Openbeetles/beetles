import test from 'node:test'
import assert from 'node:assert/strict'
import { clearCsrfToken, fetchCsrfToken } from '../client.ts'
import {
  createAccount,
  deleteAccount,
  getAccount,
  getAccounts,
  getCapabilities,
  getCapability,
  getProviders,
  probeAccount,
  revokeAccount,
  saveAccountConfig,
} from './config.ts'

interface MockResponseInit {
  status?: number
  statusText?: string
  body?: unknown
}

function jsonResponse(init: MockResponseInit): Response {
  const body =
    typeof init.body === 'string' ? init.body : JSON.stringify(init.body ?? null)
  return new Response(body, {
    status: init.status ?? 200,
    statusText: init.statusText,
    headers: { 'Content-Type': 'application/json' },
  })
}

test('account config GET endpoints hit the expected routes', async () => {
  clearCsrfToken()
  const calls: Array<{ url: string; method: string; headers: Headers }> = []
  const originalFetch = globalThis.fetch

  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    const headers = new Headers(init?.headers)
    calls.push({ url, method, headers })

    return jsonResponse({ body: { ok: true } })
  }) as typeof fetch

  try {
    await getProviders('http://device', '654321', { capability: 'mail' })
    await getCapabilities('http://device', '654321')
    await getCapability('http://device', '654321', 'documents')
    await getAccounts('http://device', '654321', {
      capability: 'calendar',
      providerKind: 'feishu_calendar',
    })
    await getAccount('http://device', '654321', 'work-feishu')

    assert.deepEqual(
      calls.map(({ url, method }) => `${method} ${url}`),
      [
        'GET http://device/api/config/providers?capability=mail',
        'GET http://device/api/config/capabilities',
        'GET http://device/api/config/capabilities/documents',
        'GET http://device/api/config/accounts?capability=calendar&provider_kind=feishu_calendar',
        'GET http://device/api/config/accounts/work-feishu',
      ],
    )
    for (const call of calls) {
      assert.equal(call.headers.get('x-pairing-code'), '654321')
    }
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('account config mutating endpoints use expected methods, csrf token, and bodies', async () => {
  clearCsrfToken()
  const calls: Array<{ url: string; method: string; headers: Headers; body?: string }> = []
  const originalFetch = globalThis.fetch

  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    const headers = new Headers(init?.headers)
    const body = typeof init?.body === 'string' ? init.body : undefined
    calls.push({ url, method, headers, body })

    if (url.endsWith('/api/csrf_token')) {
      return jsonResponse({ body: { csrf_token: 'csrf-account' } })
    }
    return jsonResponse({ body: { ok: true } })
  }) as typeof fetch

  try {
    const token = await fetchCsrfToken('http://device')
    assert.equal(token, 'csrf-account')

    await createAccount('http://device', '123456', {
      account: {
        provider_kind: 'wecom_documents',
        external_account_id: '',
        account_label: 'Work WeCom',
        identity_class: 'work',
        enabled_capabilities: ['documents'],
      },
      config: {
        fields: {
          corp_id: 'wxcorp',
          corp_secret: 'secret',
        },
      },
    })
    await saveAccountConfig('http://device', '123456', 'work-wecom', {
      fields: { corp_secret: 'new-secret' },
      clear_fields: ['refresh_token'],
    })
    await probeAccount('http://device', '123456', 'work-wecom')
    await revokeAccount('http://device', '123456', 'work-wecom', {
      clear_runtime_status: true,
    })
    await deleteAccount('http://device', '123456', 'work-wecom')

    assert.deepEqual(
      calls.map(({ url, method }) => `${method} ${url}`),
      [
        'GET http://device/api/csrf_token',
        'POST http://device/api/config/accounts',
        'POST http://device/api/config/accounts/work-wecom/config',
        'POST http://device/api/config/accounts/work-wecom/probe',
        'POST http://device/api/config/accounts/work-wecom/revoke',
        'DELETE http://device/api/config/accounts/work-wecom',
      ],
    )

    for (const call of calls.slice(1)) {
      assert.equal(call.headers.get('x-pairing-code'), '123456')
      assert.equal(call.headers.get('x-csrf-token'), 'csrf-account')
    }

    assert.deepEqual(JSON.parse(calls[1].body ?? '{}'), {
      account: {
        provider_kind: 'wecom_documents',
        external_account_id: '',
        account_label: 'Work WeCom',
        identity_class: 'work',
        enabled_capabilities: ['documents'],
      },
      config: {
        fields: {
          corp_id: 'wxcorp',
          corp_secret: 'secret',
        },
      },
    })
    assert.deepEqual(JSON.parse(calls[2].body ?? '{}'), {
      fields: { corp_secret: 'new-secret' },
      clear_fields: ['refresh_token'],
    })
    assert.equal(calls[3].body, undefined)
    assert.deepEqual(JSON.parse(calls[4].body ?? '{}'), {
      clear_runtime_status: true,
    })
    assert.equal(calls[5].body, undefined)
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})
