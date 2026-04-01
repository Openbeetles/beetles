import { request, API_ERROR } from '../client'
import type { ApiResult } from '../client'

/** 与固件 `GET /api/tools` 单条一致（见 `handlers/tools.rs`）。 */
export interface ToolInfo {
  name: string
  i18n_key: string
}

/**
 * 拉取当前固件已注册的工具列表（JSON 数组根）。
 * GET /api/tools：仅需设备已激活，不要求配对码 Header。
 */
export async function listTools(baseUrl: string): Promise<ApiResult<ToolInfo[]>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<unknown>(baseUrl, '/api/tools', {})
  if (!res.ok) return res as ApiResult<ToolInfo[]>
  const raw = res.data
  if (!Array.isArray(raw)) {
    return { ok: false, error: 'Invalid tools response' }
  }
  const out: ToolInfo[] = []
  for (const item of raw) {
    if (typeof item !== 'object' || item === null) continue
    const o = item as Record<string, unknown>
    const name = typeof o.name === 'string' ? o.name : ''
    const i18n_key = typeof o.i18n_key === 'string' ? o.i18n_key : ''
    if (name && i18n_key) out.push({ name, i18n_key })
  }
  return { ok: true, data: out }
}
