/**
 * 请求设备 API。baseUrl 不含末尾斜杠，path 如 '/api/config'。
 * 配对码仅通过 Header X-Pairing-Code 传递，避免 query 导致预检 URL 不匹配而 CORS 失败。
 */
function buildUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}${path}`
}

export const API_ERROR = {
  NO_BASE_URL: 'NO_BASE_URL',
  PAIRING_REQUIRED: 'PAIRING_REQUIRED',
} as const

let csrfToken: string | null = null

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

export interface ApiRequestOptions {
  method?: 'GET' | 'POST' | 'DELETE'
  body?: string | object
  pairingCode?: string
}

export interface ApiResult<T = unknown> {
  ok: boolean
  data?: T
  error?: string
}

export async function request<T = unknown>(
  baseUrl: string,
  path: string,
  options: ApiRequestOptions = {},
): Promise<ApiResult<T>> {
  return requestInternal(baseUrl, path, options, 0)
}

async function requestInternal<T = unknown>(
  baseUrl: string,
  path: string,
  options: ApiRequestOptions,
  csrfRetryCount: number,
): Promise<ApiResult<T>> {
  const { method = 'GET', body, pairingCode } = options
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
      if (res.status === 403 && typeof data === 'object' && data !== null && 'error' in data) {
        const errMsg = String((data as { error: unknown }).error)
        if (errMsg.includes('CSRF') && csrfRetryCount < 1) {
          const refreshedToken = await fetchCsrfToken(baseUrl)
          if (refreshedToken) {
            return requestInternal<T>(baseUrl, path, options, csrfRetryCount + 1)
          }
        }
      }
      const err = typeof data === 'object' && data !== null && 'error' in data
        ? String((data as { error: unknown }).error)
        : res.statusText
      return { ok: false, error: err } as ApiResult<T>
    }
    return { ok: true, data: data as T }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : 'Network error' } as ApiResult<T>
  }
}
