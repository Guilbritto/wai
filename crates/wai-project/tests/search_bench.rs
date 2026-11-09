//! Opt-in timing of `fn ` over real trees (read-only): this workspace, and the folder in
//! `WAI_BENCH_DIR` when set:
//! `WAI_BENCH_DIR=<folder> cargo test --release -p wai-project --test search_bench -- --ignored --nocapture`

use std::path::Path;
use std::time::Instant;

use wai_project::{SearchQuery, search};

fn time(root: &Path) {
    for run in ["cold", "warm"] {
        let t = Instant::now();
        let (_handle, rx) = search(
            root,
            SearchQuery {
                pattern: "fn ".into(),
                regex: false,
                case_sensitive: false,
            },
        )
        .expect("valid pattern");
        let (mut first, mut batches, mut matches) = (None, 0, 0);
        while let Ok(batch) = rx.recv_blocking() {
            first.get_or_insert(t.elapsed());
            batches += 1;
            matches += batch.len();
        }
        println!(
            "{} {run}: first batch {first:?}, total {:?}, {batches} batches, {matches} matches",
            root.display(),
            t.elapsed()
        );
    }
}

#[test]
#[ignore = "timing only"]
fn search_fn_over_real_repos() {
    time(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    );
    match std::env::var_os("WAI_BENCH_DIR") {
        Some(dir) => time(Path::new(&dir)),
        None => println!("SKIPPED: set WAI_BENCH_DIR to also time a folder of your own"),
    }
}
