//! Link syntax: what a wikilink or a Markdown link points to.
//!
//! Recognised (see CONTRACT.md, "Liens"): `[[target]]`, `[[target#heading]]`,
//! `[[target#^block]]`, `[[target|alias]]`, embeds `![[...]]`, Markdown links
//! `[text](path.md)` and images `![alt](path.png)` to a local file. Links to
//! a URL (`http:`, `mailto:`, any scheme) and links inside the same note
//! (`[[#heading]]`, `[text](#heading)`) are not links between files.

/// Target of a link as written in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkTarget {
    /// Target without heading nor alias.
    pub target: String,
    /// Heading or block (`^id`), without the `#`.
    pub heading: Option<String>,
    pub alias: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Parses the inside of `[[...]]`. `None` for a link inside the same note.
pub(crate) fn parse_wikilink(inner: &str) -> Option<LinkTarget> {
    let (left, alias) = match inner.find('|') {
        Some(pos) => (&inner[..pos], Some(&inner[pos + 1..])),
        None => (inner, None),
    };
    // Inside a table the separator is written `\|`.
    let left = left.strip_suffix('\\').unwrap_or(left);
    let (target, heading) = match left.find('#') {
        Some(pos) => (&left[..pos], Some(&left[pos + 1..])),
        None => (left, None),
    };
    let target = non_empty(target)?;
    Some(LinkTarget { target, heading: heading.and_then(non_empty), alias: alias.and_then(non_empty) })
}

/// True if `dest` starts with a URI scheme (`http:`, `mailto:`, `obsidian:`)
/// or is protocol-relative (`//host`).
fn has_scheme(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
    let Some(colon) = dest.find(':') else { return false };
    let scheme = &dest[..colon];
    // One letter is a Windows drive, not a scheme.
    scheme.len() >= 2
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Decodes `%20`-style escapes; invalid sequences are kept as written.
pub(crate) fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Parses a Markdown link: `text` is what is between the brackets, `dest`
/// what is between the parentheses (destination and optional title).
/// `None` for a URL or a link inside the same note.
pub(crate) fn parse_markdown_link(text: &str, dest: &str) -> Option<LinkTarget> {
    let dest = dest.trim();
    let path = if let Some(rest) = dest.strip_prefix('<') {
        &rest[..rest.find('>')?]
    } else {
        // The destination ends at the first blank; a title may follow.
        dest.split(|c: char| c.is_whitespace()).next().unwrap_or("")
    };
    if path.is_empty() || path.starts_with('#') || has_scheme(path) {
        return None;
    }
    let (file, heading) = match path.find('#') {
        Some(pos) => (&path[..pos], Some(&path[pos + 1..])),
        None => (path, None),
    };
    let target = non_empty(&percent_decode(file))?;
    Some(LinkTarget {
        target,
        heading: heading.map(percent_decode).as_deref().and_then(non_empty),
        alias: non_empty(text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(target: &str, heading: Option<&str>, alias: Option<&str>) -> Option<LinkTarget> {
        Some(LinkTarget { target: target.into(), heading: heading.map(Into::into), alias: alias.map(Into::into) })
    }

    #[test]
    fn wikilink_forms() {
        assert_eq!(parse_wikilink("Note"), t("Note", None, None));
        assert_eq!(parse_wikilink("dossier/Ma note"), t("dossier/Ma note", None, None));
        assert_eq!(parse_wikilink("Note#Titre long"), t("Note", Some("Titre long"), None));
        assert_eq!(parse_wikilink("Note#^bloc-1"), t("Note", Some("^bloc-1"), None));
        assert_eq!(parse_wikilink("Note|alias affiché"), t("Note", None, Some("alias affiché")));
        assert_eq!(parse_wikilink("Note#Titre|alias"), t("Note", Some("Titre"), Some("alias")));
        assert_eq!(parse_wikilink(" Note # Titre | a|b "), t("Note", Some("Titre"), Some("a|b")));
        assert_eq!(parse_wikilink("Note\\|alias"), t("Note", None, Some("alias")));
        assert_eq!(parse_wikilink("image.png|300"), t("image.png", None, Some("300")));
        assert_eq!(parse_wikilink("Note#"), t("Note", None, None));
        // Links inside the same note, and empty links.
        assert_eq!(parse_wikilink("#Titre"), None);
        assert_eq!(parse_wikilink("#^bloc|alias"), None);
        assert_eq!(parse_wikilink(""), None);
        assert_eq!(parse_wikilink(" | alias"), None);
    }

    #[test]
    fn markdown_link_forms() {
        assert_eq!(parse_markdown_link("texte", "note.md"), t("note.md", None, Some("texte")));
        assert_eq!(parse_markdown_link("", "img/photo.png"), t("img/photo.png", None, None));
        assert_eq!(parse_markdown_link("x", "../autre/Ma%20note.md#Un%20titre"), t("../autre/Ma note.md", Some("Un titre"), Some("x")));
        assert_eq!(parse_markdown_link("x", "note.md \"un titre\""), t("note.md", None, Some("x")));
        assert_eq!(parse_markdown_link("x", "<Ma note.md>"), t("Ma note.md", None, Some("x")));
        assert_eq!(parse_markdown_link("x", "caf%C3%A9.md"), t("café.md", None, Some("x")));
        assert_eq!(parse_markdown_link("x", "100%.md"), t("100%.md", None, Some("x")));
        // URLs and in-note anchors are not file links.
        for dest in ["http://example.com/a.md", "https://x.org", "mailto:a@b.c", "obsidian://open?vault=x", "//host/x", "#titre", "", "ftp://x/y.png", "tel:123"] {
            assert_eq!(parse_markdown_link("x", dest), None, "{dest}");
        }
        // A Windows drive letter is not a scheme.
        assert!(parse_markdown_link("x", "C:/notes/a.md").is_some());
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%C3%A9t%C3%A9"), "été");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%2"), "%zz%2");
        assert_eq!(percent_decode("%FF"), "%FF", "invalid UTF-8 is left as written");
        assert_eq!(percent_decode("plain"), "plain");
    }
}
