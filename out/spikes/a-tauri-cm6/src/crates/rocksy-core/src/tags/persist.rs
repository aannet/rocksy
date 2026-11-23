//! On-disk form of the tag index (`data_dir/tags/<hash of the root>.tags`).
//!
//! Little-endian binary file:
//!
//! ```text
//! header  : magic "RKTAGIDX" | u32 version | u64 payload length | u64 checksum of the payload
//! payload : root path
//!           tag table   : count, then each tag (spelling shown to the user)
//!           file table  : count, then for each Markdown file of the vault
//!               relative path ('/'-separated)
//!               flags (bit 0: size/mtime known, bit 1: unreadable or not UTF-8)
//!               size, mtime in ms            (only if bit 0)
//!               excerpts    : count, then each string
//!               occurrences : count, then each
//!                   tag (index into the tag table) | source (0 inline, 1 frontmatter)
//!                   start line | end line - start line | tag line - start line
//!                   tag column (UTF-16) | excerpt (index into the file's excerpts)
//! ```
//!
//! Integers are LEB128 varints unless a width is given; strings are a varint
//! byte length followed by UTF-8. Files without any tag are listed too, so
//! that a reconcile does not read them again. Anything unexpected while
//! decoding (truncation, bad checksum, other version, other root, index out
//! of range) makes the whole file be ignored.

use super::store::{FileData, Inner, Occ};
use super::TagSource;
use crate::paths::{fnv1a64, join_rel, path_key};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"RKTAGIDX";
pub(super) const VERSION: u32 = 1;
const HEADER_LEN: usize = 8 + 4 + 8 + 8;

/// Location of the persisted tag index of `root` inside `data_dir`.
pub fn tags_index_path(data_dir: &Path, root: &str) -> PathBuf {
    let hash = fnv1a64(path_key(root).as_bytes());
    data_dir.join("tags").join(format!("{hash:016x}.tags"))
}

/// Word-at-a-time checksum (integrity against truncation and bit rot, not
/// security); several times faster than a bytewise hash on a multi-megabyte
/// file.
fn checksum(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15 ^ bytes.len() as u64;
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
        h = (h ^ u64::from_le_bytes(*word)).wrapping_mul(0x0000_0100_0000_01B3).rotate_left(29);
    }
    for &b in rest {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3).rotate_left(29);
    }
    h
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Serializes the index. `None` if it has no vault root.
pub(super) fn encode(inner: &Inner) -> Option<Vec<u8>> {
    let (root, _) = inner.root.as_ref()?;
    let mut p: Vec<u8> = Vec::with_capacity(inner.occurrences * 40 + inner.files.len() * 64 + 1024);
    put_str(&mut p, root);

    // Only tags that have occurrences of their own; parents are implied.
    let mut table_index: Vec<u32> = vec![u32::MAX; inner.tags.len()];
    let used: Vec<usize> = (0..inner.tags.len()).filter(|&i| !inner.tags[i].postings.is_empty()).collect();
    put_varint(&mut p, used.len() as u64);
    for (n, &i) in used.iter().enumerate() {
        table_index[i] = n as u32;
        put_str(&mut p, &inner.tags[i].display);
    }

    let files: Vec<&FileData> = inner.files.iter().filter(|f| f.present && !f.rel.is_empty()).collect();
    put_varint(&mut p, files.len() as u64);
    for f in files {
        put_str(&mut p, &f.rel);
        p.push(u8::from(f.meta.is_some()) | (u8::from(f.unreadable) << 1));
        if let Some((size, mtime_ms)) = f.meta {
            put_varint(&mut p, size);
            put_varint(&mut p, mtime_ms);
        }
        put_varint(&mut p, f.excerpts.len() as u64);
        for e in &f.excerpts {
            put_str(&mut p, e);
        }
        put_varint(&mut p, f.occs.len() as u64);
        for o in &f.occs {
            put_varint(&mut p, u64::from(table_index[o.tag as usize]));
            p.push(match o.source {
                TagSource::Inline => 0,
                TagSource::Frontmatter => 1,
            });
            put_varint(&mut p, u64::from(o.start_line));
            put_varint(&mut p, u64::from(o.end_line.saturating_sub(o.start_line)));
            put_varint(&mut p, u64::from(o.tag_line.saturating_sub(o.start_line)));
            put_varint(&mut p, u64::from(o.tag_col));
            put_varint(&mut p, u64::from(o.excerpt));
        }
    }

    let mut out = Vec::with_capacity(HEADER_LEN + p.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(p.len() as u64).to_le_bytes());
    out.extend_from_slice(&checksum(&p).to_le_bytes());
    out.extend_from_slice(&p);
    Some(out)
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn u32(&mut self) -> Option<u32> {
        u32::try_from(self.varint()?).ok()
    }

    /// A count of items that each take at least `min_size` bytes: rejects
    /// values that cannot fit in what is left (no huge allocation on garbage).
    fn count(&mut self, min_size: usize) -> Option<usize> {
        let n = usize::try_from(self.varint()?).ok()?;
        if n > (self.buf.len() - self.pos) / min_size.max(1) {
            return None;
        }
        Some(n)
    }

    fn str(&mut self) -> Option<&'a str> {
        let len = usize::try_from(self.varint()?).ok()?;
        let end = self.pos.checked_add(len)?;
        let bytes = self.buf.get(self.pos..end)?;
        self.pos = end;
        std::str::from_utf8(bytes).ok()
    }
}

