import { API_ERROR, requestProtected, type ApiResult } from '../client'
import type { DisplayConfig } from '../../types/displayConfig'

export async function getDisplayConfig(baseUrl: string, pairingCode?: string): Promise<ApiResult<DisplayConfig>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<DisplayConfig>(baseUrl, '/api/config/display', {
    pairingCode: pairingCode?.trim(),
  })
}

export async function saveDisplayConfig(
  baseUrl: string,
  pairingCode: string,
  body: DisplayConfig,
): Promise<ApiResult<{ ok: boolean; restart_required?: boolean }>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<{ ok: boolean; restart_required?: boolean }>(baseUrl, '/api/config/display', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}
