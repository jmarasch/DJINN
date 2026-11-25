//! DJINN's own genre web: a family tree of genres (family, genre, style,
//! and so on down) with links across it for what a genre grew out of, what
//! it's close to, and what it was a reaction against.
//!
//! The web is written by hand in `genre_web/*.txt`, one file per family,
//! and read once. Each line is a genre, indented two spaces under its parent:
//!
//! ```text
//! Glam metal = hair metal, pop metal [1980-1992] < glam rock ~ arena rock ! grunge
//! ```
//!
//! `=` other names, `[from-until]` its years (`[1980-]` still going),
//! `<` what it grew out of besides its parent, `~` genres it's close to or
//! crosses over with, `!` what it was a reaction against. Every part after
//! the name may be left out, and links may name genres in any file.

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;

/// The families, one file each, in the order they're listed.
const FILES: &[(&str, &str)] = &[
    ("rock.txt", include_str!("../genre_web/rock.txt")),
    ("metal.txt", include_str!("../genre_web/metal.txt")),
    ("pop.txt", include_str!("../genre_web/pop.txt")),
    (
        "electronic.txt",
        include_str!("../genre_web/electronic.txt"),
    ),
    ("hip_hop.txt", include_str!("../genre_web/hip_hop.txt")),
    ("rnb.txt", include_str!("../genre_web/rnb.txt")),
    ("blues.txt", include_str!("../genre_web/blues.txt")),
    ("jazz.txt", include_str!("../genre_web/jazz.txt")),
    ("country.txt", include_str!("../genre_web/country.txt")),
    ("folk.txt", include_str!("../genre_web/folk.txt")),
    ("caribbean.txt", include_str!("../genre_web/caribbean.txt")),
    ("latin.txt", include_str!("../genre_web/latin.txt")),
    ("classical.txt", include_str!("../genre_web/classical.txt")),
    ("other.txt", include_str!("../genre_web/other.txt")),
];

/// How much a song counts per step it is below the genre asked for
/// ("deep house" for "house").
const BELOW: f32 = 0.95;
/// How much a song tagged one step above the genre asked for counts
/// ("metal" for "glam metal"), and per step further up.
const ABOVE: f32 = 0.55;
const ABOVE_STEP: f32 = 0.7;
/// Siblings (the same parent), and per step further apart.
const SIBLING: f32 = 0.45;
const COUSIN_STEP: f32 = 0.6;
/// Genres linked across the tree.
const NEAR: f32 = 0.7;
const GREW: f32 = 0.6;
/// A link through a parent counts this much of the direct link.
const THROUGH_PARENT: f32 = 0.8;

/// One genre in the web.
#[derive(Debug, Clone)]
pub struct Genre {
    /// The name to show.
    pub name: String,
    /// Other names and spellings, as written.
    pub aliases: Vec<String>,
    /// The genre above it; none for a family.
    pub parent: Option<usize>,
    /// The family it's in (itself for a family).
    pub family: usize,
    /// 0 for a family, 1 for the genres in it, and so on.
    pub depth: u8,
    /// The years it began and, if over, ended.
    pub years: Option<(i32, Option<i32>)>,
    /// What it grew out of besides its parent.
    pub from: Vec<usize>,
    /// Genres it's close to or crosses over with (both ways).
    pub near: Vec<usize>,
    /// What it was a reaction against.
    pub against: Vec<usize>,
    /// The genres right below it.
    pub children: Vec<usize>,
}

/// A mistake in the web's files.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("{file} line {line}: {why}")]
pub struct WebError {
    pub file: String,
    pub line: usize,
    pub why: String,
}

/// The whole web.
#[derive(Debug, Default)]
pub struct Web {
    genres: Vec<Genre>,
    by_key: HashMap<String, usize>,
}

/// Links still to look up once every file is read.
struct Pending {
    genre: usize,
    file: String,
    line: usize,
    from: Vec<String>,
    near: Vec<String>,
    against: Vec<String>,
}

/// A genre name as compared: lowercase, accents dropped, `&` as "and",
/// punctuation as spaces, "n" between words as "and" (rock 'n' roll), and a
/// trailing "music" dropped.
pub fn key(name: &str) -> String {
    let plain: String = name
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .replace(['&', '+'], " and ")
        .replace('\'', "")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = plain
        .split_whitespace()
        .map(|w| if w == "n" { "and" } else { w })
        .collect();
    if words.len() > 1 && words.last() == Some(&"music") {
        words.pop();
    }
    words.join(" ")
}

