#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell of Rocksy: thin wrappers around `rocksy-core` (see CONTRACT.md).
//! No logic lives here beyond threading, events and window plumbing.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rocksy_core as core;
use rocksy_core::{ApiError, BuildOptions, CancelToken, Fragment, Session, TagCount, TagIndex, Tree, VaultRef};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

type ApiResult<T> = Result<T, ApiError>;

struct AppState {
    data_dir: PathBuf,
    arg_files: Vec<String>,
    process_start_ms: u64,
    tags: Arc<TagIndex>,
    /// Vault currently indexed for tags, with the token cancelling its build.
    tags_job: Mutex<Option<(String, CancelToken)>>,
    /// Background thread reconciling the tag index, joined at exit so its work is persisted.
    tags_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Open files and their last known mtime, watched for external changes.
    watched: Mutex<HashMap<String, u64>>,
    /// Serialises appends to the timings log (commands run concurrently).
    timing_log: Mutex<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupInfo {
    arg_files: Vec<String>,
    vaults: Vec<VaultRef>,
    session: Option<Session>,
    data_dir: String,
    version: String,
    process_start_ms: u64,
}

#[derive(Clone, Serialize)]
struct OpenFiles {
    paths: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChanged {
    path: String,
    mtime_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Command-line arguments that designate files, made absolute against `cwd`.
fn files_from_args(args: &[String], cwd: &Path) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with("--") && !a.is_empty())
        .filter_map(|a| {
            let p = Path::new(a);
            let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
            core::paths::absolute_string(&abs).ok()
        })
        .collect()
}

fn mtime_of(path: &str) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| core::paths::mtime_ms(&m))
}

#[tauri::command]
fn startup_info(state: State<'_, AppState>) -> StartupInfo {
    StartupInfo {
        arg_files: state.arg_files.clone(),
        vaults: core::list_vaults(&state.data_dir),
        session: core::load_session(&state.data_dir),
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION").into(),
        process_start_ms: state.process_start_ms,
    }
}

#[tauri::command(async)]
fn read_file(state: State<'_, AppState>, path: String) -> ApiResult<core::FileContent> {
    let content = core::read_file(Path::new(&path))?;
    lock(&state.watched).insert(content.path.clone(), content.mtime_ms);
    Ok(content)
}

#[tauri::command(async)]
fn save_file(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_mtime_ms: Option<u64>,
) -> ApiResult<core::SaveResult> {
    let saved = core::save_file(Path::new(&path), &text, expected_mtime_ms)?;
    if let Ok(abs) = core::paths::absolute_string(Path::new(&path)) {
        lock(&state.watched).insert(abs, saved.mtime_ms);
    }
    // Keep the tag index in sync when the file belongs to the indexed vault.
    let indexed = lock(&state.tags_job).as_ref().map(|(root, _)| root.clone());
    if let Some(root) = indexed {
        if Path::new(&path).starts_with(&root) {
            // With size and mtime recorded, the next reconciliation will not reread this file.
            state.tags.update_file_with_meta(Path::new(&path), &text, saved.size, saved.mtime_ms);
        }
    }
    Ok(saved)
}

#[tauri::command(async)]
fn list_vaults(state: State<'_, AppState>) -> Vec<VaultRef> {
    core::list_vaults(&state.data_dir)
}

#[tauri::command(async)]
fn add_vault(state: State<'_, AppState>, path: String) -> ApiResult<Vec<VaultRef>> {
    core::add_vault(&state.data_dir, Path::new(&path))
}

#[tauri::command(async)]
fn remove_vault(state: State<'_, AppState>, path: String) -> ApiResult<Vec<VaultRef>> {
    core::remove_vault(&state.data_dir, Path::new(&path))
}

#[tauri::command(async)]
fn open_vault(app: AppHandle, state: State<'_, AppState>, path: String) -> ApiResult<Tree> {
    let tree = core::open_tree(&state.data_dir, Path::new(&path))?;
    if tree.from_snapshot {
        // The snapshot may be stale: list again off the UI path and push a diff if any.
        let data_dir = state.data_dir.clone();
        std::thread::spawn(move || {
            let _ = core::reconcile_tree(&data_dir, Path::new(&path), |fresh| {
                let _ = app.emit("tree-updated", fresh);
            });
        });
    }
    Ok(tree)
}

