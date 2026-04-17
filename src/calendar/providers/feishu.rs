#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::calendar::credentials::calendar_credential_from_office;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarHttpClient,
    CalendarOperation, CalendarProvider, CalendarProviderCredential, CalendarQuery,
};
use crate::error::{Error, Result};
use crate::office::{OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult};
use crate::platform::ResponseBody;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FEISHU_CALENDAR_BASE_PATH: &str = "/open-apis/calendar/v4";

pub struct FeishuCalendarProvider;

impl CalendarProvider for FeishuCalendarProvider {
    fn provider_name(&self) -> &'static str {
        "feishu_calendar"
    }

    fn display_name(&self) -> &'static str {
        "Feishu Calendar"
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
        validate_feishu_calendar_credential(credential)?;
        let tenant_access_token = fetch_tenant_access_token_http(http, credential)?;
        let auth = format!("Bearer {tenant_access_token}");
        let url = build_api_url(
            credential,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events",
                urlencoding::encode(&credential.calendar_id)
            ),
            &[
                ("page_size", query.limit.clamp(1, 100).to_string()),
                (
                    "start_time",
                    query
                        .start_from_unix_secs
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ),
                (
                    "end_time",
                    query
                        .start_to_unix_secs
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ),
            ],
        );
        let payload: FeishuEnvelope<FeishuListEventsData> = request_json(
            http.get_with_headers(&url, &[("Authorization", auth.as_str())]),
            "feishu_calendar_list",
        )?;
        let data = payload.require_data("feishu_calendar_list")?;
        data.items
            .into_iter()
            .map(|item| item.into_calendar_event(credential))
            .collect()
    }

    fn get_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        validate_feishu_calendar_credential(credential)?;
        let tenant_access_token = fetch_tenant_access_token_http(http, credential)?;
        let auth = format!("Bearer {tenant_access_token}");
        let url = build_api_url(
            credential,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events/{}",
                urlencoding::encode(&credential.calendar_id),
                urlencoding::encode(id)
            ),
            &[],
        );
        let response = http.get_with_headers(&url, &[("Authorization", auth.as_str())]);
        match response {
            Ok(response) => {
                let payload: FeishuEnvelope<FeishuEventData> =
                    parse_json_response("feishu_calendar_get", response)?;
                Ok(Some(
                    payload
                        .require_data("feishu_calendar_get")?
                        .event
                        .into_calendar_event(credential)?,
                ))
            }
            Err(Error::Http {
                status_code: 404, ..
            }) => Ok(None),
            Err(error) => Err(error.with_stage("feishu_calendar_get")),
        }
    }

    fn create_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_feishu_calendar_credential(credential)?;
        let tenant_access_token = fetch_tenant_access_token_http(http, credential)?;
        let auth = format!("Bearer {tenant_access_token}");
        let url = build_api_url(
            credential,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events",
                urlencoding::encode(&credential.calendar_id)
            ),
            &[],
        );
        let body = render_event_body(event)?;
        let payload: FeishuEnvelope<FeishuEventData> = request_json(
            http.post_with_headers(
                &url,
                &[
                    ("Authorization", auth.as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "feishu_calendar_create",
        )?;
        payload
            .require_data("feishu_calendar_create")?
            .event
            .into_calendar_event(credential)
    }

    fn update_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_feishu_calendar_credential(credential)?;
        let event_id = event
            .remote_id
            .trim()
            .if_empty_then(|| event.id.trim())
            .ok_or_else(|| {
                Error::config(
                    "feishu_calendar_update",
                    "event id must not be empty for update",
                )
            })?;
        let tenant_access_token = fetch_tenant_access_token_http(http, credential)?;
        let auth = format!("Bearer {tenant_access_token}");
        let url = build_api_url(
            credential,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events/{}",
                urlencoding::encode(&credential.calendar_id),
                urlencoding::encode(event_id)
            ),
            &[],
        );
        let body = render_event_body(event)?;
        let payload: FeishuEnvelope<FeishuEventData> = request_json(
            http.patch_with_headers(
                &url,
                &[
                    ("Authorization", auth.as_str()),
                    ("Content-Type", "application/json"),
                ],
                body.as_bytes(),
            ),
            "feishu_calendar_update",
        )?;
        payload
            .require_data("feishu_calendar_update")?
            .event
            .into_calendar_event(credential)
    }

    fn delete_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool> {
        validate_feishu_calendar_credential(credential)?;
        let tenant_access_token = fetch_tenant_access_token_http(http, credential)?;
        let auth = format!("Bearer {tenant_access_token}");
        let url = build_api_url(
            credential,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events/{}",
                urlencoding::encode(&credential.calendar_id),
                urlencoding::encode(id)
            ),
            &[],
        );
        let payload: FeishuEnvelope<Value> = request_json(
            http.delete_with_headers(&url, &[("Authorization", auth.as_str())]),
            "feishu_calendar_delete",
        )?;
        payload.require_ok("feishu_calendar_delete")?;
        Ok(true)
    }
}

