type ProviderTranslator = (
  key: string,
  options?: { defaultValue?: string },
) => string;

const ACCOUNT_PROVIDER_LABEL_KEYS: Record<string, string> = {
  imap_smtp: "accounts.providers.imap_smtp",
  feishu_mail: "accounts.providers.feishu_mail",
  wecom_mail: "accounts.providers.wecom_mail",
  caldav: "accounts.providers.caldav",
  feishu_calendar: "accounts.providers.feishu_calendar",
  wecom_calendar: "accounts.providers.wecom_calendar",
  webdav: "accounts.providers.webdav",
  feishu_documents: "accounts.providers.feishu_documents",
  wecom_documents: "accounts.providers.wecom_documents",
  feishu_contacts_directory: "accounts.providers.feishu_contacts_directory",
  wecom_contacts_directory: "accounts.providers.wecom_contacts_directory",
};

export function localizeAccountProviderName(
  t: ProviderTranslator,
  providerKind: string,
): string {
  const key = ACCOUNT_PROVIDER_LABEL_KEYS[providerKind];
  if (!key) return providerKind;
  return t(key, { defaultValue: providerKind });
}
