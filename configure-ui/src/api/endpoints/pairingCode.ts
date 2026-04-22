import { request, API_ERROR } from '../client'
import type { ApiResult } from '../client'

export interface PairingCodeResponse {
  code_set: boolean
  locale?: string
}

/** GET /api/pairing_code：设备是否已激活（已设置配对码），白名单接口不需带码。 */
export async function getPairingCode(
  baseUrl: string,
): Promise<ApiResult<PairingCodeResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<PairingCodeResponse>(baseUrl, '/api/pairing_code')
  if (res.ok && res.data != null && typeof (res.data as PairingCodeResponse).code_set === 'boolean') {
    return res as ApiResult<PairingCodeResponse>
  }
  return { ok: false, error: res.error ?? 'common.invalid_response', data: undefined }
}
