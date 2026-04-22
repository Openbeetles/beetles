use crate::calendar::{
    FEISHU_CALENDAR_DEFAULT_BASE_URL, GOOGLE_DEFAULT_CALENDAR_ID, MICROSOFT365_DEFAULT_CALENDAR_ID,
    OFFICE_METADATA_CALENDAR_APP_ID, OFFICE_METADATA_CALENDAR_CORP_ID,
};
use crate::contacts_directory::{
    FEISHU_CONTACTS_DEFAULT_BASE_URL, OFFICE_METADATA_CONTACTS_APP_ID,
    OFFICE_METADATA_CONTACTS_BASE_URL, OFFICE_METADATA_CONTACTS_CORP_ID,
};
use crate::documents::{
    FEISHU_DOCUMENTS_DEFAULT_BASE_URL, OFFICE_METADATA_DOCUMENTS_APP_ID,
    OFFICE_METADATA_DOCUMENTS_BASE_URL, OFFICE_METADATA_DOCUMENTS_CORP_ID,
    OFFICE_METADATA_DOCUMENTS_DRIVE_ID, OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
    OFFICE_METADATA_DOCUMENTS_SPACE_ID, OFFICE_METADATA_DOCUMENTS_USERNAME,
};
use crate::mail::{
    DEFAULT_DRAFT_MAILBOX, DEFAULT_MAILBOX, OFFICE_METADATA_MAIL_BASE_URL,
    OFFICE_METADATA_MAIL_CORP_ID, OFFICE_METADATA_MAIL_DRAFT_MAILBOX,
    OFFICE_METADATA_MAIL_FROM_ADDRESS, OFFICE_METADATA_MAIL_FROM_NAME,
    OFFICE_METADATA_MAIL_IMAP_HOST, OFFICE_METADATA_MAIL_IMAP_MAILBOX,
    OFFICE_METADATA_MAIL_IMAP_PORT, OFFICE_METADATA_MAIL_IMAP_TLS, OFFICE_METADATA_MAIL_SMTP_HOST,
    OFFICE_METADATA_MAIL_SMTP_PORT, OFFICE_METADATA_MAIL_SMTP_TLS, OFFICE_METADATA_MAIL_USERNAME,
};
use serde::{Deserialize, Serialize};

use super::{
    OfficeCapability, GOOGLE_CALENDAR_DEFAULT_BASE_URL, GOOGLE_DRIVE_DEFAULT_BASE_URL,
    GOOGLE_GMAIL_DEFAULT_BASE_URL, GOOGLE_PEOPLE_DEFAULT_BASE_URL,
    MICROSOFT_GRAPH_DEFAULT_BASE_URL, OFFICE_METADATA_CALENDAR_ID, WECOM_DEFAULT_BASE_URL,
};

