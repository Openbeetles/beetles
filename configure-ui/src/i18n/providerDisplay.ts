type ProviderTranslator = (
  key: string,
  options?: { defaultValue?: string },
) => string;

type ProviderDisplayInput =
  | string
  | {
      providerKind: string;
      displayNameKey?: string | null;
    };

const ACCOUNT_PROVIDER_LABEL_KEYS: Record<string, string> = {
  imap_smtp: "accounts.providers.imap_smtp",
  feishu_mail: "accounts.providers.feishu_mail",
  wecom_mail: "accounts.providers.wecom_mail",
  microsoft365_mail: "accounts.providers.microsoft365_mail",
  google_mail: "accounts.providers.google_mail",
  caldav: "accounts.providers.caldav",
  feishu_calendar: "accounts.providers.feishu_calendar",
  wecom_calendar: "accounts.providers.wecom_calendar",
  microsoft365_calendar: "accounts.providers.microsoft365_calendar",
  google_calendar: "accounts.providers.google_calendar",
  webdav: "accounts.providers.webdav",
  feishu_documents: "accounts.providers.feishu_documents",
  wecom_documents: "accounts.providers.wecom_documents",
  microsoft365_documents: "accounts.providers.microsoft365_documents",
  google_documents: "accounts.providers.google_documents",
  feishu_contacts_directory: "accounts.providers.feishu_contacts_directory",
  wecom_contacts_directory: "accounts.providers.wecom_contacts_directory",
  microsoft365_contacts_directory: "accounts.providers.microsoft365_contacts_directory",
  google_contacts_directory: "accounts.providers.google_contacts_directory",
};

export function localizeAccountProviderName(
  t: ProviderTranslator,
  provider: ProviderDisplayInput,
): string {
  const providerKind =
    typeof provider === "string" ? provider : provider.providerKind;
  const displayNameKey =
    typeof provider === "string" ? undefined : provider.displayNameKey;
  if (displayNameKey) {
    return t(displayNameKey, { defaultValue: providerKind });
  }
  const key = ACCOUNT_PROVIDER_LABEL_KEYS[providerKind];
  if (!key) return providerKind;
  return t(key, { defaultValue: providerKind });
}
