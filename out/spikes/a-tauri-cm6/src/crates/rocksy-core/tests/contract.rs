//! JSON shapes of the contract and an end-to-end pass through the public API,
//! plus the `rocksy-cli` binary.

mod common;

use common::{make_vault, TempDir};
use rocksy_core::{
    add_vault, open_tree, read_file, reconcile_tree, save_file, ApiError, BuildOptions,
    CancelToken, ErrorKind, TagIndex, TagsProgress, TagsReady, VaultRef,
};
use serde_json::{json, Value};
use std::process::Command;
use std::sync::{mpsc, Arc};

#[test]
fn error_and_payload_json_shapes() {
    for (kind, name) in [
        (ErrorKind::NotFound, "not_found"),
        (ErrorKind::NotUtf8, "not_utf8"),
        (ErrorKind::Conflict, "conflict"),
        (ErrorKind::Io, "io"),
        (ErrorKind::Invalid, "invalid"),
    ] {
        let value = serde_json::to_value(ApiError::new(kind, "msg")).unwrap();
        assert_eq!(value, json!({"kind": name, "message": "msg"}));
        let back: ApiError = serde_json::from_value(value).unwrap();
        assert_eq!(back.kind, kind);
    }
    assert_eq!(serde_json::to_value(TagsProgress { done: 3, total: 9 }).unwrap(), json!({"done": 3, "total": 9}));
    assert_eq!(
        serde_json::to_value(TagsReady { files: 1, tags: 2, occurrences: 3, ms: 4 }).unwrap(),
        json!({"files": 1, "tags": 2, "occurrences": 3, "ms": 4})
    );
    assert_eq!(
        serde_json::to_value(VaultRef { path: "/v".into(), name: "v".into() }).unwrap(),
        json!({"path": "/v", "name": "v"})
    );
}

#[test]
fn file_content_and_save_result_json() {
    let dir = TempDir::new("contract-file");
    let p = dir.write("n.md", "abc");
    let content = serde_json::to_value(read_file(&p).unwrap()).unwrap();
    let keys: Vec<&str> = content.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["mtimeMs", "path", "size", "text"]);
    let saved = serde_json::to_value(save_file(&p, "abcd", None).unwrap()).unwrap();
    assert_eq!(saved["size"], 4);
    assert!(saved["mtimeMs"].as_u64().unwrap() > 0);
    assert_eq!(saved.as_object().unwrap().len(), 2);
}

#[test]
fn background_flow_as_the_tauri_glue_will_run_it() {
    let dir = TempDir::new("contract-flow");
    let vault = make_vault(&dir, "vault");
    let data = dir.path().join("appdata");
    add_vault(&data, &vault).unwrap();

    // open_vault: tree now, reconcile on a thread, `tree-updated` if needed.
    let tree = open_tree(&data, &vault).unwrap();
    std::fs::write(vault.join("Nouveau.md"), "#nouveau\n").unwrap();
    let (tx, rx) = mpsc::channel();
    let (d, v) = (data.clone(), vault.clone());
    let handle = std::thread::spawn(move || reconcile_tree(&d, &v, |t| tx.send(t.clone()).unwrap()));
    assert!(handle.join().unwrap().unwrap());
    let updated = rx.recv().unwrap();
    assert_eq!(updated.entries.len(), tree.entries.len() + 1);

    // build_tags: shared index, built on a thread, events through a channel.
    let index = Arc::new(TagIndex::new());
    let (tx, rx) = mpsc::channel();
    let (idx, v) = (Arc::clone(&index), vault.clone());
    let cancel = CancelToken::new();
    let worker_cancel = cancel.clone();
    let handle = std::thread::spawn(move || {
        idx.build(&v, BuildOptions::default(), &worker_cancel, |p| tx.send(p).unwrap())
    });
    let ready = handle.join().unwrap().unwrap().unwrap();
    let progress: Vec<TagsProgress> = rx.iter().collect();
    assert_eq!(progress.last().unwrap().done, ready.files);
    assert_eq!(ready.files, 7);

    // Save a note, then reflect it in the index.
    let note = vault.join("Nouveau.md");
    let content = read_file(&note).unwrap();
    let text = "#nouveau/bis et #demo\n";
    save_file(&note, text, Some(content.mtime_ms)).unwrap();
    assert!(index.update_file(&note, text));
    let list = index.tags_list("nouv");
    assert_eq!(list.iter().map(|t| (t.tag.as_str(), t.count)).collect::<Vec<_>>(), [("nouveau", 1), ("nouveau/bis", 1)]);
    assert_eq!(index.tag_fragments("demo", Some(10)).len(), 2);
}

fn cli(args: &[&str]) -> (bool, Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rocksy-cli")).args(args).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    (out.status.success(), value, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn cli_commands_print_json() {
    let dir = TempDir::new("contract-cli");
    let vault = make_vault(&dir, "vault");
    let data = dir.path().join("cli-data");
    let before = common::fingerprint(&vault);

    let (ok, tree, stderr) = cli(&["tree", vault.to_str().unwrap(), "--data-dir", data.to_str().unwrap()]);
    assert!(ok, "{stderr}");
    assert_eq!(tree["command"], "tree");
    assert_eq!(tree["markdownFiles"], 6);
    assert_eq!(tree["snapshotRoundTripOk"], true);
    assert_eq!(tree["openTreeMs"]["fromSnapshot"], true);
    assert_eq!(tree["reconcileNoChangeMs"]["changed"], false);
    assert!(tree["snapshotBytes"].as_u64().unwrap() > 0);

    let (ok, tags, stderr) = cli(&["tags", vault.to_str().unwrap(), "--reps", "3", "--threads", "2"]);
    assert!(ok, "{stderr}");
    assert_eq!(tags["build"]["files"], 6);
    assert_eq!(tags["build"]["skipped"], 1);
    assert!(tags["tagsList"].as_array().unwrap().len() >= 3);
    assert!(!tags["tagFragments"].as_array().unwrap().is_empty());

    let note = vault.join("01 Travail/Reunions/Point.md");
    let (ok, fragments, stderr) = cli(&["fragments", note.to_str().unwrap()]);
    assert!(ok, "{stderr}");
    assert_eq!(fragments["count"], 2);
    assert_eq!(fragments["fragments"][0]["tag"], "reunion");
    assert_eq!(fragments["fragments"][0]["endLine"], 1);
    assert_eq!(fragments["fragments"][1]["startLine"], 5);

    // Errors: non-zero exit code, message on stderr, nothing on stdout.
    let (ok, value, stderr) = cli(&["fragments", vault.join("02 Perso/latin1.md").to_str().unwrap()]);
    assert!(!ok && value.is_null());
    assert!(stderr.starts_with("not_utf8"), "{stderr}");
    let (ok, _, stderr) = cli(&["nope"]);
    assert!(!ok && stderr.contains("usage"));

    assert_eq!(common::fingerprint(&vault), before, "measuring does not touch the vault");
}
