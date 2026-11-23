//! Headless measurement tool for rocksy-core. Every command prints one JSON
//! document on stdout; errors go to stderr with a non-zero exit code.
//!
//!   rocksy-cli tree <vault> [--data-dir D]
//!   rocksy-cli tags <vault> [--threads N] [--reps N]
//!   rocksy-cli tags-persist <vault> [--data-dir D] [--work-dir W] [--modify N] [--threads N]
//!   rocksy-cli watch <vault> [--seconds N] [--debounce-ms N] [--max-wait-ms N] [--max-paths N]
//!   rocksy-cli watch-bench <vault> [--work-dir W] [--burst N]
//!   rocksy-cli search <vault> <query> [--data-dir D] [--limit N] [--hold-seconds N]
//!   rocksy-cli links <vault> <file> [--data-dir D]
//!   rocksy-cli search-bench <vault> [--data-dir D] [--work-dir W] [--modify N]
//!   rocksy-cli fragments <file.md>

use rocksy_core::{
    file_tags, list_tree, load_tree_snapshot, open_tree, read_file, reconcile_tree,
    save_tree_snapshot, tags_index_path, tree_snapshot_path, ApiError, BuildOptions, CancelToken,
    watch_vault, ChangeBatch, ReconcileReport, TagIndex, WatchOptions,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage:
  rocksy-cli tree <vault> [--data-dir D]
  rocksy-cli tags <vault> [--threads N] [--reps N]
  rocksy-cli tags-persist <vault> [--data-dir D] [--work-dir W] [--modify N] [--threads N]
  rocksy-cli watch <vault> [--seconds N] [--debounce-ms N] [--max-wait-ms N] [--max-paths N]
  rocksy-cli watch-bench <vault> [--work-dir W] [--burst N]
  rocksy-cli search <vault> <query> [--data-dir D] [--limit N] [--hold-seconds N]
  rocksy-cli links <vault> <file> [--data-dir D]
  rocksy-cli search-bench <vault> [--data-dir D] [--work-dir W] [--modify N]
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

fn reconcile(index: &TagIndex, data: &Path, vault: &Path, threads: Option<usize>) -> Result<ReconcileReport, String> {
    index
        .reconcile(data, vault, BuildOptions { threads }, &CancelToken::new(), |_| {})
        .map_err(err)?
        .ok_or_else(|| "reconcile cancelled".to_string())
}

/// Copies the visible `.md` files of `vault` (and their folders) to `dest`.
/// The reference vault is only read.
fn copy_markdown(vault: &Path, dest: &Path) -> Result<Vec<PathBuf>, String> {
    let tree = list_tree(vault).map_err(err)?;
    let io = |e: std::io::Error| format!("copy failed: {e}");
    std::fs::create_dir_all(dest).map_err(io)?;
    let mut copied = Vec::new();
    for entry in &tree.entries {
        let mut to = dest.to_path_buf();
        entry.path.split('/').for_each(|c| to.push(c));
        if entry.is_dir {
            std::fs::create_dir_all(&to).map_err(io)?;
        } else if entry.path.to_lowercase().ends_with(".md") {
            let mut from = PathBuf::from(&tree.root);
            entry.path.split('/').for_each(|c| from.push(c));
            std::fs::copy(&from, &to).map_err(io)?;
            copied.push(to);
        }
    }
    Ok(copied)
}

fn cmd_tags_persist(
    vault: &Path,
    data_dir: Option<&str>,
    work_dir: Option<&str>,
    modify: usize,
    threads: Option<usize>,
) -> Result<Value, String> {
    let data = DataDir::new(data_dir);
    let io = |e: std::io::Error| format!("I/O error: {e}");

    // 1. Full build through reconcile (empty index, no persisted file).
    let root = rocksy_core::paths::absolute_string(vault).map_err(err)?;
    let file = tags_index_path(&data.path, &root);
    let _ = std::fs::remove_file(&file);
    let index = TagIndex::new();
    let start = Instant::now();
    let full = reconcile(&index, &data.path, vault, threads)?;
    let full_wall_ms = ms(start);
    let file_bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let stats = index.stats();

    // 2. save: the file is removed before each run so that there is something to write.
    let (save_median, save_max, saved) = bench(5, || {
        let _ = std::fs::remove_file(&file);
        index.save(&data.path)
    });
    if !saved.map_err(err)? {
        return Err("save did not write the file".into());
    }

    // 3. load_persisted into a new index.
    let (load_median, load_max, loaded) = bench(20, || {
        let fresh = TagIndex::new();
        let ready = fresh.load_persisted(&data.path, vault);
        (fresh, ready)
    });
    let (loaded_index, ready) = loaded;
    let ready = ready.ok_or("persisted index could not be loaded")?;
    let round_trip_ok = loaded_index.stats() == stats
        && loaded_index.tags_list("") == index.tags_list("")
        && index.tags_list("").iter().take(50).all(|t| {
            loaded_index.tag_fragments(&t.tag, Some(500)) == index.tag_fragments(&t.tag, Some(500))
        });
    let (list_median, list_max, _) = bench(20, || loaded_index.tags_list("").len());

    // 4. Reconcile with nothing changed, on the loaded index.
    let (same_median, same_max, same) = bench(5, || reconcile(&loaded_index, &data.path, vault, threads));
    let same = same?;

    // 5. Reconcile after modifying N files, on a temporary copy of the vault.
    let work = match work_dir {
        Some(w) => PathBuf::from(w).join(format!("rocksy-cli-work-{}", std::process::id())),
        None => std::env::temp_dir().join(format!("rocksy-cli-work-{}", std::process::id())),
    };
    let modified = (|| -> Result<Value, String> {
        let copy = work.join("vault");
        let copy_data = work.join("data");
        let start = Instant::now();
        let files = copy_markdown(vault, &copy)?;
        let copy_ms = ms(start);
        let session = TagIndex::new();
        let start = Instant::now();
        let first = reconcile(&session, &copy_data, &copy, threads)?;
        let first_wall_ms = ms(start);

        let n = modify.min(files.len());
        let mut rounds = Vec::new();
        let mut times = Vec::new();
        for round in 0..3 {
            // N files spread over the vault get one more tagged line.
            for k in 0..n {
                let target = &files[(k * files.len() / n.max(1) + round) % files.len()];
                let mut bytes = std::fs::read(target).map_err(io)?;
                bytes.extend_from_slice(format!("\nligne ajoutée #rocksy-cli/modif{round}\n").as_bytes());
                std::fs::write(target, bytes).map_err(io)?;
            }
            let start = Instant::now();
            let report = reconcile(&session, &copy_data, &copy, threads)?;
            let wall = ms(start);
            times.push(wall);
            rounds.push(json!({ "wallMs": wall, "report": report }));
        }
        times.sort_by(|a, b| a.total_cmp(b));
        // One file removed and one added.
        std::fs::remove_file(&files[0]).map_err(io)?;
        std::fs::write(copy.join("rocksy-cli-nouveau.md"), "#rocksy-cli/nouveau\n").map_err(io)?;
        let start = Instant::now();
        let add_remove = reconcile(&session, &copy_data, &copy, threads)?;
        let add_remove_ms = ms(start);
        // Baseline on the same copy: nothing changed (walk and comparison only).
        let (idle_median, idle_max, idle) = bench(3, || reconcile(&session, &copy_data, &copy, threads));
        let idle = idle?;
        Ok(json!({
            "noChangeOnCopyMs": { "median": idle_median, "max": idle_max, "reps": 3, "reread": idle.reread },
            "copiedMarkdownFiles": files.len(),
            "copyMs": copy_ms,
            "firstBuild": { "wallMs": first_wall_ms, "report": first },
            "modifiedFiles": n,
            "reconcileMs": { "median": times[times.len() / 2], "max": times[times.len() - 1], "reps": 3 },
            "rounds": rounds,
            "oneAddedOneRemoved": { "wallMs": add_remove_ms, "report": add_remove },
        }))
    })();
    let _ = std::fs::remove_dir_all(&work);
    let modified = modified?;

    Ok(json!({
        "command": "tags-persist",
        "vault": root,
        "dataDir": data.path,
        "threads": threads,
        "fullBuild": { "wallMs": full_wall_ms, "report": full },
        "filesWithTags": stats.files,
        "skipped": stats.skipped,
        "persistedBytes": file_bytes,
        "saveMs": { "median": save_median, "max": save_max, "reps": 5 },
        "loadMs": { "median": load_median, "max": load_max, "reps": 20, "reportedMs": ready.ms,
                    "files": ready.files, "tags": ready.tags, "occurrences": ready.occurrences },
        "roundTripOk": round_trip_ok,
        "tagsListAfterLoadMs": { "median": list_median, "max": list_max, "reps": 20 },
        "reconcileNoChangeMs": { "median": same_median, "max": same_max, "reps": 5, "report": same },
        "reconcileAfterChanges": modified,
    }))
}

/// Prints one JSON line per batch until `seconds` have passed (forever
/// without `--seconds`). Only reads the vault.
fn cmd_watch(vault: &Path, seconds: Option<usize>, options: WatchOptions, max_paths: usize) -> Result<Value, String> {
    let start = Instant::now();
    let batches = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&batches);
    let watcher = watch_vault(vault, options, move |batch| {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sample: Vec<&String> = batch.paths.iter().take(max_paths).collect();
        println!(
            "{}",
            json!({
                "tMs": ms(start), "paths": batch.paths.len(), "markdown": batch.markdown,
                "events": batch.events, "truncated": batch.truncated, "sample": sample,
            })
        );
    })
    .map_err(err)?;
    println!(
        "{}",
        json!({ "event": "ready", "root": watcher.root(), "setupMs": ms(start),
                "debounceMs": options.debounce_ms, "maxWaitMs": options.max_wait_ms })
    );
    match seconds {
        Some(s) => std::thread::sleep(std::time::Duration::from_secs(s as u64)),
        None => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
    }
    let stop = Instant::now();
    drop(watcher);
    Ok(json!({ "event": "done", "seconds": round3(start.elapsed().as_secs_f64()),
               "batches": batches.load(std::sync::atomic::Ordering::Relaxed), "dropMs": ms(stop) }))
}

