//! Named key icons from the Lucide set (ISC, `assets/lucide/LICENSE`),
//! embedded at build time (`build.rs`) so the daemon reads no files.
//!
//! [`resolve`] turns a name into an icon or a refusal with close names,
//! [`search`] ranks icons for words (`GET /api/v1/icons`), and [`rasterise`]
//! draws one as coverage to be tinted by the renderer.

use resvg::{tiny_skia, usvg};
use serde_json::{json, Value};

/// One Lucide icon: its name, its SVG and Lucide's tags for it.
#[derive(Debug)]
pub struct Icon {
    pub name: &'static str,
    pub svg: &'static [u8],
    pub tags: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/lucide.rs"));

/// The most icons one search answers.
pub const SEARCH_LIMIT: usize = 40;

/// The most close names an unknown name is answered with.
const SUGGESTION_LIMIT: usize = 3;

/// Lucide draws on a 24x24 grid.
const LUCIDE_GRID: f32 = 24.0;

/// The icon called `name`, exactly.
pub fn find(name: &str) -> Option<&'static Icon> {
    ICONS
        .binary_search_by(|icon| icon.name.cmp(name))
        .ok()
        .map(|index| &ICONS[index])
}

/// The icon called `name`, or why there is none: `unknown icon 'mial'; did you mean mail?`.
pub fn resolve(name: &str) -> Result<&'static Icon, String> {
    if let Some(icon) = find(name) {
        return Ok(icon);
    }
    let close = close_names(name);
    let hint = match close.as_slice() {
        [] => "search them with `streamdeck icons <words>`".to_string(),
        [one] => format!("did you mean {one}?"),
        [rest @ .., last] => format!("did you mean {} or {last}?", rest.join(", ")),
    };
    Err(format!("unknown icon '{name}'; {hint}"))
}

/// Up to three names a typo of `name` most likely meant, closest first.
pub fn close_names(name: &str) -> Vec<&'static str> {
    let wanted = name.trim().to_lowercase();
    let limit = match wanted.chars().count() {
        0 => return Vec::new(),
        1..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    let mut close: Vec<(usize, &'static str)> = ICONS
        .iter()
        .map(|icon| (edit_distance(&wanted, icon.name), icon.name))
        .filter(|(distance, _)| *distance <= limit)
        .collect();
    close.sort_unstable();
    close
        .into_iter()
        .take(SUGGESTION_LIMIT)
        .map(|(_, name)| name)
        .collect()
}

/// Optimal string alignment distance: insertions, deletions, substitutions
/// and swaps of two neighbours each cost one.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    rows[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

/// A search's answer: how many icons match, and the best of them.
#[derive(Debug)]
pub struct Found {
    pub total: usize,
    pub icons: Vec<&'static Icon>,
}

impl Found {
    /// `{ "total": n, "icons": [{ "name", "tags" }] }`, as the API answers it.
    pub fn to_json(&self) -> Value {
        let icons: Vec<Value> = self
            .icons
            .iter()
            .map(|icon| json!({ "name": icon.name, "tags": icon.tags }))
            .collect();
        json!({ "total": self.total, "icons": icons })
    }
}

/// How well an icon matches the words: lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// The name is the words joined by hyphens.
    ExactName,
    /// Every word is a whole part of the name (`play` in `circle-play`).
    NameParts,
    /// Every word is somewhere in the name.
    InName,
    /// Every word is in the name or one of its tags.
    InTags,
}

fn match_of(icon: &Icon, words: &[String]) -> Option<Match> {
    let parts: Vec<&str> = icon.name.split('-').collect();
    if icon.name == words.join("-") {
        Some(Match::ExactName)
    } else if words.iter().all(|w| parts.contains(&w.as_str())) {
        Some(Match::NameParts)
    } else if words.iter().all(|w| icon.name.contains(w.as_str())) {
        Some(Match::InName)
    } else if words
        .iter()
        .all(|w| icon.name.contains(w.as_str()) || icon.tags.iter().any(|t| t.contains(w.as_str())))
    {
        Some(Match::InTags)
    } else {
        None
    }
}

/// The icons for `query`'s words, best first: name matches before tag
/// matches, then shorter names, then by name. No words lists icons by name.
pub fn search(query: &str) -> Found {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Found {
            total: ICONS.len(),
            icons: ICONS.iter().take(SEARCH_LIMIT).collect(),
        };
    }
    let mut ranked: Vec<(Match, usize, &'static Icon)> = ICONS
        .iter()
        .filter_map(|icon| match_of(icon, &words).map(|m| (m, icon.name.len(), icon)))
        .collect();
    ranked.sort_by(|a, b| (a.0, a.1, a.2.name).cmp(&(b.0, b.1, b.2.name)));
    Found {
        total: ranked.len(),
        icons: ranked
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(_, _, icon)| icon)
            .collect(),
    }
}

/// An icon drawn at `size` x `size`: one coverage byte (0-255) per pixel,
/// row by row, to be tinted by whoever paints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub size: u32,
    pub alpha: Vec<u8>,
}