const OFFICE_METADATA_CALENDAR_USERNAME_FIELD: &str = "calendar_username";
const OFFICE_METADATA_CALENDAR_BASE_URL_FIELD: &str = "calendar_base_url";
const OFFICE_METADATA_CALENDAR_ROOT_PATH_FIELD: &str = "calendar_root_path";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeProviderFieldLocation {
    AccessToken,
    RefreshToken,
    TokenEndpoint,
    ExternalAccountId,
    Metadata,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeProviderFieldValueKind {
    Secret,
    Text,
    Url,
    Hostname,
    Identifier,
    Path,
    Email,
    Integer,
    Boolean,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeProviderFieldSchema {
    pub key: String,
    pub label: String,
    pub label_key: String,
    pub description: String,
    pub description_key: String,
    pub location: OfficeProviderFieldLocation,
    pub value_kind: OfficeProviderFieldValueKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeProviderSchema {
    pub provider_kind: String,
    pub display_name: String,
    pub display_name_key: String,
    #[serde(default)]
    pub capabilities: Vec<OfficeCapability>,
    #[serde(default)]
    pub fields: Vec<OfficeProviderFieldSchema>,
}

pub fn office_provider_display_name_key(provider_kind: &str) -> String {
    format!("accounts.providers.{provider_kind}")
}

fn provider_field_label_key(field_key: &str) -> String {
    match field_key {
        "identity_class" => "accounts.identityLabel".to_string(),
        other => format!("accounts.providerFieldLabels.{other}"),
    }
}

fn provider_field_description_key(field_key: &str) -> String {
    format!("accounts.providerFieldDescriptions.{field_key}")
}

pub fn office_provider_schema(provider_kind: &str) -> Option<OfficeProviderSchema> {
    all_provider_schemas()
        .into_iter()
        .find(|schema| schema.provider_kind == provider_kind)
}

pub fn office_provider_schemas(capability: Option<OfficeCapability>) -> Vec<OfficeProviderSchema> {
    let mut items: Vec<_> = all_provider_schemas()
        .into_iter()
        .filter(|schema| {
            capability
                .map(|value| schema.capabilities.contains(&value))
                .unwrap_or(true)
        })
        .collect();
    items.sort_by(|left, right| left.provider_kind.cmp(&right.provider_kind));
    items
}

fn all_provider_schemas() -> Vec<OfficeProviderSchema> {
    vec![
        imap_smtp_schema(),
        feishu_mail_schema(),
        wecom_mail_schema(),
        microsoft365_mail_schema(),
        google_mail_schema(),
        caldav_schema(),
        feishu_calendar_schema(),
        wecom_calendar_schema(),
        microsoft365_calendar_schema(),
        google_calendar_schema(),
        webdav_schema(),
        feishu_documents_schema(),
        wecom_documents_schema(),
        microsoft365_documents_schema(),
        google_documents_schema(),
        feishu_contacts_schema(),
        wecom_contacts_schema(),
        microsoft365_contacts_schema(),
        google_contacts_schema(),
    ]
}

fn imap_smtp_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "imap_smtp".to_string(),
        display_name: "IMAP / SMTP".to_string(),
        display_name_key: office_provider_display_name_key("imap_smtp"),
        capabilities: vec![OfficeCapability::Mail],
        fields: vec![
            access_token_field(),
            external_account_id_field(
                "Mailbox account id",
                "Optional mailbox identity. If omitted, set mail_username in credential metadata.",
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_USERNAME,
                "Mail username",
                "Explicit mailbox username. Required when external_account_id is empty.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_HOST,
                "IMAP host",
                "Inbound mail server host.",
                OfficeProviderFieldValueKind::Hostname,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_PORT,
                "IMAP port",
                "Inbound mail server port.",
                OfficeProviderFieldValueKind::Integer,
                false,
                Some("993"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_MAILBOX,
                "IMAP mailbox",
                "Default mailbox for list/search/get operations.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                Some(DEFAULT_MAILBOX),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_DRAFT_MAILBOX,
                "Draft mailbox",
                "Mailbox to store draft messages.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                Some(DEFAULT_DRAFT_MAILBOX),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_TLS,
                "IMAP TLS",
                "Whether IMAP uses TLS.",
                OfficeProviderFieldValueKind::Boolean,
                false,
                Some("true"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_HOST,
                "SMTP host",
                "Outbound mail server host.",
                OfficeProviderFieldValueKind::Hostname,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_PORT,
                "SMTP port",
                "Outbound mail server port.",
                OfficeProviderFieldValueKind::Integer,
                false,
                Some("465"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_TLS,
                "SMTP TLS",
                "Whether SMTP uses TLS.",
                OfficeProviderFieldValueKind::Boolean,
                false,
                Some("true"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_ADDRESS,
                "From address",
                "Optional override for the sender email address.",
                OfficeProviderFieldValueKind::Email,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_NAME,
                "From name",
                "Optional override for the sender display name.",
                OfficeProviderFieldValueKind::Text,
                false,
                None,
            ),
        ],
    }
}

fn feishu_mail_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "feishu_mail".to_string(),
        display_name: "Feishu Mail".to_string(),
        display_name_key: office_provider_display_name_key("feishu_mail"),
        capabilities: vec![OfficeCapability::Mail],
        fields: vec![
            access_token_field(),
            external_account_id_field(
                "Mailbox account id",
                "Optional mailbox identity. If omitted, set mail_username in credential metadata.",
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_USERNAME,
                "Mail username",
                "Explicit mailbox username. Required when external_account_id is empty.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_HOST,
                "IMAP host",
                "Feishu IMAP host.",
                OfficeProviderFieldValueKind::Hostname,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_HOST,
                "SMTP host",
                "Feishu SMTP host.",
                OfficeProviderFieldValueKind::Hostname,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_PORT,
                "IMAP port",
                "Feishu IMAP port.",
                OfficeProviderFieldValueKind::Integer,
                false,
                Some("993"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_PORT,
                "SMTP port",
                "Feishu SMTP port.",
                OfficeProviderFieldValueKind::Integer,
                false,
                Some("465"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_IMAP_TLS,
                "IMAP TLS",
                "Whether Feishu IMAP uses TLS.",
                OfficeProviderFieldValueKind::Boolean,
                false,
                Some("true"),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_SMTP_TLS,
                "SMTP TLS",
                "Whether Feishu SMTP uses TLS.",
                OfficeProviderFieldValueKind::Boolean,
                false,
                Some("true"),
            ),
        ],
    }
}

fn wecom_mail_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "wecom_mail".to_string(),
        display_name: "WeCom Mail".to_string(),
        display_name_key: office_provider_display_name_key("wecom_mail"),
        capabilities: vec![OfficeCapability::Mail],
        fields: vec![
            access_token_field(),
            metadata_field(
                OFFICE_METADATA_MAIL_CORP_ID,
                "WeCom corp id",
                "Enterprise WeCom corp id used by the mail API.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_BASE_URL,
                "Mail base URL",
                "Optional API base URL override for WeCom mail.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(WECOM_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_ADDRESS,
                "From address",
                "Optional override for the sender email address.",
                OfficeProviderFieldValueKind::Email,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_NAME,
                "From name",
                "Optional override for the sender display name.",
                OfficeProviderFieldValueKind::Text,
                false,
                None,
            ),
        ],
    }
}

fn microsoft365_mail_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "microsoft365_mail".to_string(),
        display_name: "Microsoft 365 Mail".to_string(),
        display_name_key: office_provider_display_name_key("microsoft365_mail"),
        capabilities: vec![OfficeCapability::Mail],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Mailbox account id",
                "Optional mailbox identity. Leave empty to use the signed-in Microsoft account.",
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_BASE_URL,
                "Microsoft Graph base URL",
                "Optional API base URL override for Microsoft Graph mail.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_ADDRESS,
                "From address",
                "Optional override for the sender email address.",
                OfficeProviderFieldValueKind::Email,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_NAME,
                "From name",
                "Optional override for the sender display name.",
                OfficeProviderFieldValueKind::Text,
                false,
                None,
            ),
        ],
    }
}