/// Measures the watcher on a temporary copy of the vault's Markdown files:
/// set-up time, delay between a write and its batch, behaviour on a burst.
fn cmd_watch_bench(vault: &Path, work_dir: Option<&str>, burst: usize) -> Result<Value, String> {
    use std::time::Duration;
    let io = |e: std::io::Error| format!("I/O error: {e}");
    let options = WatchOptions::default();

    // Set-up and tear-down on the reference vault itself (read-only).
    let start = Instant::now();
    let reference = watch_vault(vault, options, |_| {}).map_err(err)?;
    let setup_reference_ms = ms(start);
    let start = Instant::now();
    drop(reference);
    let drop_reference_ms = ms(start);

    let base = work_dir.map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let work = base.join(format!("rocksy-cli-watch-{}", std::process::id()));
    let result = (|| -> Result<Value, String> {
        let copy = work.join("vault");
        let files = copy_markdown(vault, &copy)?;
        if files.is_empty() {
            return Err("no Markdown file to work on".into());
        }
        let (tx, rx) = std::sync::mpsc::channel::<(Instant, ChangeBatch)>();
        let start = Instant::now();
        let watcher = watch_vault(&copy, options, move |batch| {
            let _ = tx.send((Instant::now(), batch));
        })
        .map_err(err)?;
        let setup_copy_ms = ms(start);

        // One write at a time: delay until the batch that reports it.
        let mut delays = Vec::new();
        for round in 0..5 {
            let target = &files[(round * files.len() / 5) % files.len()];
            let wanted = target.to_string_lossy().into_owned();
            let mut bytes = std::fs::read(target).map_err(io)?;
            bytes.extend_from_slice(format!("\nécriture {round} #rocksy-cli/watch\n").as_bytes());
            std::fs::write(target, bytes).map_err(io)?;
            let written = Instant::now();
            loop {
                let (at, batch) = rx
                    .recv_timeout(Duration::from_secs(30))
                    .map_err(|_| format!("no batch within 30 s for {wanted}"))?;
                if batch.paths.contains(&wanted) {
                    delays.push(round3(at.duration_since(written).as_secs_f64() * 1000.0));
                    break;
                }
            }
            // Let the watcher go quiet before the next round.
            while rx.recv_timeout(Duration::from_millis(options.debounce_ms * 2)).is_ok() {}
        }
        let mut sorted = delays.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));

        // Burst: N files rewritten as fast as possible.
        let n = burst.min(files.len());
        let targets: Vec<&PathBuf> = (0..n).map(|k| &files[k * files.len() / n]).collect();
        let mut pending: std::collections::HashSet<String> =
            targets.iter().map(|t| t.to_string_lossy().into_owned()).collect();
        let burst_start = Instant::now();
        for target in &targets {
            let mut bytes = std::fs::read(target).map_err(io)?;
            bytes.extend_from_slice("\nrafale #rocksy-cli/rafale\n".as_bytes());
            std::fs::write(target, bytes).map_err(io)?;
        }
        let write_ms = ms(burst_start);
        let mut batches = Vec::new();
        let mut truncated = false;
        let mut last_batch_ms = 0.0;
        loop {
            // After everything was reported, wait a little for stragglers.
            let wait = if pending.is_empty() { Duration::from_millis(options.debounce_ms * 3) } else { Duration::from_secs(30) };
            match rx.recv_timeout(wait) {
                Ok((at, batch)) => {
                    last_batch_ms = round3(at.duration_since(burst_start).as_secs_f64() * 1000.0);
                    truncated |= batch.truncated;
                    for p in &batch.paths {
                        pending.remove(p);
                    }
                    batches.push(json!({ "tMs": last_batch_ms, "paths": batch.paths.len(), "events": batch.events, "truncated": batch.truncated }));
                }
                Err(_) => break,
            }
        }
        let start = Instant::now();
        drop(watcher);
        let drop_ms = ms(start);
        Ok(json!({
            "copiedMarkdownFiles": files.len(),
            "setupOnCopyMs": setup_copy_ms,
            "dropOnCopyMs": drop_ms,
            "singleWrite": { "delaysMs": delays, "medianMs": sorted[sorted.len() / 2], "maxMs": sorted[sorted.len() - 1],
                             "debounceMs": options.debounce_ms },
            "burst": { "files": n, "writeMs": write_ms, "batches": batches.len(), "lastBatchMs": last_batch_ms,
                       "missingPaths": pending.len(), "truncated": truncated, "detail": batches },
        }))
    })();
    let _ = std::fs::remove_dir_all(&work);
    let result = result?;
    Ok(json!({
        "command": "watch-bench",
        "vault": vault,
        "options": { "debounceMs": options.debounce_ms, "maxWaitMs": options.max_wait_ms },
        "setupOnReferenceMs": setup_reference_ms,
        "dropOnReferenceMs": drop_reference_ms,
        "onCopy": result,
    }))
}