impl Web {
    /// The web DJINN ships with. Mistakes in its files are caught by the
    /// tests; if one slipped through anyway, the web is left empty.
    pub fn builtin() -> &'static Web {
        static WEB: OnceLock<Web> = OnceLock::new();
        WEB.get_or_init(|| match Web::parse(FILES) {
            Ok(web) => web,
            Err(errors) => {
                for e in errors {
                    tracing::warn!("genre web: {e}");
                }
                Web::default()
            }
        })
    }

    /// Reads the files (name, text), reporting every mistake found.
    pub fn parse(files: &[(&str, &str)]) -> Result<Web, Vec<WebError>> {
        let mut web = Web::default();
        let mut errors = Vec::new();
        let mut pending = Vec::new();
        for (file, text) in files {
            web.read_file(file, text, &mut pending, &mut errors);
        }
        for p in pending {
            let mut look = |names: &[String]| -> Vec<usize> {
                let mut ids = Vec::new();
                for n in names {
                    match web.find(n) {
                        Some(id) if id == p.genre => errors.push(WebError {
                            file: p.file.clone(),
                            line: p.line,
                            why: format!("links to itself: {n}"),
                        }),
                        Some(id) if !ids.contains(&id) => ids.push(id),
                        Some(_) => {}
                        None => errors.push(WebError {
                            file: p.file.clone(),
                            line: p.line,
                            why: format!("unknown genre: {n}"),
                        }),
                    }
                }
                ids
            };
            let from = look(&p.from);
            let near = look(&p.near);
            let against = look(&p.against);
            let g = &mut web.genres[p.genre];
            g.from = from;
            g.near = near;
            g.against = against;
        }
        if errors.is_empty() {
            Ok(web)
        } else {
            Err(errors)
        }
    }

    fn read_file(
        &mut self,
        file: &str,
        text: &str,
        pending: &mut Vec<Pending>,
        errors: &mut Vec<WebError>,
    ) {
        // The genre open at each depth, to find each line's parent.
        let mut open: Vec<usize> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = n + 1;
            let fail = |why: String| WebError {
                file: file.to_owned(),
                line,
                why,
            };
            let content = raw.split('#').next().unwrap_or("").trim_end();
            if content.trim().is_empty() {
                continue;
            }
            let indent = content.len() - content.trim_start().len();
            if content.starts_with('\t') || indent % 2 != 0 {
                errors.push(fail("indent with two spaces per level".into()));
                continue;
            }
            let depth = indent / 2;
            if depth > open.len() {
                errors.push(fail("indented more than one level".into()));
                continue;
            }
            let Some(parsed) = parse_line(content.trim()) else {
                errors.push(fail("couldn't read this line".into()));
                continue;
            };
            let id = self.genres.len();
            let mut names = vec![&parsed.name];
            names.extend(parsed.aliases.iter());
            let mut clash = false;
            for name in names {
                let k = key(name);
                if k.is_empty() {
                    errors.push(fail("an empty name".into()));
                    clash = true;
                } else if self.by_key.get(&k) == Some(&id) {
                    // Another spelling of a name it already has.
                } else if let Some(&other) = self.by_key.get(&k) {
                    errors.push(fail(format!(
                        "\"{name}\" is already {}",
                        self.genres[other].name
                    )));
                    clash = true;
                } else {
                    self.by_key.insert(k, id);
                }
            }
            if clash {
                // Keep the line's place so its children still find a parent.
                self.by_key.retain(|_, v| *v != id);
            }
            open.truncate(depth);
            let parent = open.last().copied();
            let family = parent.map_or(id, |p| self.genres[p].family);
            if let Some(p) = parent {
                self.genres[p].children.push(id);
            }
            self.genres.push(Genre {
                name: parsed.name,
                aliases: parsed.aliases,
                parent,
                family,
                depth: u8::try_from(depth).unwrap_or(u8::MAX),
                years: parsed.years,
                from: Vec::new(),
                near: Vec::new(),
                against: Vec::new(),
                children: Vec::new(),
            });
            open.push(id);
            if parsed.years_bad {
                errors.push(fail("years should look like [1980-1992] or [1980-]".into()));
            }
            pending.push(Pending {
                genre: id,
                file: file.to_owned(),
                line,
                from: parsed.from,
                near: parsed.near,
                against: parsed.against,
            });
        }
    }

    /// Every genre, families first in each file.
    pub fn genres(&self) -> &[Genre] {
        &self.genres
    }

    pub fn genre(&self, id: usize) -> &Genre {
        &self.genres[id]
    }

    /// The families (the tree's top level).
    pub fn families(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.genres.len()).filter(|&i| self.genres[i].parent.is_none())
    }

    /// The genre a tag or name means, by its name or any other name.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_key.get(&key(name)).copied()
    }

    /// Every genre a tag names: the tag as a whole, else each part of one
    /// that runs several together ("Rap/Hip Hop", "Heavy Metal; Hard Rock",
    /// "Hard Rock & Metal"), leaving out the parts it doesn't know.
    pub fn find_all(&self, tag: &str) -> Vec<usize> {
        if let Some(id) = self.find(tag) {
            return vec![id];
        }
        let mut ids = Vec::new();
        for part in tag.split(['/', ';', ',', '|', '\\']) {
            let found = match self.find(part) {
                Some(id) => vec![id],
                // "&" is part of names like "Rock & Roll", so it only
                // splits a part that isn't a name.
                None => {
                    let pieces: Vec<usize> = part
                        .split(['&', '+'])
                        .filter_map(|p| self.find(p))
                        .collect();
                    if pieces.is_empty() {
                        self.run_together(part)
                    } else {
                        pieces
                    }
                }
            };
            for id in found {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// Genres run together with nothing between them, as tags read after
    /// their slashes became spaces ("hard rock heavy metal"): the fewest
    /// names that use every word, or nothing if some word is left over.
    fn run_together(&self, text: &str) -> Vec<usize> {
        /// The longest name, in words, worth trying.
        const LONGEST: usize = 5;
        let k = key(text);
        let words: Vec<&str> = k.split(' ').filter(|w| !w.is_empty()).collect();
        if words.len() < 2 || words.len() > 12 {
            return Vec::new();
        }
        // best[i]: the fewest names covering the first i words.
        let mut best: Vec<Option<Vec<usize>>> = vec![None; words.len() + 1];
        best[0] = Some(Vec::new());
        for end in 1..=words.len() {
            for start in end.saturating_sub(LONGEST)..end {
                let (Some(before), Some(&id)) = (
                    best[start].as_ref(),
                    self.by_key.get(&words[start..end].join(" ")),
                ) else {
                    continue;
                };
                if best[end]
                    .as_ref()
                    .is_none_or(|b| before.len() + 1 < b.len())
                {
                    let mut found = before.clone();
                    found.push(id);
                    best[end] = Some(found);
                }
            }
        }
        let mut found = best[words.len()].take().unwrap_or_default();
        if found.len() < 2 {
            return Vec::new();
        }
        found.dedup();
        found
    }

    /// The genre and every genre above it, up to its family.
    pub fn lineage(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(id), |&g| self.genres[g].parent)
    }

    /// The genre's place at `depth` (0 is its family): itself when it isn't
    /// that deep. What a slider set to that depth folds it into.
    pub fn at_depth(&self, id: usize, depth: u8) -> usize {
        self.lineage(id)
            .find(|&g| self.genres[g].depth <= depth)
            .unwrap_or(id)
    }

    /// How well a song tagged `song` fits a request for `wanted`, from 0
    /// (not at all) to 1 (the same genre). A genre below the one asked for
    /// fits nearly fully; one above it, siblings and linked genres partly;
    /// a reaction against it not at all.
    pub fn fit(&self, song: usize, wanted: usize) -> f32 {
        if song == wanted {
            return 1.0;
        }
        let (s, w) = (&self.genres[song], &self.genres[wanted]);
        if s.against.contains(&wanted) || w.against.contains(&song) {
            return 0.0;
        }
        self.tree_fit(song, wanted).max(self.link_fit(song, wanted))
    }

    fn tree_fit(&self, song: usize, wanted: usize) -> f32 {
        if self.genres[song].family != self.genres[wanted].family {
            return 0.0;
        }
        // Steps up from each to where their lines meet.
        let up_song = self.lineage(song).position(|g| self.is_above(g, wanted));
        let Some(up_song) = up_song else {
            return 0.0;
        };
        let meet = self.lineage(song).nth(up_song).unwrap_or(song);
        let up_wanted = self.lineage(wanted).position(|g| g == meet).unwrap_or(0);
        let steps = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        match (up_song, up_wanted) {
            // The song is below the genre asked for.
            (down, 0) => BELOW.powi(steps(down)),
            // The song is above it.
            (0, up) => ABOVE * ABOVE_STEP.powi(steps(up) - 1),
            // Side by side; meeting only at the family counts little.
            (a, b) => {
                let fit = SIBLING * COUSIN_STEP.powi(steps(a + b - 2));
                if self.genres[meet].parent.is_none() {
                    fit * 0.5
                } else {
                    fit
                }
            }
        }
    }

    /// Whether `upper` is `id` or above it.
    fn is_above(&self, upper: usize, id: usize) -> bool {
        self.lineage(id).any(|g| g == upper)
    }

    fn link_fit(&self, song: usize, wanted: usize) -> f32 {
        let mut best = 0.0f32;
        for (i, s) in self.lineage(song).take(2).enumerate() {
            for (j, w) in self.lineage(wanted).take(2).enumerate() {
                let direct = self.direct_link(s, w);
                if direct > 0.0 {
                    let through = THROUGH_PARENT.powi(i32::try_from(i + j).unwrap_or(2));
                    best = best.max(direct * through);
                }
            }
        }
        best
    }

    fn direct_link(&self, a: usize, b: usize) -> f32 {
        let (ga, gb) = (&self.genres[a], &self.genres[b]);
        if ga.near.contains(&b) || gb.near.contains(&a) {
            NEAR
        } else if ga.from.contains(&b) || gb.from.contains(&a) {
            GREW
        } else {
            0.0
        }
    }
}

/// One line of a file, read.
struct Line {
    name: String,
    aliases: Vec<String>,
    years: Option<(i32, Option<i32>)>,
    years_bad: bool,
    from: Vec<String>,
    near: Vec<String>,
    against: Vec<String>,
}

fn parse_line(text: &str) -> Option<Line> {
    let mut line = Line {
        name: String::new(),
        aliases: Vec::new(),
        years: None,
        years_bad: false,
        from: Vec::new(),
        near: Vec::new(),
        against: Vec::new(),
    };
    // Split at each marker, keeping which marker began each part.
    let mut parts: Vec<(char, String)> = vec![(' ', String::new())];
    let mut in_years = false;
    for c in text.chars() {
        match c {
            '[' if !in_years => {
                in_years = true;
                parts.push(('[', String::new()));
            }
            ']' if in_years => {
                in_years = false;
                parts.push((' ', String::new()));
            }
            '=' | '<' | '~' | '!' if !in_years => parts.push((c, String::new())),
            _ => parts.last_mut()?.1.push(c),
        }
    }
    let list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .collect()
    };
    for (i, (mark, body)) in parts.into_iter().enumerate() {
        match mark {
            ' ' if i == 0 => line.name = body.trim().to_owned(),
            ' ' => {
                if !body.trim().is_empty() {
                    return None;
                }
            }
            '=' => line.aliases.extend(list(&body)),
            '<' => line.from.extend(list(&body)),
            '~' => line.near.extend(list(&body)),
            '!' => line.against.extend(list(&body)),
            '[' => match parse_years(&body) {
                Some(y) => line.years = Some(y),
                None => line.years_bad = true,
            },
            _ => return None,
        }
    }
    if line.name.is_empty() || in_years {
        return None;
    }
    Some(line)
}

