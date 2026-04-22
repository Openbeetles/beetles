import { request, requestProtected, API_ERROR } from '../client'
import type { ApiResult } from '../client'

export async function getSoul(baseUrl: string): Promise<ApiResult<string>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<unknown>(baseUrl, '/api/soul')
  if (!res.ok) return res as ApiResult<string>
  const data = res.data != null ? String(res.data) : ''
  return { ok: true, data }
}

export async function saveSoul(
  baseUrl: string,
  pairingCode: string,
  content: string,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/soul', {
    method: 'POST',
    body: { content },
    pairingCode: pairingCode.trim(),
  })
}

export async function getUser(baseUrl: string): Promise<ApiResult<string>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<unknown>(baseUrl, '/api/user')
  if (!res.ok) return res as ApiResult<string>
  const data = res.data != null ? String(res.data) : ''
  return { ok: true, data }
}

export async function saveUser(
  baseUrl: string,
  pairingCode: string,
  content: string,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/user', {
    method: 'POST',
    body: { content },
    pairingCode: pairingCode.trim(),
  })
}
