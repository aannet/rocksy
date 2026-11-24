//! Minimal YAML frontmatter reader: only the top-level `tags` / `tag` keys.
//!
//! Supported values: plain or quoted string (split on commas and spaces),
//! flow list `[a, "b"]` (possibly spanning several lines) and block list
//! (`- a`). A leading `#` is optional but, as in YAML, an unquoted `#` starts
//! a comment: `tags: "#a"` is a tag, `tags: #a` is not.

use super::parse::whole_tag;

pub(crate) struct FmTag {
    pub line: usize,
    /// Byte offset of the value in its line (the `#` if there is one).
    pub col: usize,
    /// Byte range of the tag itself (without `#`).
    pub start: usize,
    pub end: usize,
}

pub(crate) struct Frontmatter {
    /// Index of the closing `---` line.
    pub close_line: usize,
    pub tags: Vec<FmTag>,
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Splits `line[a..b]` on commas and whitespace and records the valid tags.
fn emit_tokens(line_idx: usize, line: &str, a: usize, b: usize, out: &mut Vec<FmTag>) {
    let mut pos = a;
    for token in line[a..b].split(|c: char| c == ',' || c.is_whitespace()) {
        let start = pos;
        pos += token.len();
        // Skip the separator (one char, possibly multi-byte).
        if let Some(sep) = line[pos..b].chars().next() {
            pos += sep.len_utf8();
        }
        if token.is_empty() {
            continue;
        }
        let body = token.strip_prefix('#').unwrap_or(token);
        let body_start = start + (token.len() - body.len());
        if let Some((s, e)) = whole_tag(body) {
            out.push(FmTag { line: line_idx, col: start, start: body_start + s, end: body_start + e });
        }
    }
}

/// End of an unquoted scalar starting at `from`: before a ` #` comment,
/// trailing whitespace removed.
fn plain_end(line: &str, from: usize) -> usize {
    let b = line.as_bytes();
    let mut end = b.len();
    let mut i = from;
    while i + 1 < b.len() {
        if is_ws(b[i]) && b[i + 1] == b'#' {
            end = i;
            break;
        }
        i += 1;
    }
    while end > from && is_ws(b[end - 1]) {
        end -= 1;
    }
    end
}

/// A scalar (quoted or plain) starting at `from`.
fn scalar(line_idx: usize, line: &str, from: usize, out: &mut Vec<FmTag>) {
    let b = line.as_bytes();
    match b.get(from) {
        None | Some(b'#') => {}
        Some(&q) if q == b'"' || q == b'\'' => {
            let inner = from + 1;
            let close = line[inner..].find(q as char).map_or(line.len(), |p| inner + p);
            emit_tokens(line_idx, line, inner, close, out);
        }
        Some(_) => emit_tokens(line_idx, line, from, plain_end(line, from), out),
    }
}

/// Flow list starting at `lines[line_idx][from]` (just after `[`). Returns
/// the index of the line where the list ends.
fn flow_list(lines: &[&str], mut line_idx: usize, mut from: usize, close_line: usize, out: &mut Vec<FmTag>) -> usize {
    loop {
        let line = lines[line_idx];
        let b = line.as_bytes();
        let mut i = from;
        while i < b.len() {
            let c = b[i];
            if is_ws(c) || c == b',' {
                i += 1;
            } else if c == b']' {
                return line_idx;
            } else if c == b'#' {
                break; // comment up to the end of the line
            } else if c == b'"' || c == b'\'' {
                let inner = i + 1;
                let close = line[inner..].find(c as char).map_or(line.len(), |p| inner + p);
                emit_tokens(line_idx, line, inner, close, out);
                i = (close + 1).min(b.len());
            } else {
                let mut end = i;
                while end < b.len() && b[end] != b',' && b[end] != b']' {
                    if is_ws(b[end]) && b.get(end + 1) == Some(&b'#') {
                        break;
                    }
                    end += 1;
                }
                emit_tokens(line_idx, line, i, end, out);
                if end < b.len() && is_ws(b[end]) {
                    break; // comment
                }
                i = end;
            }
        }
        if line_idx + 1 >= close_line {
            return line_idx;
        }
        line_idx += 1;
        from = 0;
    }
}

/// Parses the frontmatter if the text starts with one (first line `---`,
/// closed by `---` or `...`).
pub(crate) fn parse_frontmatter(lines: &[&str]) -> Option<Frontmatter> {
    let first = lines.first()?;
    let first = first.strip_prefix('\u{feff}').unwrap_or(first);
    if first.trim_end() != "---" {
        return None;
    }
    let close_line = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| matches!(l.trim_end(), "---" | "..."))
        .map(|(i, _)| i)?;

    let mut tags = Vec::new();
    let mut j = 1;
    while j < close_line {
        let line = lines[j];
        let b = line.as_bytes();
        j += 1;
        // Only top-level keys.
        if b.is_empty() || is_ws(b[0]) || b[0] == b'-' || b[0] == b'#' {
            continue;
        }
        let Some(colon) = line.find(':') else { continue };
        if !matches!(b.get(colon + 1), None | Some(b' ') | Some(b'\t')) {
            continue;
        }
        let key = line[..colon].trim().trim_matches(|c| c == '"' || c == '\'');
        if !key.eq_ignore_ascii_case("tags") && !key.eq_ignore_ascii_case("tag") {
            continue;
        }
        let mut p = colon + 1;
        while p < b.len() && is_ws(b[p]) {
            p += 1;
        }
        match b.get(p) {
            // Nothing (or a comment) after the key: block list on the next lines.
            None | Some(b'#') => {
                while j < close_line {
                    let item_line = lines[j];
                    let t = item_line.trim_start();
                    let indent = item_line.len() - t.len();
                    if t.is_empty() || t.starts_with('#') || t == "-" {
                        j += 1;
                    } else if t.starts_with("- ") || t.starts_with("-\t") {
                        let mut from = indent + 2;
                        let ib = item_line.as_bytes();
                        while from < ib.len() && is_ws(ib[from]) {
                            from += 1;
                        }
                        scalar(j, item_line, from, &mut tags);
                        j += 1;
                    } else {
                        break;
                    }
                }
            }
            Some(b'[') => {
                j = flow_list(lines, j - 1, p + 1, close_line, &mut tags) + 1;
            }
            Some(_) => scalar(j - 1, line, p, &mut tags),
        }
    }
    Some(Frontmatter { close_line, tags })
}
