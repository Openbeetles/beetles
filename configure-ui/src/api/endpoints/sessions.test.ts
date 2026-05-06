import test from 'node:test'
import assert from 'node:assert/strict'
import { clearCsrfToken, fetchCsrfToken } from '../client.ts'
import {
  getSessionMessages,
  listSessions,
  streamSessionMessage,
  type ChatSessionStreamEvent,
} from './sessions.ts'

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
}

function sseResponse(chunks: string[], init?: ResponseInit): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk))
      controller.close()
    },
  })
  return new Response(stream, {
    headers: { 'Content-Type': 'text/event-stream' },
    ...init,
  })
}

test('sessions GET endpoints use protected API with typed query parameters', async () => {
  clearCsrfToken()

  const calls: Array<{ path: string; headers: Headers }> = []
  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input))
    calls.push({ path: `${url.pathname}${url.search}`, headers: new Headers(init?.headers) })
    if (url.searchParams.has('chat_id')) {
      return jsonResponse({
        items: [{ message_id: 'm1', role: 'assistant', content: 'hello' }],
        next_before: null,
        limit: 20,
      })
    }
    return jsonResponse({
      items: [
        {
          chat_id: 'c1',
          title: 'Main',
          last_message: { message_id: 'm1', role: 'assistant', preview: 'hello' },
          message_count: 1,
        },
      ],
      next_cursor: 'next',
      limit: 30,
    })
  }) as typeof fetch

  try {
    const sessions = await listSessions('http://device', '123456', {
      cursor: 'cur',
      limit: 30,
    })
    const messages = await getSessionMessages('http://device', '123456', {
      chatId: 'c1',
      before: 'm2',
      limit: 20,
    })

    assert.equal(sessions.ok, true)
    assert.equal(sessions.data?.items[0]?.chat_id, 'c1')
    assert.equal(messages.ok, true)
    assert.equal(messages.data?.items[0]?.content, 'hello')
    assert.deepEqual(
      calls.map(({ path }) => path),
      ['/api/sessions?cursor=cur&limit=30', '/api/sessions?chat_id=c1&before=m2&limit=20'],
    )
    assert.equal(calls[0]?.headers.get('x-pairing-code'), '123456')
    assert.equal(calls[1]?.headers.get('x-pairing-code'), '123456')
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('session stream POST uses event-stream accept, pairing code, csrf, and parses SSE events', async () => {
  clearCsrfToken()

  const calls: Array<{ path: string; method: string; headers: Headers; body?: string }> = []
  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input))
    const method = init?.method ?? 'GET'
    calls.push({
      path: url.pathname,
      method,
      headers: new Headers(init?.headers),
      body: typeof init?.body === 'string' ? init.body : undefined,
    })
    if (url.pathname === '/api/csrf_token') {
      return jsonResponse({ csrf_token: 'csrf-1' })
    }
    if (url.pathname === '/api/sessions') {
      return sseResponse([
        'event: queued\ndata: {"chat_id":"c1"}\n\n',
        'event: delta\ndata: {"delta":"hel"}\n\n',
        'event: snapshot\ndata: {"content":"hello"}\n\n',
        'event: final\ndata: {"message_id":"a1","content":"hello"}\n\n',
        'event: done\ndata: {}\n\n',
      ])
    }
    throw new Error(`unexpected fetch ${method} ${url.pathname}`)
  }) as typeof fetch

  const events: ChatSessionStreamEvent[] = []
  try {
    const result = await streamSessionMessage(
      'http://device',
      '123456',
      { chat_id: 'c1', content: 'hello?' },
      (event) => events.push(event),
    )

    assert.equal(result.ok, true)
    assert.deepEqual(
      calls.map(({ method, path }) => `${method} ${path}`),
      ['GET /api/csrf_token', 'POST /api/sessions'],
    )
    const streamCall = calls[1]
    assert.equal(streamCall?.headers.get('accept'), 'text/event-stream')
    assert.equal(streamCall?.headers.get('content-type'), 'application/json')
    assert.equal(streamCall?.headers.get('x-pairing-code'), '123456')
    assert.equal(streamCall?.headers.get('x-csrf-token'), 'csrf-1')
    assert.equal(streamCall?.body, JSON.stringify({ chat_id: 'c1', content: 'hello?' }))
    assert.deepEqual(
      events.map((event) => event.type),
      ['queued', 'delta', 'snapshot', 'final', 'done'],
    )
    assert.equal(events[1]?.type === 'delta' ? events[1].delta : '', 'hel')
    assert.equal(events[2]?.type === 'snapshot' ? events[2].content : '', 'hello')
    assert.equal(events[3]?.type === 'final' ? events[3].content : '', 'hello')
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('session stream refreshes csrf and retries once on csrf failure', async () => {
  clearCsrfToken()

  const calls: Array<{ path: string; method: string; headers: Headers }> = []
  let csrfFetches = 0
  let postCalls = 0
  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(String(input))
    const method = init?.method ?? 'GET'
    calls.push({ path: url.pathname, method, headers: new Headers(init?.headers) })
    if (url.pathname === '/api/csrf_token') {
      csrfFetches += 1
      return jsonResponse({ csrf_token: csrfFetches === 1 ? 'old-token' : 'new-token' })
    }
    if (url.pathname === '/api/sessions') {
      postCalls += 1
      if (postCalls === 1) {
        return jsonResponse({ error_key: 'auth.csrf_invalid' }, { status: 403 })
      }
      return sseResponse([
        'event: final\ndata: {"message_id":"a1","content":"retry ok"}\n\n',
        'event: done\ndata: {}\n\n',
      ])
    }
    throw new Error(`unexpected fetch ${method} ${url.pathname}`)
  }) as typeof fetch

  try {
    assert.equal(await fetchCsrfToken('http://device'), 'old-token')

    const events: ChatSessionStreamEvent[] = []
    const result = await streamSessionMessage(
      'http://device',
      '123456',
      { chat_id: 'c1', content: 'retry' },
      (event) => events.push(event),
    )

    assert.equal(result.ok, true)
    assert.deepEqual(
      calls.map(({ method, path }) => `${method} ${path}`),
      [
        'GET /api/csrf_token',
        'POST /api/sessions',
        'GET /api/csrf_token',
        'POST /api/sessions',
      ],
    )
    assert.equal(calls[1]?.headers.get('x-csrf-token'), 'old-token')
    assert.equal(calls[3]?.headers.get('x-csrf-token'), 'new-token')
    assert.deepEqual(
      events.map((event) => event.type),
      ['final', 'done'],
    )
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('session stream prefers public error_key over internal upstream_error', async () => {
  clearCsrfToken()

  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = new URL(String(input))
    if (url.pathname === '/api/csrf_token') {
      return jsonResponse({ csrf_token: 'csrf-1' })
    }
    if (url.pathname === '/api/sessions') {
      return sseResponse([
        'event: error\ndata: {"error_key":"chat.failed","upstream_error":"llm_request"}\n\n',
        'event: done\ndata: {}\n\n',
      ])
    }
    throw new Error(`unexpected fetch ${url.pathname}`)
  }) as typeof fetch

  try {
    const events: ChatSessionStreamEvent[] = []
    const result = await streamSessionMessage(
      'http://device',
      '123456',
      { chat_id: 'c1', content: 'hello' },
      (event) => events.push(event),
    )

    assert.equal(result.ok, false)
    assert.equal(result.error, 'chat.failed')
    assert.equal(result.errorKey, 'chat.failed')
    assert.equal(result.upstreamError, 'llm_request')
    const errorEvent = events.find((event) => event.type === 'error')
    assert.equal(errorEvent?.type === 'error' ? errorEvent.error : '', 'chat.failed')
    assert.equal(
      errorEvent?.type === 'error' ? errorEvent.upstreamError : '',
      'llm_request',
    )
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})

test('session stream reports incomplete response when EOF has no final or error', async () => {
  clearCsrfToken()

  const originalFetch = globalThis.fetch
  globalThis.fetch = (async (input: string | URL | Request) => {
    const url = new URL(String(input))
    if (url.pathname === '/api/csrf_token') {
      return jsonResponse({ csrf_token: 'csrf-1' })
    }
    if (url.pathname === '/api/sessions') {
      return sseResponse(['event: delta\ndata: {"delta":"partial"}\n\n'])
    }
    throw new Error(`unexpected fetch ${url.pathname}`)
  }) as typeof fetch

  try {
    const result = await streamSessionMessage(
      'http://device',
      '123456',
      { chat_id: 'c1', content: 'hello' },
      () => {},
    )

    assert.equal(result.ok, false)
    assert.equal(result.error, 'chat.stream_incomplete')
    assert.equal(result.errorKey, 'chat.stream_incomplete')
  } finally {
    globalThis.fetch = originalFetch
    clearCsrfToken()
  }
})
