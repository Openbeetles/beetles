import type {
  AccountFieldState,
  ProviderCreateFieldSchema,
  ProviderFieldSchema,
} from "../types/accountConfig";

type ProviderFieldTranslator = (
  key: string,
  options?: { defaultValue?: string },
) => string;

type ProviderFieldLike =
  | ProviderCreateFieldSchema
  | ProviderFieldSchema
  | AccountFieldState;

const PROVIDER_FIELD_LABEL_KEYS: Record<string, string> = {
  identity_class: "accounts.identityLabel",
  access_token: "accounts.providerFieldLabels.access_token",
  refresh_token: "accounts.providerFieldLabels.refresh_token",
  token_endpoint: "accounts.providerFieldLabels.token_endpoint",
  external_account_id: "accounts.providerFieldLabels.external_account_id",
  mail_username: "accounts.providerFieldLabels.mail_username",
  mail_imap_host: "accounts.providerFieldLabels.mail_imap_host",
  mail_imap_port: "accounts.providerFieldLabels.mail_imap_port",
  mail_imap_mailbox: "accounts.providerFieldLabels.mail_imap_mailbox",
  mail_draft_mailbox: "accounts.providerFieldLabels.mail_draft_mailbox",
  mail_imap_tls: "accounts.providerFieldLabels.mail_imap_tls",
  mail_smtp_host: "accounts.providerFieldLabels.mail_smtp_host",
  mail_smtp_port: "accounts.providerFieldLabels.mail_smtp_port",
  mail_smtp_tls: "accounts.providerFieldLabels.mail_smtp_tls",
  mail_from_address: "accounts.providerFieldLabels.mail_from_address",
  mail_from_name: "accounts.providerFieldLabels.mail_from_name",
  mail_corp_id: "accounts.providerFieldLabels.mail_corp_id",
  mail_base_url: "accounts.providerFieldLabels.mail_base_url",
  calendar_username: "accounts.providerFieldLabels.calendar_username",
  calendar_base_url: "accounts.providerFieldLabels.calendar_base_url",
  calendar_root_path: "accounts.providerFieldLabels.calendar_root_path",
  calendar_id: "accounts.providerFieldLabels.calendar_id",
  calendar_app_id: "accounts.providerFieldLabels.calendar_app_id",
  calendar_corp_id: "accounts.providerFieldLabels.calendar_corp_id",
  documents_username: "accounts.providerFieldLabels.documents_username",
  documents_base_url: "accounts.providerFieldLabels.documents_base_url",
  documents_root_path: "accounts.providerFieldLabels.documents_root_path",
  documents_app_id: "accounts.providerFieldLabels.documents_app_id",
  documents_corp_id: "accounts.providerFieldLabels.documents_corp_id",
  documents_space_id: "accounts.providerFieldLabels.documents_space_id",
  documents_drive_id: "accounts.providerFieldLabels.documents_drive_id",
  contacts_app_id: "accounts.providerFieldLabels.contacts_app_id",
  contacts_base_url: "accounts.providerFieldLabels.contacts_base_url",
  contacts_corp_id: "accounts.providerFieldLabels.contacts_corp_id",
};

const PROVIDER_FIELD_DESCRIPTION_KEYS: Record<string, string> = {
  identity_class: "accounts.providerFieldDescriptions.identity_class",
  access_token: "accounts.providerFieldDescriptions.access_token",
  refresh_token: "accounts.providerFieldDescriptions.refresh_token",
  token_endpoint: "accounts.providerFieldDescriptions.token_endpoint",
  external_account_id: "accounts.providerFieldDescriptions.external_account_id",
  mail_username: "accounts.providerFieldDescriptions.mail_username",
  mail_imap_host: "accounts.providerFieldDescriptions.mail_imap_host",
  mail_imap_port: "accounts.providerFieldDescriptions.mail_imap_port",
  mail_imap_mailbox: "accounts.providerFieldDescriptions.mail_imap_mailbox",
  mail_draft_mailbox: "accounts.providerFieldDescriptions.mail_draft_mailbox",
  mail_imap_tls: "accounts.providerFieldDescriptions.mail_imap_tls",
  mail_smtp_host: "accounts.providerFieldDescriptions.mail_smtp_host",
  mail_smtp_port: "accounts.providerFieldDescriptions.mail_smtp_port",
  mail_smtp_tls: "accounts.providerFieldDescriptions.mail_smtp_tls",
  mail_from_address: "accounts.providerFieldDescriptions.mail_from_address",
  mail_from_name: "accounts.providerFieldDescriptions.mail_from_name",
  mail_corp_id: "accounts.providerFieldDescriptions.mail_corp_id",
  mail_base_url: "accounts.providerFieldDescriptions.mail_base_url",
  calendar_username: "accounts.providerFieldDescriptions.calendar_username",
  calendar_base_url: "accounts.providerFieldDescriptions.calendar_base_url",
  calendar_root_path: "accounts.providerFieldDescriptions.calendar_root_path",
  calendar_id: "accounts.providerFieldDescriptions.calendar_id",
  calendar_app_id: "accounts.providerFieldDescriptions.calendar_app_id",
  calendar_corp_id: "accounts.providerFieldDescriptions.calendar_corp_id",
  documents_username: "accounts.providerFieldDescriptions.documents_username",
  documents_base_url: "accounts.providerFieldDescriptions.documents_base_url",
  documents_root_path: "accounts.providerFieldDescriptions.documents_root_path",
  documents_app_id: "accounts.providerFieldDescriptions.documents_app_id",
  documents_corp_id: "accounts.providerFieldDescriptions.documents_corp_id",
  documents_space_id: "accounts.providerFieldDescriptions.documents_space_id",
  documents_drive_id: "accounts.providerFieldDescriptions.documents_drive_id",
  contacts_app_id: "accounts.providerFieldDescriptions.contacts_app_id",
  contacts_base_url: "accounts.providerFieldDescriptions.contacts_base_url",
  contacts_corp_id: "accounts.providerFieldDescriptions.contacts_corp_id",
};

export function localizeProviderFieldLabel(
  t: ProviderFieldTranslator,
  fieldKey: string,
  fallback: string,
): string {
  const key = PROVIDER_FIELD_LABEL_KEYS[fieldKey];
  return key ? t(key, { defaultValue: fallback }) : fallback;
}

export function localizeProviderFieldDescription(
  t: ProviderFieldTranslator,
  fieldKey: string,
  fallback: string,
): string {
  const key = PROVIDER_FIELD_DESCRIPTION_KEYS[fieldKey];
  return key ? t(key, { defaultValue: fallback }) : fallback;
}

export function localizeProviderField<TField extends ProviderFieldLike>(
  t: ProviderFieldTranslator,
  field: TField,
): TField {
  return {
    ...field,
    label: localizeProviderFieldLabel(t, field.key, field.label),
    description: localizeProviderFieldDescription(t, field.key, field.description),
  };
}
