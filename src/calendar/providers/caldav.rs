#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::calendar::{
    filter_calendar_events, normalize_calendar_event, CalendarEvent, CalendarEventStatus,
    CalendarOperation, CalendarProvider, CalendarProviderCredential, CalendarQuery,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use base64::Engine as _;
use quick_xml::events::Event;
use quick_xml::Reader;

const CALDAV_CONTENT_TYPE: &str = "text/calendar; charset=utf-8";
const REPORT_CONTENT_TYPE: &str = "application/xml; charset=utf-8";

pub struct CalDavProvider;

impl CalendarProvider for CalDavProvider {
    fn provider_name(&self) -> &'static str {
        "caldav"
    }

    fn display_name(&self) -> &'static str {
        "CalDAV"
    }

    fn supports(&self, _op: CalendarOperation) -> bool {
        true
    }

    fn list_events(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        validate_caldav_credential(credential)?;
        let url = build_calendar_collection_url(credential);
        let body = render_calendar_query_body(query);
        let response = with_caldav_headers(
            credential,
            REPORT_CONTENT_TYPE,
            &[("Depth", "1")],
            |headers| http.request_with_headers("REPORT", &url, headers, Some(body.as_bytes())),
        )?;
        if response.0 != 207 {
            return Err(Error::config(
                "caldav_list_events",
                format!("calendar REPORT failed with status {}", response.0),
            ));
        }
        let xml = response_text(response.1);
        let items = parse_calendar_query_response(credential, &xml)?;
        Ok(filter_calendar_events(items, query))
    }

    fn get_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        validate_caldav_credential(credential)?;
        let url = build_event_resource_url(credential, id)?;
        let response = with_caldav_headers(credential, CALDAV_CONTENT_TYPE, &[], |headers| {
            http.get_with_headers(&url, headers)
        })?;
        match response.0 {
            200 => {
                let ics = response_text(response.1);
                Ok(Some(parse_ical_event(credential, &url, &ics)?))
            }
            404 => Ok(None),
            status => Err(Error::config(
                "caldav_get_event",
                format!("calendar GET failed with status {status}"),
            )),
        }
    }

    fn create_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_caldav_credential(credential)?;
        let remote_id = normalize_event_resource_name(event.remote_id.as_str(), &event.id)?;
        let url = build_event_resource_url(credential, &remote_id)?;
        let payload = render_ical_event(event, &remote_id)?;
        let response = with_caldav_headers(credential, CALDAV_CONTENT_TYPE, &[], |headers| {
            http.put_with_headers(&url, headers, payload.as_bytes())
        })?;
        if !matches!(response.0, 200 | 201 | 204) {
            return Err(Error::config(
                "caldav_create_event",
                format!("calendar PUT failed with status {}", response.0),
            ));
        }
        materialize_remote_event(event, credential, &remote_id)
    }

    fn update_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent> {
        validate_caldav_credential(credential)?;
        let remote_id = normalize_event_resource_name(event.remote_id.as_str(), &event.id)?;
        let url = build_event_resource_url(credential, &remote_id)?;
        let payload = render_ical_event(event, &remote_id)?;
        let response = with_caldav_headers(credential, CALDAV_CONTENT_TYPE, &[], |headers| {
            http.put_with_headers(&url, headers, payload.as_bytes())
        })?;
        if !matches!(response.0, 200 | 201 | 204) {
            return Err(Error::config(
                "caldav_update_event",
                format!("calendar PUT failed with status {}", response.0),
            ));
        }
        materialize_remote_event(event, credential, &remote_id)
    }

    fn delete_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool> {
        validate_caldav_credential(credential)?;
        let url = build_event_resource_url(credential, id)?;
        let response = with_caldav_headers(credential, CALDAV_CONTENT_TYPE, &[], |headers| {
            http.delete_with_headers(&url, headers)
        })?;
        match response.0 {
            200 | 204 => Ok(true),
            404 => Ok(false),
            status => Err(Error::config(
                "caldav_delete_event",
                format!("calendar DELETE failed with status {status}"),
            )),
        }
    }
}

