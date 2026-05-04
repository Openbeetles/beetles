use crate::channels::outbound_text::render_markdownish_to_plain_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QqMarkdownRender {
    Markdown { content: String, fallback: String },
    Plain { text: String },
}

pub(super) fn render_qq_markdown(source: &str, max_len: usize) -> QqMarkdownRender {
    let rendered = render_qq_markdown_subset(source);
    if rendered.trim().is_empty() {
        return QqMarkdownRender::Plain {
            text: String::new(),
        };
    }
    if rendered.len() <= max_len {
        let fallback = render_markdownish_to_plain_text(&rendered);
        return QqMarkdownRender::Markdown {
            content: rendered,
            fallback,
        };
    }

    let plain = render_qq_plain_text(source);
    QqMarkdownRender::Plain {
        text: if plain.trim().is_empty() {
            source.trim().to_string()
        } else {
            plain
        },
    }
}

pub(super) fn render_qq_plain_text(source: &str) -> String {
    render_markdownish_to_plain_text(&render_qq_markdown_subset(source))
}

fn render_qq_markdown_subset(source: &str) -> String {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    let mut in_code = false;
    let mut code_language = String::new();

    while index < lines.len() {
        let line = lines[index].trim_end();
        let trimmed = line.trim_start();

        if is_code_fence(trimmed) {
            if in_code {
                in_code = false;
                code_language.clear();
            } else {
                in_code = true;
                code_language = trimmed
                    .trim_start_matches("```")
                    .trim_start_matches("~~~")
                    .trim()
                    .to_string();
                if !code_language.is_empty() {
                    out.push(format!("> Code: {code_language}"));
                }
            }
            index += 1;
            continue;
        }

        if in_code {
            out.push(format!("> {}", line.trim_end()));
            index += 1;
            continue;
        }

        if is_table_header_at(&lines, index) {
            let (next_index, rendered_table) = render_table_block(&lines, index);
            out.extend(rendered_table);
            index = next_index;
            continue;
        }

        if let Some(rendered_table) = render_inline_table_line(line) {
            out.extend(rendered_table);
        } else {
            out.push(line.to_string());
        }
        index += 1;
    }

    collapse_blank_lines(out).join("\n").trim().to_string()
}

fn is_code_fence(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_table_header_at(lines: &[&str], index: usize) -> bool {
    if index + 1 >= lines.len() {
        return false;
    }
    is_table_row(lines[index]) && is_table_separator(lines[index + 1])
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.matches('|').count() >= 2 && parse_table_row(trimmed).len() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let cells = parse_table_row(line);
    cells.len() >= 2 && cells.iter().all(|cell| is_table_separator_cell(cell))
}

fn is_table_separator_cell(cell: &str) -> bool {
    let trimmed = cell.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|ch| matches!(ch, '-' | ':' | ' ' | '\t'))
}

fn render_table_block(lines: &[&str], start: usize) -> (usize, Vec<String>) {
    let headers = parse_table_row(lines[start]);
    let mut rows = Vec::new();
    let mut index = start + 2;
    while index < lines.len() && is_table_row(lines[index]) {
        rows.push(parse_table_row(lines[index]));
        index += 1;
    }

    let mut out = Vec::new();
    for row in rows {
        let rendered = render_table_row(&headers, &row);
        out.extend(rendered);
    }
    (index, out)
}

