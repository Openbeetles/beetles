import test from 'node:test'
import assert from 'node:assert/strict'
import {
  buildProtectedApiSessionKey,
  clearCsrfToken,
  fetchCsrfToken,
  request,
  setProtectedApiAuthObserver,
} from './client.ts'

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

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

async function flushQueuedRequestStart() {
  await Promise.resolve()
  await Promise.resolve()
}

test('request serializes concurrent calls to the same device', async () => {
  clearCsrfToken()

  const first = deferred<Response>()
  const calls: string[] = []
  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input)
    calls.push(url)
    if (url.endsWith('/api/health')) return first.promise
    return jsonResponse({ body: { pressure: 'Normal' } })
  }) as typeof fetch

  let health: ReturnType<typeof request> | null = null
  let resource: ReturnType<typeof request> | null = null
  try {
    health = request('http://device', '/api/health')
    resource = request('http://device', '/api/resource')
    await flushQueuedRequestStart()

    assert.deepEqual(calls, ['http://device/api/health'])

    first.resolve(jsonResponse({ body: { status: 'ok', network_status: { sta_connected: true } } }))
    assert.equal((await health).ok, true)
    assert.equal((await resource).ok, true)
    assert.deepEqual(calls, [
      'http://device/api/health',
      'http://device/api/resource',
    ])
  } finally {
    first.resolve(jsonResponse({ body: { status: 'ok', network_status: { sta_connected: true } } }))
    await Promise.allSettled([health, resource].filter(Boolean) as Promise<unknown>[])
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('request queues csrf token fetches behind an in-flight device request', async () => {
  clearCsrfToken()

  const first = deferred<Response>()
  const calls: string[] = []
  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = String(input)
    calls.push(url)
    if (url.endsWith('/api/health')) return first.promise
    if (url.endsWith('/api/csrf_token')) {
      return jsonResponse({ body: { csrf_token: 'csrf-queued' } })
    }
    throw new Error(`unexpected fetch ${url}`)
  }) as typeof fetch

  let health: ReturnType<typeof request> | null = null
  let csrf: ReturnType<typeof fetchCsrfToken> | null = null
  try {
    health = request('http://device', '/api/health')
    csrf = fetchCsrfToken('http://device')
    await flushQueuedRequestStart()

    assert.deepEqual(calls, ['http://device/api/health'])

    first.resolve(jsonResponse({ body: { status: 'ok', network_status: { sta_connected: true } } }))
    assert.equal((await health).ok, true)
    assert.equal(await csrf, 'csrf-queued')
    assert.deepEqual(calls, [
      'http://device/api/health',
      'http://device/api/csrf_token',
    ])
  } finally {
    first.resolve(jsonResponse({ body: { status: 'ok', network_status: { sta_connected: true } } }))
    await Promise.allSettled([health, csrf].filter(Boolean) as Promise<unknown>[])
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

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
        body: {
          error_key: 'system.operator_window_required',
          open_endpoint: 'POST /api/operator/window',
        },
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

test('request surfaces error_key and upstream_error without falling back to raw status text', async () => {
  clearCsrfToken()

  const originalFetch = globalThis.fetch
  globalThis.fetch = (async () =>
    jsonResponse({
      status: 400,
      statusText: 'Bad Request',
      body: {
        error_key: 'office.provider_error',
        upstream_error: 'AADSTS7000215: Invalid client secret is provided.',
        upstream_status: 401,
      },
    })) as typeof fetch

  try {
    const result = await request('http://device', '/api/config/accounts/probe', {
      method: 'POST',
      pairingCode: '123456',
    })

    assert.equal(result.ok, false)
    assert.equal(result.status, 400)
    assert.equal(result.errorKey, 'office.provider_error')
    assert.equal(result.error, 'AADSTS7000215: Invalid client secret is provided.')
    assert.equal(result.upstreamError, 'AADSTS7000215: Invalid client secret is provided.')
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('request notifies protected auth observer with the active session key when a validated request succeeds', async () => {
  clearCsrfToken()

  const authEvents: Array<{ state: string; sessionKey: string }> = []
  const originalFetch = globalThis.fetch
  setProtectedApiAuthObserver((event) => {
    authEvents.push({ state: event.state, sessionKey: event.sessionKey })
  })
  globalThis.fetch = (async () =>
    jsonResponse({
      body: {
        wifi_ssid: 'Beetle',
      },
    })) as typeof fetch

  try {
    const result = await request('http://device', '/api/config/system', {
      pairingCode: '123456',
      authPolicy: 'validate',
    })

    assert.equal(result.ok, true)
    assert.deepEqual(authEvents, [
      {
        state: 'valid',
        sessionKey: buildProtectedApiSessionKey('http://device', '123456'),
      },
    ])
  } finally {
    setProtectedApiAuthObserver(null)
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('request notifies protected auth observer with the active session key when a validated request gets pairing_invalid', async () => {
  clearCsrfToken()

  const authEvents: Array<{ state: string; sessionKey: string }> = []
  const originalFetch = globalThis.fetch
  setProtectedApiAuthObserver((event) => {
    authEvents.push({ state: event.state, sessionKey: event.sessionKey })
  })
  globalThis.fetch = (async () =>
    jsonResponse({
      status: 403,
      statusText: 'Forbidden',
      body: {
        error_key: 'auth.pairing_invalid',
      },
    })) as typeof fetch

  try {
    const result = await request('http://device', '/api/config/system', {
      pairingCode: '123456',
      authPolicy: 'validate',
    })

    assert.equal(result.ok, false)
    assert.deepEqual(authEvents, [
      {
        state: 'invalid',
        sessionKey: buildProtectedApiSessionKey('http://device', '123456'),
      },
    ])
  } finally {
    setProtectedApiAuthObserver(null)
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})
