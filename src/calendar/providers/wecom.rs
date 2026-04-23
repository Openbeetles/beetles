#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::calendar::credentials::calendar_credential_from_office;
use crate::calendar::{
    filter_calendar_events, normalize_calendar_event, CalendarEvent, CalendarEventStatus,
    CalendarOperation, CalendarProvider, CalendarProviderCredential, CalendarQuery,
};
use crate::error::{Error, Result};
use crate::office::{
    fetch_wecom_access_token_ureq, request_wecom_json_ureq, OfficeHttpClient, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult, WecomApiEnvelope, WecomAuthCredential,
    WecomTokenPayload,
};
use crate::platform::ResponseBody;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const WECOM_SCHEDULE_LIST_PATH: &str = "/cgi-bin/oa/schedule/get_by_calendar";
const WECOM_SCHEDULE_GET_PATH: &str = "/cgi-bin/oa/schedule/get";
const WECOM_SCHEDULE_ADD_PATH: &str = "/cgi-bin/oa/schedule/add";
const WECOM_SCHEDULE_UPDATE_PATH: &str = "/cgi-bin/oa/schedule/update";
const WECOM_SCHEDULE_DELETE_PATH: &str = "/cgi-bin/oa/schedule/del";

pub struct WecomCalendarProvider;

impl CalendarProvider for WecomCalendarProvider {
    fn provider_name(&self) -> &'static str {
        "wecom_calendar"
    }

    fn display_name(&self) -> &'static str {
        "WeCom Calendar"
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
        validate_wecom_calendar_credential(credential)?;
        let access_token = fetch_access_token_http(http, credential)?;
        let url = endpoint(credential, WECOM_SCHEDULE_LIST_PATH, &access_token);
        let mut body = Map::new();
        body.insert("cal_id".to_string(), json!(credential.calendar_id));
        body.insert("offset".to_string(), json!(0));
        body.insert("limit".to_string(), json!(query.limit.clamp(1, 100)));
        if let Some(start) = query.start_from_unix_secs {
            body.insert("start_time".to_string(), json!(start));
        }
        if let Some(end) = query.start_to_unix_secs {
            body.insert("end_time".to_string(), json!(end));
        }
        let payload: WecomApiEnvelope<WecomScheduleListPayload> = request_json(
            "wecom_calendar_list",
            http.post_with_headers(
                &url,
                &[("Content-Type", "application/json")],
                Value::Object(body).to_string().as_bytes(),
            ),
        )?;
        let events = payload
            .require_ok("wecom_calendar_list")?
            .schedule_list
            .into_iter()
            .map(|item| item.into_calendar_event(credential))
            .collect::<Result<Vec<_>>>()?;
        Ok(filter_calendar_events(events, query))
    }

    fn get_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        validate_wecom_calendar_credential(credential)?;
        let access_token = fetch_access_token_http(http, credential)?;
        let payload: WecomApiEnvelope<WecomScheduleListPayload> = request_json(
            "wecom_calendar_get",
            http.post_with_headers(
                &endpoint(credential, WECOM_SCHEDULE_GET_PATH, &access_token),
                &[("Content-Type", "application/json")],
                json!({ "schedule_id_list": [id] }).to_string().as_bytes(),
            ),
        )?;
        let mut items = payload.require_ok("wecom_calendar_get")?.schedule_list;
        let Some(item) = items.pop() else {
            return Ok(None);
        };
        Ok(Some(item.into_calendar_event(credential)?))
    }

    fn create_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_wecom_calendar_credential(credential)?;
        let access_token = fetch_access_token_http(http, credential)?;
        let payload: WecomApiEnvelope<WecomScheduleMutationPayload> = request_json(
            "wecom_calendar_create",
            http.post_with_headers(
                &endpoint(credential, WECOM_SCHEDULE_ADD_PATH, &access_token),
                &[("Content-Type", "application/json")],
                json!({ "schedule": render_schedule_payload(event, credential, None) })
                    .to_string()
                    .as_bytes(),
            ),
        )?;
        let data = payload.require_ok("wecom_calendar_create")?;
        materialize_remote_event(event, credential, &data.schedule_id)
    }

    fn update_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_wecom_calendar_credential(credential)?;
        let schedule_id = event
            .remote_id
            .trim()
            .if_empty_then(|| event.id.trim())
            .ok_or_else(|| {
                Error::config(
                    "wecom_calendar_update",
                    "event id must not be empty for update",
                )
            })?;
        let access_token = fetch_access_token_http(http, credential)?;
        let payload: WecomApiEnvelope<WecomScheduleMutationPayload> = request_json(
            "wecom_calendar_update",
            http.post_with_headers(
                &endpoint(credential, WECOM_SCHEDULE_UPDATE_PATH, &access_token),
                &[("Content-Type", "application/json")],
                json!({ "schedule": render_schedule_payload(event, credential, Some(schedule_id)) })
                    .to_string()
                    .as_bytes(),
            ),
        )?;
        let data = payload.require_ok("wecom_calendar_update")?;
        materialize_remote_event(event, credential, &data.schedule_id)
    }

    fn delete_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool> {
        validate_wecom_calendar_credential(credential)?;
        let access_token = fetch_access_token_http(http, credential)?;
        let payload: WecomApiEnvelope<Value> = request_json(
            "wecom_calendar_delete",
            http.post_with_headers(
                &endpoint(credential, WECOM_SCHEDULE_DELETE_PATH, &access_token),
                &[("Content-Type", "application/json")],
                json!({ "schedule_id": id }).to_string().as_bytes(),
            ),
        )?;
        payload.require_ok("wecom_calendar_delete")?;
        Ok(true)
    }
}

