use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::{env, fs, path::Path};

use anyhow::Result;
use gio::prelude::*;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

// Tiered weights, ported from DMS's launcher scorer: a strong textual tier
// wins outright; fuzzy matching is a weak last resort for 3+ char queries
// only, so short queries must hit a strong tier or miss entirely.
const W_EXACT: u32 = 10_000;
const W_PREFIX: u32 = 5_000;
const W_WORD_BOUNDARY: u32 = 3_000;
const W_SUBSTRING: u32 = 500;
const W_GENERIC_PREFIX: u32 = 800;
const W_GENERIC: u32 = 400;
const W_ID: u32 = 350;
const W_FUZZY: u32 = 100;

/// `GenericName` + `Keywords` from the app's `.desktop` file (plain,
/// unlocalised keys only). Parsed lazily — the gio `AppInfo` interface does
/// not expose them, and they are only consulted once name/comment miss.
struct DesktopMeta {
    generic: Option<String>,
    keywords: Vec<String>,
}

/// One installed application, precomputed at first search and reused for the
/// process lifetime (the same freshness tradeoff as runner's `path_binaries`
/// cache). Lowercased fields avoid per-query re-lowering of every app.
struct CachedApp {
    id: String,
    title: String,
    title_lower: String,
    comment: Option<String>,
    comment_lower: Option<String>,
    icon_spec: Option<String>,
    meta: Option<DesktopMeta>,
}

impl CachedApp {
    /// Resolve the gio icon spec (`!!/path` for file icons, otherwise a theme
    /// name) to the absolute path the UI renders.
    fn icon_path(&self) -> Option<String> {
        let spec = self.icon_spec.as_deref()?;
        if let Some(path) = spec.strip_prefix("!!") {
            (!path.is_empty()).then(|| path.to_string())
        } else {
            find_icon_path(spec)
        }
    }
}

static APPS: LazyLock<Vec<CachedApp>> = LazyLock::new(|| {
    gio::AppInfo::all()
        .into_iter()
        .filter_map(|app| {
            if !app.should_show() {
                return None;
            }
            let id = app.id().map(|s| s.to_string())?;
            let title = app.name().to_string();
            let comment = app.description().map(|s| s.to_string());
            let icon_spec = app
                .icon()
                .and_then(|i| i.to_string())
                .map(|s| s.to_string());
            Some(CachedApp {
                title_lower: title.to_lowercase(),
                comment_lower: comment.as_ref().map(|c| c.to_lowercase()),
                meta: desktop_meta(&id),
                title,
                comment,
                id,
                icon_spec,
            })
        })
        .collect()
});

pub struct AppSearch;

impl Plugin for AppSearch {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "app-search",
            name: "Applications",
            icon: "application_default",
            ready: "Search installed applications",
            keyword: "",
        }
    }

    fn search(
        &self,
        _query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let input = full.to_string();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || do_search(&input))
                .await
                .unwrap_or_else(|_| Ok(vec![]))
        })
    }
}

/// Score every cached app against the query; the query text is lowercased
/// and tokenized once, not per app.
fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let query_lower = query.trim().to_lowercase();
    let query_words = tokenize(&query_lower);

    let mut results: Vec<(u32, ResultItem)> = Vec::new();
    for app in APPS.iter() {
        let score = score_app(
            &app.title_lower,
            app.comment_lower.as_deref(),
            app.meta.as_ref(),
            &app.id,
            &query_lower,
            &query_words,
        );
        if score > 0 {
            results.push((
                score,
                ResultItem {
                    title: app.title.clone(),
                    summary: app.comment.clone(),
                    on_click: Some(format!("launch:{}", app.id)),
                    icon: app.icon_path(),
                },
            ));
        }
    }

    Ok(crate::provider::rank_results(results, true, 50))
}