pub struct CalDavOfficeProbeAdapter;

impl OfficeProbeAdapter for CalDavOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "caldav"
    }

    fn probe(
        &self,
        _http: &mut dyn crate::office::OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = crate::calendar::credentials::calendar_credential_from_office(
            account.clone(),
            credential.clone(),
        );
        if validate_caldav_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "calendar_transport_config_missing".to_string(),
            });
        }
        probe_calendar_collection(&adapted)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "caldav_propfind_ok".to_string(),
        })
    }
}

fn validate_caldav_credential(credential: &CalendarProviderCredential) -> Result<()> {
    if credential.username.trim().is_empty() {
        return Err(Error::config(
            "caldav_provider",
            "username must not be empty",
        ));
    }
    if credential.access_token.trim().is_empty() {
        return Err(Error::config(
            "caldav_provider",
            "access_token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "caldav_provider",
            "base_url must not be empty",
        ));
    }
    if credential.calendar_id.trim().is_empty() {
        return Err(Error::config(
            "caldav_provider",
            "calendar_id must not be empty",
        ));
    }
    Ok(())
}

fn build_calendar_collection_url(credential: &CalendarProviderCredential) -> String {
    let mut url = credential.base_url.trim().trim_end_matches('/').to_string();
    for segment in path_segments(&credential.root_path) {
        url.push('/');
        url.push_str(&urlencoding::encode(segment));
    }
    for segment in path_segments(&credential.calendar_id) {
        url.push('/');
        url.push_str(&urlencoding::encode(segment));
    }
    url.push('/');
    url
}

fn parse_calendar_query_response(
    credential: &CalendarProviderCredential,
    xml: &str,
) -> Result<Vec<CalendarEvent>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current = RawCalendarResponse::default();
    let mut responses = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = local_name(event.name().as_ref()).to_string();
                if name == "response" {
                    current = RawCalendarResponse::default();
                }
                stack.push(name);
            }
            Ok(Event::Text(text)) => {
                if let Some(tag) = stack.last() {
                    let value = text
                        .decode()
                        .map(|text| text.into_owned())
                        .unwrap_or_default();
                    match tag.as_str() {
                        "href" => current.href = value,
                        "calendar-data" => current.calendar_data = value,
                        _ => {}
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if stack.last().is_some_and(|tag| tag == "calendar-data") {
                    current.calendar_data = text
                        .decode()
                        .map(|text| text.into_owned())
                        .unwrap_or_default();
                }
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref()).to_string();
                if name == "response" {
                    responses.push(current.clone());
                    current = RawCalendarResponse::default();
                }
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(Error::config("caldav_report_parse", error.to_string())),
            _ => {}
        }
        buf.clear();
    }

    let mut items = Vec::new();
    for response in responses {
        if response.href.trim().is_empty() || response.calendar_data.trim().is_empty() {
            continue;
        }
        items.push(parse_ical_event(
            credential,
            &response.href,
            &response.calendar_data,
        )?);
    }
    Ok(items)
}

