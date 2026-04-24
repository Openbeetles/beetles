export type AccountCapability = 'mail' | 'calendar' | 'documents' | 'contacts_directory'

export type AccountIdentityClass = 'work' | 'personal' | 'family' | 'shared' | 'other'

export type AccountReadiness =
  | 'needs_configuration'
  | 'ready_for_probe'
  | 'probe_unavailable'
  | 'ready'

export type AccountNextAction = 'configure_account' | 'probe' | 'none'

export type CapabilitySelectionStatus = 'selected' | 'ambiguous' | 'missing'

export type CapabilityNextAction =
  | 'create_account'
  | 'select_default_account'
  | 'configure_account'
  | 'probe'
  | 'review_runtime_error'
  | 'none'

export type ProviderFieldLocation =
  | 'access_token'
  | 'refresh_token'
  | 'token_endpoint'
  | 'external_account_id'
  | 'metadata'

export type ProviderFieldValueKind =
  | 'secret'
  | 'text'
  | 'url'
  | 'hostname'
  | 'identifier'
  | 'path'
  | 'email'
  | 'integer'
  | 'boolean'

export type ProbeDisposition = 'ready' | 'missing_credential' | 'unsupported'

export interface ProviderFieldOption {
  value: string
  label_key: string
  label?: string
}

export interface ProviderCreateFieldSchema {
  key: string
  label_key: string
  description_key: string
  label?: string
  description?: string
  value_kind: ProviderFieldValueKind
  required: boolean
  secret: boolean
  multiple: boolean
  default_value?: string
  default_values: string[]
  options: ProviderFieldOption[]
}

export interface ProviderFieldSchema {
  key: string
  label_key: string
  description_key: string
  label?: string
  description?: string
  location: ProviderFieldLocation
  value_kind: ProviderFieldValueKind
  required: boolean
  secret: boolean
  default_value?: string
}

export interface ProviderCatalogItem {
  provider_kind: string
  display_name_key: string
  capabilities: AccountCapability[]
  account_fields: ProviderCreateFieldSchema[]
  config_fields: ProviderFieldSchema[]
}

export interface ProviderCatalogResponse {
  count: number
  items: ProviderCatalogItem[]
}

export interface AccountSummary {
  account_key: string
  provider_kind: string
  display_name_key: string
  account_label: string
  identity_class: AccountIdentityClass
  enabled_capabilities: AccountCapability[]
  selected_for_capabilities: AccountCapability[]
  readiness: AccountReadiness
  next_action: AccountNextAction
  missing_fields_count: number
  has_runtime_error: boolean
}

export interface AccountSummaryListResponse {
  count: number
  items: AccountSummary[]
}

export interface AccountRuntimeStatus {
  account_key: string
  probe_ok: boolean
  last_error: string
  last_probe_at_unix_secs: number
  last_activity_kind: string
  last_activity_ok: boolean
  last_activity_at_unix_secs: number
  updated_at: number
}

export interface AccountCredentialStatus {
  present: boolean
  configured: boolean
  updated_at?: number
}

export interface AccountAuthorityStatus {
  account_key: string
  provider_kind: string
  display_name_key: string
  external_account_id: string
  account_label: string
  identity_class: AccountIdentityClass
  enabled_capabilities: AccountCapability[]
  selected_for_capabilities: AccountCapability[]
  credential_status?: AccountCredentialStatus
  runtime_status?: AccountRuntimeStatus
}

export interface AccountAssessment {
  account_key: string
  provider_kind: string
  enabled_capabilities: AccountCapability[]
  credential_present: boolean
  credential_configured: boolean
  probe_supported: boolean
  missing_fields: string[]
  missing_field_details: ProviderFieldSchema[]
  readiness: AccountReadiness
  next_action: AccountNextAction
  runtime_status?: AccountRuntimeStatus
}

export interface AccountFieldState extends ProviderFieldSchema {
  current_value?: string
  configured: boolean
}

export interface AccountDetail {
  account: AccountAuthorityStatus
  assessment: AccountAssessment
  fields: AccountFieldState[]
}

export interface CapabilityStatus {
  capability: AccountCapability
  default_account_key?: string
  selection_status: CapabilitySelectionStatus
  selected_account_key?: string
  ready: boolean
  next_action: CapabilityNextAction
  accounts: AccountSummary[]
}

export interface CapabilityStatusListResponse {
  count: number
  items: CapabilityStatus[]
}

export interface AccountConfigSaveRequest {
  fields?: Record<string, string>
  clear_fields?: string[]
}

export type AccountUpsertScalar = string | number | boolean
export type AccountUpsertMetadata = Record<string, AccountUpsertScalar>

