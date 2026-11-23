//! Link graph: resolution of the raw links of every note against the files
//! of the vault, and the reverse index (backlinks).
//!
//! The graph is derived data: it is rebuilt in memory from the links stored
//! with each note and the list of visible files, never by reading a note.
//! Rebuilding costs one hash lookup per link, which still adds up to tens of
//! milliseconds on a large vault: it is redone only when the targets of the
//! links of a note, the set of notes or the other files changed
//! (`Inner::graph_generation`), not at every save.
//!
//! Resolution rules (CONTRACT.md, "Liens"), all case-insensitive:
//! * wikilink: exact path from the vault root, else path suffix, else file
//!   name; the `.md` extension is optional; among several candidates the
//!   shortest path wins, then alphabetical order;
//! * Markdown link: relative to the folder of the note, else from the root.

use super::store::Inner;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Target {
    /// A note: index into `Inner::files`.
    Note(u32),
    /// Any other visible file: index into `Inner::others`.
    Other(u32),
}

pub(super) struct Unresolved {
    /// Target as first written.
    pub target: String,
    pub count: u32,
    /// Notes containing it (file ids), at most five.
    pub sources: Vec<u32>,
}

pub(super) struct Graph {
    /// (epoch, graph generation) of the content this graph was built from.
    pub stamp: (u64, u64),
    /// Per file id, per link: what it resolves to.
    pub resolved: Vec<Vec<Option<Target>>>,
    /// Target -> (source file id, link index), self-links excluded.
    pub backlinks: HashMap<Target, Vec<(u32, u32)>>,
    /// Lowercased relative path -> target, for lookups by path.
    pub by_rel: HashMap<String, Target>,
    /// Unresolved targets, most frequent first.
    pub unresolved: Vec<Unresolved>,
    /// Total number of links.
    pub links: usize,
}

/// Lowercases and normalises a link target: `\` -> `/`, no leading `./` or
/// `/`, no trailing `/`.
fn normalize_target(target: &str) -> String {
    let mut t = target.trim().replace('\\', "/").to_lowercase();
    while let Some(rest) = t.strip_prefix("./") {
        t = rest.to_string();
    }
    t.trim_matches('/').to_string()
}