fn parse_ical_event(
    credential: &CalendarProviderCredential,
    href: &str,
    ics: &str,
) -> Result<CalendarEvent> {
    let unfolded = unfold_ical_lines(ics);
    let mut in_event = false;
    let mut title = String::new();
    let mut location = String::new();
    let mut notes = String::new();
    let mut uid = String::new();
    let mut timezone = String::new();
    let mut start_at_unix_secs = None;
    let mut end_at_unix_secs = None;
    let mut status = CalendarEventStatus::Confirmed;
    let mut duration_secs = None;

    for line in unfolded {
        let line = line.trim();
        if line == "BEGIN:VEVENT" {
            in_event = true;
            continue;
        }
        if line == "END:VEVENT" {
            break;
        }
        if !in_event || line.is_empty() {
            continue;
        }
        let Some((name, params, value)) = parse_ical_property(line) else {
            continue;
        };
        match name {
            "UID" => uid = value.to_string(),
            "SUMMARY" => title = unescape_ical_text(value),
            "LOCATION" => location = unescape_ical_text(value),
            "DESCRIPTION" => notes = unescape_ical_text(value),
            "STATUS" => {
                if value.eq_ignore_ascii_case("CANCELLED") {
                    status = CalendarEventStatus::Cancelled;
                }
            }
            "DTSTART" => {
                let (epoch, parsed_tz) = parse_ical_datetime(value, params.get("TZID"))?;
                start_at_unix_secs = Some(epoch);
                if timezone.is_empty() {
                    timezone = parsed_tz;
                }
            }
            "DTEND" => {
                let (epoch, parsed_tz) = parse_ical_datetime(value, params.get("TZID"))?;
                end_at_unix_secs = Some(epoch);
                if timezone.is_empty() {
                    timezone = parsed_tz;
                }
            }
            "DURATION" => duration_secs = parse_ical_duration(value),
            _ => {}
        }
    }

    let start_at_unix_secs = start_at_unix_secs
        .ok_or_else(|| Error::config("caldav_ical_parse", "VEVENT is missing DTSTART"))?;
    let end_at_unix_secs = end_at_unix_secs
        .or_else(|| duration_secs.map(|seconds| start_at_unix_secs.saturating_add(seconds)))
        .unwrap_or_else(|| start_at_unix_secs.saturating_add(3600));
    let remote_id =
        href_remote_id(href).or_else(|| (!uid.trim().is_empty()).then(|| uid.trim().to_string()));
    let remote_id = remote_id.ok_or_else(|| {
        Error::config(
            "caldav_ical_parse",
            "VEVENT is missing addressable resource id",
        )
    })?;
    normalize_calendar_event(CalendarEvent {
        id: remote_id.clone(),
        title: if title.trim().is_empty() {
            "Untitled event".to_string()
        } else {
            title
        },
        start_at_unix_secs,
        end_at_unix_secs,
        timezone,
        location,
        notes,
        provider: credential.provider.clone(),
        calendar_id: credential.calendar_id.clone(),
        remote_id,
        status,
        updated_at: crate::util::current_unix_secs(),
    })
}

fn probe_calendar_collection(credential: &CalendarProviderCredential) -> Result<()> {
    let url = build_calendar_collection_url(credential);
    let response = ureq::request("PROPFIND", &url)
        .set("Depth", "0")
        .set("Content-Type", REPORT_CONTENT_TYPE)
        .set("Authorization", &basic_auth_header(credential))
        .call();
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _response)) => {
            return Err(Error::config(
                "caldav_probe",
                format!("calendar PROPFIND failed with status {status}"),
            ))
        }
        Err(ureq::Error::Transport(error)) => {
            return Err(Error::config("caldav_probe", error.to_string()))
        }
    };
    match response.status() {
        200 | 207 => Ok(()),
        status => Err(Error::config(
            "caldav_probe",
            format!("calendar PROPFIND failed with status {status}"),
        )),
    }
}

fn with_caldav_headers<T>(
    credential: &CalendarProviderCredential,
    content_type: &str,
    extra: &[(&str, &str)],
    f: impl FnOnce(&[(&str, &str)]) -> Result<T>,
) -> Result<T> {
    let auth = basic_auth_header(credential);
    let mut headers = Vec::with_capacity(extra.len() + 2);
    headers.push(("Authorization", auth.as_str()));
    headers.push(("Content-Type", content_type));
    headers.extend(extra.iter().copied());
    f(&headers)
}

fn response_text(body: crate::platform::ResponseBody) -> String {
    String::from_utf8_lossy(body.as_ref()).into_owned()
}

