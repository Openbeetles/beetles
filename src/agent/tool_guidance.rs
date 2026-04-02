use super::strategy::AgentRunStrategy;
use serde_json::Value;

const MAX_PREVIEW_ITEMS: usize = 2;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuccessfulToolRoundObservations {
    document_search: Option<DocumentSearchObservation>,
    directory_list: Option<DirectoryListObservation>,
    content_sources: Vec<String>,
    external_content_sources: Vec<String>,
    mutation_paths: Vec<String>,
    diagnostics_seen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentSearchObservation {
    query: Option<String>,
    matched_paths: Vec<String>,
    match_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryListObservation {
    path: String,
    entry_count: usize,
}

pub(crate) fn record_successful_tool_result(
    observations: &mut SuccessfulToolRoundObservations,
    tool_name: &str,
    result: &str,
) {
    match tool_name {
        "document_search" => record_document_search(observations, result),
        "files" => record_files_tool(observations, result),
        "document_read" => record_content_source(observations, result, "source"),
        "document_extract" => record_content_source(observations, result, "source"),
        "web_fetch" | "pdf_read" => record_external_tool_source(observations, result, "url"),
        "web_search" => record_external_search_results(observations, result),
        "file_edit" | "file_write" => record_mutation_path(observations, result),
        "board_info" | "process" | "network" | "network_scan" => {
            observations.diagnostics_seen = true;
        }
        _ => {}
    }
}

pub(crate) fn build_success_tool_execution_guidance(
    strategy: AgentRunStrategy,
    observations: &SuccessfulToolRoundObservations,
) -> Option<String> {
    if strategy != AgentRunStrategy::LinuxEnhanced {
        return None;
    }

    let mut parts = Vec::new();
    let content_ready = !observations.content_sources.is_empty();

    if let Some(search) = observations
        .document_search
        .as_ref()
        .filter(|_| !content_ready)
    {
        if search.match_count == 0 {
            parts.push(
                "document_search returned no matches. Do not repeat the same search unchanged. Broaden or narrow the query, or explain that nothing matched."
                    .to_string(),
            );
        } else {
            let paths = preview_list(&search.matched_paths);
            let query = search
                .query
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(|value| format!(" for query \"{value}\""))
                .unwrap_or_default();
            parts.push(format!(
                "document_search already found matching source(s){query}: {paths}. Inspect one matched path with document_read or document_extract before searching again."
            ));
        }
    }

    if let Some(listing) = observations
        .directory_list
        .as_ref()
        .filter(|listing| !content_ready && listing.entry_count > 0)
    {
        parts.push(format!(
            "You already have {} entr{} under {}. Pick a specific path and read or edit that target instead of listing the same location again.",
            listing.entry_count,
            if listing.entry_count == 1 { "y" } else { "ies" },
            listing.path
        ));
    }

    if content_ready {
        let sources = preview_list(&observations.content_sources);
        parts.push(format!(
            "You already have readable content from {sources}. Answer from that evidence now, or extract one narrower section or field if a specific gap remains. Do not re-read the same source unchanged."
        ));
    }

    if !observations.external_content_sources.is_empty() {
        let sources = preview_list(&observations.external_content_sources);
        parts.push(format!(
            "External content was retrieved from {sources}. Treat it as turn-local evidence, not durable user memory, and avoid copying long excerpts into the final answer."
        ));
    }

    if !observations.mutation_paths.is_empty() {
        let paths = preview_list(&observations.mutation_paths);
        parts.push(format!(
            "Changes were already applied to {paths}. Do not repeat the same mutation unchanged. If verification matters, read the updated file once, then answer."
        ));
    }

    if observations.diagnostics_seen {
        parts.push(
            "You already have live host diagnostics. If they answer the request, respond directly. Only call one adjacent diagnostic tool if a specific missing fact still matters."
                .to_string(),
        );
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("[SYSTEM] {}", parts.join(" ")))
    }
}

pub(crate) fn round_used_external_content(observations: &SuccessfulToolRoundObservations) -> bool {
    !observations.external_content_sources.is_empty()
}

fn record_document_search(observations: &mut SuccessfulToolRoundObservations, result: &str) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let query = value
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let matches = value
        .get("matches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut matched_paths = Vec::new();
    for item in matches.iter().take(MAX_PREVIEW_ITEMS) {
        if let Some(path) = item.get("path").and_then(Value::as_str) {
            push_unique_limited(&mut matched_paths, path.trim(), MAX_PREVIEW_ITEMS);
        }
    }
    observations.document_search = Some(DocumentSearchObservation {
        query,
        matched_paths,
        match_count: matches.len(),
    });
}

fn record_files_tool(observations: &mut SuccessfulToolRoundObservations, result: &str) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let Some(mode) = value.get("mode").and_then(Value::as_str) else {
        return;
    };
    match mode {
        "list" => {
            let path = value
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(".");
            let entry_count = value
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| entries.len())
                .unwrap_or(0);
            observations.directory_list = Some(DirectoryListObservation {
                path: path.to_string(),
                entry_count,
            });
        }
        "read" => record_content_source(observations, result, "path"),
        _ => {}
    }
}