fn parse_years(text: &str) -> Option<(i32, Option<i32>)> {
    let (from, until) = text.split_once('-')?;
    let from = from.trim().parse().ok()?;
    let until = until.trim();
    if until.is_empty() {
        return Some((from, None));
    }
    let until: i32 = until.parse().ok()?;
    (until >= from).then_some((from, Some(until)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "\
Rock = rock music
  Hard rock [1966-] < blues rock
    Arena rock = stadium rock
  Blues rock [1963-]
  Grunge [1986-1997] ~ alternative metal ! glam metal
Metal = heavy metal
  Glam metal = hair metal [1980-1992] < hard rock ~ arena rock
  Alternative metal
";

    fn small() -> Web {
        Web::parse(&[("small.txt", SMALL)]).expect("parses")
    }

    #[test]
    fn keys_ignore_spelling() {
        assert_eq!(key("Rock 'n' Roll"), "rock and roll");
        assert_eq!(key("rock & roll"), "rock and roll");
        assert_eq!(key("Nü-Metal"), "nu metal");
        assert_eq!(key("Neue Deutsche Härte"), "neue deutsche harte");
        assert_eq!(key("R&B"), "r and b");
        assert_eq!(key("Rock Music"), "rock");
        assert_eq!(key("Music"), "music");
        assert_eq!(key("Oi!"), "oi");
    }

    #[test]
    fn reads_the_tree_and_links() {
        let web = small();
        let glam = web.find("Hair Metal").expect("alias");
        let g = web.genre(glam);
        assert_eq!(g.name, "Glam metal");
        assert_eq!(g.depth, 1);
        assert_eq!(g.years, Some((1980, Some(1992))));
        assert_eq!(web.genre(g.family).name, "Metal");
        assert_eq!(g.from, vec![web.find("hard rock").expect("hard rock")]);
        let arena = web.find("stadium rock").expect("arena");
        assert_eq!(web.genre(arena).depth, 2);
        assert_eq!(web.families().count(), 2);
        assert_eq!(
            web.genre(web.find("hard rock").expect("x")).years,
            Some((1966, None))
        );
    }

    #[test]
    fn splits_tags_that_name_several() {
        let web = small();
        let id = |n: &str| web.find(n).expect(n);
        assert_eq!(web.find_all("Hair Metal"), vec![id("glam metal")]);
        assert_eq!(
            web.find_all("Hard Rock/Heavy Metal"),
            vec![id("hard rock"), id("metal")]
        );
        assert_eq!(
            web.find_all("Blues Rock & Hard Rock; Polka"),
            vec![id("blues rock"), id("hard rock")]
        );
        assert!(web.find_all("Unknown Genre").is_empty());
        // Slashes already turned into spaces.
        assert_eq!(
            web.find_all("hard rock heavy metal"),
            vec![id("hard rock"), id("metal")]
        );
        assert_eq!(
            web.find_all("blues rock hard rock arena rock"),
            vec![id("blues rock"), id("hard rock"), id("arena rock")]
        );
        // Every word must belong to a genre.
        assert!(web.find_all("hard rock opera").is_empty());
    }

    #[test]
    fn folds_to_a_depth() {
        let web = small();
        let arena = web.find("arena rock").expect("arena");
        let hard = web.find("hard rock").expect("hard");
        let rock = web.find("rock").expect("rock");
        assert_eq!(web.at_depth(arena, 2), arena);
        assert_eq!(web.at_depth(arena, 1), hard);
        assert_eq!(web.at_depth(arena, 0), rock);
        assert_eq!(web.at_depth(hard, 5), hard);
    }

    #[test]
    fn fits_close_genres_better() {
        let web = small();
        let id = |n: &str| web.find(n).expect(n);
        let fit = |song: &str, wanted: &str| web.fit(id(song), id(wanted));
        assert_eq!(fit("glam metal", "glam metal"), 1.0);
        // Below the genre asked for counts nearly fully.
        assert!(fit("arena rock", "hard rock") > 0.9);
        // Above it, partly.
        let above = fit("hard rock", "arena rock");
        assert!(above > 0.3 && above < 0.7, "{above}");
        // Linked across families.
        assert!(fit("arena rock", "glam metal") >= NEAR - 1e-6);
        assert!(fit("hard rock", "glam metal") > 0.5);
        // A reaction against it doesn't count, even though both are rock.
        assert_eq!(fit("glam metal", "grunge"), 0.0);
        assert_eq!(fit("grunge", "glam metal"), 0.0);
        // Siblings count a little, unrelated families nothing.
        assert!(fit("blues rock", "hard rock") > 0.0);
        assert!(fit("blues rock", "hard rock") < fit("arena rock", "hard rock"));
        assert_eq!(fit("alternative metal", "blues rock"), 0.0);
    }

    #[test]
    fn reports_mistakes() {
        let bad = "\
Rock
   Odd indent
  Hard rock < nothing like this
  Hard Rock
    Way [1990-1980]
      Fine
        Too deep
            Much too deep
";
        let errors = Web::parse(&[("bad.txt", bad)]).expect_err("mistakes");
        let whys: Vec<(usize, &str)> = errors.iter().map(|e| (e.line, e.why.as_str())).collect();
        assert!(whys.contains(&(2, "indent with two spaces per level")));
        assert!(whys.iter().any(|(l, w)| *l == 4 && w.contains("already")));
        assert!(whys.iter().any(|(l, w)| *l == 5 && w.contains("years")));
        assert!(whys.contains(&(8, "indented more than one level")));
        assert!(whys
            .iter()
            .any(|(l, w)| *l == 3 && w.contains("unknown genre")));
    }

    #[test]
    fn the_builtin_web_has_no_mistakes() {
        let web = Web::parse(FILES).unwrap_or_else(|errors| {
            let list: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("{} mistakes:\n{}", list.len(), list.join("\n"))
        });
        assert!(web.genres().len() > 100);
        // A few everyday tags land where they should.
        let fam = |tag: &str| {
            let id = web.find(tag).unwrap_or_else(|| panic!("{tag}"));
            web.genre(web.genre(id).family).name.clone()
        };
        assert_eq!(fam("hair metal"), "Metal");
        assert_eq!(fam("Rock & Roll"), "Rock");
        assert_eq!(fam("post-grunge"), "Rock");
    }
}