/// Resolves `.` and `..` in a `/`-separated path. `None` if it climbs above
/// the root.
fn clean_path(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

struct Candidate {
    /// Lowercased relative path.
    rel: String,
    target: Target,
}

struct Resolver {
    candidates: Vec<Candidate>,
    /// Lowercased file name -> candidates, best first (shortest path, then
    /// alphabetical).
    by_name: HashMap<String, Vec<u32>>,
    by_rel: HashMap<String, u32>,
}

impl Resolver {
    fn new(inner: &Inner) -> Self {
        let mut candidates: Vec<Candidate> = Vec::with_capacity(inner.files.len() + inner.others.len());
        for (id, file) in inner.files.iter().enumerate() {
            if file.present && !file.rel.is_empty() {
                candidates.push(Candidate { rel: file.rel.to_lowercase(), target: Target::Note(id as u32) });
            }
        }
        for (id, rel) in inner.others.iter().enumerate() {
            candidates.push(Candidate { rel: rel.to_lowercase(), target: Target::Other(id as u32) });
        }
        // Best first, so that the first match of any lookup is the winner.
        candidates.sort_by(|a, b| a.rel.len().cmp(&b.rel.len()).then_with(|| a.rel.cmp(&b.rel)));
        let mut by_name: HashMap<String, Vec<u32>> = HashMap::with_capacity(candidates.len());
        let mut by_rel: HashMap<String, u32> = HashMap::with_capacity(candidates.len());
        for (i, c) in candidates.iter().enumerate() {
            let name = c.rel.rsplit('/').next().unwrap_or(&c.rel);
            by_name.entry(name.to_string()).or_default().push(i as u32);
            by_rel.entry(c.rel.clone()).or_insert(i as u32);
        }
        Self { candidates, by_name, by_rel }
    }

    fn exact(&self, rel: &str) -> Option<Target> {
        self.by_rel.get(rel).map(|&i| self.candidates[i as usize].target)
    }

    /// Exact path or path suffix `wanted` (already lowercased).
    fn by_suffix(&self, wanted: &str) -> Option<Target> {
        if let Some(t) = self.exact(wanted) {
            return Some(t);
        }
        let name = wanted.rsplit('/').next().unwrap_or(wanted);
        let list = self.by_name.get(name)?;
        if !wanted.contains('/') {
            return list.first().map(|&i| self.candidates[i as usize].target);
        }
        list.iter()
            .map(|&i| &self.candidates[i as usize])
            .find(|c| c.rel.len() > wanted.len() && c.rel.ends_with(wanted) && c.rel.as_bytes()[c.rel.len() - wanted.len() - 1] == b'/')
            .map(|c| c.target)
    }

    fn by_name_only(&self, name: &str) -> Option<Target> {
        self.by_name.get(name)?.first().map(|&i| self.candidates[i as usize].target)
    }

    fn wikilink(&self, target: &str) -> Option<Target> {
        let t = normalize_target(target);
        if t.is_empty() {
            return None;
        }
        let with_md = format!("{t}.md");
        if let Some(found) = self.by_suffix(&with_md).or_else(|| self.by_suffix(&t)) {
            return Some(found);
        }
        // A path that matches nothing: fall back to the file name alone.
        let name = t.rsplit('/').next()?;
        if name.len() == t.len() {
            return None;
        }
        self.by_name_only(&format!("{name}.md")).or_else(|| self.by_name_only(name))
    }

    fn markdown(&self, note_rel: &str, target: &str) -> Option<Target> {
        let t = target.trim().replace('\\', "/").to_lowercase();
        let from_root = clean_path(&t);
        let relative = if t.starts_with('/') {
            None
        } else {
            let folder = match note_rel.rfind('/') {
                Some(pos) => &note_rel[..pos],
                None => "",
            };
            clean_path(&format!("{}/{t}", folder.to_lowercase()))
        };
        for path in [relative, from_root].into_iter().flatten() {
            if path.is_empty() {
                continue;
            }
            if let Some(found) = self.exact(&path).or_else(|| self.exact(&format!("{path}.md"))) {
                return Some(found);
            }
        }
        None
    }
}

impl Graph {
    pub(super) fn build(inner: &Inner) -> Graph {
        let resolver = Resolver::new(inner);
        let mut resolved: Vec<Vec<Option<Target>>> = Vec::with_capacity(inner.files.len());
        let mut backlinks: HashMap<Target, Vec<(u32, u32)>> = HashMap::new();
        let mut missing: HashMap<String, usize> = HashMap::new();
        let mut unresolved: Vec<Unresolved> = Vec::new();
        let mut links = 0;
        // The same target is usually written many times: resolve it once.
        let mut wiki_cache: HashMap<&str, Option<Target>> = HashMap::new();

        for (id, file) in inner.files.iter().enumerate() {
            let mut row = Vec::with_capacity(file.links.len());
            if file.present {
                links += file.links.len();
                let me = Target::Note(id as u32);
                for (n, link) in file.links.iter().enumerate() {
                    let target = if link.markdown {
                        resolver.markdown(&file.rel, &link.target)
                    } else {
                        *wiki_cache.entry(link.target.as_str()).or_insert_with(|| resolver.wikilink(&link.target))
                    };
                    match target {
                        Some(t) => {
                            if t != me {
                                backlinks.entry(t).or_default().push((id as u32, n as u32));
                            }
                        }
                        None => {
                            // Grouped by the exact text, so that the target
                            // can be found again among the outgoing links.
                            let slot = *missing.entry(link.target.clone()).or_insert_with(|| {
                                unresolved.push(Unresolved { target: link.target.clone(), count: 0, sources: Vec::new() });
                                unresolved.len() - 1
                            });
                            let entry = &mut unresolved[slot];
                            entry.count += 1;
                            if entry.sources.len() < 5 && !entry.sources.contains(&(id as u32)) {
                                entry.sources.push(id as u32);
                            }
                        }
                    }
                    row.push(target);
                }
            }
            resolved.push(row);
        }
        unresolved.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.target.to_lowercase().cmp(&b.target.to_lowercase()))
                .then_with(|| a.target.cmp(&b.target))
        });
        let by_rel = resolver
            .by_rel
            .iter()
            .map(|(rel, &i)| (rel.clone(), resolver.candidates[i as usize].target))
            .collect();
        Graph { stamp: (inner.epoch, inner.graph_generation), resolved, backlinks, by_rel, unresolved, links }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_cleaning() {
        assert_eq!(clean_path("a/b/../c/./d.md").as_deref(), Some("a/c/d.md"));
        assert_eq!(clean_path("a/../b.md").as_deref(), Some("b.md"));
        assert_eq!(clean_path("/a//b/").as_deref(), Some("a/b"));
        assert_eq!(clean_path("../x.md"), None);
        assert_eq!(clean_path("a/../../x.md"), None);
        assert_eq!(normalize_target("  ./Dossier\\Ma Note/ "), "dossier/ma note");
        assert_eq!(normalize_target("/Racine/N"), "racine/n");
    }
}