pub struct FeishuCalendarOfficeProbeAdapter;

impl OfficeProbeAdapter for FeishuCalendarOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "feishu_calendar"
    }

    fn probe(
        &self,
        _http: &mut dyn crate::office::OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = calendar_credential_from_office(account.clone(), credential.clone());
        if validate_feishu_calendar_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "calendar_transport_config_missing".to_string(),
            });
        }
        let token = fetch_tenant_access_token_ureq(&adapted)?;
        let url = build_api_url(
            &adapted,
            &format!(
                "{FEISHU_CALENDAR_BASE_PATH}/calendars/{}/events",
                urlencoding::encode(&adapted.calendar_id)
            ),
            &[("page_size", "1".to_string())],
        );
        let payload: FeishuEnvelope<FeishuListEventsData> = request_json_ureq(
            "feishu_calendar_probe",
            ureq::get(&url)
                .set("Authorization", &format!("Bearer {token}"))
                .call(),
        )?;
        payload.require_ok("feishu_calendar_probe")?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "feishu_calendar_events_ok".to_string(),
        })
    }
}

#[derive(Serialize)]
struct FeishuTenantAccessTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

#[derive(Deserialize)]
struct FeishuAuthResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
}

#[derive(Deserialize)]
struct FeishuEnvelope<T> {
    code: i32,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

impl<T> FeishuEnvelope<T> {
    fn require_ok(self, stage: &'static str) -> Result<Self> {
        if self.code != 0 {
            return Err(Error::config(
                stage,
                if self.msg.trim().is_empty() {
                    format!("Feishu API failed with code {}", self.code)
                } else {
                    self.msg
                },
            ));
        }
        Ok(self)
    }

