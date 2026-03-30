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

/// 按最多 max_chars 个字符分片，不拆开多字节字符。迭代器实现，不中间分配 Vec<char>。
pub fn chunk_str_by_char_count(s: &str, max_chars: usize) -> Vec<String> {
    chunk_str_by_char_count_iter(s, max_chars)
        .map(|x| x.to_string())
        .collect()
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
