//! Line-based tag and block analyser.
//!
//! A dedicated single-pass analyser rather than a full Markdown parser: only
//! the block boundaries needed for fragments are recognised (paragraph, list
//! item with its children, heading, table row, quote/callout, code), and a
//! line is scanned for tags only if it contains a `#`.
//!
//! Tag rules (see CONTRACT.md): `#` at the start of a line or after
//! whitespace, followed by letters, digits, `_`, `-`, `/` (any Unicode
//! letter or digit, plus emoji), with at least one non-digit character. Not
//! recognised inside inline code, code blocks, wikilinks, Markdown links
//! (text, destination and title), HTML tags and autolinks. URLs need no
//! special case: their `#` is never preceded by whitespace.

use super::frontmatter::parse_frontmatter;
use super::links::{parse_markdown_link, parse_wikilink, LinkTarget};
use super::TagSource;
use crate::excerpt::line_excerpt;
use memchr::{memchr, memchr2, memchr2_iter, memmem};

pub(crate) const EXCERPT_MAX: usize = 240;

pub(crate) struct RawOcc<'a> {
    pub tag: &'a str,
    pub source: TagSource,
    pub start_line: u32,
    pub end_line: u32,
    pub tag_line: u32,
    pub tag_col: u32,
    /// Index into `Parsed::excerpts` (shared by the tags of one block).
    pub excerpt: u32,
}

/// A link or embed found in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawLink {
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    /// `![[...]]` or `![alt](...)`.
    pub embed: bool,
    /// Markdown syntax (resolved relative to the note) rather than wikilink.
    pub markdown: bool,
    pub line: u32,
    /// UTF-16 column of the first character of the link (`!` for an embed).
    pub col: u32,
    /// The line of the link, as a one-line excerpt.
    pub excerpt: String,
    /// The link inside `excerpt` (UTF-16 units, end excluded).
    pub range: (u32, u32),
}

pub(crate) struct Parsed<'a> {
    pub occs: Vec<RawOcc<'a>>,
    pub excerpts: Vec<String>,
    /// Links in document order (none inside the frontmatter).
    pub links: Vec<RawLink>,
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

/// Splits on `\n`, `\r\n` and lone `\r` (CodeMirror's line model). A final
/// line terminator does not produce a trailing empty line.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::with_capacity(bytes.len() / 48 + 1);
    let mut start = 0;
    for pos in memchr2_iter(b'\n', b'\r', bytes) {
        if pos < start {
            continue; // the `\n` of a `\r\n` already consumed
        }
        lines.push(&text[start..pos]);
        start = if bytes[pos] == b'\r' && bytes.get(pos + 1) == Some(&b'\n') { pos + 2 } else { pos + 1 };
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// UTF-16 length of `line[..byte]`.
pub(crate) fn col_utf16(line: &str, byte: usize) -> u32 {
    let prefix = &line[..byte];
    if prefix.is_ascii() {
        byte as u32
    } else {
        prefix.chars().map(char::len_utf16).sum::<usize>() as u32
    }
}

// ---------------------------------------------------------------------------
// Tag syntax
// ---------------------------------------------------------------------------

pub(crate) fn is_tag_char(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/';
    }
    if c.is_alphanumeric() {
        return true;
    }
    match c as u32 {
        // Combining marks (decomposed accents), zero-width joiner and
        // variation selectors (emoji sequences).
        0x0300..=0x036F | 0x200D | 0xFE00..=0xFE0F => true,
        // Punctuation and spaces that end a tag.
        0x2000..=0x206F | 0x2E00..=0x2E7F | 0x3000..=0x303F | 0xFE10..=0xFFFF => false,
        // Arrows, symbols, pictographs, emoji.
        cp => cp >= 0x2190,
    }
}

/// Extent of the tag whose first character is at `from` (just after `#`):
/// trimmed of leading/trailing `/`, cut at an empty level (`//`). `None` if
/// nothing valid remains or the tag is purely numeric.
pub(crate) fn tag_at(line: &str, from: usize) -> Option<(usize, usize)> {
    let mut end = from;
    for (off, c) in line[from..].char_indices() {
        if !is_tag_char(c) {
            break;
        }
        end = from + off + c.len_utf8();
    }
    let bytes = line.as_bytes();
    let mut start = from;
    while start < end && bytes[start] == b'/' {
        start += 1;
    }
    if let Some(pos) = line[start..end].find("//") {
        end = start + pos;
    }
    while end > start && bytes[end - 1] == b'/' {
        end -= 1;
    }
    if start == end || bytes[start..end].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some((start, end))
}