fn google_mail_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "google_mail".to_string(),
        display_name: "Google Mail".to_string(),
        display_name_key: office_provider_display_name_key("google_mail"),
        capabilities: vec![OfficeCapability::Mail],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Mailbox account id",
                "Optional mailbox identity. Leave empty to use the signed-in Google account.",
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_BASE_URL,
                "Gmail API base URL",
                "Optional API base URL override for Gmail.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(GOOGLE_GMAIL_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_ADDRESS,
                "From address",
                "Optional override for the sender email address.",
                OfficeProviderFieldValueKind::Email,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_MAIL_FROM_NAME,
                "From name",
                "Optional override for the sender display name.",
                OfficeProviderFieldValueKind::Text,
                false,
                None,
            ),
        ],
    }
}

fn caldav_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "caldav".to_string(),
        display_name: "CalDAV".to_string(),
        display_name_key: office_provider_display_name_key("caldav"),
        capabilities: vec![OfficeCapability::Calendar],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Calendar account id",
                "Optional calendar identity. If omitted, set calendar_username in credential metadata.",
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_USERNAME_FIELD,
                "Calendar username",
                "Explicit CalDAV username. Required when external_account_id is empty.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                "Calendar base URL",
                "CalDAV base URL.",
                OfficeProviderFieldValueKind::Url,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ROOT_PATH_FIELD,
                "Calendar root path",
                "Optional CalDAV collection root path override.",
                OfficeProviderFieldValueKind::Path,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ID,
                "Calendar id",
                "Calendar collection id.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
        ],
    }
}

