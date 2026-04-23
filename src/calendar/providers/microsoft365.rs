#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::calendar::credentials::calendar_credential_from_office;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarOperation,
    CalendarProvider, CalendarProviderCredential, CalendarQuery, MICROSOFT365_DEFAULT_CALENDAR_ID,
};
use crate::error::{Error, Result};
use crate::office::{
    build_microsoft_graph_url, parse_microsoft_graph_json, request_microsoft_graph_json_ureq,
    OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use crate::platform::ResponseBody;
use crate::util::{current_unix_secs, epoch_to_ymdhms, parse_iso8601};
use serde::Deserialize;
use serde_json::json;

const DEFAULT_CALENDAR_VIEW_LOOKAHEAD_SECS: u64 = 365 * 24 * 60 * 60;

pub struct Microsoft365CalendarProvider;

impl CalendarProvider for Microsoft365CalendarProvider {
    fn provider_name(&self) -> &'static str {
        "microsoft365_calendar"
    }

    fn display_name(&self) -> &'static str {
        "Microsoft 365 Calendar"
    }

    fn supports(&self, op: CalendarOperation) -> bool {
        matches!(
            op,
            CalendarOperation::List
                | CalendarOperation::Get
                | CalendarOperation::Create
                | CalendarOperation::Update
                | CalendarOperation::Delete
        )
    }

    fn list_events(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        validate_microsoft365_calendar_credential(credential)?;
        let include_cancelled = query.include_cancelled;
        let (path, query_pairs) = calendar_list_endpoint(credential, query);
        let payload: MicrosoftGraphCalendarCollection = request_calendar_json(
            http.get_with_headers(
                &build_microsoft_graph_url(&credential.base_url, &path, &query_pairs),
                &[("Authorization", authorization_header(credential).as_str())],
            ),
            "microsoft365_calendar_list",
        )?;
        payload
            .value
            .into_iter()
            .map(|item| item.into_calendar_event(credential))
            .filter(|result| {
                result
                    .as_ref()
                    .map(|event| {
                        include_cancelled || event.status != CalendarEventStatus::Cancelled
                    })
                    .unwrap_or(true)
            })
            .collect()
    }

    fn get_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        validate_microsoft365_calendar_credential(credential)?;
        let url = build_microsoft_graph_url(
            &credential.base_url,
            &calendar_event_path(credential, id),
            &[(
                "$select",
                "id,subject,start,end,location,body,isCancelled,lastModifiedDateTime".to_string(),
            )],
        );
        match http.get_with_headers(
            &url,
            &[("Authorization", authorization_header(credential).as_str())],
        ) {
            Ok((status, body)) => {
                let item: MicrosoftGraphCalendarEvent =
                    parse_microsoft_graph_json("microsoft365_calendar_get", status, body)?;
                Ok(Some(item.into_calendar_event(credential)?))
            }
            Err(Error::Http {
                status_code: 404, ..
            }) => Ok(None),
            Err(error) => Err(error.with_stage("microsoft365_calendar_get")),
        }
    }

    fn create_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_microsoft365_calendar_credential(credential)?;
        let url = build_microsoft_graph_url(
            &credential.base_url,
            &calendar_collection_path(credential),
            &[],
        );
        let body = render_calendar_body(event);
        let item: MicrosoftGraphCalendarEvent = request_calendar_json(
            http.post_with_headers(
                &url,
                &[
                    ("Authorization", authorization_header(credential).as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "microsoft365_calendar_create",
        )?;
        item.into_calendar_event(credential)
    }

    fn update_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_microsoft365_calendar_credential(credential)?;
        let event_id = event
            .remote_id
            .trim()
            .if_empty_then(|| event.id.trim())
            .ok_or_else(|| {
                Error::config(
                    "microsoft365_calendar_update",
                    "event id must not be empty for update",
                )
            })?;
        let url = build_microsoft_graph_url(
            &credential.base_url,
            &calendar_event_path(credential, event_id),
            &[],
        );
        let body = render_calendar_body(event);
        let item: MicrosoftGraphCalendarEvent = request_calendar_json(
            http.patch_with_headers(
                &url,
                &[
                    ("Authorization", authorization_header(credential).as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "microsoft365_calendar_update",
        )?;
        item.into_calendar_event(credential)
    }

    fn delete_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool> {
        validate_microsoft365_calendar_credential(credential)?;
        let url = build_microsoft_graph_url(
            &credential.base_url,
            &calendar_event_path(credential, id),
            &[],
        );
        let (status, body) = http
            .delete_with_headers(
                &url,
                &[("Authorization", authorization_header(credential).as_str())],
            )
            .map_err(|error| error.with_stage("microsoft365_calendar_delete"))?;
        if matches!(status, 200 | 202 | 204) {
            return Ok(true);
        }
        parse_microsoft_graph_json::<serde_json::Value>(
            "microsoft365_calendar_delete",
            status,
            body,
        )
        .map(|_| true)
    }
}

pub struct Microsoft365CalendarOfficeProbeAdapter;

impl OfficeProbeAdapter for Microsoft365CalendarOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "microsoft365_calendar"
    }

    fn probe(
        &self,
        _http: &mut dyn crate::office::OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = calendar_credential_from_office(account.clone(), credential.clone());
        if validate_microsoft365_calendar_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "calendar_transport_config_missing".to_string(),
            });
        }
        let url = build_microsoft_graph_url(
            &adapted.base_url,
            &calendar_collection_path(&adapted),
            &[("$top", "1".to_string())],
        );
        let _: MicrosoftGraphCalendarCollection = request_microsoft_graph_json_ureq(
            "microsoft365_calendar_probe",
            ureq::get(&url)
                .set("Authorization", &authorization_header(&adapted))
                .call(),
        )?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "microsoft365_calendar_events_ok".to_string(),
        })
    }
}

