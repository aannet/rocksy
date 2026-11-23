//! Finds the occurrences of a query in a stored text and builds the
//! excerpts, without reading any file: the text comes from the index.

use super::fold::Tokens;
use super::query::{Clause, ParsedQuery};
use super::SearchMatch;
use crate::excerpt::line_excerpt;
use memchr::memchr2_iter;

/// Excerpts returned per file.
pub(crate) const MAX_MATCHES: usize = 3;

fn term_matches(token: &str, term: &str, prefix: bool) -> bool {
    if prefix {
        token.starts_with(term)
    } else {
        token == term
    }
}

/// Matches clauses against a stream of tokens. Keeps the last few tokens to
/// recognise phrases.
struct Matcher<'q> {
    clauses: &'q [Clause],
    /// Longest phrase, in tokens.
    window: usize,
    /// Recent tokens (folded text, start byte), oldest first.
    recent: Vec<(String, usize)>,
}

impl<'q> Matcher<'q> {
    fn new(clauses: &'q [Clause]) -> Self {
        let window = clauses.iter().map(|c| c.terms.len()).max().unwrap_or(1);
        Self { clauses, window, recent: Vec::with_capacity(window) }
    }

    /// Feeds one token; returns the start byte of each match ending at this
    /// token together with the index of the clause that matched.
    fn feed(&mut self, folded: &str, start: usize, out: &mut Vec<(usize, usize)>) {
        if folded.is_empty() {
            // Unindexed token: phrases do not match across it.
            self.recent.clear();
            return;
        }
        if self.window > 1 {
            if self.recent.len() == self.window {
                let mut recycled = self.recent.remove(0);
                recycled.0.clear();
                recycled.0.push_str(folded);
                recycled.1 = start;
                self.recent.push(recycled);
            } else {
                self.recent.push((folded.to_string(), start));
            }
        }
        for (index, clause) in self.clauses.iter().enumerate() {
            let n = clause.terms.len();
            if n == 1 {
                if term_matches(folded, &clause.terms[0], clause.prefix) {
                    out.push((start, index));
                }
            } else if self.recent.len() >= n {
                let tail = &self.recent[self.recent.len() - n..];
                let all = tail.iter().zip(&clause.terms).enumerate().all(|(k, ((token, _), term))| {
                    term_matches(token, term, clause.prefix && k == n - 1)
                });
                if all {
                    out.push((tail[0].1, index));
                }
            }
        }
    }
}

/// Follows line numbers (CodeMirror model: `\n`, `\r\n` and lone `\r`).
struct LineTracker<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    line_start: usize,
}

impl<'a> LineTracker<'a> {
    fn new(text: &'a str) -> Self {
        Self { bytes: text.as_bytes(), pos: 0, line: 0, line_start: 0 }
    }

    /// Moves to byte `to` (never backwards) and returns (line, line start).
    fn advance(&mut self, to: usize) -> (u32, usize) {
        if to > self.pos {
            for off in memchr2_iter(b'\n', b'\r', &self.bytes[self.pos..to]) {
                let at = self.pos + off;
                if self.bytes[at] == b'\r' && self.bytes.get(at + 1) == Some(&b'\n') {
                    continue; // counted with the `\n`
                }
                self.line += 1;
                self.line_start = at + 1;
            }
            self.pos = to;
        }
        (self.line, self.line_start)
    }

    fn line_end(&self, from: usize) -> usize {
        memchr::memchr2(b'\n', b'\r', &self.bytes[from..]).map_or(self.bytes.len(), |p| from + p)
    }
}

/// (line number, byte offset of the line, byte ranges matched on it).
type KeptLine = (u32, usize, Vec<(usize, usize)>);

/// Occurrences of the positive clauses of `query` in `text`: their number
/// and the first [`MAX_MATCHES`] lines containing one, in text order.
pub(crate) fn find_matches(text: &str, query: &ParsedQuery) -> (usize, Vec<SearchMatch>) {
    if query.must.is_empty() {
        return (0, Vec::new());
    }
    let mut matcher = Matcher::new(&query.must);
    let mut lines = LineTracker::new(text);
    let mut tokens = Tokens::new(text);
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut count = 0;
    let mut kept: Vec<KeptLine> = Vec::new();

    while let Some(token) = tokens.next_token() {
        found.clear();
        matcher.feed(token.folded, token.start, &mut found);
        if found.is_empty() {
            continue;
        }
        count += found.len();
        if kept.len() == MAX_MATCHES && kept.last().is_some_and(|k| lines.line_end(k.1) < token.start) {
            continue; // past the last kept line: only counting
        }
        for &(start, _) in &found {
            // A phrase can start on an earlier line; it is reported on the
            // line where it ends only if it fits there.
            let (line, line_start) = lines.advance(token.start);
            let start = start.max(line_start);
            let range = (start, token.end);
            let same_line = kept.last().is_some_and(|last| last.0 == line);
            if same_line {
                if let Some(last) = kept.last_mut() {
                    if last.2.last().is_none_or(|prev| prev.1 <= range.0) {
                        last.2.push(range);
                    }
                }
            } else if kept.len() < MAX_MATCHES {
                kept.push((line, line_start, vec![range]));
            }
        }
    }

    let matches = kept
        .into_iter()
        .map(|(line, line_start, ranges)| {
            let end = lines.line_end(line_start);
            let text_line = &text[line_start..end];
            let local: Vec<(usize, usize)> = ranges.iter().map(|&(a, b)| (a - line_start, b - line_start)).collect();
            let (excerpt, shown) = line_excerpt(text_line, &local);
            let utf16 = |s: &str| s.chars().map(char::len_utf16).sum::<usize>() as u32;
            let col = utf16(&text_line[..local[0].0]);
            let len = utf16(&text_line[local[0].0..local[0].1]);
            SearchMatch { line, col, len, excerpt, ranges: shown }
        })
        .collect();
    (count, matches)
}

