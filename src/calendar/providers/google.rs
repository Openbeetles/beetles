#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::calendar::credentials::calendar_credential_from_office;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarHttpClient,
    CalendarOperation, CalendarProvider, CalendarProviderCredential, CalendarQuery,
    GOOGLE_DEFAULT_CALENDAR_ID,
};
use crate::error::{Error, Result};
use crate::office::{
    build_google_api_url, parse_google_api_json, request_google_api_json_ureq, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult,
};
use crate::platform::ResponseBody;
use crate::util::{current_unix_secs, epoch_to_ymdhms, parse_iso8601};
use serde::Deserialize;
use serde_json::json;

const DEFAULT_CALENDAR_VIEW_LOOKAHEAD_SECS: u64 = 365 * 24 * 60 * 60;

pub struct GoogleCalendarProvider;

impl CalendarProvider for GoogleCalendarProvider {
    fn provider_name(&self) -> &'static str {
        "google_calendar"
    }

    fn display_name(&self) -> &'static str {
        "Google Calendar"
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
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        validate_google_calendar_credential(credential)?;
        let include_cancelled = query.include_cancelled;
        let (path, query_pairs) = calendar_list_endpoint(credential, query);
        let payload: GoogleCalendarList = request_calendar_json(
            http.get_with_headers(
                &build_google_api_url(&credential.base_url, &path, &query_pairs),
                &[("Authorization", authorization_header(credential).as_str())],
            ),
            "google_calendar_list",
        )?;
        payload
            .items
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
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        validate_google_calendar_credential(credential)?;
        let url = build_google_api_url(
            &credential.base_url,
            &calendar_event_path(credential, id),
            &[],
        );
        match http.get_with_headers(
            &url,
            &[("Authorization", authorization_header(credential).as_str())],
        ) {
            Ok((status, body)) => {
                let item: GoogleCalendarEvent =
                    parse_google_api_json("google_calendar_get", status, body)?;
                Ok(Some(item.into_calendar_event(credential)?))
            }
            Err(Error::Http {
                status_code: 404, ..
            }) => Ok(None),
            Err(error) => Err(error.with_stage("google_calendar_get")),
        }
    }

    fn create_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_google_calendar_credential(credential)?;
        let url = build_google_api_url(
            &credential.base_url,
            &calendar_collection_path(credential),
            &[],
        );
        let body = render_calendar_body(event);
        let item: GoogleCalendarEvent = request_calendar_json(
            http.post_with_headers(
                &url,
                &[
                    ("Authorization", authorization_header(credential).as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "google_calendar_create",
        )?;
        item.into_calendar_event(credential)
    }

    fn update_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_google_calendar_credential(credential)?;
        let event_id = event
            .remote_id
            .trim()
            .if_empty_then(|| event.id.trim())
            .ok_or_else(|| Error::config("google_calendar_update", "event id must not be empty"))?;
        let url = build_google_api_url(
            &credential.base_url,
            &calendar_event_path(credential, event_id),
            &[],
        );
        let body = render_calendar_body(event);
        let item: GoogleCalendarEvent = request_calendar_json(
            http.patch_with_headers(
                &url,
                &[
                    ("Authorization", authorization_header(credential).as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "google_calendar_update",
        )?;
        item.into_calendar_event(credential)
    }

    fn delete_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool> {
        validate_google_calendar_credential(credential)?;
        let url = build_google_api_url(
            &credential.base_url,
            &calendar_event_path(credential, id),
            &[],
        );
        let (status, body) = http
            .delete_with_headers(
                &url,
                &[("Authorization", authorization_header(credential).as_str())],
            )
            .map_err(|error| error.with_stage("google_calendar_delete"))?;
        if matches!(status, 200 | 202 | 204) {
            return Ok(true);
        }
        parse_google_api_json::<serde_json::Value>("google_calendar_delete", status, body)
            .map(|_| true)
    }
}

pub struct GoogleCalendarOfficeProbeAdapter;

impl OfficeProbeAdapter for GoogleCalendarOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "google_calendar"
    }

    fn probe(
        &self,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = calendar_credential_from_office(account.clone(), credential.clone());
        if validate_google_calendar_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "calendar_transport_config_missing".to_string(),
            });
        }
        let _: GoogleCalendarList = request_google_api_json_ureq(
            "google_calendar_probe",
            ureq::get(&build_google_api_url(
                &adapted.base_url,
                &calendar_collection_path(&adapted),
                &[("maxResults", "1".to_string())],
            ))
            .set("Authorization", &authorization_header(&adapted))
            .call(),
        )?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "google_calendar_events_ok".to_string(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct GoogleCalendarList {
    #[serde(default)]
    items: Vec<GoogleCalendarEvent>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleCalendarEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    start: Option<GoogleDateTime>,
    #[serde(default)]
    end: Option<GoogleDateTime>,
    #[serde(default)]
    status: String,
    #[serde(default, rename = "updated")]
    updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleDateTime {
    #[serde(default, rename = "dateTime")]
    date_time: String,
    #[serde(default)]
    date: String,
    #[serde(default, rename = "timeZone")]
    time_zone: String,
}

impl GoogleCalendarEvent {
    fn into_calendar_event(self, credential: &CalendarProviderCredential) -> Result<CalendarEvent> {
        let start = self
            .start
            .as_ref()
            .ok_or_else(|| Error::config("google_calendar_event", "missing start time"))?;
        let end = self
            .end
            .as_ref()
            .ok_or_else(|| Error::config("google_calendar_event", "missing end time"))?;
        let timezone = start
            .time_zone
            .trim()
            .if_empty_then(|| end.time_zone.trim())
            .unwrap_or("UTC")
            .to_string();
        normalize_calendar_event(CalendarEvent {
            id: self.id.clone(),
            title: self.summary.trim().to_string(),
            start_at_unix_secs: parse_google_time(start)?,
            end_at_unix_secs: parse_google_time(end)?,
            timezone,
            location: self.location.trim().to_string(),
            notes: self.description.trim().to_string(),
            provider: credential.provider.clone(),
            calendar_id: credential.calendar_id.clone(),
            remote_id: self.id,
            status: if self.status.eq_ignore_ascii_case("cancelled") {
                CalendarEventStatus::Cancelled
            } else {
                CalendarEventStatus::Confirmed
            },
            updated_at: parse_iso8601(&self.updated_at).unwrap_or_else(current_unix_secs),
        })
    }
}

fn validate_google_calendar_credential(credential: &CalendarProviderCredential) -> Result<()> {
    if credential.access_token.trim().is_empty() {
        return Err(Error::config(
            "google_calendar_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "google_calendar_provider",
            "calendar_base_url must not be empty",
        ));
    }
    if credential.calendar_id.trim().is_empty() {
        return Err(Error::config(
            "google_calendar_provider",
            "calendar_id must not be empty",
        ));
    }
    Ok(())
}

fn authorization_header(credential: &CalendarProviderCredential) -> String {
    format!("Bearer {}", credential.access_token)
}

fn request_calendar_json<T: for<'de> Deserialize<'de>>(
    result: Result<(u16, ResponseBody)>,
    stage: &'static str,
) -> Result<T> {
    let (status, body) = result.map_err(|error| error.with_stage(stage))?;
    parse_google_api_json(stage, status, body)
}

fn calendar_collection_path(credential: &CalendarProviderCredential) -> String {
    format!(
        "/calendars/{}/events",
        urlencoding::encode(if credential.calendar_id.trim().is_empty() {
            GOOGLE_DEFAULT_CALENDAR_ID
        } else {
            credential.calendar_id.trim()
        })
    )
}

fn calendar_event_path(credential: &CalendarProviderCredential, id: &str) -> String {
    format!(
        "/calendars/{}/events/{}",
        urlencoding::encode(if credential.calendar_id.trim().is_empty() {
            GOOGLE_DEFAULT_CALENDAR_ID
        } else {
            credential.calendar_id.trim()
        }),
        urlencoding::encode(id)
    )
}

fn calendar_list_endpoint(
    credential: &CalendarProviderCredential,
    query: CalendarQuery,
) -> (String, Vec<(&'static str, String)>) {
    let now = current_unix_secs();
    let mut query_pairs = vec![
        ("singleEvents", "true".to_string()),
        ("orderBy", "startTime".to_string()),
        ("maxResults", query.limit.clamp(1, 100).to_string()),
    ];
    let time_min = query.start_from_unix_secs.unwrap_or(now);
    query_pairs.push(("timeMin", render_google_datetime(time_min)));
    let time_max = query
        .start_to_unix_secs
        .unwrap_or(time_min + DEFAULT_CALENDAR_VIEW_LOOKAHEAD_SECS);
    query_pairs.push(("timeMax", render_google_datetime(time_max)));
    (calendar_collection_path(credential), query_pairs)
}

fn render_calendar_body(event: &CalendarEvent) -> String {
    let timezone = if event.timezone.trim().is_empty() {
        "UTC"
    } else {
        event.timezone.trim()
    };
    json!({
        "summary": event.title,
        "location": event.location,
        "description": event.notes,
        "start": {
            "dateTime": render_google_datetime(event.start_at_unix_secs),
            "timeZone": timezone,
        },
        "end": {
            "dateTime": render_google_datetime(event.end_at_unix_secs),
            "timeZone": timezone,
        },
        "status": if matches!(event.status, CalendarEventStatus::Cancelled) {
            "cancelled"
        } else {
            "confirmed"
        },
    })
    .to_string()
}

fn parse_google_time(value: &GoogleDateTime) -> Result<u64> {
    if !value.date_time.trim().is_empty() {
        return parse_iso8601(value.date_time.trim()).ok_or_else(|| {
            Error::config(
                "google_calendar_time",
                format!(
                    "invalid Google Calendar datetime '{}'",
                    value.date_time.trim()
                ),
            )
        });
    }
    if !value.date.trim().is_empty() {
        let rendered = format!("{}T00:00:00Z", value.date.trim());
        return parse_iso8601(&rendered).ok_or_else(|| {
            Error::config(
                "google_calendar_time",
                format!("invalid Google Calendar date '{}'", value.date.trim()),
            )
        });
    }
    Err(Error::config(
        "google_calendar_time",
        "missing dateTime/date value",
    ))
}

fn render_google_datetime(unix_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_ymdhms(unix_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

trait IfEmptyThen<'a> {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str>;
}

impl<'a> IfEmptyThen<'a> for &'a str {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str> {
        if self.trim().is_empty() {
            let value = fallback();
            (!value.trim().is_empty()).then_some(value)
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
    fn google_calendar_provider_reports_google_capabilities() {
        let provider = GoogleCalendarProvider;
        assert_eq!(provider.provider_name(), "google_calendar");
        assert!(provider.supports(CalendarOperation::Create));
        assert!(provider.supports(CalendarOperation::Delete));
    }

    #[test]
    fn google_calendar_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = GoogleCalendarOfficeProbeAdapter;
        let result = adapter
            .probe(
                &OfficeAccount {
                    account_key: "google-calendar".to_string(),
                    provider_kind: "google_calendar".to_string(),
                    external_account_id: "alice@gmail.com".to_string(),
                    account_label: "Google Calendar".to_string(),
                    identity_class: OfficeAccountIdentityClass::Personal,
                    enabled_capabilities: vec![OfficeCapability::Calendar],
                },
                &OfficeCredential {
                    account_key: "google-calendar".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(result.provider_kind, "google_calendar");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
    }
}