/// Rebuilds an index from the bytes of a persisted file. `root` is the vault
/// being opened; a file written for another root is rejected.
pub(super) fn decode(bytes: &[u8], root: &str) -> Option<Inner> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return None;
    }
    if u32::from_le_bytes(bytes[8..12].try_into().ok()?) != VERSION {
        return None;
    }
    let payload_len = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
    let sum = u64::from_le_bytes(bytes[20..28].try_into().ok()?);
    let payload = &bytes[HEADER_LEN..];
    if payload.len() as u64 != payload_len || checksum(payload) != sum {
        return None;
    }

    let mut r = Reader { buf: payload, pos: 0 };
    if path_key(r.str()?) != path_key(root) {
        return None;
    }
    let mut inner = Inner::with_root(root);

    let tag_count = r.count(2)?;
    let mut tag_ids: Vec<u32> = Vec::with_capacity(tag_count);
    for _ in 0..tag_count {
        let tag = r.str()?;
        if tag.is_empty() {
            return None;
        }
        tag_ids.push(inner.intern(tag));
    }

    let file_count = r.count(4)?;
    inner.files.reserve(file_count);
    inner.by_path.reserve(file_count);
    for _ in 0..file_count {
        let rel = r.str()?;
        if rel.is_empty() {
            return None;
        }
        let flags = r.u8()?;
        if flags > 3 {
            return None;
        }
        let meta = if flags & 1 == 1 { Some((r.varint()?, r.varint()?)) } else { None };

        let excerpt_count = r.count(1)?;
        let mut excerpts: Vec<Box<str>> = Vec::with_capacity(excerpt_count);
        for _ in 0..excerpt_count {
            excerpts.push(r.str()?.into());
        }
        let occ_count = r.count(7)?;
        let mut occs: Vec<Occ> = Vec::with_capacity(occ_count);
        for _ in 0..occ_count {
            let tag = *tag_ids.get(r.u32()? as usize)?;
            let source = match r.u8()? {
                0 => TagSource::Inline,
                1 => TagSource::Frontmatter,
                _ => return None,
            };
            let start_line = r.u32()?;
            let end_line = start_line.checked_add(r.u32()?)?;
            let tag_line = start_line.checked_add(r.u32()?)?;
            let tag_col = r.u32()?;
            let excerpt = r.u32()?;
            if excerpt as usize >= excerpts.len() {
                return None;
            }
            occs.push(Occ { tag, start_line, end_line, tag_line, tag_col, excerpt, source });
        }

        let path = join_rel(root, rel);
        let file = inner.files.len() as u32;
        if inner.by_path.insert(path_key(&path), file).is_some() {
            return None; // the same file twice
        }
        inner.files.push(FileData {
            path,
            rel: rel.to_string(),
            meta,
            present: true,
            unreadable: flags & 2 == 2,
            ..FileData::default()
        });
        inner.attach(file, occs, excerpts);
    }
    if r.pos != payload.len() {
        return None;
    }
    inner.generation = 0;
    inner.saved_generation = Some(0);
    Some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::parse::parse;
    use crate::tags::store::FileParsed;

    fn sample() -> Inner {
        let mut inner = Inner::with_root("/v");
        for (rel, text) in [
            ("a.md", "---\ntags: [x, y/z]\n---\npara #x #été\n"),
            ("b/c.md", "- item #y/z/w\n  - child #🎉\n"),
            ("empty.md", "no tag\n"),
        ] {
            let parsed = FileParsed::from_parsed(parse(text));
            inner.put_file(&format!("/v/{rel}"), rel, Some((text.len() as u64, 1_800_000_000_000)), false, parsed);
        }
        inner.put_file("/v/bad.md", "bad.md", None, true, FileParsed::default());
        inner
    }

    fn with_header(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&checksum(payload).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn encode_decode_round_trip() {
        let inner = sample();
        let bytes = encode(&inner).unwrap();
        let back = decode(&bytes, "/v").unwrap();
        assert_eq!(back.md_files(), 4);
        assert_eq!(back.occurrences, inner.occurrences);
        assert_eq!(back.live_tags(), inner.live_tags());
        assert!(!back.is_dirty());
        let bad = &back.files[back.by_path[&path_key("/v/bad.md")] as usize];
        assert!(bad.unreadable && bad.meta.is_none());
        let a = &back.files[back.by_path[&path_key("/v/a.md")] as usize];
        assert_eq!(a.meta, Some((38, 1_800_000_000_000)));
        // Deterministic: encoding the decoded index gives the same bytes.
        assert_eq!(encode(&back).unwrap(), bytes);
        assert!(decode(&bytes, "/other").is_none());
        assert!(encode(&Inner::default()).is_none());
    }

    #[test]
    fn decoder_survives_any_single_byte_corruption_with_a_valid_checksum() {
        // The checksum normally rejects damaged files; here it is recomputed
        // so that the structural checks themselves are exercised.
        let bytes = encode(&sample()).unwrap();
        let payload = &bytes[HEADER_LEN..];
        for pos in 0..payload.len() {
            for value in [0x00, 0x7F, 0x80, 0xFF, payload[pos] ^ 0x01, payload[pos].wrapping_add(1)] {
                let mut mutated = payload.to_vec();
                mutated[pos] = value;
                if let Some(inner) = decode(&with_header(&mutated), "/v") {
                    // Whatever was accepted must be internally consistent.
                    let own: usize = inner.tags.iter().map(|t| t.postings.len()).sum();
                    assert_eq!(own, inner.occurrences);
                    for f in &inner.files {
                        assert!(f.occs.iter().all(|o| (o.excerpt as usize) < f.excerpts.len()));
                    }
                }
            }
        }
        // Truncated payloads with a valid header.
        for len in 0..payload.len() {
            assert!(decode(&with_header(&payload[..len]), "/v").is_none(), "len {len}");
        }
        // An endless varint and an absurd count.
        assert!(decode(&with_header(&[0xFF; 32]), "/v").is_none());
        let mut huge = Vec::new();
        put_str(&mut huge, "/v");
        put_varint(&mut huge, u64::MAX);
        assert!(decode(&with_header(&huge), "/v").is_none());
    }
}
