//! 通道出站分片：按字符数或 UTF-8 字节数分片，供各通道 flush 使用。
//! Chunking helpers for channel outbound; shared to avoid code duplication.

/// 按最多 max_chars 个字符迭代分片，不拆开多字节字符。
pub fn chunk_str_by_char_count_iter<'a>(
    s: &'a str,
    max_chars: usize,
) -> impl Iterator<Item = &'a str> + 'a {
    struct CharChunkIter<'a> {
        s: &'a str,
        max_chars: usize,
        start: usize,
    }
    impl<'a> Iterator for CharChunkIter<'a> {
        type Item = &'a str;
        fn next(&mut self) -> Option<Self::Item> {
            if self.max_chars == 0 || self.start >= self.s.len() {
                return None;
            }
            let rest = &self.s[self.start..];
            let mut end_rel = 0usize;
            for (i, ch) in rest.char_indices().take(self.max_chars) {
                end_rel = i + ch.len_utf8();
            }
            if end_rel == 0 {
                return None;
            }
            let end = self.start + end_rel;
            let out = &self.s[self.start..end];
            self.start = end;
            Some(out)
        }
    }
    CharChunkIter {
        s,
        max_chars,
        start: 0,
    }
}

/// 优先按段落/换行边界切分，超长段落再回退到字符分片。
pub fn chunk_text_by_char_count(s: &str, max_chars: usize) -> Vec<String> {
    chunk_text_by_char_count_with_separator(s, max_chars, "\n\n")
}

/// 按最多 max_bytes 个 UTF-8 字节迭代分片，不拆开多字节字符。
pub fn chunk_str_by_utf8_bytes_iter<'a>(
    s: &'a str,
    max_bytes: usize,
) -> impl Iterator<Item = &'a str> + 'a {
    struct Utf8ChunkIter<'a> {
        s: &'a str,
        max_bytes: usize,
        start: usize,
    }
    impl<'a> Iterator for Utf8ChunkIter<'a> {
        type Item = &'a str;
        fn next(&mut self) -> Option<Self::Item> {
            if self.max_bytes == 0 || self.start >= self.s.len() {
                return None;
            }
            let rest = &self.s[self.start..];
            let mut end_rel = 0usize;
            for (i, ch) in rest.char_indices() {
                let next = i + ch.len_utf8();
                if next > self.max_bytes {
                    break;
                }
                end_rel = next;
            }
            if end_rel == 0 {
                // max_bytes 小于首字符字节数时，至少推进一个字符，避免死循环。
                if let Some((_, ch)) = rest.char_indices().next() {
                    end_rel = ch.len_utf8();
                } else {
                    return None;
                }
            }
            let end = self.start + end_rel;
            let out = &self.s[self.start..end];
            self.start = end;
            Some(out)
        }
    }
    Utf8ChunkIter {
        s,
        max_bytes,
        start: 0,
    }
}

/// 优先按段落/换行边界切分，超长段落再回退到 UTF-8 字节分片。
pub fn chunk_text_by_utf8_bytes(s: &str, max_bytes: usize) -> Vec<String> {
    chunk_text_by_utf8_bytes_with_separator(s, max_bytes, "\n\n")
}

fn chunk_text_by_char_count_with_separator(
    s: &str,
    max_chars: usize,
    separator: &str,
) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in trimmed.split(separator) {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        append_char_block(&mut chunks, &mut current, paragraph, max_chars, separator);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn append_char_block(
    chunks: &mut Vec<String>,
    current: &mut String,
    block: &str,
    max_chars: usize,
    separator: &str,
) {
    let candidate_len = if current.is_empty() {
        block.chars().count()
    } else {
        current.chars().count() + separator.chars().count() + block.chars().count()
    };
    if candidate_len <= max_chars {
        if !current.is_empty() {
            current.push_str(separator);
        }
        current.push_str(block);
        return;
    }
    if !current.is_empty() {
        chunks.push(std::mem::take(current));
    }
    if block.chars().count() <= max_chars {
        current.push_str(block);
        return;
    }
    append_long_char_block(chunks, block, max_chars);
}

fn append_long_char_block(chunks: &mut Vec<String>, block: &str, max_chars: usize) {
    let mut current = String::new();
    for line in block.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let candidate_len = if current.is_empty() {
            line.chars().count()
        } else {
            current.chars().count() + 1 + line.chars().count()
        };
        if candidate_len <= max_chars {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if line.chars().count() <= max_chars {
            current.push_str(line);
            continue;
        }
        for piece in chunk_str_by_char_count_iter(line, max_chars) {
            chunks.push(piece.to_string());
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
}

fn chunk_text_by_utf8_bytes_with_separator(
    s: &str,
    max_bytes: usize,
    separator: &str,
) -> Vec<String> {
    if max_bytes == 0 {
        return Vec::new();
    }
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in trimmed.split(separator) {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        append_utf8_block(&mut chunks, &mut current, paragraph, max_bytes, separator);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn append_utf8_block(
    chunks: &mut Vec<String>,
    current: &mut String,
    block: &str,
    max_bytes: usize,
    separator: &str,
) {
    let candidate_len = if current.is_empty() {
        block.len()
    } else {
        current.len() + separator.len() + block.len()
    };
    if candidate_len <= max_bytes {
        if !current.is_empty() {
            current.push_str(separator);
        }
        current.push_str(block);
        return;
    }
    if !current.is_empty() {
        chunks.push(std::mem::take(current));
    }
    if block.len() <= max_bytes {
        current.push_str(block);
        return;
    }
    append_long_utf8_block(chunks, block, max_bytes);
}

fn append_long_utf8_block(chunks: &mut Vec<String>, block: &str, max_bytes: usize) {
    let mut current = String::new();
    for line in block.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let candidate_len = if current.is_empty() {
            line.len()
        } else {
            current.len() + 1 + line.len()
        };
        if candidate_len <= max_bytes {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if line.len() <= max_bytes {
            current.push_str(line);
            continue;
        }
        for piece in chunk_str_by_utf8_bytes_iter(line, max_bytes) {
            chunks.push(piece.to_string());
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::{chunk_text_by_char_count, chunk_text_by_utf8_bytes};

    #[test]
    fn paragraph_chunking_prefers_block_boundaries() {
        let input = "第一段内容\n\n第二段内容\n\n第三段内容";
        let chunks = chunk_text_by_char_count(input, 8);
        assert_eq!(chunks, vec!["第一段内容", "第二段内容", "第三段内容"]);
    }

    #[test]
    fn paragraph_chunking_falls_back_to_line_and_char_split() {
        let input = "第一行很长很长很长\n第二行很长很长很长";
        let chunks = chunk_text_by_char_count(input, 6);
        assert!(chunks.len() >= 4);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 6));
    }

    #[test]
    fn utf8_chunking_preserves_multibyte_boundaries() {
        let input = "甲壳虫\n\n你好世界你好世界";
        let chunks = chunk_text_by_utf8_bytes(input, 12);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 12));
    }
}