export interface AccountUpsertRequest {
  provider_kind?: string
  provider?: string
  capability?: AccountCapability
  identity_class: AccountIdentityClass
  account_label?: string
  display_name?: string
  external_account_id?: string
  email?: string
  account_id?: string
  username?: string
  password?: string
  access_token?: string
  refresh_token?: string
  token_endpoint?: string
  mail_username?: string
  mail_from_address?: string
  imap_host?: string
  imap_port?: AccountUpsertScalar
  imap_tls?: boolean
  smtp_host?: string
  smtp_port?: AccountUpsertScalar
  smtp_tls?: boolean
  metadata?: AccountUpsertMetadata
}

export interface AccountProbeResult {
  account_key: string
  provider_kind: string
  configured: boolean
  disposition: ProbeDisposition
  reason: string
}

export interface AccountRevokeRequest {
  clear_runtime_status?: boolean
}

export interface AccountRevokeResult {
  ok: boolean
  account_key: string
  cleared_runtime_status: boolean
}

export interface AccountDeleteResult {
  ok: boolean
  account_key: string
  deleted: boolean
}

export interface AccountListFilters {
  capability?: AccountCapability
  providerKind?: string
}

const ACCOUNT_CAPABILITIES: readonly AccountCapability[] = [
  'mail',
  'calendar',
  'documents',
  'contacts_directory',
]

const ACCOUNT_IDENTITY_CLASSES: readonly AccountIdentityClass[] = [
  'work',
  'personal',
  'family',
  'shared',
  'other',
]

const ACCOUNT_READINESS_VALUES: readonly AccountReadiness[] = [
  'needs_configuration',
  'ready_for_probe',
  'probe_unavailable',
  'ready',
]

const ACCOUNT_NEXT_ACTION_VALUES: readonly AccountNextAction[] = [
  'configure_account',
  'probe',
  'none',
]

const CAPABILITY_SELECTION_STATUS_VALUES: readonly CapabilitySelectionStatus[] = [
  'selected',
  'ambiguous',
  'missing',
]

const CAPABILITY_NEXT_ACTION_VALUES: readonly CapabilityNextAction[] = [
  'create_account',
  'select_default_account',
  'configure_account',
  'probe',
  'review_runtime_error',
  'none',
]

const PROVIDER_FIELD_LOCATIONS: readonly ProviderFieldLocation[] = [
  'access_token',
  'refresh_token',
  'token_endpoint',
  'external_account_id',
  'metadata',
]

const PROVIDER_FIELD_VALUE_KINDS: readonly ProviderFieldValueKind[] = [
  'secret',
  'text',
  'url',
  'hostname',
  'identifier',
  'path',
  'email',
  'integer',
  'boolean',
]

const PROBE_DISPOSITIONS: readonly ProbeDisposition[] = [
  'ready',
  'missing_credential',
  'unsupported',
]

function objectRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : {}
}