/// Payload of `tags-ready`: the contract fields plus where the content comes from.
fn tags_ready_payload(files: usize, tags: usize, occurrences: usize, ms: u64, extra: serde_json::Value) -> serde_json::Value {
    let mut payload = serde_json::json!({ "files": files, "tags": tags, "occurrences": occurrences, "ms": ms });
    if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
        dst.extend(src.clone());
    }
    payload
}

/// Makes the tags of a vault available: first from the persisted index (no note
/// is read), then a background reconciliation rereads only the changed notes.
#[tauri::command(async)]
fn build_tags(app: AppHandle, state: State<'_, AppState>, vault_path: String) {
    let cancel = CancelToken::new();
    if let Some((_, previous)) = lock(&state.tags_job).replace((vault_path.clone(), cancel.clone())) {
        previous.cancel();
    }
    let index = state.tags.clone();
    let data_dir = state.data_dir.clone();
    let handle = std::thread::spawn(move || {
        let vault = Path::new(&vault_path);
        // Another vault's content must not be served (nor saved) under this one.
        if index.root().is_some_and(|root| !Path::new(&root).eq(vault)) {
            let _ = index.save(&data_dir);
            index.clear();
        }
        if let Some(ready) = index.load_persisted(&data_dir, vault) {
            let _ = app.emit(
                "tags-ready",
                tags_ready_payload(ready.files, ready.tags, ready.occurrences, ready.ms, serde_json::json!({ "fromCache": true })),
            );
        }
        let progress_app = app.clone();
        let last_emit = std::sync::atomic::AtomicU64::new(0);
        let result = index.reconcile(&data_dir, vault, BuildOptions::default(), &cancel, move |p| {
            // Throttled by time (workers report concurrently): the UI only needs a coarse progress bar.
            let now = now_ms();
            let last = last_emit.load(std::sync::atomic::Ordering::Relaxed);
            if p.done == p.total || now.saturating_sub(last) >= 100 {
                last_emit.store(now, std::sync::atomic::Ordering::Relaxed);
                let _ = progress_app.emit("tags-progress", p);
            }
        });
        match result {
            Ok(Some(r)) => {
                let extra = serde_json::json!({ "fromCache": false, "reread": r.reread, "removed": r.removed, "persisted": r.persisted });
                let _ = app.emit("tags-ready", tags_ready_payload(r.files, r.tags, r.occurrences, r.ms, extra));
            }
            // Cancelled (vault switch or exit): keep what was already reread for next time.
            Ok(None) => {
                let _ = index.save(&data_dir);
            }
            Err(_) => {}
        }
    });
    *lock(&state.tags_thread) = Some(handle);
}

#[tauri::command(async)]
fn tags_list(state: State<'_, AppState>, query: String) -> Vec<TagCount> {
    state.tags.tags_list(&query)
}

#[tauri::command(async)]
fn tag_fragments(state: State<'_, AppState>, tag: String, limit: Option<usize>) -> Vec<Fragment> {
    // Unbounded results can reach megabytes of JSON on a frequent tag.
    state.tags.tag_fragments(&tag, Some(limit.unwrap_or(500)))
}

#[tauri::command(async)]
fn file_tags(path: String, text: String) -> Vec<Fragment> {
    core::file_tags(&path, &text)
}

#[tauri::command(async)]
fn set_session(state: State<'_, AppState>, session: Session) -> ApiResult<()> {
    // Only the open tabs are watched for external changes.
    {
        let mut watched = lock(&state.watched);
        watched.retain(|p, _| session.tabs.iter().any(|t| t == p));
        for tab in &session.tabs {
            if !watched.contains_key(tab) {
                if let Some(m) = mtime_of(tab) {
                    watched.insert(tab.clone(), m);
                }
            }
        }
    }
    core::save_session(&state.data_dir, &session)
}

#[tauri::command(async)]
fn pick_folder(app: AppHandle) -> Option<String> {
    app.dialog().file().blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command(async)]
