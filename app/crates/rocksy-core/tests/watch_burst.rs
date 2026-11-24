//! A burst larger than the batch cap against a real file system: the watcher
//! keeps up, reports a truncated batch and settles (BUG-20).

mod common;

use common::TempDir;
use rocksy_core::watch::MAX_BATCH_PATHS;
use rocksy_core::{watch_vault, ChangeBatch, WatchOptions};
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(60);

#[test]
fn burst_beyond_the_cap_yields_truncated_batches_and_settles() {
    let dir = TempDir::new("watch-burst");
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join("bulk")).unwrap();
    let (tx, rx) = mpsc::channel::<ChangeBatch>();
    let options = WatchOptions { debounce_ms: 150, max_wait_ms: 60_000 };
    let watcher = watch_vault(&root, options, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();

    let total = MAX_BATCH_PATHS + 500;
    for i in 0..total {
        fs::write(root.join("bulk").join(format!("n{i}.md")), b"x").unwrap();
    }

    let start = Instant::now();
    let mut truncated = false;
    let mut paths = 0;
    // Batches stop coming once the burst has been digested.
    while let Ok(batch) = rx.recv_timeout(if truncated { Duration::from_secs(2) } else { DEADLINE }) {
        assert!(batch.paths.len() <= MAX_BATCH_PATHS);
        assert_eq!(batch.markdown, batch.paths.len(), "only notes were written");
        truncated |= batch.truncated;
        paths += batch.paths.len();
        assert!(start.elapsed() < DEADLINE, "the watcher never settles");
    }
    assert!(truncated, "a burst of {total} files must be reported as truncated ({paths} paths seen)");
    drop(watcher);
}
