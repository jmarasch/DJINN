//! Checks genre tags against DJINN's genre web: which it knows, where each
//! lands, and the ones it doesn't know yet, most used first.
//!
//! Reads lines of `count<TAB>tag` on stdin, for example from a COPY of a
//! database: `sqlite3 copy.db "select count(*), genre from tracks group by genre"
//! -separator $'\t' | cargo run -p djinn-library --example genre_web`.
//! With `--tree` it prints the whole web instead.

use std::io::BufRead;

use djinn_library::genre_web::Web;

fn main() {
    let web = Web::builtin();
    if std::env::args().any(|a| a == "--tree") {
        for g in web.genres() {
            let pad = "  ".repeat(usize::from(g.depth));
            println!("{pad}{}", g.name);
        }
        println!("{} genres", web.genres().len());
        return;
    }
    let (mut known, mut total) = (0u64, 0u64);
    let mut unknown = Vec::new();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Some((count, tags)) = line.split_once('\t') else {
            continue;
        };
        let count: u64 = count.trim().parse().unwrap_or(0);
        let tag = tags.trim();
        if tag.is_empty() {
            continue;
        }
        total += count;
        if web.find_all(tag).is_empty() {
            unknown.push((count, tag.to_owned()));
        } else {
            known += count;
        }
    }
    unknown.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (count, tag) in unknown.iter().take(60) {
        println!("{count:>7}  {tag}");
    }
    let share = if total == 0 {
        0.0
    } else {
        known as f64 * 100.0 / total as f64
    };
    println!(
        "{share:.1}% of {total} tagged songs land in the web; {} tags unknown",
        unknown.len()
    );
}
