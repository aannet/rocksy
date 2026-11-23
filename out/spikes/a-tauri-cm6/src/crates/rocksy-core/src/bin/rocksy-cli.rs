//! Headless measurement tool for rocksy-core. Every command prints one JSON
//! document on stdout; errors go to stderr with a non-zero exit code.
//!
//!   rocksy-cli tree <vault> [--data-dir D]
//!   rocksy-cli tags <vault> [--threads N] [--reps N]
//!   rocksy-cli fragments <file.md>

use rocksy_core::{
    file_tags, list_tree, load_tree_snapshot, open_tree, read_file, reconcile_tree,
    save_tree_snapshot, tree_snapshot_path, ApiError, BuildOptions, CancelToken, TagIndex,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage:
  rocksy-cli tree <vault> [--data-dir D]
  rocksy-cli tags <vault> [--threads N] [--reps N]
  rocksy-cli fragments <file.md>";

fn ms(start: Instant) -> f64 {
    round3(start.elapsed().as_secs_f64() * 1000.0)
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Runs `f` `reps` times: (median ms, max ms, last result).
fn bench<T>(reps: usize, mut f: impl FnMut() -> T) -> (f64, f64, T) {
    let mut times = Vec::with_capacity(reps);
    let mut last = None;
    for _ in 0..reps.max(1) {
        let start = Instant::now();
        let out = f();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        last = Some(out);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let median = times[times.len() / 2];
    let max = times[times.len() - 1];
    (round3(median), round3(max), last.expect("at least one repetition"))
}

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut options = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(name) = args[i].strip_prefix("--") {
            let value = args.get(i + 1).ok_or_else(|| format!("missing value for --{name}"))?;
            options.push((name.to_string(), value.clone()));
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    Ok(Args { positional, options })
}

impl Args {
    fn opt(&self, name: &str) -> Option<&str> {
        self.options.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
    fn usize_opt(&self, name: &str) -> Result<Option<usize>, String> {
        match self.opt(name) {
            None => Ok(None),
            Some(v) => v.parse().map(Some).map_err(|_| format!("--{name} expects a number, got {v}")),
        }
    }
}

/// Data directory for a measurement: the one given, or a temporary one that
/// is removed afterwards.
struct DataDir {
    path: PathBuf,
    temporary: bool,
}

impl DataDir {
    fn new(given: Option<&str>) -> Self {
        match given {
            Some(p) => Self { path: PathBuf::from(p), temporary: false },
            None => {
                let path = std::env::temp_dir().join(format!("rocksy-cli-{}", std::process::id()));
                Self { path, temporary: true }
            }
        }
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn err(e: ApiError) -> String {
    e.to_string()
}

fn cmd_tree(vault: &Path, data_dir: Option<&str>) -> Result<Value, String> {
    let data = DataDir::new(data_dir);

    // First listing separately: it is the one that pays for a cold cache.
    let start = Instant::now();
    let tree = list_tree(vault).map_err(err)?;
    let first_list_ms = ms(start);
    let (list_median, list_max, _) = bench(5, || list_tree(vault).map(|t| t.entries.len()));

    let (write_median, write_max, res) = bench(5, || save_tree_snapshot(&data.path, &tree));
    res.map_err(err)?;
    let snapshot_file = tree_snapshot_path(&data.path, &tree.root);
    let snapshot_bytes = std::fs::metadata(&snapshot_file).map(|m| m.len()).unwrap_or(0);

    let (load_median, load_max, loaded) = bench(50, || load_tree_snapshot(&data.path, vault));
    let loaded = loaded.ok_or("snapshot could not be read back")?;
    let round_trip_ok = loaded.entries == tree.entries;

    let (open_median, open_max, opened) = bench(20, || open_tree(&data.path, vault));
    let from_snapshot = opened.map_err(err)?.from_snapshot;

    let (reconcile_median, reconcile_max, changed) = bench(5, || reconcile_tree(&data.path, vault, |_| {}));
    let changed = changed.map_err(err)?;

    let json_start = Instant::now();
    let json_bytes = serde_json::to_string(&tree).map(|s| s.len()).unwrap_or(0);
    let json_ms = ms(json_start);

    let dirs = tree.entries.iter().filter(|e| e.is_dir).count();
    Ok(json!({
        "command": "tree",
        "vault": tree.root,
        "dataDir": data.path,
        "entries": tree.entries.len(),
        "dirs": dirs,
        "files": tree.entries.len() - dirs,
        "markdownFiles": tree.entries.iter().filter(|e| !e.is_dir && e.path.to_lowercase().ends_with(".md")).count(),
        "listFirstMs": first_list_ms,
        "listMs": { "median": list_median, "max": list_max, "reps": 5 },
        "snapshotWriteMs": { "median": write_median, "max": write_max, "reps": 5 },
        "snapshotBytes": snapshot_bytes,
        "snapshotLoadMs": { "median": load_median, "max": load_max, "reps": 50 },
        "snapshotRoundTripOk": round_trip_ok,
        "openTreeMs": { "median": open_median, "max": open_max, "reps": 20, "fromSnapshot": from_snapshot },
        "reconcileNoChangeMs": { "median": reconcile_median, "max": reconcile_max, "reps": 5, "changed": changed },
        "treeJson": { "bytes": json_bytes, "serializeMs": json_ms },
    }))
}

fn cmd_tags(vault: &Path, threads: Option<usize>, reps: usize) -> Result<Value, String> {
    let index = TagIndex::new();
    let start = Instant::now();
    let ready = index
        .build(vault, BuildOptions { threads }, &CancelToken::new(), |_| {})
        .map_err(err)?
        .ok_or("build cancelled")?;
    let build_wall_ms = ms(start);
    let stats = index.stats();

    let all = index.tags_list("");
    let top = all.first().map(|t| t.tag.clone()).unwrap_or_default();
    // Queries: everything, one letter, a prefix, a substring, no match.
    let mut queries: Vec<String> = vec![String::new()];
    let mut push = |q: String| {
        if !q.is_empty() && !queries.contains(&q) {
            queries.push(q);
        }
    };
    push(top.chars().take(1).collect());
    push(top.chars().take(3).collect());
    if let Some(nested) = all.iter().find(|t| t.tag.contains('/')) {
        let leaf = nested.tag.rsplit('/').next().unwrap_or("");
        push(leaf.chars().take(3).collect());
        push(nested.tag.clone());
    }
    push(top.chars().skip(1).take(3).collect());
    push("zzzz-no-match".to_string());

    let list_results: Vec<Value> = queries
        .iter()
        .map(|q| {
            let (median, max, out) = bench(reps, || index.tags_list(q));
            let (json_median, json_max, bytes) =
                bench(reps, || serde_json::to_string(&index.tags_list(q)).map(|s| s.len()).unwrap_or(0));
            json!({
                "query": q, "results": out.len(),
                "medianMs": median, "maxMs": max,
                "withJson": { "medianMs": json_median, "maxMs": json_max, "bytes": bytes },
            })
        })
        .collect();

    // Tags: the largest, the largest nested one, a median one, the rarest.
    let mut targets: Vec<String> = Vec::new();
    let mut add = |t: Option<&str>| {
        if let Some(t) = t {
            if !targets.iter().any(|x| x == t) {
                targets.push(t.to_string());
            }
        }
    };
    add(all.first().map(|t| t.tag.as_str()));
    add(all.iter().find(|t| t.tag.contains('/')).map(|t| t.tag.as_str()));
    add(all.get(all.len() / 2).map(|t| t.tag.as_str()));
    add(all.last().map(|t| t.tag.as_str()));

    let mut fragment_results: Vec<Value> = Vec::new();
    for tag in &targets {
        for limit in [None, Some(200)] {
            let (median, max, out) = bench(reps, || index.tag_fragments(tag, limit));
            let (json_median, json_max, bytes) = bench(reps, || {
                serde_json::to_string(&index.tag_fragments(tag, limit)).map(|s| s.len()).unwrap_or(0)
            });
            fragment_results.push(json!({
                "tag": tag, "limit": limit, "results": out.len(),
                "medianMs": median, "maxMs": max,
                "withJson": { "medianMs": json_median, "maxMs": json_max, "bytes": bytes },
            }));
        }
    }

    Ok(json!({
        "command": "tags",
        "vault": vault,
        "threads": threads,
        "reps": reps,
        "build": {
            "ms": ready.ms, "wallMs": build_wall_ms,
            "files": ready.files, "tags": ready.tags, "occurrences": ready.occurrences,
            "filesWithTags": stats.files, "skipped": stats.skipped,
        },
        "topTags": all.iter().take(10).collect::<Vec<_>>(),
        "tagsList": list_results,
        "tagFragments": fragment_results,
    }))
}

fn cmd_fragments(file: &Path) -> Result<Value, String> {
    let content = read_file(file).map_err(err)?;
    let start = Instant::now();
    let fragments = file_tags(&content.path, &content.text);
    let parse_ms = ms(start);
    Ok(json!({
        "command": "fragments",
        "path": content.path,
        "size": content.size,
        "parseMs": parse_ms,
        "count": fragments.len(),
        "fragments": fragments,
    }))
}

fn run(raw: &[String]) -> Result<Value, String> {
    let args = parse_args(raw)?;
    let command = args.positional.first().map(String::as_str).ok_or(USAGE)?;
    let target = args.positional.get(1).map(PathBuf::from).ok_or(USAGE)?;
    match command {
        "tree" => cmd_tree(&target, args.opt("data-dir")),
        "tags" => cmd_tags(&target, args.usize_opt("threads")?, args.usize_opt("reps")?.unwrap_or(100)),
        "fragments" => cmd_fragments(&target),
        other => Err(format!("unknown command: {other}\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(&raw) {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