pub struct WecomCalendarOfficeProbeAdapter;

impl OfficeProbeAdapter for WecomCalendarOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "wecom_calendar"
    }

    fn probe(
        &self,
        _http: &mut dyn crate::office::OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = calendar_credential_from_office(account.clone(), credential.clone());
        if validate_wecom_calendar_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "calendar_transport_config_missing".to_string(),
            });
        }
        let access_token = fetch_wecom_access_token_ureq(
            "wecom_calendar_auth",
            WecomAuthCredential {
                corp_id: adapted.app_id.as_str(),
                corp_secret: adapted.access_token.as_str(),
                base_url: adapted.base_url.as_str(),
            },
        )?;
        let payload: WecomApiEnvelope<WecomScheduleListPayload> = request_wecom_json_ureq(
            "wecom_calendar_probe",
            ureq::post(&endpoint(&adapted, WECOM_SCHEDULE_LIST_PATH, &access_token))
                .set("Content-Type", "application/json")
                .send_string(
                    &json!({
                        "cal_id": adapted.calendar_id,
                        "offset": 0,
                        "limit": 1
                    })
                    .to_string(),
                ),
        )?;
        payload.require_ok("wecom_calendar_probe")?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "wecom_calendar_schedule_ok".to_string(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct WecomScheduleListPayload {
    #[serde(default)]
    schedule_list: Vec<WecomSchedule>,
}

#[derive(Debug, Default, Deserialize)]
struct WecomScheduleMutationPayload {
    #[serde(default)]
    schedule_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct WecomSchedule {
    #[serde(default)]
    schedule_id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    start_time: u64,
    #[serde(default)]
    end_time: u64,
    #[serde(default)]
    cal_id: String,
}

impl WecomSchedule {
    fn into_calendar_event(self, credential: &CalendarProviderCredential) -> Result<CalendarEvent> {
        let schedule_id = self
            .schedule_id
            .trim()
            .if_empty_then(|| self.summary.trim())
            .ok_or_else(|| Error::config("wecom_calendar_parse", "missing schedule_id"))?;
        let schedule_id = schedule_id.to_string();
        normalize_calendar_event(CalendarEvent {
            id: schedule_id.clone(),
            title: self.summary,
            start_at_unix_secs: self.start_time,
            end_at_unix_secs: self.end_time,
            timezone: "Asia/Shanghai".to_string(),
            location: self.location,
            notes: self.description,
            provider: credential.provider.clone(),
            calendar_id: self
                .cal_id
                .if_empty_then(|| credential.calendar_id.as_str())
                .unwrap_or_default()
                .to_string(),
            remote_id: schedule_id,
            status: CalendarEventStatus::Confirmed,
            updated_at: crate::util::current_unix_secs(),
        })
    }
}

fn validate_wecom_calendar_credential(credential: &CalendarProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_calendar_provider",
            "calendar_corp_id must not be empty",
        ));
    }
    if credential.access_token.trim().is_empty() {
        return Err(Error::config(
            "wecom_calendar_provider",
            "access_token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "wecom_calendar_provider",
            "calendar_base_url must not be empty",
        ));
    }
    if credential.calendar_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_calendar_provider",
            "calendar_id must not be empty",
        ));
    }
    Ok(())
}