fn materialize_remote_event(
    event: &CalendarEvent,
    credential: &CalendarProviderCredential,
    remote_id: &str,
) -> Result<CalendarEvent> {
    normalize_calendar_event(CalendarEvent {
        id: remote_id.to_string(),
        title: event.title.clone(),
        start_at_unix_secs: event.start_at_unix_secs,
        end_at_unix_secs: event.end_at_unix_secs,
        timezone: event.timezone.clone(),
        location: event.location.clone(),
        notes: event.notes.clone(),
        provider: credential.provider.clone(),
        calendar_id: credential.calendar_id.clone(),
        remote_id: remote_id.to_string(),
        status: event.status,
        updated_at: crate::util::current_unix_secs(),
    })
}

fn render_calendar_query_body(query: CalendarQuery) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT">"#,
    );
    if query.start_from_unix_secs.is_some() || query.start_to_unix_secs.is_some() {
        xml.push_str(r#"<c:time-range"#);
        if let Some(start) = query.start_from_unix_secs {
            xml.push_str(r#" start=""#);
            xml.push_str(&format_ical_utc(start));
            xml.push('"');
        }
        if let Some(end) = query.start_to_unix_secs {
            xml.push_str(r#" end=""#);
            xml.push_str(&format_ical_utc(end));
            xml.push('"');
        }
        xml.push_str("/>");
    }
    xml.push_str(r#"</c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#);
    xml
}

fn render_ical_event(event: &CalendarEvent, remote_id: &str) -> Result<String> {
    let uid = if event.remote_id.trim().is_empty() {
        remote_id.to_string()
    } else {
        event.remote_id.trim().to_string()
    };
    let normalized = normalize_calendar_event(CalendarEvent {
        id: remote_id.to_string(),
        title: event.title.clone(),
        start_at_unix_secs: event.start_at_unix_secs,
        end_at_unix_secs: event.end_at_unix_secs,
        timezone: event.timezone.clone(),
        location: event.location.clone(),
        notes: event.notes.clone(),
        provider: event.provider.clone(),
        calendar_id: event.calendar_id.clone(),
        remote_id: uid.clone(),
        status: event.status,
        updated_at: event.updated_at,
    })?;
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str("VERSION:2.0\r\n");
    out.push_str("PRODID:-//beetle//CalDAV//EN\r\n");
    out.push_str("BEGIN:VEVENT\r\n");
    out.push_str("UID:");
    out.push_str(&escape_ical_text(&uid));
    out.push_str("\r\n");
    out.push_str("SUMMARY:");
    out.push_str(&escape_ical_text(&normalized.title));
    out.push_str("\r\n");
    out.push_str("DTSTART:");
    out.push_str(&format_ical_utc(normalized.start_at_unix_secs));
    out.push_str("\r\n");
    out.push_str("DTEND:");
    out.push_str(&format_ical_utc(normalized.end_at_unix_secs));
    out.push_str("\r\n");
    if !normalized.location.trim().is_empty() {
        out.push_str("LOCATION:");
        out.push_str(&escape_ical_text(&normalized.location));
        out.push_str("\r\n");
    }
    if !normalized.notes.trim().is_empty() {
        out.push_str("DESCRIPTION:");
        out.push_str(&escape_ical_text(&normalized.notes));
        out.push_str("\r\n");
    }
    out.push_str("STATUS:");
    out.push_str(match normalized.status {
        CalendarEventStatus::Confirmed => "CONFIRMED",
        CalendarEventStatus::Cancelled => "CANCELLED",
    });
    out.push_str("\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
    Ok(out)
}

fn format_ical_utc(epoch: u64) -> String {
    let (year, month, day, hour, min, sec) = crate::util::epoch_to_ymdhms(epoch);
    format!("{year:04}{month:02}{day:02}T{hour:02}{min:02}{sec:02}Z")
}

fn parse_ical_datetime(value: &str, tzid: Option<&String>) -> Result<(u64, String)> {
    let trimmed = value.trim();
    if trimmed.len() == 8 {
        let year = trimmed[0..4]
            .parse::<i32>()
            .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
        let month = trimmed[4..6]
            .parse::<u32>()
            .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
        let day = trimmed[6..8]
            .parse::<u32>()
            .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
        return Ok((
            crate::util::ymdhms_to_epoch(year, month, day, 0, 0, 0),
            tzid.cloned().unwrap_or_else(|| "UTC".to_string()),
        ));
    }
    if trimmed.len() < 15 {
        return Err(Error::config(
            "caldav_ical_parse",
            format!("unsupported datetime value: {trimmed}"),
        ));
    }
    let core = trimmed.trim_end_matches('Z');
    let year = core[0..4]
        .parse::<i32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let month = core[4..6]
        .parse::<u32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let day = core[6..8]
        .parse::<u32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let hour = core[9..11]
        .parse::<u32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let min = core[11..13]
        .parse::<u32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let sec = core[13..15]
        .parse::<u32>()
        .map_err(|error| Error::config("caldav_ical_parse", error.to_string()))?;
    let timezone = if trimmed.ends_with('Z') {
        "UTC".to_string()
    } else {
        tzid.cloned().unwrap_or_else(|| "UTC".to_string())
    };
    Ok((
        crate::util::ymdhms_to_epoch(year, month, day, hour, min, sec),
        timezone,
    ))
}

fn parse_ical_duration(value: &str) -> Option<u64> {
    let mut rest = value.trim();
    if !rest.starts_with('P') {
        return None;
    }
    rest = &rest[1..];
    let mut total = 0u64;
    let mut number = String::new();
    let mut in_time = false;
    for ch in rest.chars() {
        match ch {
            'T' => in_time = true,
            '0'..='9' => number.push(ch),
            'D' => {
                total = total.checked_add(number.parse::<u64>().ok()? * 86_400)?;
                number.clear();
            }
            'H' if in_time => {
                total = total.checked_add(number.parse::<u64>().ok()? * 3_600)?;
                number.clear();
            }
            'M' if in_time => {
                total = total.checked_add(number.parse::<u64>().ok()? * 60)?;
                number.clear();
            }
            'S' if in_time => {
                total = total.checked_add(number.parse::<u64>().ok()?)?;
                number.clear();
            }
            _ => return None,
        }
    }
    Some(total)
}

fn unfold_ical_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if raw.starts_with(' ') || raw.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(raw.trim_start());
            }
            continue;
        }
        lines.push(raw.to_string());
    }
    lines
}