fn feishu_calendar_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "feishu_calendar".to_string(),
        display_name: "Feishu Calendar".to_string(),
        display_name_key: office_provider_display_name_key("feishu_calendar"),
        capabilities: vec![OfficeCapability::Calendar],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            metadata_field(
                OFFICE_METADATA_CALENDAR_APP_ID,
                "Feishu app id",
                "Feishu app id for calendar API access.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                "Calendar base URL",
                "Optional API base URL override for Feishu calendar.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(FEISHU_CALENDAR_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ID,
                "Calendar id",
                "Target Feishu calendar id.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
        ],
    }
}

fn wecom_calendar_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "wecom_calendar".to_string(),
        display_name: "WeCom Calendar".to_string(),
        display_name_key: office_provider_display_name_key("wecom_calendar"),
        capabilities: vec![OfficeCapability::Calendar],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            metadata_field(
                OFFICE_METADATA_CALENDAR_CORP_ID,
                "WeCom corp id",
                "Enterprise WeCom corp id used by the calendar API.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                "Calendar base URL",
                "Optional API base URL override for WeCom calendar.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(WECOM_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ID,
                "Calendar id",
                "Target WeCom calendar id.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
        ],
    }
}

fn microsoft365_calendar_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "microsoft365_calendar".to_string(),
        display_name: "Microsoft 365 Calendar".to_string(),
        display_name_key: office_provider_display_name_key("microsoft365_calendar"),
        capabilities: vec![OfficeCapability::Calendar],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Calendar account id",
                "Optional calendar identity. Leave empty to use the signed-in Microsoft account.",
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                "Microsoft Graph base URL",
                "Optional API base URL override for Microsoft Graph calendar.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ID,
                "Calendar id",
                "Optional Microsoft calendar id. Use the primary calendar if omitted.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                Some(MICROSOFT365_DEFAULT_CALENDAR_ID),
            ),
        ],
    }
}

fn google_calendar_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "google_calendar".to_string(),
        display_name: "Google Calendar".to_string(),
        display_name_key: office_provider_display_name_key("google_calendar"),
        capabilities: vec![OfficeCapability::Calendar],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Calendar account id",
                "Optional calendar identity. Leave empty to use the signed-in Google account.",
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                "Google Calendar API base URL",
                "Optional API base URL override for Google Calendar.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(GOOGLE_CALENDAR_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_CALENDAR_ID,
                "Calendar id",
                "Optional calendar id. Use the primary calendar if omitted.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                Some(GOOGLE_DEFAULT_CALENDAR_ID),
            ),
        ],
    }
}

fn webdav_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "webdav".to_string(),
        display_name: "WebDAV".to_string(),
        display_name_key: office_provider_display_name_key("webdav"),
        capabilities: vec![OfficeCapability::Documents],
        fields: vec![
            access_token_field(),
            external_account_id_field(
                "Documents username",
                "Optional document identity. If omitted, set documents_username in credential metadata.",
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_USERNAME,
                "Documents username",
                "Explicit WebDAV username. Required when external_account_id is empty.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                "Documents base URL",
                "WebDAV base URL.",
                OfficeProviderFieldValueKind::Url,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                "Documents root path",
                "Default path prefix for document access.",
                OfficeProviderFieldValueKind::Path,
                true,
                None,
            ),
        ],
    }
}

fn feishu_documents_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "feishu_documents".to_string(),
        display_name: "Feishu Documents".to_string(),
        display_name_key: office_provider_display_name_key("feishu_documents"),
        capabilities: vec![OfficeCapability::Documents],
        fields: vec![
            access_token_field(),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_APP_ID,
                "Feishu app id",
                "Feishu app id for docs API access.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                "Documents base URL",
                "Optional API base URL override for Feishu documents.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(FEISHU_DOCUMENTS_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                "Documents root path",
                "Root folder or namespace path for document operations.",
                OfficeProviderFieldValueKind::Path,
                true,
                None,
            ),
        ],
    }
}

