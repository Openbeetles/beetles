//! Selfhood scope helpers.
//! 主体/关系作用域辅助：把“板级主体”与“当前关系层”明确分开。

pub const BOARD_SUBJECT_SCOPE_ID: &str = "board.self";
pub const PRIVATE_GARDEN_SCOPE_ID: &str = BOARD_SUBJECT_SCOPE_ID;

pub fn board_subject_scope_id() -> &'static str {
    BOARD_SUBJECT_SCOPE_ID
}

pub fn private_garden_scope_id() -> &'static str {
    PRIVATE_GARDEN_SCOPE_ID
}

pub fn relationship_scope_id(channel: &str, chat_id: &str) -> String {
    let channel = encode_scope_component(channel);
    let chat = encode_scope_component(chat_id);
    format!("rel:{}:{}", channel, chat)
}

fn encode_scope_component(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "_".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for byte in trimmed.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            push_hex_escape(&mut out, byte);
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

fn push_hex_escape(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('_');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0f) as usize] as char);
}

#[cfg(test)]
mod tests {
    use super::{board_subject_scope_id, private_garden_scope_id, relationship_scope_id};

    #[test]
    fn board_subject_scope_id_is_stable() {
        assert_eq!(board_subject_scope_id(), "board.self");
    }

    #[test]
    fn relationship_scope_id_encodes_channel_and_chat() {
        assert_eq!(
            relationship_scope_id("qq/channel", "user:1"),
            "rel:qq_2fchannel:user_3a1"
        );
    }

    #[test]
    fn private_garden_scope_id_is_board_subject_scope() {
        assert_eq!(private_garden_scope_id(), board_subject_scope_id());
    }
}