fn parse_ical_property(
    line: &str,
) -> Option<(&str, std::collections::BTreeMap<String, String>, &str)> {
    let (name_and_params, value) = line.split_once(':')?;
    let mut parts = name_and_params.split(';');
    let name = parts.next()?.trim();
    let mut params = std::collections::BTreeMap::new();
    for part in parts {
        let (key, value) = part.split_once('=')?;
        params.insert(key.trim().to_string(), value.trim().to_string());
    }
    Some((name, params, value))
}

fn escape_ical_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn unescape_ical_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(',') => out.push(','),
            Some(';') => out.push(';'),
            Some(other) => out.push(other),
            None => break,
        }
    }
    out
}

fn basic_auth_header(credential: &CalendarProviderCredential) -> String {
    let raw = format!("{}:{}", credential.username.trim(), credential.access_token);
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    )
}

fn path_segments(path: &str) -> Vec<&str> {
    path.trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect()
}

fn build_event_resource_url(credential: &CalendarProviderCredential, id: &str) -> Result<String> {
    let resource = normalize_event_resource_name(id, id)?;
    Ok(format!(
        "{}{}",
        build_calendar_collection_url(credential),
        urlencoding::encode(&resource)
    ))
}

fn normalize_event_resource_name(remote_id: &str, fallback_id: &str) -> Result<String> {
    let candidate = if remote_id.trim().is_empty() {
        fallback_id.trim()
    } else {
        remote_id.trim()
    };
    let candidate = candidate
        .split('?')
        .next()
        .unwrap_or(candidate)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(candidate)
        .trim();
    if candidate.is_empty() {
        return Err(Error::config(
            "caldav_provider",
            "event resource id must not be empty",
        ));
    }
    if candidate.ends_with(".ics") {
        Ok(candidate.to_string())
    } else {
        Ok(format!("{candidate}.ics"))
    }
}

