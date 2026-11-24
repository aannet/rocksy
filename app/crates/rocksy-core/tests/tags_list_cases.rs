//! `tags_list`: the sort and filter rule of CONTRACT.md, on a set of cases
//! shared with the UI (`testdata/tags_list_cases.json`, also played by
//! `ui/tests/tagcases.test.ts` against the filter the UI applies to its
//! cached tag list). Both sides must give the same lists.

mod common;

use common::TempDir;
use rocksy_core::{BuildOptions, CancelToken, TagCount, TagIndex};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    query: String,
    expected: Vec<String>,
}

#[derive(Deserialize)]
struct Cases {
    notes: Vec<String>,
    library: Vec<TagCount>,
    cases: Vec<Case>,
}

const CASES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/tags_list_cases.json"));

#[test]
fn tags_list_follows_the_shared_cases() {
    let cases: Cases = serde_json::from_str(CASES).unwrap();
    let dir = TempDir::new("tag-cases");
    for (i, note) in cases.notes.iter().enumerate() {
        dir.write(&format!("v/n{i:02}.md"), format!("{note}\n"));
    }
    let index = TagIndex::new();
    index.build(&dir.path().join("v"), BuildOptions::default(), &CancelToken::new(), |_| {}).unwrap();

    // ROCKSY_PRINT_TAG_CASES=1 prints what the core answers (to write the file).
    if std::env::var_os("ROCKSY_PRINT_TAG_CASES").is_some() {
        println!("LIBRARY {}", serde_json::to_string(&index.tags_list("")).unwrap());
        for case in &cases.cases {
            let names: Vec<String> = index.tags_list(&case.query).into_iter().map(|t| t.tag).collect();
            println!("CASE {}", serde_json::to_string(&(&case.query, names)).unwrap());
        }
        return;
    }

    assert_eq!(index.tags_list(""), cases.library, "tags_list(\"\")");
    assert!(cases.cases.len() >= 15);
    for case in &cases.cases {
        let listed = index.tags_list(&case.query);
        let names: Vec<&str> = listed.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(names, case.expected, "query {:?}", case.query);
        // The counts are those of the whole list.
        for tag in &listed {
            assert!(cases.library.contains(tag), "query {:?}: {tag:?}", case.query);
        }
    }
}
