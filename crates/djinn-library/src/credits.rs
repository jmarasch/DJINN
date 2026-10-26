//! Splitting artist credits into individual artists.
//!
//! Proper multi-artist tags (ID3v2.4 multi-value, repeated Vorbis ARTIST
//! fields, Picard's ARTISTS) are used as they are. Most files have one
//! string instead, like "Zara Larsson & David Guetta" or "Song (feat. X)",
//! so those are split:
//!
//! - "feat.", "ft.", and "featuring" always split, in the artist or the
//!   title; those artists are marked featured.
//! - "&", ",", " x ", "vs.", "/", and ";" split too, except inside names on
//!   the never-split list ("Simon & Garfunkel", "Earth, Wind & Fire").

/// One credited artist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    pub featured: bool,
}

/// Band names that contain a separator and must stay whole. Admins can
/// replace this list in Rules.
pub const DEFAULT_KEEP_WHOLE: &[&str] = &[
    "Simon & Garfunkel",
    "Earth, Wind & Fire",
    "Mumford & Sons",
    "Hall & Oates",
    "Daryl Hall & John Oates",
    "Crosby, Stills, Nash & Young",
    "Crosby, Stills & Nash",
    "Emerson, Lake & Palmer",
    "Belle & Sebastian",
    "Sly & the Family Stone",
    "Kool & the Gang",
    "Peter, Paul and Mary",
    "Tyler, the Creator",
    "Iron & Wine",
    "Chase & Status",
    "Above & Beyond",
    "Sam & Dave",
    "Ashford & Simpson",
    "Brooks & Dunn",
    "Big & Rich",
    "Florence + the Machine",
    "Bob Marley & the Wailers",
    "Tom Petty and the Heartbreakers",
    "Huey Lewis & the News",
    "Prince & the Revolution",
    "Echo & the Bunnymen",
    "Siouxsie and the Banshees",
    "Derek & the Dominos",
    "Hootie & the Blowfish",
    "Me & My Monkey",
    "Axwell /\\ Ingrosso",
    "Dimitri Vegas & Like Mike",
    "Aly & Fila",
    "Bingo Players",
    "Lost Frequencies",
];

/// Separators between main artists.
const MAIN_SEPARATORS: &[&str] = &[" & ", ", ", " x ", " vs. ", " vs ", " / ", "; "];
/// Featured lists also split on "and" ("feat. A and B").
const FEATURED_SEPARATORS: &[&str] = &[" & ", ", ", " and ", " x ", " / ", "; "];
/// What introduces featured artists (matched case-insensitively).
const FEATURING: &[&str] = &["featuring", "feat.", "feat", "ft.", "ft"];

/// The artists credited on a track: from its artist tag values (one or
/// more), plus any featured in its title.
pub fn track_credits(artist_values: &[String], title: &str, keep_whole: &[String]) -> Vec<Credit> {
    let mut credits = Vec::new();
    for value in artist_values {
        // ID3v2.4 multi-value frames can arrive joined with NUL.
        for part in value.split('\0') {
            credits.extend(split_credit(part, keep_whole));
        }
    }
    if let Some((_, featured)) = split_featuring(title) {
        credits.extend(split_names(featured, FEATURED_SEPARATORS, keep_whole, true));
    }
    dedup(credits)
}

/// Splits one credit string, like "A & B feat. C".
pub fn split_credit(text: &str, keep_whole: &[String]) -> Vec<Credit> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let (main, featured) = match split_featuring(&text) {
        Some((main, featured)) => (main, Some(featured)),
        None => (text.as_str(), None),
    };
    let mut credits = split_names(main, MAIN_SEPARATORS, keep_whole, false);
    if let Some(featured) = featured {
        credits.extend(split_names(featured, FEATURED_SEPARATORS, keep_whole, true));
    }
    dedup(credits)
}

/// Splits "main feat. featured" (also "(feat. ...)" and "[ft. ...]").
/// The featured list ends at its closing bracket, or, without brackets,
/// before a following "(...)", "[...]", or " - " part like "(Remix)".
/// Returns `None` if nobody is featured.
fn split_featuring(text: &str) -> Option<(&str, &str)> {
    let lower = text.to_lowercase();
    // Byte offsets line up because lowercasing ASCII keeps lengths, and the
    // markers are ASCII; bail out on texts where it doesn't.
    if lower.len() != text.len() {
        return None;
    }
    let (at, end) = FEATURING
        .iter()
        .filter_map(|marker| {
            lower.match_indices(marker).find_map(|(at, _)| {
                let before = lower[..at].chars().next_back();
                let after = lower[at + marker.len()..].chars().next();
                let starts_word = matches!(before, Some(' ' | '(' | '['));
                let ends_word = matches!(after, Some(' ')) || marker.ends_with('.');
                (starts_word && ends_word).then_some((at, at + marker.len()))
            })
        })
        .min()?;
    let opener = text[..at].chars().next_back();
    let rest = &text[end..];
    let stop = match opener {
        Some('(') => rest.find(')'),
        Some('[') => rest.find(']'),
        _ => [" (", " [", " - "]
            .iter()
            .filter_map(|s| rest.find(s))
            .min(),
    }
    .unwrap_or(rest.len());
    let main = text[..at]
        .trim_end()
        .trim_end_matches(['(', '['])
        .trim_end();
    let featured = rest[..stop].trim();
    (!main.is_empty() && !featured.is_empty()).then_some((main, featured))
}