fn pick_file(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Startup milestones measured by the UI, appended to `<data_dir>/timings.jsonl`.
#[tauri::command(async)]
fn log_timing(state: State<'_, AppState>, name: String, ms: f64) {
    let _guard = lock(&state.timing_log);
    let file = state.data_dir.join("timings.jsonl");
    if std::fs::metadata(&file).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&file);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let line = serde_json::json!({
            "name": name,
            "ms": ms,
            "sinceProcessStartMs": now_ms().saturating_sub(state.process_start_ms),
            "processStartMs": state.process_start_ms,
        });
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// Opens a link in the system browser or mail client. Only web and mail links:
/// a note must not be able to launch an arbitrary program or file.
#[tauri::command(async)]
fn open_external(url: String) -> ApiResult<()> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")) {
        return Err(ApiError::invalid(format!("unsupported link scheme: {url}")));
    }
    tauri_plugin_opener::open_url(url.trim(), None::<&str>).map_err(|e| ApiError::io(format!("cannot open link: {e}")))
}

fn emit_open_files(app: &AppHandle, paths: Vec<String>) {
    if !paths.is_empty() {
        let _ = app.emit("open-files", OpenFiles { paths });
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Persists the tag index a few seconds after it changed (a note was saved).
fn spawn_tags_saver(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(5000));
        let state = app.state::<AppState>();
        if state.tags.is_dirty() {
            let _ = state.tags.save(&state.data_dir);
        }
    });
}

/// Stops the background reconciliation and persists the tag index before the process exits.
fn flush_tags(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some((_, cancel)) = lock(&state.tags_job).as_ref() {
        cancel.cancel();
    }
    if let Some(handle) = lock(&state.tags_thread).take() {
        let _ = handle.join();
    }
    let _ = state.tags.save(&state.data_dir);
}

/// Polls the mtime of open files; cheap (a handful of `stat` calls) and good
/// enough until the core gets a real watcher.
fn spawn_file_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(2000));
        let state = app.state::<AppState>();
        let snapshot: Vec<(String, u64)> = lock(&state.watched).iter().map(|(p, m)| (p.clone(), *m)).collect();
        for (path, known) in snapshot {
            let Some(current) = mtime_of(&path) else { continue };
            if current != known {
                let mut watched = lock(&state.watched);
                // A save may have updated the entry while we were looking.
                if watched.get(&path) == Some(&known) {
                    watched.insert(path.clone(), current);
                    drop(watched);
                    let _ = app.emit("file-changed", FileChanged { path, mtime_ms: current });
                }
            }
        }
    });
}

fn main() {
    let process_start_ms = now_ms();
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let arg_files = files_from_args(&args, &cwd);

    tauri::Builder::default()
        // Must be the first plugin: a second launch forwards its arguments here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            emit_open_files(app, files_from_args(&argv, Path::new(&cwd)));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // ROCKSY_DATA_DIR isolates test runs from the real profile.
            let isolated = std::env::var_os("ROCKSY_DATA_DIR").map(PathBuf::from);
            let data_dir = isolated
                .clone()
                .or_else(|| app.path().app_data_dir().ok())
                .ok_or("no application data directory")?;
            std::fs::create_dir_all(&data_dir)?;

            // The window is created here rather than in tauri.conf.json so that an isolated
            // profile also isolates the WebView's own storage (UI preferences).
            let mut window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Rocksy")
                .inner_size(1280.0, 820.0)
                .min_inner_size(480.0, 320.0)
                .background_color(tauri::window::Color(0x14, 0x18, 0x1f, 0xff));
            if let Some(dir) = &isolated {
                window = window.data_directory(dir.join("webview"));
            }
            window.build()?;
            app.manage(AppState {
                data_dir,
                arg_files,
                process_start_ms,
                tags: Arc::new(TagIndex::new()),
                tags_job: Mutex::new(None),
                tags_thread: Mutex::new(None),
                watched: Mutex::new(HashMap::new()),
                timing_log: Mutex::new(()),
            });
            spawn_file_watcher(app.handle().clone());
            spawn_tags_saver(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let files: Vec<String> = paths
                    .iter()
                    .filter(|p| p.is_file())
                    .filter_map(|p| core::paths::absolute_string(p).ok())
                    .collect();
                emit_open_files(window.app_handle(), files);
            }
        })
        .invoke_handler(tauri::generate_handler![
            startup_info,
            read_file,
            save_file,
            list_vaults,
            add_vault,
            remove_vault,
            open_vault,
            build_tags,
            tags_list,
            tag_fragments,
            file_tags,
            set_session,
            pick_folder,
            pick_file,
            log_timing,
            open_external
        ])
        .build(tauri::generate_context!())
        .expect("error while running Rocksy")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                flush_tags(app);
            }
        });
}
