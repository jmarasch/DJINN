# DJINN's genre web

A family tree of genres, one file per family, with links across the tree.
Read by `djinn-library::genre_web` and built into the program.

## How a line is written

```text
Glam metal = hair metal, pop metal [1980-1992] < glam rock, hard rock ~ arena rock ! grunge
```

- Indent two spaces under the genre above it. A line with no indent is a family.
- `=` other names and spellings. Case, accents, hyphens, `&` and `'n'` don't matter
  ("Rock & Roll", "rock 'n' roll" and "Rock and roll" are one name).
- `[1980-1992]` the years it began and ended; `[1980-]` if it's still going.
- `<` what it grew out of, besides the genre it sits under.
- `~` genres it's close to or crosses over with. One direction is enough.
- `!` what it was a reaction against. A song in that genre never counts as a fit.
- `#` starts a comment.

Every part after the name can be left out. Links can name a genre in any file.

## Rules

- Every name and other name is used once in the whole web.
- Every link names a genre in the web. `cargo test -p djinn-library genre_web` checks both.
- The links are our own decisions. Check a link against more than one source and
  never copy another site's descriptions or lists.
- Put a genre where people would look for it, and use `<` and `~` for the rest of
  its history.

## Trying it

```sh
cargo run -p djinn-library --example genre_web -- --tree
sqlite3 -separator $'\t' COPY.db "select count(*), genre from tracks group by genre" \
  | cargo run -p djinn-library --example genre_web
```

The second command shows how many of a library's tagged songs land in the web and
lists the tags it doesn't know yet. Use a copy of a database, never the live one.