fn href_remote_id(href: &str) -> Option<String> {
    let path = strip_url_origin(href);
    let segment = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|segment| !segment.trim().is_empty())?;
    Some(
        urlencoding::decode(segment)
            .unwrap_or(std::borrow::Cow::Borrowed(segment))
            .into_owned(),
    )
}

fn local_name(name: &[u8]) -> &str {
    let raw = std::str::from_utf8(name).unwrap_or_default();
    raw.rsplit(':').next().unwrap_or(raw)
}

fn strip_url_origin(url: &str) -> String {
    let without_origin = if let Some(scheme_pos) = url.find("://") {
        let rest = &url[scheme_pos + 3..];
        match rest.find('/') {
            Some(path_pos) => &rest[path_pos..],
            None => "/",
        }
    } else {
        url
    };
    without_origin.to_string()
}

#[derive(Clone, Debug, Default)]
struct RawCalendarResponse {
    href: String,
    calendar_data: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccount, OfficeAccountIdentityClass, OfficeCapability};

    fn credential() -> CalendarProviderCredential {
        CalendarProviderCredential {
            account_key: "calendar-work".to_string(),
            provider: "caldav".to_string(),
            account_id: "work@example.com".to_string(),
            account_label: "Work Calendar".to_string(),
            calendar_id: "team".to_string(),
            username: "caldav-user".to_string(),
            app_id: String::new(),
            base_url: "https://dav.example.com/remote.php/dav/calendars".to_string(),
            root_path: "/work".to_string(),
            access_token: "app-password".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 1,
        }
    }

    #[test]
    fn validate_caldav_credential_accepts_complete_transport_shape() {
        validate_caldav_credential(&credential()).expect("valid caldav credential");
    }

    #[test]
    fn build_calendar_collection_url_joins_root_and_calendar_id() {
        assert_eq!(
            build_calendar_collection_url(&credential()),
            "https://dav.example.com/remote.php/dav/calendars/work/team/"
        );
    }

    #[test]
    fn parse_ical_event_maps_basic_fields() {
        let event = parse_ical_event(
            &credential(),
            "/remote.php/dav/calendars/work/team/demo.ics",
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:demo.ics\r\nSUMMARY:Weekly Sync\r\nDTSTART:20260414T010000Z\r\nDTEND:20260414T020000Z\r\nLOCATION:Room 101\r\nDESCRIPTION:Bring roadmap\r\nSTATUS:CONFIRMED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .expect("parse ical");
        assert_eq!(event.id, "demo.ics");
        assert_eq!(event.remote_id, "demo.ics");
        assert_eq!(event.title, "Weekly Sync");
        assert_eq!(event.location, "Room 101");
        assert_eq!(event.notes, "Bring roadmap");
        assert_eq!(event.calendar_id, "team");
    }

    #[test]
    fn parse_calendar_query_response_collects_calendar_data_entries() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/remote.php/dav/calendars/work/team/demo.ics</d:href>
    <d:propstat>
      <d:prop>
        <c:calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:demo.ics
SUMMARY:Weekly Sync
DTSTART:20260414T010000Z
DTEND:20260414T020000Z
STATUS:CONFIRMED
END:VEVENT
END:VCALENDAR</c:calendar-data>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let items = parse_calendar_query_response(&credential(), xml).expect("parse report");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "demo.ics");
        assert_eq!(items[0].title, "Weekly Sync");
    }

    #[test]
    fn probe_adapter_reports_missing_transport_shape_before_network() {
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = CalDavOfficeProbeAdapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "calendar-work".to_string(),
                    provider_kind: "caldav".to_string(),
                    external_account_id: "work@example.com".to_string(),
                    account_label: "Work".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Calendar],
                },
                &crate::office::OfficeCredential {
                    account_key: "calendar-work".to_string(),
                    access_token: "secret".to_string(),
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
        assert_eq!(result.reason, "calendar_transport_config_missing");
    }
}
