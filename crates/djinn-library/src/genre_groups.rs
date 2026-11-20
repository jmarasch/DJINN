//! Grouping similar genres into containers ("death metal", "black metal" and
//! "speed metal" into "Metal"). The genres are clustered once by how related
//! they are (shared songs and sound, as on the genre map), merging the two
//! closest groups at a time; any point in that history is a way of grouping
//! them, from many small, close containers to a few broad ones. The library
//! itself is never changed: containers only fold names when they're shown.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::Serialize;

use crate::genres::{self, GenreMap, Shape};
use crate::sound::SoundIndex;
use crate::vibe;

/// Groups less alike than this on average aren't merged at all.
const FLOOR: f32 = 0.05;

/// Two clusters and how alike they are, ordered most alike first (ties by
/// the lower ids, so the result is the same every time).
#[derive(Clone, Copy, Debug)]
struct Pair {
    alike: f32,
    a: usize,
    b: usize,
}

impl Pair {
    fn new(a: usize, b: usize, alike: f32) -> Self {
        Self { alike, a, b }
    }
}

impl PartialEq for Pair {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Pair {}

impl PartialOrd for Pair {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pair {
    fn cmp(&self, other: &Self) -> Ordering {
        self.alike
            .total_cmp(&other.alike)
            .then_with(|| (other.a, other.b).cmp(&(self.a, self.b)))
    }
}

/// The history of merging genres, closest first.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// The genres clustered, and how many songs each has.
    names: Vec<String>,
    songs: Vec<usize>,
    /// Each merge joins two clusters (leaves are `0..names.len()`, merge `k`
    /// makes cluster `names.len() + k`), with how alike they were.
    merges: Vec<(usize, usize, f32)>,
}

/// A way of grouping: after `step` merges there are `containers` groups of
/// two or more genres, holding `grouped` genres in all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub containers: usize,
    pub grouped: usize,
    pub step: usize,
}

/// Clusters the genres on `map`, leaving out the ones in `skip` (placed by
/// hand).
pub fn tree(map: &GenreMap, skip: &HashSet<String>) -> Tree {
    let keep: Vec<usize> = (0..map.genres.len())
        .filter(|&g| !skip.contains(&map.genres[g].name))
        .collect();
    let place: HashMap<usize, usize> = keep.iter().enumerate().map(|(i, &g)| (g, i)).collect();
    let names: Vec<String> = keep.iter().map(|&g| map.genres[g].name.clone()).collect();
    let songs: Vec<usize> = keep.iter().map(|&g| map.genres[g].songs).collect();
    let n = names.len();

    // Summed strength between clusters, both ways round.
    let mut links: HashMap<usize, HashMap<usize, f32>> = HashMap::new();
    for link in &map.links {
        if let (Some(&a), Some(&b)) = (place.get(&link.a), place.get(&link.b)) {
            *links.entry(a).or_default().entry(b).or_default() += link.strength;
            *links.entry(b).or_default().entry(a).or_default() += link.strength;
        }
    }
    // Names count too, so related genres group even when songs carry one
    // genre each and nothing has been analyzed: "black metal" is like "death
    // metal", and both are kinds of "metal".
    for a in 0..n {
        for b in a + 1..n {
            let by_name = name_alike(&names[a], &names[b]);
            if by_name <= 0.0 {
                continue;
            }
            for (x, y) in [(a, b), (b, a)] {
                let sum = links.entry(x).or_default().entry(y).or_default();
                *sum = sum.max(by_name);
            }
        }
    }
    let mut size: Vec<usize> = vec![1; n];
    let mut alive: Vec<bool> = vec![true; n];
    let mut merges = Vec::new();
    // Every pair of clusters with links, most alike first. A pair's score
    // never changes while both are alive, so stale entries (a side merged
    // away) are just skipped.
    let mut queue: BinaryHeap<Pair> = BinaryHeap::new();
    for (&a, near) in &links {
        for (&b, &sum) in near {
            if a < b {
                queue.push(Pair::new(a, b, sum / (size[a] * size[b]) as f32));
            }
        }
    }
    while let Some(Pair { alike, a, b }) = queue.pop() {
        if !alive[a] || !alive[b] {
            continue;
        }
        if alike < FLOOR {
            break;
        }
        let joined = n + merges.len();
        merges.push((a, b, alike));
        alive[a] = false;
        alive[b] = false;
        alive.push(true);
        size.push(size[a] + size[b]);
        let from_a = links.remove(&a).unwrap_or_default();
        let from_b = links.remove(&b).unwrap_or_default();
        let mut mine: HashMap<usize, f32> = HashMap::new();
        for (other, sum) in from_a.into_iter().chain(from_b) {
            if other != a && other != b {
                *mine.entry(other).or_default() += sum;
            }
        }
        for (&other, &sum) in &mine {
            if let Some(near) = links.get_mut(&other) {
                near.remove(&a);
                near.remove(&b);
                near.insert(joined, sum);
            }
            queue.push(Pair::new(
                other.min(joined),
                other.max(joined),
                sum / (size[other] * size[joined]) as f32,
            ));
        }
        if !mine.is_empty() {
            links.insert(joined, mine);
        }
    }
    Tree {
        names,
        songs,
        merges,
    }
}

