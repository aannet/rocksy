//! Query language of the search box (CONTRACT.md, "Recherche").
//!
//! * words are combined with AND; the last word typed is a prefix;
//! * `"exact phrase"`; `-word` and `-"phrase"` exclude;
//! * `path:text` keeps the notes whose path contains the text;
//! * `tag:a/b` (or `tag:#a/b`) keeps the notes carrying the tag or one of
//!   its descendants; both filters can be negated with `-`.
//!
//! Everything is compared folded (case and accents ignored).

use super::fold::{fold_str, fold_tokens};

/// One word or phrase, as folded tokens that must be adjacent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Clause {
    pub terms: Vec<String>,
    /// The last term is a prefix (word still being typed).
    pub prefix: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedQuery {
    pub must: Vec<Clause>,
    pub must_not: Vec<Clause>,
    /// (negated, folded text contained in the path).
    pub paths: Vec<(bool, String)>,
    /// (negated, lowercased tag without `#`).
    pub tags: Vec<(bool, String)>,
}

impl ParsedQuery {
    /// True when nothing positive is asked: there is no result to give.
    pub(crate) fn is_empty(&self) -> bool {
        self.must.is_empty() && !self.paths.iter().any(|(neg, _)| !neg) && !self.tags.iter().any(|(neg, _)| !neg)
    }
}

/// Reads a value starting at `i`: quoted (up to the closing quote) or up to
/// the next blank. Returns the value, whether it was quoted, and the index
/// after it.
fn read_value(chars: &[char], mut i: usize) -> (String, bool, usize) {
    if chars.get(i) == Some(&'"') {
        i += 1;
        let start = i;
        while i < chars.len() && chars[i] != '"' {
            i += 1;
        }
        let value: String = chars[start..i].iter().collect();
        (value, true, (i + 1).min(chars.len()))
    } else {
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        (chars[start..i].iter().collect(), false, i)
    }
}

pub(crate) fn parse_query(input: &str) -> ParsedQuery {
    let chars: Vec<char> = input.chars().collect();
    let mut query = ParsedQuery::default();
    // Index in `query.must` of the last plain word, if it ends the input.
    let mut last_word: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        last_word = None;
        let negated = chars[i] == '-' && chars.get(i + 1).is_some_and(|c| !c.is_whitespace());
        if negated {
            i += 1;
        }
        let rest: String = chars[i..chars.len().min(i + 5)].iter().collect::<String>().to_lowercase();
        if rest.starts_with("path:") {
            let (value, _, next) = read_value(&chars, i + 5);
            i = next;
            let folded = fold_str(value.trim()).replace('\\', "/");
            if !folded.is_empty() {
                query.paths.push((negated, folded));
            }
            continue;
        }
        if rest.starts_with("tag:") {
            let (value, _, next) = read_value(&chars, i + 4);
            i = next;
            let tag = value.trim().trim_start_matches('#').trim_matches('/').to_lowercase();
            if !tag.is_empty() {
                query.tags.push((negated, tag));
            }
            continue;
        }
        let (value, quoted, next) = read_value(&chars, i);
        i = next;
        let terms = fold_tokens(&value);
        if terms.is_empty() {
            continue;
        }
        let clause = Clause { terms, prefix: false };
        if negated {
            query.must_not.push(clause);
        } else {
            query.must.push(clause);
            if !quoted {
                last_word = Some(query.must.len() - 1);
            }
        }
    }
    if let Some(index) = last_word {
        query.must[index].prefix = true;
    }
    query
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(terms: &[&str], prefix: bool) -> Clause {
        Clause { terms: terms.iter().map(|t| t.to_string()).collect(), prefix }
    }

    #[test]
    fn words_phrases_and_prefix() {
        let q = parse_query("Résumé du projet archi");
        assert_eq!(q.must, [c(&["resume"], false), c(&["du"], false), c(&["projet"], false), c(&["archi"], true)]);
        assert!(q.must_not.is_empty() && !q.is_empty());

        // A phrase is never a prefix, even at the end.
        let q = parse_query("avant \"Expression Exacte\"");
        assert_eq!(q.must, [c(&["avant"], false), c(&["expression", "exacte"], false)]);
        // A word made of several tokens behaves like a phrase whose last token is a prefix.
        assert_eq!(parse_query("l'archi").must, [c(&["l", "archi"], true)]);
        assert_eq!(parse_query("rocksy-co").must, [c(&["rocksy", "co"], true)]);
        // Unclosed quote: up to the end.
        assert_eq!(parse_query("\"deux mots").must, [c(&["deux", "mots"], false)]);
        // Trailing blank: the last word is still the one being typed.
        assert_eq!(parse_query("mot  ").must, [c(&["mot"], true)]);
    }

    #[test]
    fn exclusions_and_filters() {
        let q = parse_query("projet -brouillon -\"à jeter\" path:Travail/2026 tag:#Projet/Rocksy -tag:archive -path:\"vieux dossier\" fin");
        assert_eq!(q.must, [c(&["projet"], false), c(&["fin"], true)]);
        assert_eq!(q.must_not, [c(&["brouillon"], false), c(&["a", "jeter"], false)]);
        assert_eq!(q.paths, [(false, "travail/2026".to_string()), (true, "vieux dossier".to_string())]);
        assert_eq!(q.tags, [(false, "projet/rocksy".to_string()), (true, "archive".to_string())]);

        // Filters only: a valid query that lists notes. The word before a
        // filter is not the last word typed.
        let q = parse_query("TAG:été PATH:Été");
        assert!(q.must.is_empty() && !q.is_empty());
        assert_eq!(q.tags, [(false, "été".to_string())]);
        assert_eq!(q.paths, [(false, "ete".to_string())]);
        assert_eq!(parse_query("mot tag:x").must, [c(&["mot"], false)]);
    }

    #[test]
    fn empty_and_degenerate_queries() {
        for input in ["", "   ", "-", "- -", "\"\"", "!!! ---", "path:", "tag:#", "-exclu", "-tag:x -path:y"] {
            assert!(parse_query(input).is_empty(), "{input:?}");
        }
        // A lone dash followed by a blank is not a negation.
        assert_eq!(parse_query("a - b").must, [c(&["a"], false), c(&["b"], true)]);
    }
}
