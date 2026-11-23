//! Tokenisation and folding shared by indexing, querying and highlighting.
//!
//! A token is a maximal run of letters and digits (any script). It is
//! compared after folding: lowercased, Latin diacritics removed (`É` -> `e`,
//! `œ` -> `oe`, `ß` -> `ss`), combining accents dropped. The same code feeds
//! the index and re-scans stored text for excerpts, so what is highlighted is
//! exactly what matched.

/// Tokens whose folded form is longer than this are neither indexed nor
/// matched (base64 blobs, hashes).
pub(crate) const MAX_TOKEN_BYTES: usize = 64;

fn is_combining(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining(c)
}

/// Appends the folded form of `c` to `out`.
pub(crate) fn fold_char(c: char, out: &mut String) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    let mapped: &str = match c as u32 {
        0x0300..=0x036F => "",
        0xC0..=0xC5 | 0xE0..=0xE5 | 0x100..=0x105 => "a",
        0xC6 | 0xE6 => "ae",
        0xC7 | 0xE7 | 0x106..=0x10D => "c",
        0xD0 | 0xF0 | 0x10E..=0x111 => "d",
        0xC8..=0xCB | 0xE8..=0xEB | 0x112..=0x11B => "e",
        0x11C..=0x123 => "g",
        0x124..=0x127 => "h",
        0xCC..=0xCF | 0xEC..=0xEF | 0x128..=0x131 => "i",
        0x134 | 0x135 => "j",
        0x136..=0x138 => "k",
        0x139..=0x142 => "l",
        0xD1 | 0xF1 | 0x143..=0x14B => "n",
        0xD2..=0xD6 | 0xD8 | 0xF2..=0xF6 | 0xF8 | 0x14C..=0x151 => "o",
        0x152 | 0x153 => "oe",
        0x154..=0x159 => "r",
        0x15A..=0x161 | 0x17F => "s",
        0xDF => "ss",
        0x162..=0x167 => "t",
        0xDE | 0xFE => "th",
        0xD9..=0xDC | 0xF9..=0xFC | 0x168..=0x173 => "u",
        0x174 | 0x175 => "w",
        0xDD | 0xFD | 0xFF | 0x176..=0x178 => "y",
        0x179..=0x17E => "z",
        _ => {
            out.extend(c.to_lowercase());
            return;
        }
    };
    out.push_str(mapped);
}

/// Folds a whole string, keeping every character that is not a letter
/// (used for paths).
pub(crate) fn fold_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        fold_char(c, &mut out);
    }
    out
}

/// Iterator over the tokens of a text: byte range and folded form (borrowed
/// from an internal buffer, valid until the next call).
pub(crate) struct Tokens<'a> {
    text: &'a str,
    pos: usize,
    folded: String,
}

pub(crate) struct Token<'t> {
    pub start: usize,
    pub end: usize,
    /// Folded text; empty if the token is too long to be indexed.
    pub folded: &'t str,
}

impl<'a> Tokens<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self { text, pos: 0, folded: String::with_capacity(32) }
    }

    /// Next token, or `None` at the end. A token longer than
    /// [`MAX_TOKEN_BYTES`] once folded is returned with an empty `folded`
    /// (it still occupies a position, so phrases do not match across it).
    pub(crate) fn next_token(&mut self) -> Option<Token<'_>> {
        let bytes = self.text.as_bytes();
        // Skip separators. ASCII fast path, full decoding otherwise.
        let mut start = self.pos;
        loop {
            match bytes.get(start) {
                None => {
                    self.pos = start;
                    return None;
                }
                Some(&b) if b < 0x80 => {
                    if b.is_ascii_alphanumeric() {
                        break;
                    }
                    start += 1;
                }
                Some(_) => {
                    let c = self.text[start..].chars().next()?;
                    if is_token_char(c) {
                        break;
                    }
                    start += c.len_utf8();
                }
            }
        }
        self.folded.clear();
        let mut end = start;
        loop {
            match bytes.get(end) {
                Some(&b) if b < 0x80 => {
                    if !b.is_ascii_alphanumeric() {
                        break;
                    }
                    self.folded.push(b.to_ascii_lowercase() as char);
                    end += 1;
                }
                Some(_) => {
                    let Some(c) = self.text[end..].chars().next() else { break };
                    if !is_token_char(c) {
                        break;
                    }
                    fold_char(c, &mut self.folded);
                    end += c.len_utf8();
                }
                None => break,
            }
        }
        self.pos = end;
        if self.folded.len() > MAX_TOKEN_BYTES {
            self.folded.clear();
        }
        Some(Token { start, end, folded: &self.folded })
    }
}

/// Folded tokens of a short text (query word, file name).
pub(crate) fn fold_tokens(text: &str) -> Vec<String> {
    let mut tokens = Tokens::new(text);
    let mut out = Vec::new();
    while let Some(t) = tokens.next_token() {
        if !t.folded.is_empty() {
            out.push(t.folded.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(String, String)> {
        let mut tokens = Tokens::new(text);
        let mut out = Vec::new();
        while let Some(t) = tokens.next_token() {
            out.push((text[t.start..t.end].to_string(), t.folded.to_string()));
        }
        out
    }

    #[test]
    fn folding_ignores_case_and_accents() {
        assert_eq!(fold_tokens("Résumé de l'ÉTÉ à Noël"), ["resume", "de", "l", "ete", "a", "noel"]);
        assert_eq!(fold_tokens("Cœur Straße Ærø Łódź Ñandú Çà"), ["coeur", "strasse", "aero", "lodz", "nandu", "ca"]);
        // Decomposed accent (e + combining acute) folds like the composed form.
        assert_eq!(fold_tokens("re\u{301}sume\u{301}"), ["resume"]);
        assert_eq!(fold_tokens("re\u{301}sume\u{301}"), fold_tokens("résumé"));
        // Other scripts are lowercased and kept.
        assert_eq!(fold_tokens("Привет 日本語 ΑΒΓ"), ["привет", "日本語", "αβγ"]);
        assert_eq!(fold_str("Dossier Été/Ma Note.md"), "dossier ete/ma note.md");
    }

    #[test]
    fn tokens_split_on_everything_but_letters_and_digits() {
        assert_eq!(
            fold_tokens("#projet/rocksy-core v0.1_beta [[Lien|alias]] a+b=c 🎉x"),
            ["projet", "rocksy", "core", "v0", "1", "beta", "lien", "alias", "a", "b", "c", "x"]
        );
        assert!(fold_tokens("  ---  \n\t!!").is_empty());
        assert!(fold_tokens("").is_empty());
    }

    #[test]
    fn spans_point_at_the_original_text() {
        let text = "Un Résumé, deux";
        assert_eq!(
            spans(text),
            [("Un".into(), "un".into()), ("Résumé".into(), "resume".into()), ("deux".into(), "deux".into())]
        );
    }

    #[test]
    fn overlong_tokens_keep_their_place_but_are_not_indexed() {
        let blob = "A".repeat(MAX_TOKEN_BYTES + 1);
        let text = format!("avant {blob} après");
        let got = spans(&text);
        assert_eq!(got.len(), 3);
        assert_eq!(got[1].1, "");
        assert_eq!(fold_tokens(&text), ["avant", "apres"]);
        assert_eq!(fold_tokens(&"b".repeat(MAX_TOKEN_BYTES)).len(), 1);
    }
}