fn render_inline_table_line(line: &str) -> Option<Vec<String>> {
    if line.matches('|').count() < 4 {
        return None;
    }
    let cells = parse_table_row(line);
    let (separator_start, separator_len) = find_separator_run(&cells)?;
    if separator_len < 2 || separator_start < separator_len {
        return None;
    }
    let header_start = separator_start - separator_len;
    let headers = &cells[header_start..separator_start];
    let values = &cells[separator_start + separator_len..];
    if values.len() < separator_len {
        return None;
    }

    let mut out = Vec::new();
    let prefix = cells[..header_start]
        .iter()
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    if !prefix.is_empty() {
        out.push(prefix);
    }

    let mut index = 0usize;
    while index + separator_len <= values.len() {
        out.extend(render_table_row(
            headers,
            &values[index..index + separator_len],
        ));
        index += separator_len;
    }

    let tail = values[index..]
        .iter()
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !tail.is_empty() {
        out.push(tail);
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn find_separator_run(cells: &[String]) -> Option<(usize, usize)> {
    let mut index = 0usize;
    while index < cells.len() {
        if !is_table_separator_cell(&cells[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < cells.len() && is_table_separator_cell(&cells[index]) {
            index += 1;
        }
        let len = index - start;
        if len >= 2 {
            return Some((start, len));
        }
    }
    None
}

fn render_table_row(headers: &[String], row: &[String]) -> Vec<String> {
    if headers.len() == 2 {
        let label = table_cell(row, 0);
        let value = table_cell(row, 1);
        return vec![format!("- {label}: {value}")];
    }

    let mut out = Vec::new();
    let label = table_cell(row, 0);
    if !label.is_empty() {
        out.push(format!("**{label}**"));
    }
    let max = headers.len().max(row.len());
    for column in 1..max {
        let key = table_cell(headers, column);
        let value = table_cell(row, column);
        if key.is_empty() && value.is_empty() {
            continue;
        }
        if key.is_empty() {
            out.push(format!("- {value}"));
        } else if value.is_empty() {
            out.push(format!("- {key}: -"));
        } else {
            out.push(format!("- {key}: {value}"));
        }
    }
    out
}

fn table_cell(cells: &[String], index: usize) -> String {
    cells
        .get(index)
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn parse_table_row(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = line
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.first().is_some_and(|cell| cell.is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|cell| cell.is_empty()) {
        cells.pop();
    }
    cells
}

fn collapse_blank_lines(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut previous_blank = false;
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        previous_blank = blank;
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_rendered_as_qq_supported_bullets() {
        let rendered = render_qq_markdown_subset(
            "# 状态\n\n| 项目 | 值 |\n| --- | --- |\n| CPU | 正常 |\n| 内存 | 256KB |",
        );

        assert!(rendered.contains("# 状态"));
        assert!(rendered.contains("- CPU: 正常"));
        assert!(rendered.contains("- 内存: 256KB"));
        assert!(!rendered.contains("| 项目 | 值 |"));
    }

    #[test]
    fn collapsed_inline_table_is_rendered_as_qq_supported_bullets() {
        let rendered = render_qq_markdown_subset(
            "验收测试 D4：提醒已设置 ✅ | 项目 | 值 |---|---| 提醒 ID | rem_1 | 触发时间 | 11:34:53 UTC | 内容 | P4 QQ 定时提醒链路通过 | 通道 | QQ 当前会话",
        );

        assert!(rendered.contains("验收测试 D4：提醒已设置 ✅"));
        assert!(rendered.contains("- 提醒 ID: rem_1"));
        assert!(rendered.contains("- 触发时间: 11:34:53 UTC"));
        assert!(rendered.contains("- 内容: P4 QQ 定时提醒链路通过"));
        assert!(rendered.contains("- 通道: QQ 当前会话"));
        assert!(!rendered.contains("|---|"));
        assert!(!rendered.contains("| 项目 | 值 |"));
    }

    #[test]
    fn collapsed_multicolumn_table_is_rendered_without_pipe_table_syntax() {
        let rendered = render_qq_markdown_subset(
            "资源报告 | 指标 | S3 | P4 | --- | --- | --- | internal_free | 348KB | 475KB | tls | Healthy | Healthy |",
        );

        assert!(rendered.contains("资源报告"));
        assert!(rendered.contains("**internal_free**"));
        assert!(rendered.contains("- S3: 348KB"));
        assert!(rendered.contains("- P4: 475KB"));
        assert!(rendered.contains("**tls**"));
        assert!(rendered.contains("- S3: Healthy"));
        assert!(rendered.contains("- P4: Healthy"));
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn natural_pipe_text_without_separator_is_preserved() {
        let source = "日志说明：A | B 只是候选项，不是表格。";
        let rendered = render_qq_markdown_subset(source);

        assert_eq!(rendered, source);
    }

    #[test]
    fn plain_text_projection_uses_qq_table_projection() {
        let rendered = render_qq_plain_text(
            "验收测试 D4：提醒已设置 ✅ | 项目 | 值 |---|---| 提醒 ID | rem_1",
        );

        assert!(rendered.contains("验收测试 D4：提醒已设置 ✅"));
        assert!(rendered.contains("• 提醒 ID: rem_1"));
        assert!(!rendered.contains("|---|"));
    }

    #[test]
    fn code_fence_is_rendered_as_blockquote_lines() {
        let rendered = render_qq_markdown_subset("```rust\nlet x = 1;\n```");

        assert!(rendered.contains("> Code: rust"));
        assert!(rendered.contains("> let x = 1;"));
        assert!(!rendered.contains("```"));
    }

    #[test]
    fn oversized_markdown_downgrades_to_plain_text() {
        let source = format!("# 标题\n{}", "甲".repeat(8));
        let rendered = render_qq_markdown(&source, 4);

        assert!(matches!(rendered, QqMarkdownRender::Plain { .. }));
    }
}