impl Tree {
    /// The genres in each container after `step` merges (two or more genres
    /// each), biggest first.
    pub fn cut(&self, step: usize) -> Vec<Vec<String>> {
        let n = self.names.len();
        let mut members: Vec<Vec<usize>> = (0..n).map(|g| vec![g]).collect();
        let mut alive: Vec<bool> = vec![true; n];
        for &(a, b, _) in self.merges.iter().take(step) {
            let mut joined = std::mem::take(&mut members[a]);
            joined.append(&mut std::mem::take(&mut members[b]));
            alive[a] = false;
            alive[b] = false;
            members.push(joined);
            alive.push(true);
        }
        let mut groups: Vec<Vec<String>> = members
            .into_iter()
            .zip(alive)
            .filter(|(m, alive)| *alive && m.len() >= 2)
            .map(|(m, _)| {
                let mut names: Vec<String> = m.into_iter().map(|g| self.names[g].clone()).collect();
                names.sort();
                names
            })
            .collect();
        groups.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        groups
    }

    /// Each whole number of containers the history passes through on its way
    /// from the most containers to the fewest, fewest first. Before the most,
    /// genres are still pairing up and most are left out, so only the stretch
    /// after it counts; where a number comes up more than once, the later
    /// (broader) point is kept.
    pub fn choices(&self) -> Vec<Choice> {
        let n = self.names.len();
        let mut groups: HashMap<usize, usize> = HashMap::new();
        let mut sizes: Vec<usize> = vec![1; n];
        let mut counts = vec![(0usize, 0usize)];
        let (mut containers, mut grouped) = (0usize, 0usize);
        for (k, &(a, b, _)) in self.merges.iter().enumerate() {
            for side in [a, b] {
                if sizes[side] >= 2 {
                    containers -= 1;
                    grouped -= sizes[side];
                    groups.remove(&side);
                }
            }
            let joined = sizes[a] + sizes[b];
            sizes.push(joined);
            groups.insert(n + k, joined);
            containers += 1;
            grouped += joined;
            counts.push((containers, grouped));
        }
        let peak = counts
            .iter()
            .enumerate()
            .max_by(|(i, x), (j, y)| x.0.cmp(&y.0).then(i.cmp(j)))
            .map_or(0, |(i, _)| i);
        let mut by_count: HashMap<usize, Choice> = HashMap::new();
        for (step, &(containers, grouped)) in counts.iter().enumerate().skip(peak) {
            if containers == 0 {
                continue;
            }
            by_count.insert(
                containers,
                Choice {
                    containers,
                    grouped,
                    step,
                },
            );
        }
        let mut out: Vec<Choice> = by_count.into_values().collect();
        out.sort_by_key(|c| c.containers);
        out
    }

    /// The choice closest to `containers`.
    pub fn nearest(&self, containers: usize) -> Option<Choice> {
        self.choices()
            .into_iter()
            .min_by_key(|c| (c.containers.abs_diff(containers), c.containers))
    }

    fn songs_of(&self, genre: &str) -> usize {
        self.names
            .iter()
            .position(|g| g == genre)
            .map_or(0, |i| self.songs[i])
    }