type MicrosoftGraphCalendarCollection =
    crate::office::MicrosoftGraphCollection<MicrosoftGraphCalendarEvent>;

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphCalendarEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    start: Option<MicrosoftGraphDateTimeTimeZone>,
    #[serde(default)]
    end: Option<MicrosoftGraphDateTimeTimeZone>,
    #[serde(default)]
    location: Option<MicrosoftGraphLocation>,
    #[serde(default)]
    body: Option<MicrosoftGraphBody>,
    #[serde(default, rename = "isCancelled")]
    is_cancelled: bool,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphDateTimeTimeZone {
    #[serde(default, rename = "dateTime")]
    date_time: String,
    #[serde(default, rename = "timeZone")]
    time_zone: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphLocation {
    #[serde(default, rename = "displayName")]
    display_name: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphBody {
    #[serde(default)]
    content: String,
}

impl MicrosoftGraphCalendarEvent {
    fn into_calendar_event(self, credential: &CalendarProviderCredential) -> Result<CalendarEvent> {
        let start = self
            .start
            .as_ref()
            .ok_or_else(|| Error::config("microsoft365_calendar_event", "missing start time"))?;
        let end = self
            .end
            .as_ref()
            .ok_or_else(|| Error::config("microsoft365_calendar_event", "missing end time"))?;
        normalize_calendar_event(CalendarEvent {
            id: self.id.clone(),
            title: self.subject.trim().to_string(),
            start_at_unix_secs: parse_iso8601(&start.date_time).unwrap_or_default(),
            end_at_unix_secs: parse_iso8601(&end.date_time).unwrap_or_default(),
            timezone: start.time_zone.trim().to_string(),
            location: self
                .location
                .as_ref()
                .map(|value| value.display_name.trim().to_string())
                .unwrap_or_default(),
            notes: self
                .body
                .as_ref()
                .map(|value| value.content.trim().to_string())
                .unwrap_or_default(),
            provider: credential.provider.clone(),
            calendar_id: credential.calendar_id.clone(),
            remote_id: self.id,
            status: if self.is_cancelled {
                CalendarEventStatus::Cancelled
            } else {
                CalendarEventStatus::Confirmed
            },
            updated_at: current_unix_secs(),
        })
    }
}

fn validate_microsoft365_calendar_credential(
    credential: &CalendarProviderCredential,
) -> Result<()> {
    if credential.access_token.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_calendar_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_calendar_provider",
            "calendar_base_url must not be empty",
        ));
    }
    if credential.calendar_id.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_calendar_provider",
            "calendar_id must not be empty",
        ));
    }
    Ok(())
}

fn authorization_header(credential: &CalendarProviderCredential) -> String {
    format!("Bearer {}", credential.access_token)
}

fn calendar_collection_path(credential: &CalendarProviderCredential) -> String {
    if is_primary_calendar(&credential.calendar_id) {
        "/me/events".to_string()
    } else {
        format!(
            "/me/calendars/{}/events",
            urlencoding::encode(&credential.calendar_id)
        )
    }
}

fn calendar_event_path(credential: &CalendarProviderCredential, event_id: &str) -> String {
    if is_primary_calendar(&credential.calendar_id) {
        format!("/me/events/{}", urlencoding::encode(event_id))
    } else {
        format!(
            "/me/calendars/{}/events/{}",
            urlencoding::encode(&credential.calendar_id),
            urlencoding::encode(event_id)
        )
    }
}

