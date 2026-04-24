import { requestProtected, API_ERROR } from '../client.ts'
import type {
  ChannelsConfigView,
  LlmConfigSegment,
  ChannelsConfigSegment,
  SystemConfigSegment,
} from '../../types/appConfig'
import {
  normalizeLlmConfigFromDevice,
  normalizeSystemConfigFromDevice,
} from '../../types/appConfig.ts'
import type {
  AccountCapability,
  AccountConfigSaveRequest,
  AccountDeleteResult,
  AccountDetail,
  AccountListFilters,
  AccountProbeResult,
  AccountRevokeRequest,
  AccountRevokeResult,
  AccountSummaryListResponse,
  AccountUpsertRequest,
  CapabilityStatus,
  CapabilityStatusListResponse,
  ProviderCatalogResponse,
} from '../../types/accountConfig'
import {
  normalizeAccountDetailFromDevice,
  normalizeAccountProbeResultFromDevice,
  normalizeAccountSummaryListResponseFromDevice,
  normalizeCapabilityStatusListResponseFromDevice,
  normalizeProviderCatalogResponseFromDevice,
} from '../../types/accountConfig.ts'
import type { ApiResult } from '../client.ts'

function buildConfigQuery(path: string, query: Record<string, string | undefined>): string {
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(query)) {
    if (!value?.trim()) continue
    params.set(key, value.trim())
  }
  const encoded = params.toString()
  return encoded ? `${path}?${encoded}` : path
}

function normalizeOkData<T>(
  result: ApiResult<unknown>,
  normalize: (data: unknown) => T,
): ApiResult<T> {
  if (!result.ok) return result as ApiResult<T>
  return { ...result, data: normalize(result.data) }
}

export async function getLlm(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<LlmConfigSegment>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(baseUrl, '/api/config/llm', {
    pairingCode: pairingCode?.trim(),
  })
  return normalizeOkData(result, normalizeLlmConfigFromDevice)
}

export async function getChannels(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<ChannelsConfigView>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<ChannelsConfigView>(baseUrl, '/api/config/channels', {
    pairingCode: pairingCode?.trim(),
  })
}

export async function getSystem(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<SystemConfigSegment>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(baseUrl, '/api/config/system', {
    pairingCode: pairingCode?.trim(),
  })
  return normalizeOkData(result, normalizeSystemConfigFromDevice)
}

export async function saveLlm(
  baseUrl: string,
  pairingCode: string,
  body: LlmConfigSegment,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/config/llm', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function saveChannels(
  baseUrl: string,
  pairingCode: string,
  body: ChannelsConfigSegment,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/config/channels', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function saveSystem(
  baseUrl: string,
  pairingCode: string,
  body: SystemConfigSegment,
): Promise<ApiResult<void>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<void>(baseUrl, '/api/config/system', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function getProviders(
  baseUrl: string,
  pairingCode?: string,
  filters?: { capability?: AccountCapability },
): Promise<ApiResult<ProviderCatalogResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(
    baseUrl,
    buildConfigQuery('/api/config/providers', {
      capability: filters?.capability,
    }),
    {
      pairingCode: pairingCode?.trim(),
    },
  )
  return normalizeOkData(result, normalizeProviderCatalogResponseFromDevice)
}

export async function getCapabilities(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<CapabilityStatusListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(baseUrl, '/api/config/capabilities', {
    pairingCode: pairingCode?.trim(),
  })
  return normalizeOkData(result, normalizeCapabilityStatusListResponseFromDevice)
}

export async function getCapability(
  baseUrl: string,
  pairingCode: string | undefined,
  capability: AccountCapability,
): Promise<ApiResult<CapabilityStatus>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<CapabilityStatus>(
    baseUrl,
    `/api/config/capabilities/${encodeURIComponent(capability)}`,
    {
      pairingCode: pairingCode?.trim(),
    },
  )
}

export async function getAccounts(
  baseUrl: string,
  pairingCode?: string,
  filters?: AccountListFilters,
): Promise<ApiResult<AccountSummaryListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(
    baseUrl,
    buildConfigQuery('/api/config/accounts', {
      capability: filters?.capability,
      provider_kind: filters?.providerKind,
    }),
    {
      pairingCode: pairingCode?.trim(),
    },
  )
  return normalizeOkData(result, normalizeAccountSummaryListResponseFromDevice)
}

export async function createAccount(
  baseUrl: string,
  pairingCode: string,
  body: AccountUpsertRequest,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  const result = await requestProtected<unknown>(baseUrl, '/api/config/accounts', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
  return normalizeOkData(result, normalizeAccountDetailFromDevice)
}

export async function getAccount(
  baseUrl: string,
  pairingCode: string | undefined,
  accountKey: string,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}`,
    {
      pairingCode: pairingCode?.trim(),
    },
  )
  return normalizeOkData(result, normalizeAccountDetailFromDevice)
}

export async function saveAccountConfig(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
  body: AccountConfigSaveRequest,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  const result = await requestProtected<unknown>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}/config`,
    {
      method: 'POST',
      body,
      pairingCode: pairingCode.trim(),
    },
  )
  return normalizeOkData(result, normalizeAccountDetailFromDevice)
}

export async function probeAccount(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
): Promise<ApiResult<AccountProbeResult>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  const result = await requestProtected<unknown>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}/probe`,
    {
      method: 'POST',
      pairingCode: pairingCode.trim(),
    },
  )
  return normalizeOkData(result, normalizeAccountProbeResultFromDevice)
}

export async function revokeAccount(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
  body?: AccountRevokeRequest,
): Promise<ApiResult<AccountRevokeResult>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<AccountRevokeResult>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}/revoke`,
    {
      method: 'POST',
      body,
      pairingCode: pairingCode.trim(),
    },
  )
}

export async function deleteAccount(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
): Promise<ApiResult<AccountDeleteResult>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<AccountDeleteResult>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}`,
    {
      method: 'DELETE',
      pairingCode: pairingCode.trim(),
    },
  )
}