    /// A name for a container: a member the others are kinds of ("metal" for
    /// "black metal" and "death metal"), else a last word most members share,
    /// else its biggest genre. Capitalized.
    pub fn name(&self, members: &[String]) -> String {
        let count = members.len();
        let broad = members.iter().max_by_key(|m| {
            let fits = members.iter().filter(|o| vibe::genre_fits(o, m)).count();
            (fits, std::cmp::Reverse(m.len()))
        });
        if let Some(broad) = broad {
            let fits = members
                .iter()
                .filter(|o| vibe::genre_fits(o, broad))
                .count();
            if fits * 2 > count {
                return title(broad);
            }
        }
        let mut words: HashMap<&str, usize> = HashMap::new();
        for m in members {
            if let Some(last) = m.split_whitespace().last() {
                *words.entry(last).or_default() += 1;
            }
        }
        if let Some((word, n)) = words
            .into_iter()
            .max_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)))
        {
            if n * 2 > count {
                return title(word);
            }
        }
        members
            .iter()
            .max_by(|a, b| self.songs_of(a).cmp(&self.songs_of(b)).then(b.cmp(a)))
            .map_or_else(String::new, |m| title(m))
    }
}

/// Words that say nothing about a genre.
const FILLER: [&str; 6] = ["music", "and", "the", "of", "n", "&"];

/// How alike two genre names are, 0 to 1: one a kind of the other ("metal",
/// "black metal"), two kinds of one thing ("black metal", "death metal"), or
/// a word in common ("hard rock", "rock and roll").
fn name_alike(a: &str, b: &str) -> f32 {
    if vibe::genre_fits(a, b) || vibe::genre_fits(b, a) {
        return 0.6;
    }
    let words = |g: &str| -> Vec<String> {
        g.split_whitespace()
            .filter(|w| w.len() >= 3 && !FILLER.contains(w))
            .map(str::to_owned)
            .collect()
    };
    let (wa, wb) = (words(a), words(b));
    if wa.last().is_some() && wa.last() == wb.last() {
        return 0.45;
    }
    if wa.iter().any(|w| wb.contains(w)) {
        return 0.25;
    }
    0.0
}

impl Tree {
    /// The containers after `step` merges, each with its name; ones that
    /// come out with the same name are one container.
    pub fn named(&self, step: usize) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for members in self.cut(step) {
            let name = self.name(&members);
            match out
                .iter_mut()
                .find(|(n, _)| n.to_lowercase() == name.to_lowercase())
            {
                Some((_, kept)) => {
                    kept.extend(members);
                    kept.sort();
                }
                None => out.push((name, members)),
            }
        }
        out
    }
}

/// The genre map with every link kept, for clustering.
pub fn full_map(tags: &HashMap<i64, Vec<String>>, index: &SoundIndex) -> GenreMap {
    genres::shaped(
        tags,
        index,
        Shape {
            top: 0,
            links_per_genre: usize::MAX,
        },
    )
}

