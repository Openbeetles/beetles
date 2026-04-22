import { requestProtected, API_ERROR } from '../client'
import type { ApiResult } from '../client'

export interface SkillItem {
  name: string
  enabled: boolean
}

export interface SkillsListResponse {
  skills: SkillItem[]
  order?: string[]
}

export async function listSkills(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<SkillsListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await requestProtected<SkillsListResponse>(baseUrl, '/api/skills', {
    pairingCode: pairingCode?.trim() || undefined,
  })
  if (res.ok && res.data) {
    const d = res.data
    return { ok: true, data: { skills: d.skills ?? [], order: d.order ?? d.skills?.map((s) => s.name) ?? [] } }
  }
  return res
}

export async function getSkillContent(
  baseUrl: string,
  name: string,
  pairingCode?: string,
): Promise<ApiResult<string>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await requestProtected<unknown>(baseUrl, `/api/skills?name=${encodeURIComponent(name)}`, {
    pairingCode: pairingCode?.trim() || undefined,
  })
  if (!res.ok) return res as ApiResult<string>
  return { ok: true, data: res.data != null ? String(res.data) : '' }
}

export async function postSkill(
  baseUrl: string,
  pairingCode: string,
  body: { name: string; enabled?: boolean; content?: string },
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/skills', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function deleteSkill(
  baseUrl: string,
  pairingCode: string,
  name: string,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, `/api/skills?name=${encodeURIComponent(name)}`, {
    method: 'DELETE',
    pairingCode: pairingCode.trim(),
  })
}

export async function importSkill(
  baseUrl: string,
  pairingCode: string,
  url: string,
  name: string,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/skills/import', {
    method: 'POST',
    body: { url, name },
    pairingCode: pairingCode.trim(),
  })
}
