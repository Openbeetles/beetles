use crate::util::scrub_credentials;

const PRIVATE_ECHO_MIN_CHARS: usize = 16;
const PRIVATE_ECHO_CHUNK_CHARS: usize = 28;
const PRIVATE_ECHO_CHUNK_STEP: usize = 14;

pub(crate) fn scrub_memory_prompt_block(input: &str) -> String {
    redact_prompt_identifiers(scrub_credentials(input.trim()).as_ref())
}

pub(crate) fn scrub_private_source_echoes(input: &str, private_sources: &[&str]) -> String {
    let mut output = scrub_memory_prompt_block(input);
    for source in private_sources {
        for fragment in private_echo_fragments(source) {
            output = output.replace(&fragment, "[redacted:private_echo]");
        }
    }
    output
}

fn redact_prompt_identifiers(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_identifier_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_identifier_token(token: &str) -> String {
    let mut value = token.to_string();
    for marker in ["chat_id=", "relationship_scope=", "channel="] {
        if let Some(start) = value.find(marker) {
            let prefix_end = start + marker.len();
            let suffix_start = value[prefix_end..]
                .find([',', ';', ')', ']'])
                .map(|offset| prefix_end + offset)
                .unwrap_or(value.len());
            let replacement = match marker {
                "chat_id=" => "[redacted:chat_id]",
                "relationship_scope=" => "[redacted:relationship_scope]",
                "channel=" => "[redacted:channel]",
                _ => "[redacted:id]",
            };
            value.replace_range(prefix_end..suffix_start, replacement);
        }
    }
    if value.contains(':')
        && (value.contains("qq_channel")
            || value.contains("telegram")
            || value.contains("tg")
            || value.contains("feishu"))
    {
        return "[redacted:relationship_scope]".to_string();
    }
    value
}

fn private_echo_fragments(source: &str) -> Vec<String> {
    let scrubbed = scrub_memory_prompt_block(source);
    let chars = scrubbed.chars().collect::<Vec<char>>();
    if chars.len() < PRIVATE_ECHO_MIN_CHARS {
        return Vec::new();
    }
    let mut fragments = vec![scrubbed];
    if chars.len() <= PRIVATE_ECHO_CHUNK_CHARS {
        return fragments;
    }
    let mut start = 0usize;
    while start + PRIVATE_ECHO_CHUNK_CHARS <= chars.len() {
        fragments.push(
            chars[start..start + PRIVATE_ECHO_CHUNK_CHARS]
                .iter()
                .collect(),
        );
        start = start.saturating_add(PRIVATE_ECHO_CHUNK_STEP);
    }
    let tail_start = chars.len().saturating_sub(PRIVATE_ECHO_CHUNK_CHARS);
    fragments.push(chars[tail_start..].iter().collect());
    fragments.sort();
    fragments.dedup();
    fragments
}