/// "hip hop" to "Hip Hop".
fn title(name: &str) -> String {
    name.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every song's genres with each one in a container replaced by the
/// container's name (once per song). `fold` maps genre names to containers.
pub fn fold_tags(
    tags: &HashMap<i64, Vec<String>>,
    fold: &HashMap<String, String>,
) -> HashMap<i64, Vec<String>> {
    tags.iter()
        .map(|(&id, names)| {
            let mut out: Vec<String> = Vec::with_capacity(names.len());
            for name in names {
                let shown = fold.get(name).unwrap_or(name);
                if !out.contains(shown) {
                    out.push(shown.clone());
                }
            }
            (id, out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[(i64, &[&str])]) -> HashMap<i64, Vec<String>> {
        list.iter()
            .map(|(id, names)| (*id, names.iter().map(|n| (*n).to_owned()).collect()))
            .collect()
    }

    /// Two families of genres that share songs among themselves only.
    fn library() -> HashMap<i64, Vec<String>> {
        let mut list: Vec<(i64, Vec<&str>)> = Vec::new();
        let mut id = 0;
        for pair in [
            ["metal", "black metal"],
            ["metal", "death metal"],
            ["black metal", "death metal"],
            ["metal", "speed metal"],
            ["house", "deep house"],
            ["house", "tech house"],
            ["deep house", "tech house"],
        ] {
            for _ in 0..4 {
                list.push((id, pair.to_vec()));
                id += 1;
            }
        }
        list.push((id, vec!["polka"]));
        list.into_iter()
            .map(|(id, names)| (id, names.into_iter().map(str::to_owned).collect()))
            .collect()
    }

    #[test]
    fn related_genres_end_up_in_one_container_named_for_the_broad_one() {
        let map = full_map(&library(), &SoundIndex::default());
        let tree = tree(&map, &HashSet::new());
        let choices = tree.choices();
        assert!(!choices.is_empty());
        // Two families: two containers, neither holding polka.
        let two = tree.nearest(2).unwrap();
        assert_eq!(two.containers, 2);
        let groups = tree.cut(two.step);
        assert_eq!(groups.len(), 2);
        let metal = groups
            .iter()
            .find(|g| g.contains(&"metal".to_owned()))
            .unwrap();
        assert_eq!(
            metal,
            &["black metal", "death metal", "metal", "speed metal"]
        );
        assert_eq!(tree.name(metal), "Metal");
        let house = groups
            .iter()
            .find(|g| g.contains(&"house".to_owned()))
            .unwrap();
        assert_eq!(tree.name(house), "House");
        assert!(groups.iter().all(|g| !g.contains(&"polka".to_owned())));
        let named: Vec<String> = tree.named(two.step).into_iter().map(|(n, _)| n).collect();
        assert_eq!(named, ["Metal", "House"]);
    }

    #[test]
    fn choices_go_from_fewest_to_most_in_whole_numbers() {
        let map = full_map(&library(), &SoundIndex::default());
        let tree = tree(&map, &HashSet::new());
        let counts: Vec<usize> = tree.choices().iter().map(|c| c.containers).collect();
        let mut sorted = counts.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(counts, sorted);
        // Every choice cuts into the number of containers it says.
        for c in tree.choices() {
            assert_eq!(tree.cut(c.step).len(), c.containers);
        }
    }

    #[test]
    fn genres_placed_by_hand_are_left_out() {
        let map = full_map(&library(), &SoundIndex::default());
        let skip: HashSet<String> = ["metal".to_owned()].into();
        let tree = tree(&map, &skip);
        for c in tree.choices() {
            assert!(tree.cut(c.step).iter().flatten().all(|g| g != "metal"));
        }
    }

    #[test]
    fn a_name_comes_from_a_shared_word_or_the_biggest_genre() {
        let t = tree(
            &full_map(
                &tags(&[(1, &["pop"]), (2, &["pop"]), (3, &["dance"])]),
                &SoundIndex::default(),
            ),
            &HashSet::new(),
        );
        let shared = ["hard rock".to_owned(), "soft rock".to_owned()];
        assert_eq!(t.name(&shared), "Rock");
        assert_eq!(t.name(&["dance".to_owned(), "pop".to_owned()]), "Pop");
    }

    #[test]
    fn a_big_library_is_grouped_quickly() {
        // 800 genres, each song with three of them, neighbours more often.
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        let mut seed: u64 = 7;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as usize
        };
        for id in 0..36_000 {
            let base = next() % 800;
            let names = [base, (base + 1 + next() % 2) % 800, next() % 800];
            tags.insert(id, names.iter().map(|g| format!("genre {g}")).collect());
        }
        let started = std::time::Instant::now();
        let map = full_map(&tags, &SoundIndex::default());
        let tree = tree(&map, &HashSet::new());
        let choices = tree.choices();
        assert!(choices.len() > 5);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn genres_with_related_names_group_without_shared_songs() {
        // One genre per song and no sound: only the names relate them.
        let map = full_map(
            &tags(&[
                (1, &["black metal"]),
                (2, &["death metal"]),
                (3, &["metal"]),
                (4, &["deep house"]),
                (5, &["house"]),
                (6, &["polka"]),
            ]),
            &SoundIndex::default(),
        );
        let tree = tree(&map, &HashSet::new());
        let two = tree.nearest(2).unwrap();
        let groups = tree.cut(two.step);
        assert_eq!(
            groups,
            [
                vec!["black metal", "death metal", "metal"],
                vec!["deep house", "house"]
            ]
        );
        assert_eq!(name_alike("hard rock", "rock and roll"), 0.25);
        assert_eq!(name_alike("jazz", "polka"), 0.0);
    }

    #[test]
    fn folding_replaces_genres_with_their_container_once() {
        let fold: HashMap<String, String> = [
            ("black metal".to_owned(), "Metal".to_owned()),
            ("death metal".to_owned(), "Metal".to_owned()),
        ]
        .into();
        let folded = fold_tags(
            &tags(&[(1, &["black metal", "death metal", "rock"])]),
            &fold,
        );
        assert_eq!(folded[&1], ["Metal", "rock"]);
    }
}