fn wecom_documents_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "wecom_documents".to_string(),
        display_name: "WeCom Documents".to_string(),
        display_name_key: office_provider_display_name_key("wecom_documents"),
        capabilities: vec![OfficeCapability::Documents],
        fields: vec![
            access_token_field(),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_CORP_ID,
                "WeCom corp id",
                "Enterprise WeCom corp id used by the docs API.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_SPACE_ID,
                "Space id",
                "Target WeCom document space id.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                "Documents base URL",
                "Optional API base URL override for WeCom documents.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(WECOM_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                "Documents root path",
                "Root folder or namespace path for document operations.",
                OfficeProviderFieldValueKind::Path,
                true,
                None,
            ),
        ],
    }
}

fn microsoft365_documents_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "microsoft365_documents".to_string(),
        display_name: "Microsoft 365 Documents".to_string(),
        display_name_key: office_provider_display_name_key("microsoft365_documents"),
        capabilities: vec![OfficeCapability::Documents],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Documents account id",
                "Optional documents identity. Leave empty to use the signed-in Microsoft account.",
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                "Microsoft Graph base URL",
                "Optional API base URL override for Microsoft Graph documents.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_DRIVE_ID,
                "Drive id",
                "Optional Microsoft drive id. Leave empty to use the signed-in user's default drive.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                "Documents root path",
                "Root folder path inside the selected Microsoft drive.",
                OfficeProviderFieldValueKind::Path,
                false,
                Some("/"),
            ),
        ],
    }
}

fn google_documents_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "google_documents".to_string(),
        display_name: "Google Documents".to_string(),
        display_name_key: office_provider_display_name_key("google_documents"),
        capabilities: vec![OfficeCapability::Documents],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            external_account_id_field(
                "Documents account id",
                "Optional documents identity. Leave empty to use the signed-in Google account.",
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                "Google Drive API base URL",
                "Optional API base URL override for Google Drive.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(GOOGLE_DRIVE_DEFAULT_BASE_URL),
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_DRIVE_ID,
                "Drive id",
                "Optional shared drive id. Leave empty to use the signed-in user's default drive.",
                OfficeProviderFieldValueKind::Identifier,
                false,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                "Documents root path",
                "Root folder path inside the selected Google Drive.",
                OfficeProviderFieldValueKind::Path,
                false,
                Some("/"),
            ),
        ],
    }
}

fn feishu_contacts_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "feishu_contacts_directory".to_string(),
        display_name: "Feishu Contacts Directory".to_string(),
        display_name_key: office_provider_display_name_key("feishu_contacts_directory"),
        capabilities: vec![OfficeCapability::ContactsDirectory],
        fields: vec![
            access_token_field(),
            metadata_field(
                OFFICE_METADATA_CONTACTS_APP_ID,
                "Feishu app id",
                "Feishu app id for contacts API access.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CONTACTS_BASE_URL,
                "Contacts base URL",
                "Optional API base URL override for Feishu contacts.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(FEISHU_CONTACTS_DEFAULT_BASE_URL),
            ),
        ],
    }
}

fn wecom_contacts_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "wecom_contacts_directory".to_string(),
        display_name: "WeCom Contacts Directory".to_string(),
        display_name_key: office_provider_display_name_key("wecom_contacts_directory"),
        capabilities: vec![OfficeCapability::ContactsDirectory],
        fields: vec![
            access_token_field(),
            metadata_field(
                OFFICE_METADATA_CONTACTS_CORP_ID,
                "WeCom corp id",
                "Enterprise WeCom corp id used by the contacts API.",
                OfficeProviderFieldValueKind::Identifier,
                true,
                None,
            ),
            metadata_field(
                OFFICE_METADATA_CONTACTS_BASE_URL,
                "Contacts base URL",
                "Optional API base URL override for WeCom contacts.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(WECOM_DEFAULT_BASE_URL),
            ),
        ],
    }
}

