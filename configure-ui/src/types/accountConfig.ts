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
  label: string
}

export interface ProviderCreateFieldSchema {
  key: string
  label: string
  description: string
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
  label: string
  description: string
  location: ProviderFieldLocation
  value_kind: ProviderFieldValueKind
  required: boolean
  secret: boolean
  default_value?: string
}

export interface ProviderCatalogItem {
  provider_kind: string
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
