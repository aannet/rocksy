//! One-line excerpts with highlight ranges, shared by links and search.

/// Longest excerpt, in characters.
pub(crate) const EXCERPT_MAX: usize = 240;
/// Characters kept before the first highlight when the line must be cut.
const LEAD: usize = 60;

fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() {
        s.len() as u32
    } else {
        s.chars().map(char::len_utf16).sum::<usize>() as u32
    }
}

/// Excerpt of one line around the given byte ranges (sorted, not
/// overlapping, on character boundaries).
///
/// Returns the excerpt (at most [`EXCERPT_MAX`] characters, leading blanks
/// removed, tabs shown as spaces, `…` where the line was cut) and the ranges
/// that fall entirely inside it, in UTF-16 units of the excerpt.
pub(crate) fn line_excerpt(line: &str, ranges: &[(usize, usize)]) -> (String, Vec<(u32, u32)>) {
    // Skip a byte-order mark and leading blanks.
    let trimmed = line.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let mut start = line.len() - trimmed.len();
    let first = ranges.first().map_or(start, |r| r.0.max(start));

    // Cut the beginning if the first highlight would not be visible.
    let mut cut_before = false;
    let first_len = ranges.first().map_or(0, |r| line[r.0..r.1].chars().count());
    if line[start..first].chars().count() + first_len > EXCERPT_MAX - 40 {
        let mut pos = first;
        for _ in 0..LEAD {
            if pos <= start {
                break;
            }
            pos -= 1;
            while !line.is_char_boundary(pos) {
                pos -= 1;
            }
        }
        // Prefer a word boundary.
        // The whitespace may be multi-byte (no-break space): skip its full width.
        if let Some((ws, c)) = line[pos..first].char_indices().find(|(_, c)| c.is_whitespace()) {
            if pos + ws < first {
                pos += ws + c.len_utf8();
            }
        }
        if pos > start {
            start = pos;
            cut_before = true;
        }
    }

    let mut out = String::new();
    let mut count = 0;
    if cut_before {
        out.push('…');
        count = 1;
    }
    let prefix16 = utf16_len(&out);
    let mut end = start;
    let mut cut_after = false;
    for (off, c) in line[start..].char_indices() {
        if count + 1 > EXCERPT_MAX - 1 && start + off + c.len_utf8() < line.len() {
            cut_after = true;
            break;
        }
        if count + 1 > EXCERPT_MAX {
            cut_after = true;
            break;
        }
        out.push(if c == '\t' { ' ' } else { c });
        count += 1;
        end = start + off + c.len_utf8();
    }
    if cut_after {
        out.push('…');
    }

    let mut shown = Vec::new();
    for &(a, b) in ranges {
        if a >= start && b <= end && a < b {
            let from = prefix16 + utf16_len(&line[start..a]);
            shown.push((from, from + utf16_len(&line[a..b])));
        }
    }
    (out, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice16(s: &str, range: (u32, u32)) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16(&units[range.0 as usize..range.1 as usize]).unwrap()
    }

    #[test]
    fn short_line_is_kept_and_ranges_are_utf16() {
        let line = "\t  🎉 été [[Note]] fin";
        let a = line.find("[[").unwrap();
        let (text, ranges) = line_excerpt(line, &[(a, a + 8)]);
        assert_eq!(text, "🎉 été [[Note]] fin");
        assert_eq!(ranges, [(7, 15)]);
        assert_eq!(slice16(&text, ranges[0]), "[[Note]]");
        // Byte-order mark removed, no range at all.
        assert_eq!(line_excerpt("\u{feff}# Titre", &[]), ("# Titre".to_string(), vec![]));
        assert_eq!(line_excerpt("", &[]), (String::new(), vec![]));
    }

    #[test]
    fn no_break_space_before_the_range_does_not_panic() {
        // French typography: a no-break space (2 bytes) sits before ":".
        let mut line = "x".repeat(400);
        line.push_str("\u{a0}: voir ");
        let start = line.len();
        line.push_str("[[cible]]");
        let (excerpt, ranges) = line_excerpt(&line, &[(start, line.len())]);
        assert!(excerpt.contains("[[cible]]"));
        assert_eq!(ranges.len(), 1);
    }

    #[test]
    fn long_line_is_windowed_around_the_first_range() {
        let line = format!("{} cible ici {}", "é".repeat(500), "z ".repeat(400));
        let a = line.find("cible").unwrap();
        let b = line.find("ici").unwrap();
        let (text, ranges) = line_excerpt(&line, &[(a, a + 5), (b, b + 3)]);
        assert_eq!(text.chars().count(), EXCERPT_MAX);
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert_eq!(ranges.len(), 2);
        assert_eq!(slice16(&text, ranges[0]), "cible");
        assert_eq!(slice16(&text, ranges[1]), "ici");

        // A range beyond the window is dropped; the excerpt stays bounded.
        let line = format!("debut {}fin", "mot ".repeat(100));
        let far = line.len() - 3;
        let (text, ranges) = line_excerpt(&line, &[(0, 5), (far, far + 3)]);
        assert!(text.starts_with("debut mot") && text.ends_with('…'));
        assert_eq!(text.chars().count(), EXCERPT_MAX);
        assert_eq!(ranges, [(0, 5)]);

        // Exactly at the limit: nothing cut.
        let exact = "x".repeat(EXCERPT_MAX);
        assert_eq!(line_excerpt(&exact, &[(0, 1)]).0, exact);
        let over = "x".repeat(EXCERPT_MAX + 1);
        let (text, _) = line_excerpt(&over, &[(0, 1)]);
        assert_eq!(text.chars().count(), EXCERPT_MAX);
        assert!(text.ends_with('…'));
    }
}