fn dir_size(dir: &Path) -> (u64, usize) {
    let mut total = (0, 0);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let (bytes, files) = dir_size(&path);
                total = (total.0 + bytes, total.1 + files);
            } else if let Ok(meta) = entry.metadata() {
                total = (total.0 + meta.len(), total.1 + 1);
            }
        }
    }
    total
}

/// Resident memory of this process in kB (Linux only; `null` elsewhere).
fn resident_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Opens the persisted index of a vault as a new process would, then
/// reconciles. Returns the index and the timings.
fn open_index(vault: &Path, data: &Path) -> Result<(TagIndex, Value), String> {
    let index = TagIndex::new();
    let start = Instant::now();
    let loaded = index.load_persisted(data, vault);
    index.open_search(data, vault);
    let load_ms = ms(start);
    let start = Instant::now();
    let report = reconcile(&index, data, vault, None)?;
    let reconcile_ms = ms(start);
    let state = index.index_ready(false, report.ms, Some(&report));
    let timings = json!({
        "loadPersistedMs": load_ms, "fromCache": loaded.is_some(),
        "reconcileMs": reconcile_ms, "reconcile": report, "index": state,
    });
    Ok((index, timings))
}

fn cmd_search(vault: &Path, query: &str, data_dir: Option<&str>, limit: Option<usize>, hold: Option<usize>) -> Result<Value, String> {
    let data = DataDir::new(data_dir);
    let index = TagIndex::new();
    let start = Instant::now();
    let loaded = index.load_persisted(&data.path, vault);
    let load_ms = ms(start);
    let start = Instant::now();
    let ready = index.open_search(&data.path, vault);
    let open_search_ms = ms(start);
    // First query of the process, before any reconcile: the cold path.
    let start = Instant::now();
    let cold = index.search(query, limit);
    let cold_ms = ms(start);
    let start = Instant::now();
    let report = reconcile(&index, &data.path, vault, None)?;
    let reconcile_ms = ms(start);
    let result = index.search(query, limit);
    let resident = resident_kb();
    if let Some(seconds) = hold {
        std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
    }
    Ok(json!({
        "command": "search",
        "loadPersistedMs": load_ms, "fromCache": loaded.is_some(),
        "openSearchMs": open_search_ms, "searchReadyAfterOpen": ready,
        "coldSearch": { "ms": cold_ms, "total": cold.total, "searchReady": cold.search_ready },
        "reconcileMs": reconcile_ms, "reconcile": report,
        "index": index.index_ready(loaded.is_some(), report.ms, Some(&report)),
        "residentKb": resident,
        "result": result,
    }))
}

