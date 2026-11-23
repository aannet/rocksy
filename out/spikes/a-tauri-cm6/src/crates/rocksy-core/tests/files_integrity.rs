//! Data-integrity rules of `read_file` and `save_file` seen through the
//! public API: write extension rule and size limit on open.

mod common;

use common::TempDir;
use rocksy_core::files::MAX_READ_BYTES;
use rocksy_core::{read_file, save_file, ErrorKind};
use std::fs;

#[test]
fn save_accepts_only_markdown_extensions() {
    let dir = TempDir::new("files-ext");
    let txt = dir.write("notes.txt", "plain v1");
    let bare = dir.write("README", "bare v1");

    // Existing non-Markdown files: readable, never written.
    assert_eq!(read_file(&txt).unwrap().text, "plain v1");
    assert_eq!(read_file(&bare).unwrap().text, "bare v1");
    for p in [&txt, &bare] {
        let mtime = read_file(p).unwrap().mtime_ms;
        for expected in [None, Some(mtime)] {
            let err = save_file(p, "changed", expected).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Invalid, "{}", p.display());
            assert!(err.message.contains(".md"), "clear message expected: {}", err.message);
        }
    }
    assert_eq!(fs::read_to_string(&txt).unwrap(), "plain v1");
    assert_eq!(fs::read_to_string(&bare).unwrap(), "bare v1");

    // Creating a file with another extension is refused too, and leaves nothing.
    for name in ["x.cmd", "x.ps1", "x.lnk", ".bashrc", "x.md.bak", "x.mdx", "x.json", "noext"] {
        let err = save_file(&dir.path().join(name), "x", None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Invalid, "{name}");
        assert!(!dir.path().join(name).exists(), "{name}");
    }

    // Markdown, any case: created and overwritten.
    for name in ["a.md", "B.MD", "c.markdown", "D.MarkDown", "e.v2.md"] {
        let p = dir.path().join(name);
        let created = save_file(&p, "one\r\n", None).unwrap();
        save_file(&p, "two\r\n", Some(created.mtime_ms)).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two\r\n", "{name}");
    }

    let mut names: Vec<String> =
        fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["B.MD", "D.MarkDown", "README", "a.md", "c.markdown", "e.v2.md", "notes.txt"]);
}

#[test]
fn read_refuses_files_over_the_size_limit() {
    let dir = TempDir::new("files-size");
    // Sparse files: no real disk space or write time.
    let big = dir.path().join("huge.txt");
    fs::File::create(&big).unwrap().set_len(MAX_READ_BYTES + 1).unwrap();
    let err = read_file(&big).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Invalid);
    assert!(err.message.contains("too large") && err.message.contains("50.0 MiB"), "{}", err.message);

    // An ordinary large note is not affected.
    let note = dir.write("note.md", "x".repeat(400 * 1024));
    assert_eq!(read_file(&note).unwrap().size, 400 * 1024);
}
