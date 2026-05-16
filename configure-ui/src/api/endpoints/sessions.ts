import {
  API_ERROR,
  buildProtectedApiSessionKey,
  fetchCsrfToken,
  getCsrfToken,
  notifyProtectedApiAuthState,
  requestProtected,
  type ApiResult,
} from '../client.ts'

export type ChatMessageRole = 'user' | 'assistant' | 'system' | string

export interface ChatSessionLastMessage {
  message_id: string
  role: ChatMessageRole
  preview: string
}

export interface ChatSessionSummary {
  chat_id: string
  title: string
  last_message: ChatSessionLastMessage | null
  message_count: number
}

export interface ChatSessionListResponse {
  items: ChatSessionSummary[]
  next_cursor: string | null
  limit: number
}

export interface ChatSessionMessage {
  message_id: string
  role: ChatMessageRole
  content: string
}

export interface ChatSessionMessagesResponse {
  items: ChatSessionMessage[]
  next_before: string | null
  limit: number
}

export interface ChatSessionListOptions {
  cursor?: string | null
  limit?: number
}

export interface ChatSessionMessagesOptions {
  chatId: string
  before?: string | null
  limit?: number
}

export interface ChatSessionPostBody {
  chat_id: string
  content: string
}

export type ChatSessionStreamEvent =
  | { type: 'queued'; data: unknown }
  | { type: 'delta'; delta: string; messageId?: string; data: unknown }
  | { type: 'final'; messageId?: string; data: unknown }
  | { type: 'error'; error: string; errorKey?: string; errorStage?: string; data: unknown }
  | { type: 'done'; data: unknown }

export type ChatSessionStreamHandler = (event: ChatSessionStreamEvent) => void

export interface ChatSessionStreamOptions {
  signal?: AbortSignal
  timeoutMs?: number
}

interface SseParseResult {
  terminal: 'final' | 'error' | null
  error?: string
  errorKey?: string
  errorStage?: string
}

const CHAT_STREAM_TIMEOUT_MS = 120_000

function buildUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}${path}`
}

function buildSessionsQuery(params: Record<string, string | number | null | undefined>): string {
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null) continue
    const stringValue = String(value).trim()
    if (!stringValue) continue
    query.set(key, stringValue)
  }
  const encoded = query.toString()
  return encoded ? `/api/sessions?${encoded}` : '/api/sessions'
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function parseSseData(data: string): unknown {
  const trimmed = data.trim()
  if (!trimmed) return {}
  try {
    return JSON.parse(trimmed)
  } catch {
    return trimmed
  }
}

function readString(data: unknown, key: string): string | undefined {
  if (!isObject(data) || !(key in data)) return undefined
  const value = data[key]
  return typeof value === 'string' ? value : undefined
}

function normalizeSseEvent(eventName: string, dataText: string): ChatSessionStreamEvent | null {
  const data = parseSseData(dataText)
  const type = eventName.trim() || 'message'
  switch (type) {
    case 'queued':
      return { type: 'queued', data }
    case 'delta': {
      const delta = readString(data, 'delta') ?? (typeof data === 'string' ? data : '')
      const messageId = readString(data, 'message_id')
      return {
        type: 'delta',
        delta,
        messageId,
        data,
      }
    }
    case 'final':
      return {
        type: 'final',
        messageId: readString(data, 'message_id'),
        data,
    }
    case 'error': {
      const errorKey = readString(data, 'error_key')
      const errorStage = readString(data, 'error_stage')
      return {
        type: 'error',
        error: errorKey ?? readString(data, 'error') ?? 'common.error',
        errorKey,
        errorStage,
        data,
      }
    }
    case 'done':
      return { type: 'done', data }
    default:
      return null
  }
}

function parseSseBlock(block: string): ChatSessionStreamEvent | null {
  let eventName = ''
  const dataLines: string[] = []
  for (const rawLine of block.split(/\r?\n/)) {
    const line = rawLine.trimEnd()
    if (!line || line.startsWith(':')) continue
    if (line.startsWith('event:')) {
      eventName = line.slice('event:'.length).trim()
    } else if (line.startsWith('data:')) {
      dataLines.push(line.slice('data:'.length).replace(/^ /, ''))
    }
  }
  return normalizeSseEvent(eventName, dataLines.join('\n'))
}

async function parseSseStream(
  response: Response,
  onEvent: ChatSessionStreamHandler,
  signal: AbortSignal,
): Promise<SseParseResult> {
  const result: SseParseResult = { terminal: null }
  if (!response.body) return result
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  const handleEvent = (event: ChatSessionStreamEvent | null) => {
    if (!event) return
    onEvent(event)
    if (event.type === 'final') {
      result.terminal = 'final'
    } else if (event.type === 'error') {
      result.terminal = 'error'
      result.error = event.error
      result.errorKey = event.errorKey
      result.errorStage = event.errorStage
    }
  }
  try {
    for (;;) {
      const { done, value } = await readSseChunk(reader, signal)
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      const parts = buffer.split(/\r?\n\r?\n/)
      buffer = parts.pop() ?? ''
      for (const part of parts) {
        handleEvent(parseSseBlock(part))
      }
    }
    buffer += decoder.decode()
    const trailing = buffer.trim()
    if (trailing) {
      handleEvent(parseSseBlock(trailing))
    }
  } finally {
    reader.releaseLock()
  }
  return result
}

async function readSseChunk(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  signal: AbortSignal,
): Promise<ReadableStreamReadResult<Uint8Array>> {
  if (signal.aborted) throw new DOMException('Aborted', 'AbortError')
  let cleanup: () => void = () => undefined
  try {
    const result = await Promise.race([
      reader.read(),
      new Promise<ReadableStreamReadResult<Uint8Array>>((_, reject) => {
        const abort = () => {
          void reader.cancel().catch(() => undefined)
          reject(new DOMException('Aborted', 'AbortError'))
        }
        signal.addEventListener('abort', abort, { once: true })
        cleanup = () => signal.removeEventListener('abort', abort)
      }),
    ])
    if (signal.aborted) throw new DOMException('Aborted', 'AbortError')
    return result
  } finally {
    cleanup()
  }
}

async function parseErrorResponse(response: Response): Promise<ApiResult<void>> {
  let data: unknown
  try {
    const text = await response.text()
    data = text ? JSON.parse(text) : null
  } catch {
    data = null
  }
  const errorKey = readString(data, 'error_key')
  const upstreamError = readString(data, 'upstream_error')
  const rawError = readString(data, 'error')
  return {
    ok: false,
    status: response.status,
    error: errorKey ?? rawError ?? 'common.http_status',
    errorKey,
    upstreamError,
  }
}

function notifyPairingAuthFailure(
  baseUrl: string,
  pairingCode: string,
  method: 'GET' | 'POST' | 'DELETE',
  path: string,
  errorKey?: string,
): void {
  if (!pairingCode.trim()) return
  if (errorKey === 'auth.pairing_invalid' || errorKey === 'auth.pairing_required') {
    notifyProtectedApiAuthState({
      state: errorKey === 'auth.pairing_invalid' ? 'invalid' : 'required',
      method,
      path,
      sessionKey: buildProtectedApiSessionKey(baseUrl, pairingCode),
    })
  }
}

function linkAbortSignal(controller: AbortController, signal?: AbortSignal): () => void {
  if (!signal) return () => undefined
  if (signal.aborted) {
    controller.abort()
    return () => undefined
  }
  const abort = () => controller.abort()
  signal.addEventListener('abort', abort, { once: true })
  return () => signal.removeEventListener('abort', abort)
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError'
}

async function streamSessionMessageInternal(
  baseUrl: string,
  pairingCode: string,
  body: ChatSessionPostBody,
  onEvent: ChatSessionStreamHandler,
  csrfRetryCount: number,
  options: ChatSessionStreamOptions,
): Promise<ApiResult<void>> {
  const csrf = getCsrfToken() ?? (await fetchCsrfToken(baseUrl))
  if (!csrf) return { ok: false, error: 'auth.csrf_required', errorKey: 'auth.csrf_required' }

  const controller = new AbortController()
  const unlinkAbortSignal = linkAbortSignal(controller, options.signal)
  const timeout = globalThis.setTimeout(() => controller.abort(), options.timeoutMs ?? CHAT_STREAM_TIMEOUT_MS)
  try {
    const response = await fetch(buildUrl(baseUrl, '/api/sessions'), {
      method: 'POST',
      signal: controller.signal,
      headers: {
        Accept: 'text/event-stream',
        'Content-Type': 'application/json',
        'X-Pairing-Code': pairingCode.trim(),
        'X-CSRF-Token': csrf,
      },
      body: JSON.stringify(body),
    })

    if (!response.ok) {
      const result = await parseErrorResponse(response)
      notifyPairingAuthFailure(baseUrl, pairingCode, 'POST', '/api/sessions', result.errorKey)
      if (
        response.status === 403 &&
        (result.errorKey === 'auth.csrf_invalid' || result.errorKey === 'auth.csrf_required') &&
        csrfRetryCount < 1
      ) {
        const refreshedToken = await fetchCsrfToken(baseUrl)
        if (refreshedToken) {
          return streamSessionMessageInternal(
            baseUrl,
            pairingCode,
            body,
            onEvent,
            csrfRetryCount + 1,
            options,
          )
        }
      }
      return result
    }

    if (pairingCode.trim()) {
      notifyProtectedApiAuthState({
        state: 'valid',
        method: 'POST',
        path: '/api/sessions',
        sessionKey: buildProtectedApiSessionKey(baseUrl, pairingCode),
      })
    }

    const streamResult = await parseSseStream(response, onEvent, controller.signal)
    if (streamResult.terminal === 'error') {
      return {
        ok: false,
        status: response.status,
        error: streamResult.error ?? streamResult.errorKey ?? 'common.error',
        errorKey: streamResult.errorKey,
      }
    }
    if (streamResult.terminal !== 'final') {
      return {
        ok: false,
        status: response.status,
        error: 'chat.stream_incomplete',
        errorKey: 'chat.stream_incomplete',
      }
    }
    return { ok: true, status: response.status }
  } finally {
    unlinkAbortSignal()
    globalThis.clearTimeout(timeout)
  }
}

export async function listSessions(
  baseUrl: string,
  pairingCode?: string,
  options: ChatSessionListOptions = {},
): Promise<ApiResult<ChatSessionListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<ChatSessionListResponse>(
    baseUrl,
    buildSessionsQuery({ cursor: options.cursor, limit: options.limit }),
    { pairingCode: pairingCode?.trim() },
  )
}

export async function getSessionMessages(
  baseUrl: string,
  pairingCode: string | undefined,
  options: ChatSessionMessagesOptions,
): Promise<ApiResult<ChatSessionMessagesResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<ChatSessionMessagesResponse>(
    baseUrl,
    buildSessionsQuery({
      chat_id: options.chatId,
      before: options.before,
      limit: options.limit,
    }),
    { pairingCode: pairingCode?.trim() },
  )
}

export async function streamSessionMessage(
  baseUrl: string,
  pairingCode: string,
  body: ChatSessionPostBody,
  onEvent: ChatSessionStreamHandler,
  options: ChatSessionStreamOptions = {},
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  if (!body.content.trim()) {
    return { ok: false, error: 'chat.content_required', errorKey: 'chat.content_required' }
  }
  try {
    return await streamSessionMessageInternal(baseUrl, pairingCode, body, onEvent, 0, options)
  } catch (error) {
    if (isAbortError(error)) {
      return {
        ok: false,
        error: 'chat.stream_timeout',
        errorKey: 'chat.stream_timeout',
      }
    }
    return {
      ok: false,
      error: 'network.request_failed',
      errorKey: 'network.request_failed',
    }
  }
}