fn fetch_access_token_http(
    http: &mut dyn OfficeHttpClient,
    credential: &CalendarProviderCredential,
) -> Result<String> {
    let url = format!(
        "{}/cgi-bin/gettoken?corpid={}&corpsecret={}",
        credential.base_url.trim_end_matches('/'),
        urlencoding::encode(&credential.app_id),
        urlencoding::encode(&credential.access_token)
    );
    let payload: WecomApiEnvelope<WecomTokenPayload> =
        request_json("wecom_calendar_auth", http.get_with_headers(&url, &[]))?;
    let data = payload.require_ok("wecom_calendar_auth")?;
    if data.access_token.trim().is_empty() {
        return Err(Error::config("wecom_calendar_auth", "missing access_token"));
    }
    Ok(data.access_token)
}

fn request_json<T: DeserializeOwned>(
    stage: &'static str,
    response: Result<(u16, ResponseBody)>,
) -> Result<T> {
    let (status, body) = response.map_err(|error| error.with_stage(stage))?;
    if !(200..300).contains(&status) {
        return Err(Error::http(stage, status));
    }
    serde_json::from_slice(body.as_slice()).map_err(|error| Error::config(stage, error.to_string()))
}

fn endpoint(credential: &CalendarProviderCredential, path: &str, access_token: &str) -> String {
    format!(
        "{}{}?access_token={}",
        credential.base_url.trim_end_matches('/'),
        path,
        urlencoding::encode(access_token)
    )
}

fn render_schedule_payload(
    event: &CalendarEvent,
    credential: &CalendarProviderCredential,
    schedule_id: Option<&str>,
) -> Value {
    let mut schedule = Map::new();
    if let Some(schedule_id) = schedule_id {
        schedule.insert("schedule_id".to_string(), json!(schedule_id));
    }
    schedule.insert("summary".to_string(), json!(event.title));
    schedule.insert("description".to_string(), json!(event.notes));
    schedule.insert("location".to_string(), json!(event.location));
    schedule.insert("start_time".to_string(), json!(event.start_at_unix_secs));
    schedule.insert("end_time".to_string(), json!(event.end_at_unix_secs));
    schedule.insert("cal_id".to_string(), json!(credential.calendar_id));
    Value::Object(schedule)
}

fn materialize_remote_event(
    event: &CalendarEvent,
    credential: &CalendarProviderCredential,
    schedule_id: &str,
) -> Result<CalendarEvent> {
    normalize_calendar_event(CalendarEvent {
        id: schedule_id.to_string(),
        title: event.title.clone(),
        start_at_unix_secs: event.start_at_unix_secs,
        end_at_unix_secs: event.end_at_unix_secs,
        timezone: if event.timezone.trim().is_empty() {
            "Asia/Shanghai".to_string()
        } else {
            event.timezone.clone()
        },
        location: event.location.clone(),
        notes: event.notes.clone(),
        provider: credential.provider.clone(),
        calendar_id: credential.calendar_id.clone(),
        remote_id: schedule_id.to_string(),
        status: event.status,
        updated_at: crate::util::current_unix_secs(),
    })
}

trait IfEmptyThen<'a> {
    fn if_empty_then<F>(&'a self, fallback: F) -> Option<&'a str>
    where
        F: FnOnce() -> &'a str;
}