fn tokenize(s: &str) -> Vec<String> {
    s.split([' ', '-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Score one match surface. Fields are tried in order with decaying
/// weights: exact name, prefix, word-boundary (each query word prefixes a
/// consecutive name word), plain substring, then edit-distance fuzziness.
/// All string inputs must already be lowercased.
fn field_score(field_lower: &str, query_lower: &str, query_words: &[String]) -> u32 {
    if field_lower == query_lower {
        return W_EXACT;
    }
    if field_lower.starts_with(query_lower) {
        return W_PREFIX;
    }

    let words = tokenize(field_lower);
    if query_words.len() <= words.len() {
        let bounded = (0..=words.len() - query_words.len())
            .any(|i| (0..query_words.len()).all(|j| words[i + j].starts_with(&query_words[j])));
        if bounded {
            return W_WORD_BOUNDARY;
        }
    }

    if field_lower.contains(query_lower) {
        return W_SUBSTRING;
    }

    // fuzzy only for queries of 3+ chars; short queries must match a strong
    // tier or they are simply not a hit
    if query_lower.chars().count() >= 3 {
        let fs = fuzzy_score(field_lower, query_lower);
        if fs > 0.0 {
            return (fs * W_FUZZY as f64) as u32;
        }
    }
    0
}

/// Edit-distance similarity (0..1) between a whole text or any of its words
/// and the query, within a tight per-length tolerance window.
fn fuzzy_score(text: &str, query: &str) -> f64 {
    let text_chars: Vec<char> = text.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    let max_dist = match query_chars.len() {
        3 => 1,
        4..=6 => 2,
        _ => 3,
    };

    let mut best = 0.0f64;
    if (text_chars.len() as isize - query_chars.len() as isize).unsigned_abs() <= max_dist {
        let dist = levenshtein(&text_chars, &query_chars);
        if dist <= max_dist {
            best = 1.0 - dist as f64 / text_chars.len().max(query_chars.len()) as f64;
        }
    }

    for word in tokenize(text) {
        if best >= 0.8 {
            break;
        }
        let word_chars: Vec<char> = word.chars().collect();
        if (word_chars.len() as isize - query_chars.len() as isize).unsigned_abs() > max_dist {
            continue;
        }
        let dist = levenshtein(&word_chars, &query_chars);
        if dist <= max_dist {
            let score = 1.0 - dist as f64 / word_chars.len().max(query_chars.len()) as f64;
            best = best.max(score);
        }
    }
    best
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Full app relevance. Name at full weight, comment at 0.5x, each keyword at
/// 0.3x, GenericName prefix/contains, then the desktop id (`.desktop`
/// stripped) — first non-zero tier wins. All string inputs must already be
/// lowercased.
fn score_app(
    name_lower: &str,
    comment_lower: Option<&str>,
    meta: Option<&DesktopMeta>,
    id: &str,
    query_lower: &str,
    query_words: &[String],
) -> u32 {
    if query_lower.is_empty() {
        return 1;
    }

    let mut score = field_score(name_lower, query_lower, query_words);
    if score == 0
        && let Some(c) = comment_lower
    {
        score = field_score(c, query_lower, query_words) * 5 / 10;
    }
    if score == 0
        && let Some(v) = meta.and_then(|m| {
            m.keywords.iter().find_map(|keyword| {
                let ks = field_score(&keyword.to_lowercase(), query_lower, query_words);
                (ks > 0).then_some(ks * 3 / 10)
            })
        })
    {
        score = v;
    }
    if score == 0
        && let Some(g) = meta.and_then(|m| m.generic.as_deref())
    {
        let generic_lower = g.to_lowercase();
        score = if generic_lower.starts_with(query_lower) {
            W_GENERIC_PREFIX
        } else if generic_lower.contains(query_lower) {
            W_GENERIC
        } else {
            0
        };
    }
    if score == 0 {
        let id_lower = id.to_lowercase().trim_end_matches(".desktop").to_string();
        if id_lower.contains(query_lower) {
            score = W_ID;
        }
    }
    score
}

/// Locate the `.desktop` file by id through the XDG data dirs and read the
/// plain `GenericName`/`Keywords` keys. gio-rs does not bind GDesktopAppInfo,
/// so this is the only way to reach them.
fn desktop_meta(id: &str) -> Option<DesktopMeta> {
    let mut bases: Vec<String> = env::var("XDG_DATA_DIRS")
        .map(|s| s.split(':').map(String::from).collect())
        .unwrap_or_else(|_| vec!["/usr/local/share/".into(), "/usr/share/".into()]);
    if let Ok(home) = env::var("HOME") {
        bases.push(format!("{home}/.local/share"));
    }

    for base in bases {
        let file = Path::new(&base).join("applications").join(id);
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };

        let mut generic: Option<String> = None;
        let mut keywords: Vec<String> = Vec::new();
        let mut in_entry = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
            } else if in_entry {
                // plain (unlocalised) keys only — the [xx] variants follow
                // later in the file and would otherwise overwrite these
                if let Some(v) = line.strip_prefix("GenericName=") {
                    if generic.is_none() {
                        let v = v.trim();
                        if !v.is_empty() {
                            generic = Some(v.to_string());
                        }
                    }
                } else if let Some(v) = line.strip_prefix("Keywords=") {
                    let kw = v.split(';').filter(|s| !s.trim().is_empty());
                    keywords.extend(kw.map(|s| s.trim().to_string()));
                }
            }
        }

        if generic.is_some() || !keywords.is_empty() {
            return Some(DesktopMeta { generic, keywords });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(generic: Option<&str>, keywords: &[&str]) -> DesktopMeta {
        DesktopMeta {
            generic: generic.map(String::from),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn s(name: &str, comment: Option<&str>, m: Option<&DesktopMeta>, id: &str, q: &str) -> u32 {
        let name_lower = name.to_lowercase();
        let comment_lower = comment.map(|c| c.to_lowercase());
        let query_lower = q.trim().to_lowercase();
        let query_words = tokenize(&query_lower);
        score_app(
            &name_lower,
            comment_lower.as_deref(),
            m,
            id,
            &query_lower,
            &query_words,
        )
    }

    #[test]
    fn exact_beats_prefix_beats_substring() {
        let m = meta(None, &[]);
        assert_eq!(
            s("Telegram", None, Some(&m), "telegram.desktop", "telegram"),
            W_EXACT
        );
        assert_eq!(
            s("Telegram", None, Some(&m), "telegram.desktop", "tele"),
            W_PREFIX
        );
        assert!(s("Bottles", None, Some(&m), "bottles.desktop", "bo") == W_PREFIX);
        assert_eq!(
            s(
                "LibreOffice",
                None,
                Some(&m),
                "libreoffice.desktop",
                "office"
            ),
            W_SUBSTRING
        );
    }

    #[test]
    fn short_query_never_fuzzes() {
        // two chars: only strong tiers count — no weak mid-word hits
        let m = meta(None, &[]);
        assert_eq!(
            s("Yazi File Manager", None, Some(&m), "yazi.desktop", "bo"),
            0
        );
        assert_eq!(
            s(
                "GNU Image Manipulation Program",
                None,
                Some(&m),
                "gimp.desktop",
                "bo"
            ),
            0
        );
    }

    #[test]
    fn gimp_found_via_keyword_tier() {
        // Name/Comment are prose; 'gimp' lives only in Keywords (flatpak GIMP)
        let m = meta(Some("Image Editor"), &["GIMP", "graphic", "design"]);
        let sc = s(
            "GNU Image Manipulation Program",
            Some("Create images and edit photographs"),
            Some(&m),
            "org.gimp.GIMP.desktop",
            "gimp",
        );
        assert_eq!(sc, W_EXACT * 3 / 10);
    }

    #[test]
    fn word_boundary_handles_multiword() {
        let m = meta(None, &[]);
        assert_eq!(
            s(
                "Dank Material Shell Settings",
                None,
                Some(&m),
                "dms.desktop",
                "material shell"
            ),
            W_WORD_BOUNDARY
        );
        // non-consecutive order is not a word-boundary hit
        assert!(
            s(
                "Dank Material Shell Settings",
                None,
                Some(&m),
                "dms.desktop",
                "shell material"
            ) < W_WORD_BOUNDARY
        );
    }

    #[test]
    fn generic_name_fallback() {
        let m = meta(Some("Text Editor"), &[]);
        assert_eq!(
            s("DMS Notes", None, Some(&m), "dms-notes.desktop", "editor"),
            W_GENERIC
        );
        assert_eq!(
            s("DMS Notes", None, Some(&m), "dms-notes.desktop", "text"),
            W_GENERIC_PREFIX
        );
    }

    #[test]
    fn desktop_id_is_last_resort() {
        let m = meta(None, &[]);
        assert_eq!(
            s("Strange Name", None, Some(&m), "firefox.desktop", "firefox"),
            W_ID
        );
        assert_eq!(
            s("Strange Name", None, Some(&m), "firefox.desktop", "zzz"),
            0
        );
    }

    #[test]
    fn fuzzy_only_from_three_chars() {
        let m = meta(None, &[]);
        // krta vs Krita: one transposition-ish edit, len 4
        assert!(s("Krita", None, Some(&m), "krita.desktop", "krta") > 0);
        assert!(s("Krita", None, Some(&m), "krita.desktop", "krta") < W_FUZZY);
        // two-char typo is not enough to matter
        assert_eq!(s("Krita", None, Some(&m), "krita.desktop", "kt"), 0);
    }

    #[test]
    fn empty_query_matches_everything() {
        let m = meta(None, &[]);
        assert_eq!(s("Anything", None, Some(&m), "a.desktop", ""), 1);
    }
}