/// Splits on `separators`, keeping names on the keep-whole list intact.
fn split_names(
    text: &str,
    separators: &[&str],
    keep_whole: &[String],
    featured: bool,
) -> Vec<Credit> {
    // Swap protected names for placeholders so separators inside them are
    // left alone, then put them back.
    let mut text = text.to_owned();
    let mut protected = Vec::new();
    for name in keep_whole.iter().filter(|n| !n.is_empty()) {
        while let Some(at) = find_ignore_case(&text, name) {
            let placeholder = format!("\u{E000}{}\u{E001}", protected.len());
            protected.push(text[at..at + name.len()].to_owned());
            text.replace_range(at..at + name.len(), &placeholder);
        }
    }
    let mut parts = vec![text];
    for separator in separators {
        parts = parts
            .iter()
            .flat_map(|p| p.split(separator).map(str::to_owned).collect::<Vec<_>>())
            .collect();
    }
    parts
        .into_iter()
        .map(|part| {
            let mut name = part.trim().to_owned();
            for (i, original) in protected.iter().enumerate() {
                name = name.replace(&format!("\u{E000}{i}\u{E001}"), original);
            }
            name
        })
        .filter(|name| !name.is_empty())
        .map(|name| Credit { name, featured })
        .collect()
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.to_lowercase(), needle.to_lowercase());
    // Only trust offsets when lowercasing kept byte lengths.
    if h.len() != haystack.len() || n.len() != needle.len() {
        return haystack.find(needle);
    }
    h.find(&n)
}

/// First mention wins, so a main artist isn't repeated as featured.
fn dedup(credits: Vec<Credit>) -> Vec<Credit> {
    let mut seen = std::collections::HashSet::new();
    credits
        .into_iter()
        .filter(|c| seen.insert(c.name.to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keep() -> Vec<String> {
        DEFAULT_KEEP_WHOLE.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(credits: &[Credit]) -> Vec<(&str, bool)> {
        credits
            .iter()
            .map(|c| (c.name.as_str(), c.featured))
            .collect()
    }

    #[test]
    fn splits_collaborations() {
        let k = keep();
        assert_eq!(
            names(&split_credit("Zara Larsson & David Guetta", &k)),
            [("Zara Larsson", false), ("David Guetta", false)]
        );
        assert_eq!(
            names(&split_credit("Kygo x Imagine Dragons", &k)),
            [("Kygo", false), ("Imagine Dragons", false)]
        );
        assert_eq!(
            names(&split_credit("A, B & C", &k)),
            [("A", false), ("B", false), ("C", false)]
        );
    }

    #[test]
    fn featured_artists_are_marked() {
        let k = keep();
        assert_eq!(
            names(&split_credit("KREAM feat. ZOHARA", &k)),
            [("KREAM", false), ("ZOHARA", true)]
        );
        assert_eq!(
            names(&split_credit("Calvin Harris ft. Rihanna & Ne-Yo", &k)),
            [("Calvin Harris", false), ("Rihanna", true), ("Ne-Yo", true)]
        );
        assert_eq!(
            names(&split_credit("DJ Snake (featuring Justin Bieber)", &k)),
            [("DJ Snake", false), ("Justin Bieber", true)]
        );
    }

    #[test]
    fn band_names_stay_whole() {
        let k = keep();
        assert_eq!(
            names(&split_credit("Simon & Garfunkel", &k)),
            [("Simon & Garfunkel", false)]
        );
        assert_eq!(
            names(&split_credit("earth, wind & fire", &k)),
            [("earth, wind & fire", false)],
            "case doesn't matter, the original spelling is kept"
        );
        assert_eq!(
            names(&split_credit("Mumford & Sons & Baaba Maal", &k)),
            [("Mumford & Sons", false), ("Baaba Maal", false)]
        );
    }

    #[test]
    fn words_that_only_look_like_markers_stay() {
        let k = keep();
        assert_eq!(
            names(&split_credit("Feather Lane", &k)),
            [("Feather Lane", false)]
        );
        assert_eq!(names(&split_credit("Ft Smith", &k)), [("Ft Smith", false)]);
        assert_eq!(
            names(&split_credit("Lefty Mixx", &k)),
            [("Lefty Mixx", false)]
        );
    }

    #[test]
    fn featured_list_stops_before_version_notes() {
        let k = keep();
        let featured = |title: &str| {
            track_credits(&["KREAM".into()], title, &k)
                .into_iter()
                .filter(|c| c.featured)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(featured("Deep End (feat. JHart) (Disto Remix)"), ["JHart"]);
        assert_eq!(
            featured("Roads (feat. Dan Caplen) [AM1R Extended Mix]"),
            ["Dan Caplen"]
        );
        assert_eq!(featured("Song feat. A & B (VIP Mix)"), ["A", "B"]);
        assert_eq!(
            featured("Song [ft. Louisa] [Dozer Extended Mix]"),
            ["Louisa"]
        );
        assert_eq!(featured("Song feat. Clara Mae - Acoustic"), ["Clara Mae"]);
        assert!(featured("Deep End (Disto Remix)").is_empty());
    }

    #[test]
    fn track_credits_use_tags_and_title() {
        let k = keep();
        let credits = track_credits(
            &["Zara Larsson".into(), "David Guetta".into()],
            "Volcano (feat. KREAM)",
            &k,
        );
        assert_eq!(
            names(&credits),
            [
                ("Zara Larsson", false),
                ("David Guetta", false),
                ("KREAM", true)
            ]
        );
        // NUL-joined multi-value, and a featured artist already credited.
        let credits = track_credits(&["A\0B".into()], "Song (feat. B)", &k);
        assert_eq!(names(&credits), [("A", false), ("B", false)]);
    }
}