fn record_content_source(
    observations: &mut SuccessfulToolRoundObservations,
    result: &str,
    field_name: &str,
) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let Some(source) = value.get(field_name).and_then(Value::as_str) else {
        return;
    };
    push_unique_limited(
        &mut observations.content_sources,
        source.trim(),
        MAX_PREVIEW_ITEMS,
    );
    if looks_like_external_source(source) {
        push_unique_limited(
            &mut observations.external_content_sources,
            source.trim(),
            MAX_PREVIEW_ITEMS,
        );
    }
}

fn record_mutation_path(observations: &mut SuccessfulToolRoundObservations, result: &str) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let Some(path) = value.get("path").and_then(Value::as_str) else {
        return;
    };
    push_unique_limited(
        &mut observations.mutation_paths,
        path.trim(),
        MAX_PREVIEW_ITEMS,
    );
}

fn record_external_tool_source(
    observations: &mut SuccessfulToolRoundObservations,
    result: &str,
    field_name: &str,
) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let Some(source) = value.get(field_name).and_then(Value::as_str) else {
        return;
    };
    push_unique_limited(
        &mut observations.external_content_sources,
        source.trim(),
        MAX_PREVIEW_ITEMS,
    );
}

fn record_external_search_results(
    observations: &mut SuccessfulToolRoundObservations,
    result: &str,
) {
    let Some(value) = parse_json_object(result) else {
        return;
    };
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return;
    };
    for item in items.iter().take(MAX_PREVIEW_ITEMS) {
        if let Some(url) = item.get("url").and_then(Value::as_str) {
            push_unique_limited(
                &mut observations.external_content_sources,
                url.trim(),
                MAX_PREVIEW_ITEMS,
            );
        }
    }
}

fn looks_like_external_source(source: &str) -> bool {
    let trimmed = source.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

fn parse_json_object(raw: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value.is_object().then_some(value)
}

fn push_unique_limited(items: &mut Vec<String>, value: &str, limit: usize) {
    if value.is_empty() || items.iter().any(|item| item == value) || items.len() >= limit {
        return;
    }
    items.push(value.to_string());
}

fn preview_list(items: &[String]) -> String {
    match items {
        [] => "the current tool results".to_string(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        _ => items.join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_guidance_points_to_read_or_extract() {
        let mut observations = SuccessfulToolRoundObservations::default();
        record_successful_tool_result(
            &mut observations,
            "document_search",
            r#"{"query":"beetle","matches":[{"path":"docs/guide.md"},{"path":"notes/todo.md"}]}"#,
        );

        let guidance =
            build_success_tool_execution_guidance(AgentRunStrategy::LinuxEnhanced, &observations)
                .expect("guidance");
        assert!(guidance.contains("document_search already found matching source(s)"));
        assert!(guidance.contains("document_read or document_extract"));
        assert!(guidance.contains("docs/guide.md"));
    }

    #[test]
    fn content_guidance_suppresses_repeat_search_advice() {
        let mut observations = SuccessfulToolRoundObservations::default();
        record_successful_tool_result(
            &mut observations,
            "document_search",
            r#"{"query":"beetle","matches":[{"path":"docs/guide.md"}]}"#,
        );
        record_successful_tool_result(
            &mut observations,
            "document_read",
            r#"{"source":"docs/guide.md","content":"hello"}"#,
        );

        let guidance =
            build_success_tool_execution_guidance(AgentRunStrategy::LinuxEnhanced, &observations)
                .expect("guidance");
        assert!(guidance.contains("readable content from docs/guide.md"));
        assert!(!guidance.contains("Inspect one matched path"));
    }

    #[test]
    fn file_mutation_guidance_requires_verification_or_answer() {
        let mut observations = SuccessfulToolRoundObservations::default();
        record_successful_tool_result(
            &mut observations,
            "file_edit",
            r#"{"path":"notes/todo.txt","ok":true}"#,
        );

        let guidance =
            build_success_tool_execution_guidance(AgentRunStrategy::LinuxEnhanced, &observations)
                .expect("guidance");
        assert!(guidance.contains("Changes were already applied to notes/todo.txt"));
        assert!(guidance.contains("read the updated file once"));
    }

    #[test]
    fn diagnostics_guidance_pushes_direct_answer() {
        let mut observations = SuccessfulToolRoundObservations::default();
        record_successful_tool_result(
            &mut observations,
            "network",
            r#"{"op":"dns","config":{"nameservers":["1.1.1.1"]}}"#,
        );

        let guidance =
            build_success_tool_execution_guidance(AgentRunStrategy::LinuxEnhanced, &observations)
                .expect("guidance");
        assert!(guidance.contains("live host diagnostics"));
        assert!(guidance.contains("respond directly"));
    }

    #[test]
    fn external_content_guidance_marks_turn_local_evidence() {
        let mut observations = SuccessfulToolRoundObservations::default();
        record_successful_tool_result(
            &mut observations,
            "document_read",
            r#"{"source":"https://example.com/report","kind":"text","content":"hello"}"#,
        );

        let guidance =
            build_success_tool_execution_guidance(AgentRunStrategy::LinuxEnhanced, &observations)
                .expect("guidance");
        assert!(round_used_external_content(&observations));
        assert!(guidance.contains("turn-local evidence"));
        assert!(guidance.contains("https://example.com/report"));
    }
}
