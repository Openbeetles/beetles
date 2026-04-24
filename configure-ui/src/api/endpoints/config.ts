import { requestProtected, API_ERROR } from '../client.ts'
import type {
  ChannelsConfigView,
  LlmConfigSegment,
  ChannelsConfigSegment,
  SystemConfigSegment,
} from '../../types/appConfig'
import type {
  AccountCapability,
  AccountConfigSaveRequest,
  AccountDeleteResult,
  AccountDetail,
  AccountListFilters,
  AccountProbeResult,
  AccountRevokeRequest,
  AccountRevokeResult,
  AccountSummary,
  AccountSummaryListResponse,
  AccountUpsertRequest,
  CapabilityStatus,
  CapabilityStatusListResponse,
  ProviderCatalogItem,
  ProviderCatalogResponse,
} from '../../types/accountConfig'
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

function objectRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : {}
}

function normalizeListResponse<T>(
  data: unknown,
  legacyArrayKeys: string[] = [],
): { count: number; items: T[] } {
  const record = objectRecord(data)
  let rawItems = record.items
  if (!Array.isArray(rawItems)) {
    for (const key of legacyArrayKeys) {
      if (Array.isArray(record[key])) {
        rawItems = record[key]
        break
      }
    }
  }
  const items = Array.isArray(rawItems) ? (rawItems as T[]) : []
  return {
    count:
      typeof record.count === 'number' && Number.isFinite(record.count)
        ? record.count
        : items.length,
    items,
  }
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
  return requestProtected<LlmConfigSegment>(baseUrl, '/api/config/llm', {
    pairingCode: pairingCode?.trim(),
  })
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
  return requestProtected<SystemConfigSegment>(baseUrl, '/api/config/system', {
    pairingCode: pairingCode?.trim(),
  })
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
  return normalizeOkData(result, (data) =>
    normalizeListResponse<ProviderCatalogItem>(data, ['providers']),
  )
}

export async function getCapabilities(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<CapabilityStatusListResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const result = await requestProtected<unknown>(baseUrl, '/api/config/capabilities', {
    pairingCode: pairingCode?.trim(),
  })
  return normalizeOkData(result, (data) =>
    normalizeListResponse<CapabilityStatus>(data),
  )
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
  return normalizeOkData(result, (data) =>
    normalizeListResponse<AccountSummary>(data, ['accounts']),
  )
}

export async function createAccount(
  baseUrl: string,
  pairingCode: string,
  body: AccountUpsertRequest,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<AccountDetail>(baseUrl, '/api/config/accounts', {
    method: 'POST',
    body,
    pairingCode: pairingCode.trim(),
  })
}

export async function getAccount(
  baseUrl: string,
  pairingCode: string | undefined,
  accountKey: string,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return requestProtected<AccountDetail>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}`,
    {
      pairingCode: pairingCode?.trim(),
    },
  )
}

export async function saveAccountConfig(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
  body: AccountConfigSaveRequest,
): Promise<ApiResult<AccountDetail>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<AccountDetail>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}/config`,
    {
      method: 'POST',
      body,
      pairingCode: pairingCode.trim(),
    },
  )
}

export async function probeAccount(
  baseUrl: string,
  pairingCode: string,
  accountKey: string,
): Promise<ApiResult<AccountProbeResult>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<AccountProbeResult>(
    baseUrl,
    `/api/config/accounts/${encodeURIComponent(accountKey)}/probe`,
    {
      method: 'POST',
      pairingCode: pairingCode.trim(),
    },
  )
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
