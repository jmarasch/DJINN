//! Compares every song's file tags with a database, printing counts (read
//! only on the files; point it at a COPY of a database, since opening one
//! applies migrations): `compare_library <db copy> [limit]`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use djinn_library::compare;
use djinn_store::Store;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let db = PathBuf::from(args.next().expect("db copy"));
    let limit: usize = args
        .next()
        .and_then(|a| a.parse().ok())
        .unwrap_or(usize::MAX);
    let store = Store::open(&db).await.expect("open");
    let rows = store.compare_rows().await.expect("rows");
    let edits = store.tag_edits_by_path().await.expect("edits");
    let identified = store.identified_paths().await.expect("ids");
    let years = djinn_library::original_years(&store).await.expect("years");
    let started = std::time::Instant::now();
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut songs = 0;
    let mut unreadable = 0;
    let mut samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows.iter().take(limit) {
        songs += 1;
        let Ok(file) = compare::read(Path::new(&row.path), years) else {
            unreadable += 1;
            continue;
        };
        for d in compare::differences(
            row,
            &file,
            edits.get(&row.path),
            identified.contains(&row.path),
        ) {
            let key = format!("{:?}", d.why);
            *counts.entry((d.field.to_owned(), key.clone())).or_default() += 1;
            let list = samples.entry(format!("{}/{}", d.field, key)).or_default();
            if list.len() < 3 {
                list.push(format!("db={:?} file={:?}", d.database, d.file));
            }
        }
    }
    println!(
        "{songs} songs, {unreadable} unreadable, {:?}",
        started.elapsed()
    );
    for ((field, why), n) in &counts {
        println!("{field:13} {why:11} {n}");
    }
    for (k, v) in &samples {
        println!("-- {k}");
        for s in v {
            println!("     {s}");
        }
    }
}