impl<'a> IfEmptyThen<'a> for str {
    fn if_empty_then<F>(&'a self, fallback: F) -> Option<&'a str>
    where
        F: FnOnce() -> &'a str,
    {
        let trimmed = self.trim();
        if trimmed.is_empty() {
            let next = fallback().trim();
            (!next.is_empty()).then_some(next)
        } else {
            Some(trimmed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ResponseBody;

    fn credential() -> CalendarProviderCredential {
        CalendarProviderCredential {
            account_key: "calendar-wecom".to_string(),
            provider: "wecom_calendar".to_string(),
            account_id: "calendar-admin".to_string(),
            account_label: "WeCom Calendar".to_string(),
            calendar_id: "cal-wecom-1".to_string(),
            username: String::new(),
            app_id: "wwcorp123".to_string(),
            base_url: crate::office::WECOM_DEFAULT_BASE_URL.to_string(),
            root_path: String::new(),
            access_token: "corp-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
        }
    }

    fn event() -> CalendarEvent {
        CalendarEvent {
            id: "sched_123".to_string(),
            title: "Weekly Sync".to_string(),
            start_at_unix_secs: 1_775_000_000,
            end_at_unix_secs: 1_775_003_600,
            timezone: "Asia/Shanghai".to_string(),
            location: "Room A".to_string(),
            notes: "Discuss launch".to_string(),
            provider: "wecom_calendar".to_string(),
            calendar_id: "cal-wecom-1".to_string(),
            remote_id: "sched_123".to_string(),
            status: CalendarEventStatus::Confirmed,
            updated_at: 0,
        }
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    struct RecordingHttp {
        responses: std::collections::VecDeque<(u16, &'static str)>,
        requests: Vec<RecordedRequest>,
    }

    impl RecordingHttp {
        fn new(responses: Vec<(u16, &'static str)>) -> Self {
            Self {
                responses: responses.into(),
                requests: Vec::new(),
            }
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
            let (status, body) = self.responses.pop_front().expect("response");
            Ok((status, ResponseBody::Heap(body.as_bytes().to_vec())))
        }
    }

    impl OfficeHttpClient for RecordingHttp {
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
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
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
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }
    }

    #[test]
    fn wecom_calendar_provider_reports_supported_operations() {
        let provider = WecomCalendarProvider;
        assert!(provider.supports(CalendarOperation::List));
        assert!(provider.supports(CalendarOperation::Get));
        assert!(provider.supports(CalendarOperation::Create));
        assert!(provider.supports(CalendarOperation::Update));
        assert!(provider.supports(CalendarOperation::Delete));
    }

    #[test]
    fn wecom_schedule_maps_to_calendar_event() {
        let event = WecomSchedule {
            schedule_id: "sched_123".to_string(),
            summary: "Weekly Sync".to_string(),
            description: "Discuss launch".to_string(),
            location: "Room A".to_string(),
            start_time: 1_775_000_000,
            end_time: 1_775_003_600,
            cal_id: "cal-wecom-1".to_string(),
        }
        .into_calendar_event(&credential())
        .expect("calendar event");
        assert_eq!(event.id, "sched_123");
        assert_eq!(event.title, "Weekly Sync");
        assert_eq!(event.calendar_id, "cal-wecom-1");
    }

    #[test]
    fn validate_wecom_calendar_credential_rejects_missing_corp_id() {
        let error = validate_wecom_calendar_credential(&CalendarProviderCredential {
            app_id: String::new(),
            ..credential()
        })
        .expect_err("missing corp id");
        assert_eq!(error.stage(), "wecom_calendar_provider");
    }

    #[test]
    fn wecom_calendar_provider_lists_gets_and_mutates_events() {
        let provider = WecomCalendarProvider;
        let mut http = RecordingHttp::new(vec![
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","schedule_list":[{"schedule_id":"sched_123","summary":"Weekly Sync","description":"Discuss launch","location":"Room A","start_time":1775000000,"end_time":1775003600,"cal_id":"cal-wecom-1"}]}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","schedule_list":[{"schedule_id":"sched_123","summary":"Weekly Sync","description":"Discuss launch","location":"Room A","start_time":1775000000,"end_time":1775003600,"cal_id":"cal-wecom-1"}]}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","schedule_id":"sched_123"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","schedule_id":"sched_123"}"#,
            ),
            (
                200,
                r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#,
            ),
            (200, r#"{"errcode":0,"errmsg":"ok"}"#),
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
        assert_eq!(listed[0].id, "sched_123");

        let fetched = provider
            .get_event(&mut http, &credential(), "sched_123")
            .expect("get event")
            .expect("event exists");
        assert_eq!(fetched.title, "Weekly Sync");

        let created = provider
            .create_event(&mut http, &credential(), &event())
            .expect("create event");
        assert_eq!(created.remote_id, "sched_123");

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
            .delete_event(&mut http, &credential(), "sched_123")
            .expect("delete event");
        assert!(deleted);

        assert!(http.requests.iter().any(|request| {
            request.method == "POST"
                && request.url.contains("/cgi-bin/oa/schedule/get_by_calendar")
                && request
                    .headers
                    .iter()
                    .any(|(key, value)| key == "Content-Type" && value == "application/json")
        }));
        assert!(http.requests.iter().any(|request| {
            request.method == "POST"
                && request.url.contains("/cgi-bin/oa/schedule/update")
                && std::str::from_utf8(&request.body)
                    .unwrap_or_default()
                    .contains("Weekly Sync 2")
        }));
    }
}