/// A whole string as a tag (frontmatter value, without `#`).
pub(crate) fn whole_tag(s: &str) -> Option<(usize, usize)> {
    match tag_at(s, 0) {
        // Everything but surrounding slashes must belong to the tag.
        Some((start, end)) if s[end..].bytes().all(|b| b == b'/') => Some((start, end)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Inline scanner
// ---------------------------------------------------------------------------

pub(crate) struct Hit {
    /// Byte offset of `#`.
    pub hash: usize,
    pub start: usize,
    pub end: usize,
}

fn closing_backticks(b: &[u8], mut i: usize, n: usize) -> Option<usize> {
    while i < b.len() {
        if b[i] == b'`' {
            let run_start = i;
            while i < b.len() && b[i] == b'`' {
                i += 1;
            }
            if i - run_start == n {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Bracket pairs of a line: (opener, index just past its closer), sorted by
/// opener. Unclosed openers are absent.
type Pairs = Vec<(usize, usize)>;

/// What the inline scanner remembers about a line so that every lookup
/// ahead (closing bracket, `]]`, `>`, closing backticks) costs one pass over
/// the line in total, however many openers the line holds.
///
/// All positions asked for only grow, and each one starts a token (never
/// the byte after an unescaped backslash), which is why one pass from the
/// first opener gives the same answers as a scan restarted at each opener.
#[derive(Default)]
struct LineMemo {
    /// `[`/`]` and `(`/`)` pairs from the first bracket needing them.
    pairs: Option<(Pairs, Pairs)>,
    /// A `]]` search was made; `wiki_close` is its result.
    wiki_searched: bool,
    wiki_close: Option<usize>,
    /// No `>` further on the line.
    no_angle_close: bool,
    /// Lengths of backtick runs with no closing run further on the line.
    unclosed_backticks: Vec<usize>,
}

/// Matches every `[` with its `]` and every `(` with its `)` in `b[from..]`,
/// each kind on its own, honouring nesting and backslash escapes.
fn bracket_pairs(b: &[u8], from: usize) -> (Pairs, Pairs) {
    fn open(pairs: &mut Pairs, stack: &mut Vec<usize>, at: usize) {
        stack.push(pairs.len());
        pairs.push((at, usize::MAX));
    }
    fn close(pairs: &mut Pairs, stack: &mut Vec<usize>, at: usize) {
        if let Some(k) = stack.pop() {
            pairs[k].1 = at + 1;
        }
    }
    let (mut square, mut round) = (Pairs::new(), Pairs::new());
    let (mut square_open, mut round_open) = (Vec::new(), Vec::new());
    let mut j = from;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            b'[' => open(&mut square, &mut square_open, j),
            b']' => close(&mut square, &mut square_open, j),
            b'(' => open(&mut round, &mut round_open, j),
            b')' => close(&mut round, &mut round_open, j),
            _ => {}
        }
        j += 1;
    }
    square.retain(|p| p.1 != usize::MAX);
    round.retain(|p| p.1 != usize::MAX);
    (square, round)
}

impl LineMemo {
    /// Index just past the bracket matching the opener at `open` (`[` or
    /// `(`), honouring nesting and backslash escapes.
    fn matching(&mut self, b: &[u8], open: usize) -> Option<usize> {
        let (square, round) = self.pairs.get_or_insert_with(|| bracket_pairs(b, open));
        let pairs = if b[open] == b'[' { square } else { round };
        pairs.binary_search_by_key(&open, |p| p.0).ok().map(|k| pairs[k].1)
    }

    /// Position of the first `]]` at or after `from`.
    fn wiki_close(&mut self, b: &[u8], from: usize) -> Option<usize> {
        if self.wiki_searched && self.wiki_close.is_none_or(|p| p >= from) {
            return self.wiki_close;
        }
        self.wiki_searched = true;
        self.wiki_close = memmem::find(&b[from..], b"]]").map(|p| from + p);
        self.wiki_close
    }

    /// Index after the run of exactly `n` backticks closing a code span
    /// whose content starts at `from`.
    fn closing_backticks(&mut self, b: &[u8], from: usize, n: usize) -> Option<usize> {
        if self.unclosed_backticks.contains(&n) {
            return None;
        }
        let end = closing_backticks(b, from, n);
        if end.is_none() {
            self.unclosed_backticks.push(n);
        }
        end
    }

    /// HTML tag, comment or autolink starting at `i` (`<a href="#x">`,
    /// `<https://x/#y>`): returns the index after the closing `>`.
    fn skip_angle(&mut self, b: &[u8], i: usize) -> Option<usize> {
        let next = *b.get(i + 1)?;
        if self.no_angle_close || !(next.is_ascii_alphabetic() || next == b'/' || next == b'!') {
            return None;
        }
        let end = memchr(b'>', &b[i + 1..]).map(|p| i + 1 + p + 1);
        self.no_angle_close = end.is_none();
        end
    }
}

/// A link found on a line: byte span and what it points to.
pub(crate) struct LinkHit {
    pub start: usize,
    pub end: usize,
    pub embed: bool,
    pub markdown: bool,
    pub target: LinkTarget,
}

/// If a wikilink (`[[...]]`), an inline link (`[text](dest "title")`) or a
/// reference link (`[text][ref]`) starts at `i`, returns the index after it
/// and, when it is a link to a file, its target (with `true` for Markdown
/// syntax).
fn scan_link(line: &str, i: usize, memo: &mut LineMemo) -> Option<(usize, Option<(bool, LinkTarget)>)> {
    let b = line.as_bytes();
    if b.get(i + 1) == Some(&b'[') {
        let j = memo.wiki_close(b, i + 2)?;
        return Some((j + 2, parse_wikilink(&line[i + 2..j]).map(|t| (false, t))));
    }
    let after_text = memo.matching(b, i)?;
    match b.get(after_text) {
        Some(b'(') => {
            let end = memo.matching(b, after_text)?;
            let target = parse_markdown_link(&line[i + 1..after_text - 1], &line[after_text + 1..end - 1]);
            Some((end, target.map(|t| (true, t))))
        }
        Some(b'[') => Some((memo.matching(b, after_text)?, None)),
        _ => None,
    }
}

/// Appends the tags of `line[from..]` to `out` and its links to `links`.
/// `from` is treated as a position where a tag may start.
pub(crate) fn scan_inline(line: &str, from: usize, out: &mut Vec<Hit>, links: &mut Vec<LinkHit>) {
    let b = line.as_bytes();
    if from >= b.len() || memchr2(b'#', b'[', &b[from..]).is_none() {
        return;
    }
    let mut memo = LineMemo::default();
    let mut i = from;
    let mut prev_ws = true;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' => {
                prev_ws = true;
                i += 1;
            }
            b'#' => {
                if prev_ws {
                    if let Some((start, end)) = tag_at(line, i + 1) {
                        out.push(Hit { hash: i, start, end });
                        i = end;
                        prev_ws = false;
                        continue;
                    }
                }
                prev_ws = false;
                i += 1;
            }
            b'`' => {
                let mut n = 1;
                while i + n < b.len() && b[i + n] == b'`' {
                    n += 1;
                }
                i = memo.closing_backticks(b, i + n, n).unwrap_or(i + n);
                prev_ws = false;
            }
            b'\\' => {
                i += 1;
                if let Some(ch) = line[i..].chars().next() {
                    i += ch.len_utf8();
                }
                prev_ws = false;
            }
            b'[' => {
                let embed = i > from && b[i - 1] == b'!';
                let start = if embed { i - 1 } else { i };
                i = match scan_link(line, i, &mut memo) {
                    Some((end, found)) => {
                        if let Some((markdown, target)) = found {
                            links.push(LinkHit { start, end, embed, markdown, target });
                        }
                        end
                    }
                    None => i + 1,
                };
                prev_ws = false;
            }
            b'<' => {
                i = memo.skip_angle(b, i).unwrap_or(i + 1);
                prev_ws = false;
            }
            _ if c < 0x80 => {
                prev_ws = false;
                i += 1;
            }
            _ => {
                let ch = line[i..].chars().next().unwrap_or(' ');
                prev_ws = ch.is_whitespace();
                i += ch.len_utf8();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Block-level line classification
// ---------------------------------------------------------------------------

/// Indentation of a line: (columns with tabs expanded to multiples of 4,
/// bytes).
fn indent_of(line: &str) -> (usize, usize) {
    let mut cols = 0;
    for (i, b) in line.bytes().enumerate() {
        match b {
            b' ' => cols += 1,
            b'\t' => cols = (cols / 4 + 1) * 4,
            _ => return (cols, i),
        }
    }
    (cols, line.len())
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| b == b' ' || b == b'\t')
}

/// Thematic break: three or more `-`, `*` or `_`, optionally spaced.
fn is_hr(rest: &str) -> bool {
    let b = rest.as_bytes();
    let first = b[0];
    if first != b'-' && first != b'*' && first != b'_' {
        return false;
    }
    let mut count = 0;
    for &c in b {
        if c == first {
            count += 1;
        } else if c != b' ' && c != b'\t' {
            return false;
        }
    }
    count >= 3
}

/// ATX heading: number of `#` (1 to 6) when followed by a space or the end.
fn heading_len(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let n = b.iter().take_while(|&&c| c == b'#').count();
    if (1..=6).contains(&n) && matches!(b.get(n), None | Some(b' ') | Some(b'\t')) {
        Some(n)
    } else {
        None
    }
}

type Fence = (u8, usize);

fn fence_open(rest: &str) -> Option<Fence> {
    let b = rest.as_bytes();
    let ch = *b.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let n = b.iter().take_while(|&&c| c == ch).count();
    if n < 3 || (ch == b'`' && b[n..].contains(&b'`')) {
        return None;
    }
    Some((ch, n))
}

fn fence_close(rest: &str, (ch, len): Fence) -> bool {
    let b = rest.as_bytes();
    let n = b.iter().take_while(|&&c| c == ch).count();
    n >= len && is_blank(&rest[n..])
}

/// List marker (`-`, `*`, `+`, `1.`, `1)`) followed by a space or the end of
/// the line: length of the marker.
fn list_marker(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let after = |n: usize| matches!(b.get(n), None | Some(b' ') | Some(b'\t'));
    match b[0] {
        b'-' | b'*' | b'+' if after(1) => Some(1),
        b'0'..=b'9' => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits <= 9 && matches!(b.get(digits), Some(b'.') | Some(b')')) && after(digits + 1) {
                Some(digits + 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Offset of the content of a quote line, after its `>` markers.
fn quote_content(rest: &str) -> usize {
    let b = rest.as_bytes();
    let mut i = 0;
    loop {
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') && j - i < 3 {
            j += 1;
        }
        if b.get(j) == Some(&b'>') {
            i = j + 1;
        } else {
            break;
        }
    }
    if matches!(b.get(i), Some(b' ') | Some(b'\t')) {
        i += 1;
    }
    i
}

/// Delimiter row of a table (`| --- | :-: |`).
fn is_table_delim(line: &str) -> bool {
    let t = line.trim_matches(|c| c == ' ' || c == '\t');
    !t.is_empty()
        && t.contains('-')
        && t.contains('|')
        && t.bytes().all(|c| matches!(c, b'|' | b'-' | b':' | b' ' | b'\t'))
}

// ---------------------------------------------------------------------------
// Excerpts
// ---------------------------------------------------------------------------

/// Collapses `lines[first..=last]` (starting at `first_byte` of the first
/// line) into one line of at most `EXCERPT_MAX` characters.
fn build_excerpt(lines: &[&str], first: usize, first_byte: usize, last: usize, cut_before: bool) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    if cut_before {
        out.push('…');
        count = 1;
    }
    let mut started = false;
    let mut pending_space = false;
    'lines: for (i, line) in lines[first..=last].iter().enumerate() {
        let s = if i == 0 { &line[first_byte..] } else { line };
        pending_space = pending_space || i > 0;
        for c in s.chars() {
            if c.is_whitespace() || c == '\u{feff}' {
                pending_space = true;
                continue;
            }
            let need = 1 + usize::from(pending_space && started);
            if count + need > EXCERPT_MAX {
                // Truncated: make room for the ellipsis.
                if count + 1 > EXCERPT_MAX {
                    out.pop();
                }
                let trimmed = out.trim_end().len();
                out.truncate(trimmed);
                out.push('…');
                break 'lines;
            }
            if pending_space && started {
                out.push(' ');
            }
            pending_space = false;
            started = true;
            out.push(c);
            count += need;
        }
    }
    out
}

/// More than `limit` characters between the start of the block and the tag?
///
/// Counts at most `limit + 1` characters per line and stops at the first
/// line past the limit, so the cost per tag does not depend on the length
/// of the block or of its lines.
fn tag_is_far(lines: &[&str], start: usize, tag_line: usize, hash: usize, limit: usize) -> bool {
    let mut chars = 0;
    for line in &lines[start..tag_line] {
        chars += line.chars().take(limit + 1).count() + 1;
        if chars > limit {
            return true;
        }
    }
    chars + lines[tag_line][..hash].chars().take(limit + 1).count() > limit
}

/// Where an excerpt centred on a distant tag starts inside the tag's line.
fn window_start(line: &str, hash: usize) -> usize {
    const BACK: usize = 60;
    if line[..hash].chars().take(BACK + 21).count() <= BACK + 20 {
        return 0;
    }
    let mut start = hash;
    for _ in 0..BACK {
        start -= 1;
        while !line.is_char_boundary(start) {
            start -= 1;
        }
    }
    // Prefer starting on a word boundary.
    // (The blank may be wider than one byte: a no-break space.)
    match line[start..hash].char_indices().find(|(_, c)| c.is_whitespace()) {
        Some((ws, c)) if start + ws + c.len_utf8() < hash => start + ws + c.len_utf8(),
        _ => start,
    }
}

/// UTF-16 columns of positions asked in document order: each call counts
/// from the previous position on the same line rather than from the start
/// of the line.
struct ColCursor {
    line: u32,
    byte: usize,
    col: u32,
}

impl ColCursor {
    fn new() -> Self {
        Self { line: u32::MAX, byte: 0, col: 0 }
    }

    fn col(&mut self, line_no: u32, line: &str, byte: usize) -> u32 {
        if line_no != self.line || byte < self.byte {
            *self = Self { line: line_no, byte: 0, col: 0 };
        }
        self.col += col_utf16(&line[self.byte..], byte - self.byte);
        self.byte = byte;
        self.col
    }
}

/// Start of a suffix of `line` whose [`line_excerpt`] for a highlight at
/// byte `at` is the one of the whole line: far enough before `at` for the
/// excerpt to be cut inside it, and on a visible character so that nothing
/// is trimmed. 0 when the line must be given whole.
fn excerpt_window(line: &str, at: usize) -> usize {
    // More than the distance at which `line_excerpt` cuts the beginning.
    const BACK: usize = EXCERPT_MAX + 60;
    for (n, (pos, c)) in line[..at].char_indices().rev().enumerate() {
        if n + 1 >= BACK && !c.is_whitespace() && c != '\u{feff}' {
            return pos;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Block pass
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Ctx {
    None,
    Para(u32),
    Quote { block: u32, fence: Option<Fence> },
    Table,
    Code,
}

struct Item {
    indent: usize,
    block: u32,
}

struct Pending {
    line: u32,
    hit: Hit,
    block: u32,
}

struct Blocks {
    spans: Vec<(u32, u32)>,
    pending: Vec<Pending>,
    hits: Vec<Hit>,
    /// (line, link) in document order.
    links: Vec<(u32, LinkHit)>,
    link_hits: Vec<LinkHit>,
}

impl Blocks {
    fn open(&mut self, line: usize) -> u32 {
        self.spans.push((line as u32, line as u32));
        (self.spans.len() - 1) as u32
    }
    fn extend(&mut self, block: u32, line: usize) {
        self.spans[block as usize].1 = line as u32;
    }
    fn scan(&mut self, full: &str, from: usize, line: usize, block: u32) {
        scan_inline(full, from, &mut self.hits, &mut self.link_hits);
        for link in self.link_hits.drain(..) {
            self.links.push((line as u32, link));
        }
        for hit in self.hits.drain(..) {
            self.pending.push(Pending { line: line as u32, hit, block });
        }
    }
}

fn pop_items(items: &mut Vec<Item>, blocks: &mut Blocks, min_indent: usize, last_content: usize) {
    while let Some(top) = items.last() {
        if top.indent < min_indent {
            break;
        }
        blocks.extend(top.block, last_content);
        items.pop();
    }
}

fn block_pass(lines: &[&str], body_start: usize, blocks: &mut Blocks) {
    let n = lines.len();
    let mut items: Vec<Item> = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut ctx = Ctx::None;
    let mut prev_blank = true;
    let mut last_content = body_start;

    for i in body_start..n {
        let full = lines[i];
        let base = if i == 0 && full.starts_with('\u{feff}') { 3 } else { 0 };
        let line = &full[base..];

        // Inside a fenced code block (top level or within a list item).
        if let Some(f) = fence {
            if fence_close(line.trim_start_matches([' ', '\t']), f) {
                fence = None;
            }
            last_content = i;
            prev_blank = false;
            continue;
        }

        let (indent, indent_bytes) = indent_of(line);
        let rest = &line[indent_bytes..];
        if rest.is_empty() {
            if !matches!(ctx, Ctx::Code) {
                ctx = Ctx::None;
            }
            prev_blank = true;
            continue;
        }
        let off = base + indent_bytes;
        let hr = is_hr(rest);
        let marker = if hr { None } else { list_marker(rest) };

        // ---- Inside a list ------------------------------------------------
        if !items.is_empty() {
            let mut handled = false;
            if let Some(m) = marker {
                pop_items(&mut items, blocks, indent, last_content);
                let block = blocks.open(i);
                items.push(Item { indent, block });
                let content = rest[m..].trim_start_matches([' ', '\t']);
                match fence_open(content) {
                    Some(f) => fence = Some(f),
                    None => blocks.scan(full, off, i, block),
                }
                handled = true;
            } else {
                let opens_fence = fence_open(rest);
                let is_quote = rest.starts_with('>');
                let starts_block = hr || is_quote || opens_fence.is_some() || heading_len(rest).is_some();
                // A plain line right after item text continues that item
                // whatever its indentation (lazy continuation).
                let lazy = !prev_blank && !starts_block;
                if !lazy {
                    pop_items(&mut items, blocks, indent, last_content);
                }
                if let Some(top) = items.last() {
                    if let Some(f) = opens_fence {
                        fence = Some(f);
                    } else if is_quote {
                        blocks.scan(full, off + quote_content(rest), i, top.block);
                    } else {
                        blocks.scan(full, off, i, top.block);
                    }
                    handled = true;
                }
            }
            if handled {
                ctx = Ctx::None;
                last_content = i;
                prev_blank = false;
                continue;
            }
        }

        // ---- Outside any list ---------------------------------------------
        if indent >= 4 && marker.is_none() && matches!(ctx, Ctx::None | Ctx::Code) {
            // Indented code block: no tags.
            ctx = Ctx::Code;
        } else if hr {
            ctx = Ctx::None;
        } else if let Some(h) = heading_len(rest) {
            let block = blocks.open(i);
            blocks.scan(full, off + h, i, block);
            ctx = Ctx::None;
        } else if let Some(f) = fence_open(rest) {
            fence = Some(f);
            ctx = Ctx::None;
        } else if rest.starts_with('>') {
            let (block, mut qfence) = match ctx {
                Ctx::Quote { block, fence } => {
                    blocks.extend(block, i);
                    (block, fence)
                }
                _ => (blocks.open(i), None),
            };
            let c = quote_content(rest);
            let content = rest[c..].trim_start_matches([' ', '\t']);
            match qfence {
                Some(f) => {
                    if fence_close(content, f) {
                        qfence = None;
                    }
                }
                None => match fence_open(content) {
                    Some(f) => qfence = Some(f),
                    None => blocks.scan(full, off + c, i, block),
                },
            }
            ctx = Ctx::Quote { block, fence: qfence };
        } else if let Some(m) = marker {
            let block = blocks.open(i);
            items.push(Item { indent, block });
            let content = rest[m..].trim_start_matches([' ', '\t']);
            match fence_open(content) {
                Some(f) => fence = Some(f),
                None => blocks.scan(full, off, i, block),
            }
            ctx = Ctx::None;
        } else if matches!(ctx, Ctx::Table) && rest.contains('|') {
            let block = blocks.open(i);
            blocks.scan(full, off, i, block);
        } else if !matches!(ctx, Ctx::Table)
            && rest.contains('|')
            && i + 1 < n
            && is_table_delim(lines[i + 1])
        {
            let block = blocks.open(i);
            blocks.scan(full, off, i, block);
            ctx = Ctx::Table;
        } else {
            let block = match ctx {
                Ctx::Para(block) => {
                    blocks.extend(block, i);
                    block
                }
                _ => blocks.open(i),
            };
            blocks.scan(full, off, i, block);
            ctx = Ctx::Para(block);
        }
        last_content = i;
        prev_blank = false;
    }
    pop_items(&mut items, blocks, 0, last_content);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Analyses a text: frontmatter tags first, then inline tags in document
/// order.
pub(crate) fn parse(text: &str) -> Parsed<'_> {
    let mut parsed = Parsed { occs: Vec::new(), excerpts: Vec::new(), links: Vec::new() };
    // Cheapest exit: without a `#`, a `[` or a frontmatter there is neither
    // a tag nor a link.
    let has_hash = memchr2(b'#', b'[', text.as_bytes()).is_some();
    let may_have_frontmatter = text.starts_with("---") || text.starts_with("\u{feff}---");
    if !has_hash && !may_have_frontmatter {
        return parsed;
    }

    let lines = split_lines(text);
    let n = lines.len();
    let last_line = n.saturating_sub(1);
    let mut body_start = 0;

    if may_have_frontmatter {
        if let Some(fm) = parse_frontmatter(&lines) {
            body_start = fm.close_line + 1;
            if !fm.tags.is_empty() {
                let excerpt = if body_start < n {
                    build_excerpt(&lines, body_start, 0, last_line, false)
                } else {
                    String::new()
                };
                parsed.excerpts.push(excerpt);
                for t in &fm.tags {
                    let line = lines[t.line];
                    parsed.occs.push(RawOcc {
                        tag: &line[t.start..t.end],
                        source: TagSource::Frontmatter,
                        start_line: 0,
                        end_line: last_line as u32,
                        tag_line: t.line as u32,
                        tag_col: col_utf16(line, t.col),
                        excerpt: 0,
                    });
                }
            }
        }
    }
    if !has_hash || body_start >= n {
        return parsed;
    }

    let mut blocks = Blocks {
        spans: Vec::new(),
        pending: Vec::new(),
        hits: Vec::new(),
        links: Vec::new(),
        link_hits: Vec::new(),
    };
    block_pass(&lines, body_start, &mut blocks);

    let mut cols = ColCursor::new();
    for (line_no, hit) in blocks.links.drain(..) {
        let line = lines[line_no as usize];
        let from = excerpt_window(line, hit.start);
        let (excerpt, ranges) = line_excerpt(&line[from..], &[(hit.start - from, hit.end - from)]);
        parsed.links.push(RawLink {
            target: hit.target.target,
            heading: hit.target.heading,
            alias: hit.target.alias,
            embed: hit.embed,
            markdown: hit.markdown,
            line: line_no,
            col: cols.col(line_no, line, hit.start),
            excerpt,
            range: ranges.first().copied().unwrap_or((0, 0)),
        });
    }

    // One excerpt per block, shared by its tags unless a tag is too far from
    // the start of the block to be visible in it.
    let mut block_excerpt: Vec<u32> = vec![u32::MAX; blocks.spans.len()];
    let mut cols = ColCursor::new();
    for p in &blocks.pending {
        let (start, end) = blocks.spans[p.block as usize];
        let (start, end, tag_line) = (start as usize, end as usize, p.line as usize);
        let line = lines[tag_line];
        let excerpt = if tag_is_far(&lines, start, tag_line, p.hit.hash, EXCERPT_MAX - 40) {
            let from = window_start(line, p.hit.hash);
            parsed.excerpts.push(build_excerpt(&lines, tag_line, from, end, true));
            (parsed.excerpts.len() - 1) as u32
        } else {
            let slot = &mut block_excerpt[p.block as usize];
            if *slot == u32::MAX {
                parsed.excerpts.push(build_excerpt(&lines, start, 0, end, false));
                *slot = (parsed.excerpts.len() - 1) as u32;
            }
            *slot
        };
        parsed.occs.push(RawOcc {
            tag: &line[p.hit.start..p.hit.end],
            source: TagSource::Inline,
            start_line: start as u32,
            end_line: end as u32,
            tag_line: p.line,
            tag_col: cols.col(p.line, line, p.hit.hash),
            excerpt,
        });
    }
    parsed
}

// ---------------------------------------------------------------------------
// Tests: the linear scanner against the straightforward one
// ---------------------------------------------------------------------------

#[cfg(test)]
mod linear_tests {
    use super::*;

    // The scanner as first written: each opener rescans the rest of the
    // line. Quadratic, but obviously right; kept as the reference.

    fn ref_matching(b: &[u8], open: usize, open_ch: u8, close_ch: u8) -> Option<usize> {
        let mut depth = 0usize;
        let mut j = open;
        while j < b.len() {
            let c = b[j];
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == open_ch {
                depth += 1;
            } else if c == close_ch {
                depth -= 1;
                if depth == 0 {
                    return Some(j + 1);
                }
            }
            j += 1;
        }
        None
    }

    fn ref_scan_link(line: &str, i: usize) -> Option<(usize, Option<(bool, LinkTarget)>)> {
        let b = line.as_bytes();
        if b.get(i + 1) == Some(&b'[') {
            let mut j = i + 2;
            while j + 1 < b.len() {
                if b[j] == b']' && b[j + 1] == b']' {
                    return Some((j + 2, parse_wikilink(&line[i + 2..j]).map(|t| (false, t))));
                }
                j += 1;
            }
            return None;
        }
        let after_text = ref_matching(b, i, b'[', b']')?;
        match b.get(after_text) {
            Some(b'(') => {
                let end = ref_matching(b, after_text, b'(', b')')?;
                let target = parse_markdown_link(&line[i + 1..after_text - 1], &line[after_text + 1..end - 1]);
                Some((end, target.map(|t| (true, t))))
            }
            Some(b'[') => Some((ref_matching(b, after_text, b'[', b']')?, None)),
            _ => None,
        }
    }

    fn ref_skip_angle(b: &[u8], i: usize) -> Option<usize> {
        let next = *b.get(i + 1)?;
        if next.is_ascii_alphabetic() || next == b'/' || next == b'!' {
            memchr(b'>', &b[i + 1..]).map(|p| i + 1 + p + 1)
        } else {
            None
        }
    }

    fn ref_scan_inline(line: &str, from: usize, out: &mut Vec<Hit>, links: &mut Vec<LinkHit>) {
        let b = line.as_bytes();
        let mut i = from;
        let mut prev_ws = true;
        while i < b.len() {
            let c = b[i];
            match c {
                b' ' | b'\t' => {
                    prev_ws = true;
                    i += 1;
                }
                b'#' => {
                    if prev_ws {
                        if let Some((start, end)) = tag_at(line, i + 1) {
                            out.push(Hit { hash: i, start, end });
                            i = end;
                            prev_ws = false;
                            continue;
                        }
                    }
                    prev_ws = false;
                    i += 1;
                }
                b'`' => {
                    let mut n = 1;
                    while i + n < b.len() && b[i + n] == b'`' {
                        n += 1;
                    }
                    i = closing_backticks(b, i + n, n).unwrap_or(i + n);
                    prev_ws = false;
                }
                b'\\' => {
                    i += 1;
                    if let Some(ch) = line[i..].chars().next() {
                        i += ch.len_utf8();
                    }
                    prev_ws = false;
                }
                b'[' => {
                    let embed = i > from && b[i - 1] == b'!';
                    let start = if embed { i - 1 } else { i };
                    i = match ref_scan_link(line, i) {
                        Some((end, found)) => {
                            if let Some((markdown, target)) = found {
                                links.push(LinkHit { start, end, embed, markdown, target });
                            }
                            end
                        }
                        None => i + 1,
                    };
                    prev_ws = false;
                }
                b'<' => {
                    i = ref_skip_angle(b, i).unwrap_or(i + 1);
                    prev_ws = false;
                }
                _ if c < 0x80 => {
                    prev_ws = false;
                    i += 1;
                }
                _ => {
                    let ch = line[i..].chars().next().unwrap_or(' ');
                    prev_ws = ch.is_whitespace();
                    i += ch.len_utf8();
                }
            }
        }
    }

    fn ref_tag_is_far(lines: &[&str], start: usize, tag_line: usize, hash: usize, limit: usize) -> bool {
        let mut chars = 0;
        for line in &lines[start..tag_line] {
            chars += line.chars().count() + 1;
        }
        chars + lines[tag_line][..hash].chars().count() > limit
    }

    fn ref_window_start(line: &str, hash: usize) -> usize {
        const BACK: usize = 60;
        if line[..hash].chars().count() <= BACK + 20 {
            return 0;
        }
        let mut start = hash;
        for _ in 0..BACK {
            start -= 1;
            while !line.is_char_boundary(start) {
                start -= 1;
            }
        }
        match line[start..hash].char_indices().find(|(_, c)| c.is_whitespace()) {
            Some((ws, c)) if start + ws + c.len_utf8() < hash => start + ws + c.len_utf8(),
            _ => start,
        }
    }

    /// Deterministic generator (xorshift): no dependency, same lines on
    /// every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as usize
        }

        /// A line made of the pieces that matter to the scanner, heavily
        /// biased towards brackets, escapes and unclosed openers.
        fn line(&mut self, pieces: usize) -> String {
            const PIECES: &[&str] = &[
                "[", "[", "]", "]", "(", ")", "[[", "]]", "\\", "\\", "!", "`", "``", "```", "<", ">", "<a", "</", "#",
                " #tag", " #a/b", "#é", " ", " ", "\t", "a", "b.md", "é", "🎉", "|", "x y", "\u{a0}", "[a](b.md)",
                "[[n|a]]", "![[i.png]]", "[t][r]", "\"t\"", "%20", "://",
            ];
            let mut line = String::new();
            for _ in 0..pieces {
                line.push_str(PIECES[self.next() % PIECES.len()]);
            }
            line
        }
    }

    type Scan = (Vec<(usize, usize, usize)>, Vec<(usize, usize, bool, bool, LinkTarget)>);

    fn run(scan: fn(&str, usize, &mut Vec<Hit>, &mut Vec<LinkHit>), line: &str, from: usize) -> Scan {
        let (mut hits, mut links) = (Vec::new(), Vec::new());
        scan(line, from, &mut hits, &mut links);
        (
            hits.into_iter().map(|h| (h.hash, h.start, h.end)).collect(),
            links.into_iter().map(|l| (l.start, l.end, l.embed, l.markdown, l.target)).collect(),
        )
    }

    #[test]
    fn same_tags_and_links_as_the_reference_scanner() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut tags, mut links) = (0, 0);
        for round in 0..6000 {
            let line = rng.line(1 + round % 40);
            // Any position where the block pass may start the scan.
            let from = if round % 3 == 0 { 0 } else { line.find(' ').map_or(0, |p| p + 1) };
            let got = run(scan_inline, &line, from);
            assert_eq!(got, run(ref_scan_inline, &line, from), "line {line:?} from {from}");
            tags += got.0.len();
            links += got.1.len();
        }
        assert!(tags > 1000 && links > 1000, "the generator must produce both ({tags} tags, {links} links)");
    }

    #[test]
    fn unclosed_openers_before_a_real_link() {
        let target = |line: &str| {
            let (_, links) = run(scan_inline, line, 0);
            links.into_iter().map(|l| (l.0, l.4.target)).collect::<Vec<_>>()
        };
        assert_eq!(target("[ voir [a](b.md)"), [(7, "b.md".to_string())]);
        assert_eq!(target("[[ voir [a](b.md)"), [(8, "b.md".to_string())]);
        assert_eq!(target("[ ( [ [a](b.md) et [[c]]"), [(6, "b.md".to_string()), (19, "c".to_string())]);
        assert_eq!(target("[x [y [a [b](c.md)](d.md)"), [(6, "d.md".to_string())]);
        assert_eq!(target("\\[ [a](b.md) \\] [c](d.md"), [(3, "b.md".to_string())]);
        assert_eq!(target("[a](b(1).md) [c](d(.md)"), [(0, "b(1).md".to_string())]);
    }

    #[test]
    fn positions_and_excerpts_on_long_lines_match_the_whole_line() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let (mut links, mut far) = (0, 0);
        for round in 0..300 {
            // One paragraph line starting with a letter: the block pass
            // scans it whole, so the scanner gives the expected positions.
            // (No no-break space here: `line_excerpt`, which this test
            // calls on whole lines, does not accept one before a far link.)
            let mut line = format!("a{}", rng.line(20 + round)).replace('\u{a0}', "é");
            if round % 4 == 0 {
                line = line.replace(' ', &" ".repeat(1 + round % 400));
            }
            let (mut hits, mut link_hits) = (Vec::new(), Vec::new());
            ref_scan_inline(&line, 0, &mut hits, &mut link_hits);
            let parsed = parse(&line);

            assert_eq!(parsed.links.len(), link_hits.len(), "{line:?}");
            for (link, hit) in parsed.links.iter().zip(&link_hits) {
                assert_eq!(link.col, col_utf16(&line, hit.start), "{line:?}");
                let (excerpt, ranges) = line_excerpt(&line, &[(hit.start, hit.end)]);
                assert_eq!((&link.excerpt, link.range), (&excerpt, ranges.first().copied().unwrap_or((0, 0))), "{line:?}");
                links += 1;
            }
            assert_eq!(parsed.occs.len(), hits.len(), "{line:?}");
            for (occ, hit) in parsed.occs.iter().zip(&hits) {
                assert_eq!(occ.tag_col, col_utf16(&line, hit.hash), "{line:?}");
                let limit = EXCERPT_MAX - 40;
                assert_eq!(tag_is_far(&[&line], 0, 0, hit.hash, limit), ref_tag_is_far(&[&line], 0, 0, hit.hash, limit));
                assert_eq!(window_start(&line, hit.hash), ref_window_start(&line, hit.hash), "{line:?}");
                far += usize::from(ref_tag_is_far(&[&line], 0, 0, hit.hash, limit));
            }
        }
        assert!(links > 300 && far > 300, "the generator must reach long lines ({links} links, {far} far tags)");

        // A blank wider than one byte before a far tag: the excerpt starts
        // after it, on a character boundary.
        let line = format!("{}mot\u{a0}: suite #tag fin", "x".repeat(300));
        let parsed = parse(&line);
        assert_eq!(parsed.excerpts[parsed.occs[0].excerpt as usize], "…: suite #tag fin");

        // Several lines before the tag: same answer as counting everything.
        let lines = ["é".repeat(90), "x".repeat(500), "court #tag".to_string(), String::new()];
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        for (start, tag_line, hash) in [(0, 2, 6), (1, 2, 6), (2, 2, 6), (0, 0, 180), (0, 1, 108), (0, 1, 109), (0, 1, 110)] {
            for limit in [0, 1, 90, 199, 200, 201] {
                assert_eq!(
                    tag_is_far(&lines, start, tag_line, hash, limit),
                    ref_tag_is_far(&lines, start, tag_line, hash, limit),
                    "start {start} line {tag_line} byte {hash} limit {limit}"
                );
            }
        }
    }
}