fn cmd_links(vault: &Path, file: &str, data_dir: Option<&str>) -> Result<Value, String> {
    let data = DataDir::new(data_dir);
    let (index, timings) = open_index(vault, &data.path)?;
    let path = if Path::new(file).is_absolute() { PathBuf::from(file) } else { vault.join(file) };
    Ok(json!({
        "command": "links",
        "path": path,
        "open": timings,
        "outgoing": index.outgoing_links(&path),
        "backlinks": index.backlinks(&path),
        "unresolvedInVault": index.unresolved_links(Some(20)),
    }))
}

/// Words of a text (letters and digits), lowercased, at least `min` long.
fn words(text: &str, min: usize) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= min && w.chars().all(char::is_alphabetic))
        .map(str::to_lowercase)
        .collect()
}

fn cmd_search_bench(vault: &Path, data_dir: Option<&str>, work_dir: Option<&str>, modify: usize) -> Result<Value, String> {
    let data = DataDir::new(data_dir);
    let io = |e: std::io::Error| format!("I/O error: {e}");
    let root = rocksy_core::paths::absolute_string(vault).map_err(err)?;

    // 1. Full indexing from nothing: tags, links and full text in one pass.
    let tags_file = tags_index_path(&data.path, &root);
    let search_dir = data.path.join("search").join(tags_file.file_stem().unwrap_or_default());
    let _ = std::fs::remove_file(&tags_file);
    let _ = std::fs::remove_dir_all(&search_dir);
    let index = TagIndex::new();
    let start = Instant::now();
    let full = reconcile(&index, &data.path, vault, None)?;
    let full_ms = ms(start);
    let state = index.index_ready(false, full.ms, Some(&full));
    let tags_bytes = std::fs::metadata(&tags_file).map(|m| m.len()).unwrap_or(0);
    let (search_bytes_after_build, search_files_after_build) = dir_size(&search_dir);
    let resident_after_build = resident_kb();

    // Sample notes to pick realistic query words and link targets.
    let tree = list_tree(vault).map_err(err)?;
    let notes: Vec<PathBuf> = tree
        .entries
        .iter()
        .filter(|e| !e.is_dir && e.path.to_lowercase().ends_with(".md"))
        .map(|e| {
            let mut p = PathBuf::from(&tree.root);
            e.path.split('/').for_each(|c| p.push(c));
            p
        })
        .collect();
    let mut candidates: Vec<String> = Vec::new();
    let mut phrase = String::new();
    for note in notes.iter().step_by((notes.len() / 12).max(1)).take(12) {
        if let Ok(text) = std::fs::read_to_string(note) {
            let w = words(&text, 5);
            if phrase.is_empty() {
                if let Some(pair) = w.windows(2).find(|p| text.to_lowercase().contains(&format!("{} {}", p[0], p[1]))) {
                    phrase = format!("\"{} {}\"", pair[0], pair[1]);
                }
            }
            candidates.extend(w.into_iter().take(40));
        }
    }
    candidates.sort();
    candidates.dedup();
    let mut counted: Vec<(usize, String)> =
        candidates.iter().map(|w| (index.search(&format!("\"{w}\""), Some(1)).total, w.clone())).filter(|(n, _)| *n > 0).collect();
    counted.sort();
    let frequent = counted.last().map(|c| c.1.clone()).unwrap_or_default();
    let rare = counted.first().map(|c| c.1.clone()).unwrap_or_default();
    let middle = counted.get(counted.len() / 2).map(|c| c.1.clone()).unwrap_or_default();
    let letter: String = frequent.chars().take(1).collect();
    let top_tag = index.tags_list("").first().map(|t| t.tag.clone()).unwrap_or_default();
    let folder = tree.entries.iter().find(|e| e.is_dir).map(|e| e.path.clone()).unwrap_or_default();
    let stem: String = frequent.chars().take(3).collect();
    let queries: Vec<(&str, String)> = vec![
        ("frequentWord", format!("\"{frequent}\"")),
        ("frequentWordAsPrefix", frequent.clone()),
        ("rareWord", rare.clone()),
        ("oneLetterPrefix", letter.clone()),
        ("oneLetterThenWord", format!("{letter} {middle}")),
        ("wordThenOneLetter", format!("{middle} {letter}")),
        ("threeLetterPrefix", stem),
        ("twoWords", format!("{frequent} {middle}")),
        ("phrase", phrase.clone()),
        ("tagFilter", format!("tag:{top_tag} {middle}")),
        ("pathFilter", format!("path:{folder} {frequent}")),
        ("exclusion", format!("-{rare} {frequent}")),
        ("tagOnly", format!("tag:{top_tag}")),
        ("noResult", "zzzzqqqq".to_string()),
    ];
    let measure = |index: &TagIndex| -> Vec<Value> {
        queries
            .iter()
            .map(|(name, query)| {
                let (median, max, result) = bench(20, || index.search(query, Some(50)));
                let (json_median, _, bytes) = bench(5, || serde_json::to_string(&index.search(query, Some(50))).map(|s| s.len()).unwrap_or(0));
                json!({ "name": name, "query": query, "total": result.total, "hits": result.hits.len(),
                        "medianMs": median, "maxMs": max, "withJsonMs": json_median, "jsonBytes": bytes })
            })
            .collect()
    };
    let queries_after_build = measure(&index);

    // 2. Links: the note with the most outgoing links among a sample, and
    // the most linked file among their targets.
    let mut busiest = notes.first().cloned().unwrap_or_default();
    let mut most = 0;
    let mut targets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for note in notes.iter().take(2000) {
        let out = index.outgoing_links(note);
        if out.len() > most {
            most = out.len();
            busiest = note.clone();
        }
        for link in out {
            if let Some(resolved) = link.resolved {
                *targets.entry(resolved).or_default() += 1;
            }
        }
    }
    let popular = targets.iter().max_by_key(|(_, n)| **n).map(|(p, _)| PathBuf::from(p)).unwrap_or_else(|| busiest.clone());
    let (out_median, out_max, out) = bench(100, || index.outgoing_links(&busiest));
    let (back_median, back_max, back) = bench(100, || index.backlinks(&popular));
    let (unres_median, unres_max, unres) = bench(20, || index.unresolved_links(Some(5000)));

    // 3. Reconcile with nothing changed.
    let (same_median, same_max, same) = bench(5, || reconcile(&index, &data.path, vault, None));
    let same = same?;
    drop(index);
    // Let background merges of the initial indexing settle before sizing.
    let (search_bytes, search_files) = dir_size(&search_dir);

    // 4. What a new process does: load, resolve links, first query.
    let mut loads = Vec::new();
    let mut opens = Vec::new();
    let mut graphs = Vec::new();
    let mut firsts = Vec::new();
    let mut resident_after_load = None;
    let mut reopened = None;
    for _ in 0..10 {
        drop(reopened.take());
        let fresh = TagIndex::new();
        let start = Instant::now();
        let loaded = fresh.load_persisted(&data.path, vault);
        loads.push(ms(start));
        if loaded.is_none() {
            return Err("persisted index could not be loaded".into());
        }
        let start = Instant::now();
        fresh.open_search(&data.path, vault);
        opens.push(ms(start));
        // The first link query builds the whole link graph.
        let start = Instant::now();
        let _ = fresh.backlinks(&popular);
        graphs.push(ms(start));
        let start = Instant::now();
        let _ = fresh.search(&frequent, Some(50));
        firsts.push(ms(start));
        resident_after_load = resident_kb();
        reopened = Some(fresh);
    }
    let stat = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        json!({ "median": v[v.len() / 2], "max": v[v.len() - 1], "reps": v.len() })
    };
    let reopened = reopened.ok_or("no index")?;
    let ready_after_load = reopened.search_ready();
    let queries_after_load = measure(&reopened);
    drop(reopened);

    // 5. On a temporary copy: reconcile after N modified notes, and the
    // cost of reflecting one save.
    let base = work_dir.map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let work = base.join(format!("rocksy-cli-search-{}", std::process::id()));
    let changed = (|| -> Result<Value, String> {
        let copy = work.join("vault");
        let copy_data = work.join("data");
        let files = copy_markdown(vault, &copy)?;
        let session = TagIndex::new();
        let start = Instant::now();
        let first = reconcile(&session, &copy_data, &copy, None)?;
        let first_ms = ms(start);
        let n = modify.min(files.len());
        let mut rounds = Vec::new();
        let mut times = Vec::new();
        for round in 0..3 {
            for k in 0..n {
                let target = &files[(k * files.len() / n.max(1) + round) % files.len()];
                let mut bytes = std::fs::read(target).map_err(io)?;
                bytes.extend_from_slice(format!("\nligne ajoutée rocksyclimodif{round} #rocksy-cli/modif{round} [[Lien ajouté {round}]]\n").as_bytes());
                std::fs::write(target, bytes).map_err(io)?;
            }
            let start = Instant::now();
            let report = reconcile(&session, &copy_data, &copy, None)?;
            let wall = ms(start);
            times.push(wall);
            let visible = session.search(&format!("rocksyclimodif{round}"), Some(1)).total;
            rounds.push(json!({ "wallMs": wall, "reread": report.reread, "unchanged": report.unchanged, "searchableAfter": visible }));
        }
        // One save by the application: text in memory into the three.
        let mut saves = Vec::new();
        for k in 0..10 {
            let target = &files[(k * 7919) % files.len()];
            let Ok(mut text) = std::fs::read_to_string(target) else { continue };
            text.push_str(&format!("\nenregistrement rocksyclisave{k}\n"));
            let saved = rocksy_core::save_file(target, &text, None).map_err(err)?;
            let start = Instant::now();
            session.update_file_with_meta(target, &text, saved.size, saved.mtime_ms);
            saves.push(ms(start));
            if session.search(&format!("rocksyclisave{k}"), Some(1)).total != 1 {
                return Err("a saved note was not searchable right away".into());
            }
        }
        let start = Instant::now();
        let after_saves = reconcile(&session, &copy_data, &copy, None)?;
        let after_saves_ms = ms(start);
        Ok(json!({
            "copiedMarkdownFiles": files.len(),
            "firstIndexing": { "wallMs": first_ms, "report": first },
            "modifiedFiles": n,
            "reconcileMs": stat(times),
            "rounds": rounds,
            "updateFileWithMetaMs": stat(saves),
            "reconcileAfterSaves": { "wallMs": after_saves_ms, "reread": after_saves.reread },
        }))
    })();
    let _ = std::fs::remove_dir_all(&work);
    let changed = changed?;

    Ok(json!({
        "command": "search-bench",
        "vault": root,
        "dataDir": data.path,
        "fullIndexing": { "wallMs": full_ms, "report": full, "index": state },
        "persisted": {
            "tagsAndLinksBytes": tags_bytes,
            "searchBytesRightAfterIndexing": search_bytes_after_build, "searchFilesRightAfterIndexing": search_files_after_build,
            "searchBytes": search_bytes, "searchFiles": search_files,
        },
        "residentKb": { "afterIndexing": resident_after_build, "afterLoad": resident_after_load },
        "queriesUsed": { "frequent": frequent, "rare": rare, "middle": middle, "phrase": phrase, "tag": top_tag, "folder": folder },
        "queriesAfterIndexing": queries_after_build,
        "links": {
            "outgoing": { "medianMs": out_median, "maxMs": out_max, "links": out.len() },
            "backlinks": { "medianMs": back_median, "maxMs": back_max, "links": back.len() },
            "unresolved": { "medianMs": unres_median, "maxMs": unres_max, "targets": unres.len() },
        },
        "reconcileNoChangeMs": { "median": same_median, "max": same_max, "reps": 5, "reread": same.reread, "persisted": same.persisted },
        "newProcess": {
            "loadPersistedMs": stat(loads), "openSearchMs": stat(opens), "linkGraphMs": stat(graphs), "firstSearchMs": stat(firsts),
            "searchReady": ready_after_load,
        },
        "queriesAfterLoad": queries_after_load,
        "onCopy": changed,
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
        "tags-persist" => cmd_tags_persist(
            &target,
            args.opt("data-dir"),
            args.opt("work-dir"),
            args.usize_opt("modify")?.unwrap_or(50),
            args.usize_opt("threads")?,
        ),
        "watch" => {
            let defaults = WatchOptions::default();
            let options = WatchOptions {
                debounce_ms: args.usize_opt("debounce-ms")?.map_or(defaults.debounce_ms, |v| v as u64),
                max_wait_ms: args.usize_opt("max-wait-ms")?.map_or(defaults.max_wait_ms, |v| v as u64),
            };
            cmd_watch(&target, args.usize_opt("seconds")?, options, args.usize_opt("max-paths")?.unwrap_or(20))
        }
        "watch-bench" => cmd_watch_bench(&target, args.opt("work-dir"), args.usize_opt("burst")?.unwrap_or(300)),
        "search" => {
            let query = args.positional.get(2).cloned().unwrap_or_default();
            cmd_search(&target, &query, args.opt("data-dir"), args.usize_opt("limit")?, args.usize_opt("hold-seconds")?)
        }
        "links" => {
            let file = args.positional.get(2).ok_or(USAGE)?;
            cmd_links(&target, file, args.opt("data-dir"))
        }
        "search-bench" => cmd_search_bench(
            &target,
            args.opt("data-dir"),
            args.opt("work-dir"),
            args.usize_opt("modify")?.unwrap_or(50),
        ),
        "fragments" => cmd_fragments(&target),
        other => Err(format!("unknown command: {other}\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(&raw) {
        Ok(value) => {
            if value.get("event").is_some() {
                println!("{value}"); // line-oriented output of `watch`
            } else {
                println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