fn microsoft365_contacts_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "microsoft365_contacts_directory".to_string(),
        display_name: "Microsoft 365 People Directory".to_string(),
        display_name_key: office_provider_display_name_key("microsoft365_contacts_directory"),
        capabilities: vec![OfficeCapability::ContactsDirectory],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            metadata_field(
                OFFICE_METADATA_CONTACTS_BASE_URL,
                "Microsoft Graph base URL",
                "Optional API base URL override for Microsoft Graph people lookup.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
            ),
        ],
    }
}

fn google_contacts_schema() -> OfficeProviderSchema {
    OfficeProviderSchema {
        provider_kind: "google_contacts_directory".to_string(),
        display_name: "Google People Directory".to_string(),
        display_name_key: office_provider_display_name_key("google_contacts_directory"),
        capabilities: vec![OfficeCapability::ContactsDirectory],
        fields: vec![
            access_token_field(),
            refresh_token_field(),
            token_endpoint_field(),
            metadata_field(
                OFFICE_METADATA_CONTACTS_BASE_URL,
                "Google People API base URL",
                "Optional API base URL override for Google People lookup.",
                OfficeProviderFieldValueKind::Url,
                false,
                Some(GOOGLE_PEOPLE_DEFAULT_BASE_URL),
            ),
        ],
    }
}

fn access_token_field() -> OfficeProviderFieldSchema {
    OfficeProviderFieldSchema {
        key: "access_token".to_string(),
        label: "Access token / app secret".to_string(),
        label_key: provider_field_label_key("access_token"),
        description: "Secret used to authenticate the provider account.".to_string(),
        description_key: provider_field_description_key("access_token"),
        location: OfficeProviderFieldLocation::AccessToken,
        value_kind: OfficeProviderFieldValueKind::Secret,
        required: true,
        secret: true,
        default_value: None,
    }
}

fn refresh_token_field() -> OfficeProviderFieldSchema {
    OfficeProviderFieldSchema {
        key: "refresh_token".to_string(),
        label: "Refresh token".to_string(),
        label_key: provider_field_label_key("refresh_token"),
        description: "Optional refresh token for OAuth-backed providers.".to_string(),
        description_key: provider_field_description_key("refresh_token"),
        location: OfficeProviderFieldLocation::RefreshToken,
        value_kind: OfficeProviderFieldValueKind::Secret,
        required: false,
        secret: true,
        default_value: None,
    }
}

fn token_endpoint_field() -> OfficeProviderFieldSchema {
    OfficeProviderFieldSchema {
        key: "token_endpoint".to_string(),
        label: "Token endpoint".to_string(),
        label_key: provider_field_label_key("token_endpoint"),
        description: "Optional OAuth token endpoint override.".to_string(),
        description_key: provider_field_description_key("token_endpoint"),
        location: OfficeProviderFieldLocation::TokenEndpoint,
        value_kind: OfficeProviderFieldValueKind::Url,
        required: false,
        secret: false,
        default_value: None,
    }
}

fn external_account_id_field(label: &str, description: &str) -> OfficeProviderFieldSchema {
    OfficeProviderFieldSchema {
        key: "external_account_id".to_string(),
        label: label.to_string(),
        label_key: provider_field_label_key("external_account_id"),
        description: description.to_string(),
        description_key: provider_field_description_key("external_account_id"),
        location: OfficeProviderFieldLocation::ExternalAccountId,
        value_kind: OfficeProviderFieldValueKind::Identifier,
        required: false,
        secret: false,
        default_value: None,
    }
}

fn metadata_field(
    key: &str,
    label: &str,
    description: &str,
    value_kind: OfficeProviderFieldValueKind,
    required: bool,
    default_value: Option<&str>,
) -> OfficeProviderFieldSchema {
    OfficeProviderFieldSchema {
        key: key.to_string(),
        label: label.to_string(),
        label_key: provider_field_label_key(key),
        description: description.to_string(),
        description_key: provider_field_description_key(key),
        location: OfficeProviderFieldLocation::Metadata,
        value_kind,
        required,
        secret: false,
        default_value: default_value.map(str::to_string),
    }
}