fn calendar_list_endpoint(
    credential: &CalendarProviderCredential,
    query: CalendarQuery,
) -> (String, Vec<(&'static str, String)>) {
    let start = query
        .start_from_unix_secs
        .unwrap_or_else(|| current_unix_secs().saturating_sub(24 * 60 * 60));
    let end = query
        .start_to_unix_secs
        .unwrap_or_else(|| start.saturating_add(DEFAULT_CALENDAR_VIEW_LOOKAHEAD_SECS));
    let path = if is_primary_calendar(&credential.calendar_id) {
        "/me/calendarView".to_string()
    } else {
        format!(
            "/me/calendars/{}/calendarView",
            urlencoding::encode(&credential.calendar_id)
        )
    };
    (
        path,
        vec![
            ("$top", query.limit.clamp(1, 50).to_string()),
            (
                "$select",
                "id,subject,start,end,location,body,isCancelled,lastModifiedDateTime".to_string(),
            ),
            ("startDateTime", render_graph_datetime(start)),
            ("endDateTime", render_graph_datetime(end)),
            ("$orderby", "start/dateTime".to_string()),
        ],
    )
}

fn is_primary_calendar(calendar_id: &str) -> bool {
    let trimmed = calendar_id.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case(MICROSOFT365_DEFAULT_CALENDAR_ID)
}

fn render_calendar_body(event: &CalendarEvent) -> String {
    json!({
        "subject": event.title,
        "start": {
            "dateTime": render_graph_datetime(event.start_at_unix_secs),
            "timeZone": normalized_timezone(&event.timezone),
        },
        "end": {
            "dateTime": render_graph_datetime(event.end_at_unix_secs),
            "timeZone": normalized_timezone(&event.timezone),
        },
        "location": {
            "displayName": event.location,
        },
        "body": {
            "contentType": "Text",
            "content": event.notes,
        },
    })
    .to_string()
}

fn normalized_timezone(timezone: &str) -> &str {
    let trimmed = timezone.trim();
    if trimmed.is_empty() {
        "UTC"
    } else {
        trimmed
    }
}

fn render_graph_datetime(unix_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_ymdhms(unix_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn request_calendar_json<T: for<'de> Deserialize<'de>>(
    response: Result<(u16, ResponseBody)>,
    stage: &'static str,
) -> Result<T> {
    let (status, body) = response.map_err(|error| error.with_stage(stage))?;
    parse_microsoft_graph_json(stage, status, body)
}

trait IfEmptyThen<'a> {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str>;
}

impl<'a> IfEmptyThen<'a> for &'a str {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str> {
        if self.trim().is_empty() {
            let fallback = fallback();
            (!fallback.trim().is_empty()).then_some(fallback)
        } else {
            Some(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeCapability, OfficeCredential,
    };

    #[test]
    fn microsoft365_calendar_provider_reports_graph_capabilities() {
        let provider = Microsoft365CalendarProvider;
        assert_eq!(provider.provider_name(), "microsoft365_calendar");
        assert!(provider.supports(CalendarOperation::List));
        assert!(provider.supports(CalendarOperation::Get));
        assert!(provider.supports(CalendarOperation::Create));
        assert!(provider.supports(CalendarOperation::Update));
        assert!(provider.supports(CalendarOperation::Delete));
    }

    #[test]
    fn microsoft365_calendar_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = Microsoft365CalendarOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "calendar-ms".to_string(),
                    provider_kind: "microsoft365_calendar".to_string(),
                    external_account_id: "alice@contoso.com".to_string(),
                    account_label: "Microsoft Calendar".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Calendar],
                },
                &OfficeCredential {
                    account_key: "calendar-ms".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
        assert_eq!(result.provider_kind, "microsoft365_calendar");
    }

    #[test]
    fn calendar_list_endpoint_uses_primary_calendar_view() {
        let credential = CalendarProviderCredential {
            account_key: "calendar-ms".to_string(),
            provider: "microsoft365_calendar".to_string(),
            account_id: String::new(),
            account_label: String::new(),
            calendar_id: MICROSOFT365_DEFAULT_CALENDAR_ID.to_string(),
            username: String::new(),
            app_id: String::new(),
            base_url: "https://graph.microsoft.com/v1.0".to_string(),
            root_path: String::new(),
            access_token: "token".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
        };
        let (path, _) = calendar_list_endpoint(
            &credential,
            CalendarQuery {
                start_from_unix_secs: Some(100),
                start_to_unix_secs: Some(200),
                limit: 10,
                include_cancelled: false,
            },
        );
        assert_eq!(path, "/me/calendarView");
    }
}