function stringValue(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

function numberValue(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function booleanValue(value: unknown, fallback = false): boolean {
  return typeof value === 'boolean' ? value : fallback
}

function normalizeStringList(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value.filter((item): item is string => typeof item === 'string')
}

function normalizeEnum<T extends string>(
  value: unknown,
  allowed: readonly T[],
  fallback: T,
): T {
  return normalizeEnumOrNull(value, allowed) ?? fallback
}

function normalizeEnumOrNull<T extends string>(
  value: unknown,
  allowed: readonly T[],
): T | null {
  return typeof value === 'string' && (allowed as readonly string[]).includes(value)
    ? (value as T)
    : null
}

function normalizeCapabilityList(value: unknown): AccountCapability[] {
  if (!Array.isArray(value)) return []
  return value
    .map((item) => normalizeEnumOrNull(item, ACCOUNT_CAPABILITIES))
    .filter((item): item is AccountCapability => item !== null)
}

function normalizeProviderFieldOption(value: unknown): ProviderFieldOption | null {
  const record = objectRecord(value)
  const optionValue = stringValue(record.value)
  if (!optionValue) return null
  return {
    value: optionValue,
    label_key: stringValue(record.label_key),
    label: typeof record.label === 'string' ? record.label : undefined,
  }
}

function normalizeProviderCreateFieldSchema(value: unknown): ProviderCreateFieldSchema | null {
  const record = objectRecord(value)
  const key = stringValue(record.key)
  if (!key) return null
  return {
    key,
    label_key: stringValue(record.label_key),
    description_key: stringValue(record.description_key),
    label: typeof record.label === 'string' ? record.label : undefined,
    description: typeof record.description === 'string' ? record.description : undefined,
    value_kind: normalizeEnum(record.value_kind, PROVIDER_FIELD_VALUE_KINDS, 'text'),
    required: booleanValue(record.required),
    secret: booleanValue(record.secret),
    multiple: booleanValue(record.multiple),
    default_value:
      typeof record.default_value === 'string' ? record.default_value : undefined,
    default_values: normalizeStringList(record.default_values),
    options: Array.isArray(record.options)
      ? record.options
          .map((option) => normalizeProviderFieldOption(option))
          .filter((option): option is ProviderFieldOption => option !== null)
      : [],
  }
}

function normalizeProviderFieldSchema(value: unknown): ProviderFieldSchema | null {
  const record = objectRecord(value)
  const key = stringValue(record.key)
  if (!key) return null
  return {
    key,
    label_key: stringValue(record.label_key),
    description_key: stringValue(record.description_key),
    label: typeof record.label === 'string' ? record.label : undefined,
    description: typeof record.description === 'string' ? record.description : undefined,
    location: normalizeEnum(record.location, PROVIDER_FIELD_LOCATIONS, 'metadata'),
    value_kind: normalizeEnum(record.value_kind, PROVIDER_FIELD_VALUE_KINDS, 'text'),
    required: booleanValue(record.required),
    secret: booleanValue(record.secret),
    default_value:
      typeof record.default_value === 'string' ? record.default_value : undefined,
  }
}

function normalizeAccountRuntimeStatus(value: unknown): AccountRuntimeStatus | undefined {
  const record = objectRecord(value)
  if (Object.keys(record).length === 0) return undefined
  return {
    account_key: stringValue(record.account_key),
    probe_ok: booleanValue(record.probe_ok),
    last_error: stringValue(record.last_error),
    last_probe_at_unix_secs: numberValue(record.last_probe_at_unix_secs),
    last_activity_kind: stringValue(record.last_activity_kind),
    last_activity_ok: booleanValue(record.last_activity_ok),
    last_activity_at_unix_secs: numberValue(record.last_activity_at_unix_secs),
    updated_at: numberValue(record.updated_at),
  }
}

function normalizeCredentialStatus(value: unknown): AccountCredentialStatus | undefined {
  const record = objectRecord(value)
  if (Object.keys(record).length === 0) return undefined
  const updatedAt =
    typeof record.updated_at === 'number' && Number.isFinite(record.updated_at)
      ? record.updated_at
      : undefined
  return {
    present: booleanValue(record.present),
    configured: booleanValue(record.configured),
    ...(updatedAt === undefined ? {} : { updated_at: updatedAt }),
  }
}

function normalizeAccountSummary(value: unknown): AccountSummary | null {
  const record = objectRecord(value)
  const accountKey = stringValue(record.account_key)
  if (!accountKey) return null
  return {
    account_key: accountKey,
    provider_kind: stringValue(record.provider_kind),
    display_name_key: stringValue(record.display_name_key),
    account_label: stringValue(record.account_label),
    identity_class: normalizeEnum(record.identity_class, ACCOUNT_IDENTITY_CLASSES, 'other'),
    enabled_capabilities: normalizeCapabilityList(record.enabled_capabilities),
    selected_for_capabilities: normalizeCapabilityList(record.selected_for_capabilities),
    readiness: normalizeEnum(
      record.readiness,
      ACCOUNT_READINESS_VALUES,
      'needs_configuration',
    ),
    next_action: normalizeEnum(
      record.next_action,
      ACCOUNT_NEXT_ACTION_VALUES,
      'configure_account',
    ),
    missing_fields_count: numberValue(record.missing_fields_count),
    has_runtime_error: booleanValue(record.has_runtime_error),
  }
}

function normalizeAccountAuthorityStatus(value: unknown): AccountAuthorityStatus {
  const record = objectRecord(value)
  return {
    account_key: stringValue(record.account_key),
    provider_kind: stringValue(record.provider_kind),
    display_name_key: stringValue(record.display_name_key),
    external_account_id: stringValue(record.external_account_id),
    account_label: stringValue(record.account_label),
    identity_class: normalizeEnum(record.identity_class, ACCOUNT_IDENTITY_CLASSES, 'other'),
    enabled_capabilities: normalizeCapabilityList(record.enabled_capabilities),
    selected_for_capabilities: normalizeCapabilityList(record.selected_for_capabilities),
    credential_status: normalizeCredentialStatus(record.credential_status),
    runtime_status: normalizeAccountRuntimeStatus(record.runtime_status),
  }
}

function normalizeAccountAssessment(
  value: unknown,
  account: AccountAuthorityStatus,
): AccountAssessment {
  const record = objectRecord(value)
  const enabledCapabilities = Array.isArray(record.enabled_capabilities)
    ? normalizeCapabilityList(record.enabled_capabilities)
    : account.enabled_capabilities
  return {
    account_key: stringValue(record.account_key, account.account_key),
    provider_kind: stringValue(record.provider_kind, account.provider_kind),
    enabled_capabilities: enabledCapabilities,
    credential_present: booleanValue(record.credential_present),
    credential_configured: booleanValue(record.credential_configured),
    probe_supported: booleanValue(record.probe_supported),
    missing_fields: normalizeStringList(record.missing_fields),
    missing_field_details: Array.isArray(record.missing_field_details)
      ? record.missing_field_details
          .map((field) => normalizeProviderFieldSchema(field))
          .filter((field): field is ProviderFieldSchema => field !== null)
      : [],
    readiness: normalizeEnum(
      record.readiness,
      ACCOUNT_READINESS_VALUES,
      'needs_configuration',
    ),
    next_action: normalizeEnum(
      record.next_action,
      ACCOUNT_NEXT_ACTION_VALUES,
      'configure_account',
    ),
    runtime_status: normalizeAccountRuntimeStatus(record.runtime_status),
  }
}

function normalizeAccountFieldState(value: unknown): AccountFieldState | null {
  const field = normalizeProviderFieldSchema(value)
  if (!field) return null
  const record = objectRecord(value)
  return {
    ...field,
    current_value:
      typeof record.current_value === 'string' ? record.current_value : undefined,
    configured: booleanValue(record.configured),
  }
}

function normalizeCountedItems<T>(
  data: unknown,
  legacyArrayKeys: string[],
  normalizeItem: (item: unknown) => T | null,
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
  const items = Array.isArray(rawItems)
    ? rawItems
        .map((item) => normalizeItem(item))
        .filter((item): item is T => item !== null)
    : []
  return {
    count:
      typeof record.count === 'number' && Number.isFinite(record.count)
        ? record.count
        : items.length,
    items,
  }
}

export function normalizeProviderCatalogItem(value: unknown): ProviderCatalogItem | null {
  const record = objectRecord(value)
  const providerKind = stringValue(record.provider_kind)
  if (!providerKind) return null
  return {
    provider_kind: providerKind,
    display_name_key: stringValue(record.display_name_key),
    capabilities: normalizeCapabilityList(record.capabilities),
    account_fields: Array.isArray(record.account_fields)
      ? record.account_fields
          .map((field) => normalizeProviderCreateFieldSchema(field))
          .filter((field): field is ProviderCreateFieldSchema => field !== null)
      : [],
    config_fields: Array.isArray(record.config_fields)
      ? record.config_fields
          .map((field) => normalizeProviderFieldSchema(field))
          .filter((field): field is ProviderFieldSchema => field !== null)
      : [],
  }
}

export function normalizeProviderCatalogResponseFromDevice(
  data: unknown,
): ProviderCatalogResponse {
  return normalizeCountedItems(data, ['providers'], normalizeProviderCatalogItem)
}

export function normalizeAccountSummaryListResponseFromDevice(
  data: unknown,
): AccountSummaryListResponse {
  return normalizeCountedItems(data, ['accounts'], normalizeAccountSummary)
}

export function normalizeCapabilityStatusListResponseFromDevice(
  data: unknown,
): CapabilityStatusListResponse {
  return normalizeCountedItems(data, [], (item) => {
    const record = objectRecord(item)
    const capability = normalizeEnumOrNull(record.capability, ACCOUNT_CAPABILITIES)
    if (!capability) return null
    return {
      capability,
      default_account_key:
        typeof record.default_account_key === 'string'
          ? record.default_account_key
          : undefined,
      selection_status: normalizeEnum(
        record.selection_status,
        CAPABILITY_SELECTION_STATUS_VALUES,
        'missing',
      ),
      selected_account_key:
        typeof record.selected_account_key === 'string'
          ? record.selected_account_key
          : undefined,
      ready: booleanValue(record.ready),
      next_action: normalizeEnum(
        record.next_action,
        CAPABILITY_NEXT_ACTION_VALUES,
        'none',
      ),
      accounts: Array.isArray(record.accounts)
        ? record.accounts
            .map((account) => normalizeAccountSummary(account))
            .filter((account): account is AccountSummary => account !== null)
        : [],
    }
  })
}

export function normalizeAccountDetailFromDevice(data: unknown): AccountDetail {
  const record = objectRecord(data)
  const account = normalizeAccountAuthorityStatus(record.account)
  return {
    account,
    assessment: normalizeAccountAssessment(record.assessment, account),
    fields: Array.isArray(record.fields)
      ? record.fields
          .map((field) => normalizeAccountFieldState(field))
          .filter((field): field is AccountFieldState => field !== null)
      : [],
  }
}

export function normalizeAccountProbeResultFromDevice(data: unknown): AccountProbeResult {
  const record = objectRecord(data)
  return {
    account_key: stringValue(record.account_key),
    provider_kind: stringValue(record.provider_kind),
    configured: booleanValue(record.configured),
    disposition: normalizeEnum(record.disposition, PROBE_DISPOSITIONS, 'unsupported'),
    reason: stringValue(record.reason),
  }
}
