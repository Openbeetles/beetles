/**
 * 请求设备 API。baseUrl 不含末尾斜杠，path 如 '/api/config/system'。
 * 配对码仅通过 Header X-Pairing-Code 传递，避免 query 导致预检 URL 不匹配而 CORS 失败。
 */
function buildUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}${path}`
}

export const API_ERROR = {
  NO_BASE_URL: 'NO_BASE_URL',
  PAIRING_REQUIRED: 'auth.pairing_required',
} as const

export type ProtectedApiAuthState = 'valid' | 'invalid' | 'required'

export interface ProtectedApiAuthEvent {
  state: ProtectedApiAuthState
  method: 'GET' | 'POST' | 'DELETE'
  path: string
  sessionKey: string
}

type ProtectedApiAuthObserver = ((event: ProtectedApiAuthEvent) => void) | null

let csrfToken: string | null = null
let protectedApiAuthObserver: ProtectedApiAuthObserver = null

export function clearCsrfToken(): void {
  csrfToken = null
}

export async function fetchCsrfToken(baseUrl: string): Promise<string | null> {
  try {
    const res = await fetch(buildUrl(baseUrl, '/api/csrf_token'))
    if (!res.ok) {
      csrfToken = null
      return null
    }
    const data = await res.json()
    csrfToken = data.csrf_token || null
    return csrfToken
  } catch {
    csrfToken = null
    return null
  }
}

export function getCsrfToken(): string | null {
  return csrfToken
}

export function setProtectedApiAuthObserver(
  observer: ProtectedApiAuthObserver,
): void {
  protectedApiAuthObserver = observer
}

export function buildProtectedApiSessionKey(
  baseUrl: string,
  pairingCode?: string,
): string {
  return `${baseUrl.trim().replace(/\/$/, '')}\0${(pairingCode ?? '').trim()}`
}

function notifyProtectedApiAuthObserver(event: ProtectedApiAuthEvent): void {
  protectedApiAuthObserver?.(event)
}

export interface ApiRequestOptions {
  method?: 'GET' | 'POST' | 'DELETE'
  body?: string | object
  pairingCode?: string
  authPolicy?: 'none' | 'validate'
}

export interface ApiResult<T = unknown> {
  ok: boolean
  status?: number
  data?: T
  error?: string
  errorKey?: string
  upstreamError?: string
  openEndpoint?: string
}

export async function request<T = unknown>(
  baseUrl: string,
  path: string,
  options: ApiRequestOptions = {},
): Promise<ApiResult<T>> {
  return requestInternal(baseUrl, path, options, 0)
}

export async function requestProtected<T = unknown>(
  baseUrl: string,
  path: string,
  options: ApiRequestOptions = {},
): Promise<ApiResult<T>> {
  return request(baseUrl, path, { ...options, authPolicy: 'validate' })
}

async function requestInternal<T = unknown>(
  baseUrl: string,
  path: string,
  options: ApiRequestOptions,
  csrfRetryCount: number,
  operatorWindowRetryCount = 0,
): Promise<ApiResult<T>> {
  const { method = 'GET', body, pairingCode, authPolicy = 'none' } = options
  const url = buildUrl(baseUrl, path)
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }
  if (pairingCode?.trim()) headers['X-Pairing-Code'] = pairingCode.trim()
  if (method === 'POST' || method === 'DELETE') {
    const token = getCsrfToken()
    if (token) headers['X-CSRF-Token'] = token
  }
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json'
  }

  try {
    const res = await fetch(url, {
      method,
      headers,
      body: typeof body === 'object' ? JSON.stringify(body) : body,
    })
    const text = await res.text()
    let data: unknown
    try {
      data = text ? JSON.parse(text) : null
    } catch {
      data = text
    }

    if (!res.ok) {
      const errorKey =
        typeof data === 'object' && data !== null && 'error_key' in data
          ? String((data as { error_key: unknown }).error_key)
          : undefined
      const upstreamError =
        typeof data === 'object' && data !== null && 'upstream_error' in data
          ? String((data as { upstream_error: unknown }).upstream_error)
          : undefined
      const rawError =
        typeof data === 'object' && data !== null && 'error' in data
          ? String((data as { error: unknown }).error)
          : undefined
      const openEndpoint =
        typeof data === 'object' && data !== null && 'open_endpoint' in data
          ? String((data as { open_endpoint: unknown }).open_endpoint)
          : undefined

      if (res.status === 403) {
        if (
          (errorKey === 'auth.csrf_invalid' || errorKey === 'auth.csrf_required') &&
          csrfRetryCount < 1
        ) {
          const refreshedToken = await fetchCsrfToken(baseUrl)
          if (refreshedToken) {
            return requestInternal<T>(
              baseUrl,
              path,
              options,
              csrfRetryCount + 1,
              operatorWindowRetryCount,
            )
          }
        }
        if (
          errorKey === 'system.operator_window_required' &&
          pairingCode?.trim() &&
          path !== '/api/operator/window' &&
          operatorWindowRetryCount < 1
        ) {
          const openResult = await requestInternal<{ ok?: boolean }>(
            baseUrl,
            '/api/operator/window',
            {
              method: 'POST',
              pairingCode: pairingCode.trim(),
            },
            0,
            operatorWindowRetryCount + 1,
          )
          if (openResult.ok) {
            return requestInternal<T>(
              baseUrl,
              path,
              options,
              csrfRetryCount,
              operatorWindowRetryCount + 1,
            )
          }
        }
      }
      if (authPolicy === 'validate' && pairingCode?.trim()) {
        const sessionKey = buildProtectedApiSessionKey(baseUrl, pairingCode)
        if (errorKey === 'auth.pairing_invalid') {
          notifyProtectedApiAuthObserver({
            state: 'invalid',
            method,
            path,
            sessionKey,
          })
        } else if (errorKey === 'auth.pairing_required') {
          notifyProtectedApiAuthObserver({
            state: 'required',
            method,
            path,
            sessionKey,
          })
        }
      }
      const err = upstreamError ?? errorKey ?? rawError ?? 'common.http_status'
      return {
        ok: false,
        status: res.status,
        error: err,
        errorKey,
        upstreamError,
        openEndpoint,
      } as ApiResult<T>
    }
    if (authPolicy === 'validate' && pairingCode?.trim()) {
      notifyProtectedApiAuthObserver({
        state: 'valid',
        method,
        path,
        sessionKey: buildProtectedApiSessionKey(baseUrl, pairingCode),
      })
    }
    return { ok: true, status: res.status, data: data as T }
  } catch {
    return {
      ok: false,
      error: 'network.request_failed',
      errorKey: 'network.request_failed',
      upstreamError: undefined,
    } as ApiResult<T>
  }
}
