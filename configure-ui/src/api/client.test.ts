import test from 'node:test'
import assert from 'node:assert/strict'
import { clearCsrfToken, fetchCsrfToken, request } from './client.ts'

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

test('request opens operator window and retries the original request once', async () => {
  clearCsrfToken()

  const calls: Array<{ url: string; method: string; headers: Headers }> = []
  let toolGetCalls = 0
  const originalFetch = globalThis.fetch

  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    const headers = new Headers(init?.headers)
    calls.push({ url, method, headers })

    if (url.endsWith('/api/tools') && method === 'GET' && toolGetCalls === 0) {
      toolGetCalls += 1
      return jsonResponse({
        status: 403,
        statusText: 'Forbidden',
        body: { error: 'operator window required', open_endpoint: 'POST /api/operator/window' },
      })
    }
    if (url.endsWith('/api/csrf_token')) {
      return jsonResponse({ body: { csrf_token: 'csrf-1' } })
    }
    if (url.endsWith('/api/operator/window')) {
      assert.equal(method, 'POST')
      assert.equal(headers.get('x-pairing-code'), '123456')
      assert.equal(headers.get('x-csrf-token'), 'csrf-1')
      return jsonResponse({ body: { ok: true } })
    }
    if (url.endsWith('/api/tools') && method === 'GET') {
      toolGetCalls += 1
      return jsonResponse({ body: [{ name: 'shell', i18n_key: 'tools.shell' }] })
    }
    throw new Error(`unexpected fetch ${method} ${url}`)
  }) as typeof fetch

  try {
    const token = await fetchCsrfToken('http://device')
    assert.equal(token, 'csrf-1')

    const result = await request<Array<{ name: string }>>('http://device', '/api/tools', {
      pairingCode: '123456',
    })

    assert.equal(result.ok, true)
    assert.deepEqual(result.data, [{ name: 'shell', i18n_key: 'tools.shell' }])
    assert.deepEqual(
      calls.map(({ url, method }) => `${method} ${url}`),
      [
        'GET http://device/api/csrf_token',
        'GET http://device/api/tools',
        'POST http://device/api/operator/window',
        'GET http://device/api/tools',
      ],
    )
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})