/// Draw `icon` at `size` x `size` pixels.
pub fn rasterise(icon: &Icon, size: u32) -> Result<Coverage, String> {
    let tree = usvg::Tree::from_data(icon.svg, &usvg::Options::default())
        .map_err(|err| format!("icon '{}' does not parse: {err}", icon.name))?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| format!("icon '{}' cannot be drawn at {size}px", icon.name))?;
    let scale = size as f32 / LUCIDE_GRID;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let alpha = pixmap.pixels().iter().map(|p| p.alpha()).collect();
    Ok(Coverage { size, alpha })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &Found) -> Vec<&'static str> {
        found.icons.iter().map(|icon| icon.name).collect()
    }

    #[test]
    fn embeds_the_vendored_set_sorted_by_name() {
        assert_eq!(VERSION, include_str!("../assets/lucide/VERSION").trim());
        assert!(ICONS.len() > 1000, "only {} icons", ICONS.len());
        assert!(ICONS.windows(2).all(|w| w[0].name < w[1].name));
        assert!(ICONS
            .iter()
            .all(|icon| icon.svg.starts_with(b"<!-- @license lucide-static")));
    }

    #[test]
    fn finds_an_icon_by_its_exact_name() {
        let mail = find("mail").unwrap();
        assert_eq!(mail.name, "mail");
        assert!(mail.tags.contains(&"email"));
        assert!(find("Mail").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn refuses_an_unknown_name_with_close_names() {
        assert_eq!(resolve("mail").unwrap().name, "mail");
        assert_eq!(
            resolve("mial").unwrap_err(),
            "unknown icon 'mial'; did you mean mail?"
        );
        assert_eq!(
            resolve("xqzzyq").unwrap_err(),
            "unknown icon 'xqzzyq'; search them with `streamdeck icons <words>`"
        );
    }

    #[test]
    fn offers_at_most_three_close_names_closest_first() {
        assert_eq!(close_names("lamp-flor"), ["lamp-floor"]);
        assert_eq!(
            resolve("monitr-of").unwrap_err(),
            "unknown icon 'monitr-of'; did you mean monitor-off, monitor or monitor-cog?"
        );
        assert!(close_names("").is_empty());
    }

    #[test]
    fn counts_swaps_of_neighbours_as_one_edit() {
        assert_eq!(edit_distance("mial", "mail"), 1);
        assert_eq!(edit_distance("mail", "mail"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn lists_the_first_icons_by_name_without_words() {
        let found = search("  ");
        assert_eq!(found.total, ICONS.len());
        assert_eq!(found.icons.len(), SEARCH_LIMIT);
        assert_eq!(found.icons[0].name, ICONS[0].name);
    }

    #[test]
    fn ranks_name_matches_before_tag_matches() {
        let found = search("mail");
        let names = names(&found);
        assert_eq!(names[0], "mail");
        let first_tag_only = names
            .iter()
            .position(|n| !n.contains("mail"))
            .unwrap_or(names.len());
        assert!(names[first_tag_only..].iter().all(|n| !n.contains("mail")));
        assert!(
            first_tag_only < names.len(),
            "tag matches follow: {names:?}"
        );
        assert!(names.len() <= SEARCH_LIMIT);
    }

    #[test]
    fn matches_every_word_across_name_parts_and_tags() {
        let names_up = names(&search("Arrow UP"));
        assert_eq!(names_up[0], "arrow-up");
        let lamp = names(&search("lamp"));
        assert_eq!(lamp[0], "lamp");
        assert!(lamp[1..].iter().take(5).all(|n| n.starts_with("lamp-")));
        let email = search("email");
        assert!(names(&email).contains(&"mail"), "email is mail's tag");
        assert_eq!(search("zzqqxx").total, 0);
    }

    #[test]
    fn answers_a_search_as_total_and_named_icons_with_tags() {
        let value = search("mail").to_json();
        assert!(value["total"].as_u64().unwrap() >= 1);
        assert_eq!(value["icons"][0]["name"], "mail");
        assert!(value["icons"][0]["tags"].as_array().unwrap().len() > 1);
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    #[test]
    fn rasterises_an_icon_as_coverage_at_the_asked_size() {
        let coverage = rasterise(find("mail").unwrap(), 48).unwrap();
        assert_eq!(coverage.size, 48);
        assert_eq!(coverage.alpha.len(), 48 * 48);
        let inked = coverage.alpha.iter().filter(|a| **a > 128).count();
        assert!(inked > 100, "only {inked} inked pixels");
        assert_eq!(coverage.alpha[0], 0, "the corner is outside the envelope");
        assert!(rasterise(find("mail").unwrap(), 0).is_err());
    }

    #[test]
    fn every_vendored_icon_draws_ink() {
        let blank: Vec<&str> = ICONS
            .iter()
            .filter(|icon| {
                rasterise(icon, 24)
                    .map(|c| c.alpha.iter().all(|a| *a == 0))
                    .unwrap_or(true)
            })
            .map(|icon| icon.name)
            .collect();
        assert!(blank.is_empty(), "these draw nothing: {blank:?}");
    }
}