/// True if every positive clause of the query matches in `name` (the file
/// name without extension).
pub(crate) fn name_matches(name: &str, query: &ParsedQuery) -> bool {
    if query.must.is_empty() {
        return false;
    }
    let mut matcher = Matcher::new(&query.must);
    let mut tokens = Tokens::new(name);
    let mut found = Vec::new();
    let mut seen = vec![false; query.must.len()];
    while let Some(token) = tokens.next_token() {
        found.clear();
        matcher.feed(token.folded, token.start, &mut found);
        for &(_, clause) in &found {
            seen[clause] = true;
        }
    }
    seen.iter().all(|&s| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::query::parse_query;

    fn slice16(s: &str, range: (u32, u32)) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16(&units[range.0 as usize..range.1 as usize]).unwrap()
    }

    fn highlighted(m: &SearchMatch) -> Vec<String> {
        m.ranges.iter().map(|&r| slice16(&m.excerpt, r)).collect()
    }

    #[test]
    fn words_are_found_folded_with_lines_and_columns() {
        let text = "# Titre\r\n\r\nUn Résumé de l'été.\r\nRien ici\rAutre RÉSUMÉ, résumé";
        let (count, matches) = find_matches(text, &parse_query("resume "));
        // "resume" is the last word: prefix, three occurrences on two lines.
        assert_eq!(count, 3);
        assert_eq!(matches.len(), 2);
        assert_eq!((matches[0].line, matches[0].col, matches[0].len), (2, 3, 6));
        assert_eq!(matches[0].excerpt, "Un Résumé de l'été.");
        assert_eq!(highlighted(&matches[0]), ["Résumé"]);
        assert_eq!((matches[1].line, matches[1].col), (4, 6));
        assert_eq!(highlighted(&matches[1]), ["RÉSUMÉ", "résumé"]);
    }

    #[test]
    fn prefix_phrase_and_columns_in_utf16() {
        let text = "🎉 architecture et archives\nl'architecture du projet\nprojet d'architecture";
        let (count, matches) = find_matches(text, &parse_query("archi"));
        assert_eq!(count, 4);
        assert_eq!(matches[0].col, 3, "the emoji counts for two units");
        assert_eq!(highlighted(&matches[0]), ["architecture", "archives"]);

        let (count, matches) = find_matches(text, &parse_query("\"architecture du projet\""));
        assert_eq!(count, 1);
        assert_eq!((matches[0].line, matches[0].col, matches[0].len), (1, 2, 22));
        assert_eq!(highlighted(&matches[0]), ["architecture du projet"]);
        // A byte-order mark counts for one unit on the first line, as for tags.
        let (_, with_bom) = find_matches("\u{feff}🎉 mot", &parse_query("mot"));
        assert_eq!((with_bom[0].line, with_bom[0].col, with_bom[0].len), (0, 4, 3));
        assert_eq!(with_bom[0].excerpt, "🎉 mot");

        // Exact word: "archi" alone is not in the text.
        let (count, _) = find_matches(text, &parse_query("archi autre"));
        assert_eq!(count, 0);
        // Phrase with a prefix at the end (word made of two tokens).
        let (count, matches) = find_matches(text, &parse_query("d'archi"));
        assert_eq!(count, 1);
        assert_eq!(highlighted(&matches[0]), ["d'architecture"]);
    }

    #[test]
    fn at_most_three_lines_but_every_occurrence_is_counted() {
        let text = (0..10).map(|i| format!("ligne {i} avec mot et mot")).collect::<Vec<_>>().join("\n");
        let (count, matches) = find_matches(&text, &parse_query("mot "));
        assert_eq!(count, 20);
        assert_eq!(matches.iter().map(|m| m.line).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(matches.iter().all(|m| highlighted(m) == ["mot", "mot"]));
        // Several words: each contributes.
        let (count, matches) = find_matches(&text, &parse_query("ligne mot"));
        assert_eq!(count, 30);
        assert_eq!(highlighted(&matches[0]), ["ligne", "mot", "mot"]);
        // Exclusions and filters do not highlight anything.
        assert_eq!(find_matches(&text, &parse_query("-mot tag:x")), (0, vec![]));
    }

    #[test]
    fn long_lines_are_windowed_and_bounded() {
        let text = format!("{} cible {}", "é".repeat(3000), "z".repeat(3000));
        let (count, matches) = find_matches(&text, &parse_query("cible"));
        assert_eq!(count, 1);
        assert_eq!(matches[0].col, 3001);
        assert!(matches[0].excerpt.chars().count() <= 240);
        assert_eq!(highlighted(&matches[0]), ["cible"]);
    }

    #[test]
    fn file_name_matching() {
        assert!(name_matches("Résumé du Projet", &parse_query("projet resu")));
        assert!(name_matches("Résumé du Projet", &parse_query("\"du projet\"")));
        assert!(!name_matches("Résumé du Projet", &parse_query("projet autre")));
        assert!(!name_matches("Résumé du Projet", &parse_query("tag:x")));
        assert!(!name_matches("Résumé", &parse_query("resu x")), "resu is not the last word: exact");
    }
}
