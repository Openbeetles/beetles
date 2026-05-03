import { requestProtected, API_ERROR } from '../client.ts'
import type { ApiRequestOptions, ApiResult } from '../client.ts'

export interface SkillItem {
  name: string
  enabled: boolean
}

export interface SkillsListResponse {
  skills: SkillItem[]
  order?: string[]
}

export interface SkillsListOptions {
  operatorWindowPolicy?: ApiRequestOptions['operatorWindowPolicy']
}

function objectRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : {}
}

function normalizeSkillItem(value: unknown): SkillItem | null {
  const record = objectRecord(value)
  const name = typeof record.name === 'string' ? record.name.trim() : ''
  if (!name) return null
  return {
    name,
    enabled: record.enabled === true,
  }
}

function normalizeSkillsListResponse(data: unknown): SkillsListResponse {
  const record = objectRecord(data)
  const skills = Array.isArray(record.skills)
    ? record.skills
        .map((item) => normalizeSkillItem(item))
        .filter((item): item is SkillItem => item !== null)
    : []
  const order = Array.isArray(record.order)
    ? record.order.filter((item): item is string => typeof item === 'string')
    : skills.map((skill) => skill.name)
  return { skills, order }
}

export async function listSkills(
  baseUrl: string,
  pairingCode?: string,
  options: SkillsListOptions = {},
): Promise<ApiResult<SkillsListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await requestProtected<unknown>(baseUrl, '/api/skills', {
    pairingCode: pairingCode?.trim() || undefined,
    operatorWindowPolicy: options.operatorWindowPolicy,
  })
  if (res.ok) {
    return { ...res, data: normalizeSkillsListResponse(res.data) }
  }
  return res as ApiResult<SkillsListResponse>
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