    fn require_data(self, stage: &'static str) -> Result<T> {
        let this = self.require_ok(stage)?;
        this.data
            .ok_or_else(|| Error::config(stage, "Feishu API returned no data"))
    }
}

#[derive(Deserialize)]
struct FeishuListEventsData {
    #[serde(default)]
    items: Vec<FeishuCalendarEvent>,
}

#[derive(Deserialize)]
struct FeishuEventData {
    event: FeishuCalendarEvent,
}

#[derive(Clone, Debug, Deserialize)]
struct FeishuCalendarEvent {
    event_id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    start_time: FeishuEventTime,
    end_time: FeishuEventTime,
    #[serde(default)]
    location: Option<FeishuEventLocation>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    update_time: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct FeishuEventTime {
    timestamp: String,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum FeishuEventLocation {
    Text(String),
    Object {
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        name: Option<String>,
    },
}

impl FeishuCalendarEvent {
    fn into_calendar_event(self, credential: &CalendarProviderCredential) -> Result<CalendarEvent> {
        let timezone = self
            .start_time
            .timezone
            .clone()
            .or(self.end_time.timezone.clone())
            .unwrap_or_default();
        normalize_calendar_event(CalendarEvent {
            id: self.event_id.clone(),
            title: self.summary,
            start_at_unix_secs: parse_u64("feishu_calendar_parse", &self.start_time.timestamp)?,
            end_at_unix_secs: parse_u64("feishu_calendar_parse", &self.end_time.timestamp)?,
            timezone,
            location: self
                .location
                .map(|location| location.display_name())
                .unwrap_or_default(),
            notes: self.description,
            provider: "feishu_calendar".to_string(),
            calendar_id: credential.calendar_id.clone(),
            remote_id: self.event_id,
            status: match self.status.to_ascii_lowercase().as_str() {
                "cancelled" => CalendarEventStatus::Cancelled,
                _ => CalendarEventStatus::Confirmed,
            },
            updated_at: self
                .update_time
                .as_deref()
                .map(|value| parse_u64("feishu_calendar_parse", value))
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

impl FeishuEventLocation {
    fn display_name(self) -> String {
        match self {
            FeishuEventLocation::Text(value) => value.trim().to_string(),
            FeishuEventLocation::Object { display_name, name } => {
                display_name.or(name).unwrap_or_default().trim().to_string()
            }
        }
    }
}

trait IfEmptyThen {
    fn if_empty_then<'a>(&'a self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str>;
}

impl IfEmptyThen for str {
    fn if_empty_then<'a>(&'a self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str> {
        if !self.trim().is_empty() {
            Some(self.trim())
        } else {
            let alt = fallback();
            (!alt.trim().is_empty()).then_some(alt.trim())
        }
    }
}

fn validate_feishu_calendar_credential(credential: &CalendarProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "feishu_calendar_provider",
            "app_id must not be empty",
        ));
    }
    if credential.access_token.trim().is_empty() {
        return Err(Error::config(
            "feishu_calendar_provider",
            "access_token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "feishu_calendar_provider",
            "base_url must not be empty",
        ));
    }
    if credential.calendar_id.trim().is_empty() {
        return Err(Error::config(
            "feishu_calendar_provider",
            "calendar_id must not be empty",
        ));
    }
    Ok(())
}

fn fetch_tenant_access_token_http(
    http: &mut dyn CalendarHttpClient,
    credential: &CalendarProviderCredential,
) -> Result<String> {
    let body = serde_json::to_string(&FeishuTenantAccessTokenRequest {
        app_id: credential.app_id.as_str(),
        app_secret: credential.access_token.as_str(),
    })
    .map_err(|error| Error::config("feishu_calendar_auth", error.to_string()))?;
    let url = build_api_url(
        credential,
        "/open-apis/auth/v3/tenant_access_token/internal",
        &[],
    );
    let payload: FeishuAuthResponse = request_json(
        http.post_with_headers(
            &url,
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        ),
        "feishu_calendar_auth",
    )?;
    parse_tenant_access_token(payload)
}

fn fetch_tenant_access_token_ureq(credential: &CalendarProviderCredential) -> Result<String> {
    let body = serde_json::to_string(&FeishuTenantAccessTokenRequest {
        app_id: credential.app_id.as_str(),
        app_secret: credential.access_token.as_str(),
    })
    .map_err(|error| Error::config("feishu_calendar_auth", error.to_string()))?;
    let url = build_api_url(
        credential,
        "/open-apis/auth/v3/tenant_access_token/internal",
        &[],
    );
    let payload: FeishuAuthResponse = request_json_ureq(
        "feishu_calendar_auth",
        ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body),
    )?;
    parse_tenant_access_token(payload)
}

fn parse_tenant_access_token(payload: FeishuAuthResponse) -> Result<String> {
    if payload.code != 0 {
        return Err(Error::config(
            "feishu_calendar_auth",
            if payload.msg.trim().is_empty() {
                format!("Feishu auth failed with code {}", payload.code)
            } else {
                payload.msg
            },
        ));
    }
    if payload.tenant_access_token.trim().is_empty() {
        return Err(Error::config(
            "feishu_calendar_auth",
            "tenant_access_token must not be empty",
        ));
    }
    Ok(payload.tenant_access_token)
}

fn render_event_body(event: &CalendarEvent) -> Result<String> {
    let mut payload = json!({
        "summary": event.title,
        "description": event.notes,
        "start_time": {
            "timestamp": event.start_at_unix_secs.to_string(),
        },
        "end_time": {
            "timestamp": event.end_at_unix_secs.to_string(),
        },
    });
    if let Some(timezone) = non_empty(&event.timezone) {
        payload["start_time"]["timezone"] = Value::String(timezone.to_string());
        payload["end_time"]["timezone"] = Value::String(timezone.to_string());
    }
    if let Some(location) = non_empty(&event.location) {
        payload["location"] = json!({ "display_name": location });
    }
    serde_json::to_string(&payload)
        .map_err(|error| Error::config("feishu_calendar_serialize", error.to_string()))
}

fn request_json<T: DeserializeOwned>(
    response: Result<(u16, ResponseBody)>,
    stage: &'static str,
) -> Result<T> {
    let (status, body) = response.map_err(|error| error.with_stage(stage))?;
    parse_json_response(stage, (status, body))
}

fn parse_json_response<T: DeserializeOwned>(
    stage: &'static str,
    response: (u16, ResponseBody),
) -> Result<T> {
    let (status, body) = response;
    if !(200..300).contains(&status) {
        return Err(Error::http(stage, status));
    }
    serde_json::from_slice(body.as_slice()).map_err(|error| Error::config(stage, error.to_string()))
}

fn request_json_ureq<T: DeserializeOwned>(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<T> {
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _)) => return Err(Error::http(stage, status)),
        Err(error) => return Err(Error::config(stage, error.to_string())),
    };
    let body = response
        .into_string()
        .map_err(|error| Error::config(stage, error.to_string()))?;
    serde_json::from_str(&body).map_err(|error| Error::config(stage, error.to_string()))
}

fn build_api_url(
    credential: &CalendarProviderCredential,
    path: &str,
    params: &[(&str, String)],
) -> String {
    let mut url = format!("{}{}", credential.base_url.trim_end_matches('/'), path);
    let query = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>();
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    url
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn parse_u64(stage: &'static str, raw: &str) -> Result<u64> {
    raw.trim()
        .parse::<u64>()
        .map_err(|error| Error::config(stage, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordedRequest {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    fn credential() -> CalendarProviderCredential {
        CalendarProviderCredential {
            account_key: "calendar-feishu".to_string(),
            provider: "feishu_calendar".to_string(),
            account_id: "work-calendar".to_string(),
            account_label: "Feishu Calendar".to_string(),
            calendar_id: "cal_a1b2".to_string(),
            username: String::new(),
            app_id: "cli_calendar".to_string(),
            base_url: "https://open.feishu.cn".to_string(),
            root_path: String::new(),
            access_token: "app-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 1,
        }
    }

    fn event() -> CalendarEvent {
        CalendarEvent {
            id: "evt_123".to_string(),
            title: "Weekly Sync".to_string(),
            start_at_unix_secs: 1_775_000_000,
            end_at_unix_secs: 1_775_003_600,
            timezone: "Asia/Shanghai".to_string(),
            location: "Room A".to_string(),
            notes: "Discuss launch".to_string(),
            provider: "feishu_calendar".to_string(),
            calendar_id: "cal_a1b2".to_string(),
            remote_id: "evt_123".to_string(),
            status: CalendarEventStatus::Confirmed,
            updated_at: 1,
        }
    }

    struct RecordingHttp {
        requests: Vec<RecordedRequest>,
        responses: Vec<(u16, String)>,
    }

    impl RecordingHttp {
        fn new(responses: Vec<(u16, &str)>) -> Self {
            Self {
                requests: Vec::new(),
                responses: responses
                    .into_iter()
                    .map(|(status, body)| (status, body.to_string()))
                    .collect(),
            }
        }

        fn next_response(&mut self) -> Result<(u16, ResponseBody)> {
            let (status, body) = self.responses.remove(0);
            Ok((status, ResponseBody::Heap(body.into_bytes())))
        }

        fn record(
            &mut self,
            method: &str,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.requests.push(RecordedRequest {
                method: method.to_string(),
                url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                    .collect(),
                body: body.to_vec(),
            });
            self.next_response()
        }
    }

    impl CalendarHttpClient for RecordingHttp {
        fn get_with_headers(
            &mut self,
            url: &str,
            headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            self.record("GET", url, headers, &[])
        }

        fn post_with_headers(
            &mut self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.record("POST", url, headers, body)
        }

        fn patch_with_headers(
            &mut self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            self.record("PATCH", url, headers, body)
        }

        fn put_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn delete_with_headers(
            &mut self,
            url: &str,
            headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            self.record("DELETE", url, headers, &[])
        }
    }

    #[test]
    fn feishu_calendar_provider_reports_supported_operations() {
        let provider = FeishuCalendarProvider;
        assert!(provider.supports(CalendarOperation::List));
        assert!(provider.supports(CalendarOperation::Get));
        assert!(provider.supports(CalendarOperation::Create));
        assert!(provider.supports(CalendarOperation::Update));
        assert!(provider.supports(CalendarOperation::Delete));
    }

    #[test]
    fn feishu_calendar_provider_lists_gets_and_mutates_events() {
        let provider = FeishuCalendarProvider;
        let mut http = RecordingHttp::new(vec![
            (200, r#"{"code":0,"tenant_access_token":"tenant-token"}"#),
            (
                200,
                r#"{"code":0,"data":{"items":[{"event_id":"evt_123","summary":"Weekly Sync","description":"Discuss launch","start_time":{"timestamp":"1775000000","timezone":"Asia/Shanghai"},"end_time":{"timestamp":"1775003600","timezone":"Asia/Shanghai"},"location":{"display_name":"Room A"},"status":"confirmed"}]}}"#,
            ),
            (200, r#"{"code":0,"tenant_access_token":"tenant-token"}"#),
            (
                200,
                r#"{"code":0,"data":{"event":{"event_id":"evt_123","summary":"Weekly Sync","description":"Discuss launch","start_time":{"timestamp":"1775000000","timezone":"Asia/Shanghai"},"end_time":{"timestamp":"1775003600","timezone":"Asia/Shanghai"},"location":{"display_name":"Room A"},"status":"confirmed"}}}"#,
            ),
            (200, r#"{"code":0,"tenant_access_token":"tenant-token"}"#),
            (
                200,
                r#"{"code":0,"data":{"event":{"event_id":"evt_123","summary":"Weekly Sync","description":"Discuss launch","start_time":{"timestamp":"1775000000","timezone":"Asia/Shanghai"},"end_time":{"timestamp":"1775003600","timezone":"Asia/Shanghai"},"location":{"display_name":"Room A"},"status":"confirmed"}}}"#,
            ),
            (200, r#"{"code":0,"tenant_access_token":"tenant-token"}"#),
            (
                200,
                r#"{"code":0,"data":{"event":{"event_id":"evt_123","summary":"Weekly Sync 2","description":"Discuss launch","start_time":{"timestamp":"1775000000","timezone":"Asia/Shanghai"},"end_time":{"timestamp":"1775003600","timezone":"Asia/Shanghai"},"location":{"display_name":"Room A"},"status":"confirmed"}}}"#,
            ),
            (200, r#"{"code":0,"tenant_access_token":"tenant-token"}"#),
            (200, r#"{"code":0}"#),
        ]);

        let listed = provider
            .list_events(
                &mut http,
                &credential(),
                CalendarQuery {
                    start_from_unix_secs: Some(1_775_000_000),
                    start_to_unix_secs: Some(1_775_100_000),
                    limit: 10,
                    include_cancelled: false,
                },
            )
            .expect("list events");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "evt_123");

        let fetched = provider
            .get_event(&mut http, &credential(), "evt_123")
            .expect("get event")
            .expect("event exists");
        assert_eq!(fetched.title, "Weekly Sync");

        let created = provider
            .create_event(&mut http, &credential(), &event())
            .expect("create event");
        assert_eq!(created.remote_id, "evt_123");

        let updated = provider
            .update_event(
                &mut http,
                &credential(),
                &CalendarEvent {
                    title: "Weekly Sync 2".to_string(),
                    ..event()
                },
            )
            .expect("update event");
        assert_eq!(updated.title, "Weekly Sync 2");

        let deleted = provider
            .delete_event(&mut http, &credential(), "evt_123")
            .expect("delete event");
        assert!(deleted);

        assert!(http.requests.iter().any(|request| {
            request.method == "GET"
                && request
                    .url
                    .contains("/calendar/v4/calendars/cal_a1b2/events")
        }));
        assert!(http.requests.iter().any(|request| {
            request.method == "POST"
                && request
                    .headers
                    .iter()
                    .any(|(key, value)| key == "Authorization" && value == "Bearer tenant-token")
        }));
        assert!(http
            .requests
            .iter()
            .any(|request| { request.method == "PATCH" && !request.body.is_empty() }));
    }
}
