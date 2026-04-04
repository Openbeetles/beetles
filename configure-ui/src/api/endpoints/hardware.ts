import { API_ERROR, request, type ApiResult } from '../client'
import type { HardwareSegment } from '../../types/hardwareConfig'

export type HardwareDiscoveryBus = 'usb'
export type HardwareDiscoveryCapability = 'audio_output' | 'audio_input' | 'camera' | 'serial' | 'hid'

export interface HardwareDiscoveryItem {
  device_ref: string
  label: string
  kind: string
  capabilities: HardwareDiscoveryCapability[]
  is_default: boolean
  metadata?: Record<string, unknown>
}

export interface HardwareDiscoveryResponse {
  bus: HardwareDiscoveryBus
  capability: HardwareDiscoveryCapability
  items: HardwareDiscoveryItem[]
}

export async function getHardwareConfig(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<HardwareSegment>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<HardwareSegment>(baseUrl, '/api/config/hardware', {
    pairingCode: pairingCode?.trim(),
  })
}

export async function saveHardwareConfig(
  baseUrl: string,
  pairingCode: string,
  body: HardwareSegment,
): Promise<ApiResult<{ ok: boolean }>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return request<{ ok: boolean }>(baseUrl, '/api/config/hardware', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function discoverHardware(
  baseUrl: string,
  pairingCode: string | undefined,
  bus: HardwareDiscoveryBus,
  capability: HardwareDiscoveryCapability,
): Promise<ApiResult<HardwareDiscoveryResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<HardwareDiscoveryResponse>(
    baseUrl,
    `/api/hardware/discovery?bus=${encodeURIComponent(bus)}&capability=${encodeURIComponent(capability)}`,
    {
      pairingCode: pairingCode?.trim(),
    },
  )
}
