use std::fs;
use std::pin::Pin;
use std::sync::LazyLock;

use anyhow::Result;
use gio::prelude::*;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::fs as system_fs;
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
const W_ACTION_EXACT: u32 = 8_000;
const W_ACTION_PREFIX: u32 = 4_000;
const W_ACTION_SUBSTRING: u32 = 400;

/// `GenericName` + `Keywords` from the app's `.desktop` file (plain, unlocalised
/// keys only — gio does not expose them). Lowercased and tokenized once at
/// cache-build time: a search visits every app.
#[derive(Clone)]
struct DesktopMeta {
    generic_lower: Option<String>,
    keywords: Vec<Keyword>,
    actions: Vec<DesktopAction>,
}

/// One `Keywords=` entry: lowercased plus its tokenization.
#[derive(Clone)]
struct Keyword {
    lower: String,
    words: Vec<String>,
}

/// One `[Desktop Action <id>]` group, surfaced as its own row (DMS-style).
#[derive(Clone)]
struct DesktopAction {
    id: String,
    name: String,
    name_lower: String,
}

/// One installed application, precomputed at first search and reused for the
/// process lifetime (same freshness tradeoff as runner's `path_binaries`). Every
/// lowercase form, tokenization and the `.desktop`-stripped id is precomputed.
struct CachedApp {
    id: String,
    icon_spec: Option<String>,
    title: String,
    title_lower: String,
    title_words: Vec<String>,
    comment: Option<String>,
    comment_lower: Option<String>,
    comment_words: Vec<String>,
    id_lower: String,
    meta: Option<DesktopMeta>,
}

/// The gio icon spec (`!!/path` for file icons, otherwise a theme name) as the
/// absolute path the UI renders.
fn resolve_icon_spec(spec: &str) -> Option<String> {
    if let Some(path) = spec.strip_prefix("!!") {
        (!path.is_empty()).then(|| path.to_string())
    } else {
        find_icon_path(spec)
    }
}

/// Resolve the icons of the rows that survived ranking: a first-time spec costs
/// a filesystem scan, which a truncated row should not pay for.
fn resolve_icons(items: Vec<ResultItem>) -> Vec<ResultItem> {
    items
        .into_iter()
        .map(|mut item| {
            item.icon = item.icon.as_deref().and_then(resolve_icon_spec);
            item
        })
        .collect()
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
            let title_lower = title.to_lowercase();
            let comment_lower = comment.as_ref().map(|c| c.to_lowercase());
            Some(CachedApp {
                title_words: tokenize(&title_lower),
                comment_words: comment_lower
                    .as_deref()
                    .map(tokenize)
                    .unwrap_or_default(),
                id_lower: id.to_lowercase().trim_end_matches(".desktop").to_string(),
                meta: desktop_meta(&id),
                title_lower,
                comment_lower,
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

/// Score every cached app; the query is lowercased/tokenized/chars-split once.
fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let query_lower = query.trim().to_lowercase();
    let query_words = tokenize(&query_lower);
    let query_chars: Vec<char> = query_lower.chars().collect();

    let mut results: Vec<(u32, ResultItem)> = Vec::new();
    for app in APPS.iter() {
        let score = score_app(app, &query_lower, &query_chars, &query_words);
        if score > 0 {
            results.push((
                score,
                ResultItem {
                    title: app.title.clone(),
                    summary: app.comment.clone(),
                    on_click: Some(format!("launch:{}", app.id)),
                    // raw spec: `resolve_icons` runs after the cap
                    icon: app.icon_spec.clone(),
                },
            ));
        }

        // each action is its own row (DMS-style), titled after the action and
        // launched by the UI; the empty query is the history view, so no rows
        if !query_lower.is_empty() {
            for action in app.meta.iter().flat_map(|m| &m.actions) {
                let action_score = action_score(&action.name_lower, &query_lower);
                if action_score > 0 {
                    results.push((
                        action_score,
                        ResultItem {
                            title: action.name.clone(),
                            summary: Some(app.title.clone()),
                            on_click: Some(format!("action:{}:{}", app.id, action.id)),
                            icon: app.icon_spec.clone(),
                        },
                    ));
                }
            }
        }
    }

    Ok(resolve_icons(crate::provider::rank_results(results, true, 50)))
}

fn tokenize(s: &str) -> Vec<String> {
    s.split([' ', '-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// DMS's action tiers: exact, prefix, substring.
fn action_score(name_lower: &str, query_lower: &str) -> u32 {
    if name_lower == query_lower {
        W_ACTION_EXACT
    } else if name_lower.starts_with(query_lower) {
        W_ACTION_PREFIX
    } else if name_lower.contains(query_lower) {
        W_ACTION_SUBSTRING
    } else {
        0
    }
}

/// Score one match surface. Fields are tried in order with decaying
/// weights: exact name, prefix, word-boundary (each query word prefixes a
/// consecutive name word), plain substring, then edit-distance fuzziness.
/// `field_lower`/`field_words` and the query must already be lowercased, and
/// `field_words` must be `tokenize(field_lower)` — precomputed once per app.
fn field_score(
    field_lower: &str,
    field_words: &[String],
    query_lower: &str,
    query_chars: &[char],
    query_words: &[String],
) -> u32 {
    if field_lower == query_lower {
        return W_EXACT;
    }
    if field_lower.starts_with(query_lower) {
        return W_PREFIX;
    }

    if query_words.len() <= field_words.len() {
        let bounded = (0..=field_words.len() - query_words.len()).any(|i| {
            (0..query_words.len()).all(|j| field_words[i + j].starts_with(&query_words[j]))
        });
        if bounded {
            return W_WORD_BOUNDARY;
        }
    }

    if field_lower.contains(query_lower) {
        return W_SUBSTRING;
    }

    // fuzzy only for queries of 3+ chars; short queries must match a strong
    // tier or they are simply not a hit
    if query_chars.len() >= 3 {
        let fs = fuzzy_score(field_lower, field_words, query_chars);
        if fs > 0.0 {
            return (fs * W_FUZZY as f64) as u32;
        }
    }
    0
}

/// Edit-distance similarity (0..1) between a whole text or any of its words
/// and the query, within a tight per-length tolerance window.
fn fuzzy_score(text: &str, text_words: &[String], query_chars: &[char]) -> f64 {
    let max_dist = match query_chars.len() {
        3 => 1,
        4..=6 => 2,
        _ => 3,
    };
    let text_chars: Vec<char> = text.chars().collect();

    let mut best = 0.0f64;
    if (text_chars.len() as isize - query_chars.len() as isize).unsigned_abs() <= max_dist {
        let dist = levenshtein(&text_chars, query_chars);
        if dist <= max_dist {
            best = 1.0 - dist as f64 / text_chars.len().max(query_chars.len()) as f64;
        }
    }

    for word in text_words {
        if best >= 0.8 {
            break;
        }
        let word_chars: Vec<char> = word.chars().collect();
        if (word_chars.len() as isize - query_chars.len() as isize).unsigned_abs() > max_dist {
            continue;
        }
        let dist = levenshtein(&word_chars, query_chars);
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
    app: &CachedApp,
    query_lower: &str,
    query_chars: &[char],
    query_words: &[String],
) -> u32 {
    if query_lower.is_empty() {
        return 1;
    }

    let mut score = field_score(
        &app.title_lower,
        &app.title_words,
        query_lower,
        query_chars,
        query_words,
    );
    if score == 0
        && let Some(comment) = app.comment_lower.as_deref()
    {
        score = field_score(
            comment,
            &app.comment_words,
            query_lower,
            query_chars,
            query_words,
        ) * 5
            / 10;
    }
    if score == 0
        && let Some(meta) = app.meta.as_ref()
    {
        if let Some(keyword_score) = meta.keywords.iter().find_map(|keyword| {
            let ks = field_score(
                &keyword.lower,
                &keyword.words,
                query_lower,
                query_chars,
                query_words,
            );
            (ks > 0).then_some(ks * 3 / 10)
        }) {
            score = keyword_score;
        }
        if score == 0
            && let Some(generic) = meta.generic_lower.as_deref()
        {
            score = if generic.starts_with(query_lower) {
                W_GENERIC_PREFIX
            } else if generic.contains(query_lower) {
                W_GENERIC
            } else {
                0
            };
        }
    }
    if score == 0 && app.id_lower.contains(query_lower) {
        score = W_ID;
    }
    score
}

/// Read `[Desktop Entry]` keys and `[Desktop Action <id>]` groups. Plain
/// (unlocalised) keys only — the `[xx]` variants follow later and would win.
fn parse_meta(content: &str) -> DesktopMeta {
    let mut generic: Option<String> = None;
    let mut keywords: Vec<Keyword> = Vec::new();
    let mut actions: Vec<DesktopAction> = Vec::new();
    let mut in_entry = false;
    let mut action: Option<DesktopAction> = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(id) = line
            .strip_prefix("[Desktop Action ")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some(prev) = action.replace(DesktopAction {
                id: id.to_string(),
                name: String::new(),
                name_lower: String::new(),
            }) {
                actions.push(prev);
            }
            in_entry = false;
        } else if line.starts_with('[') {
            if let Some(prev) = action.take() {
                actions.push(prev);
            }
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("GenericName=") {
                if generic.is_none() {
                    let v = v.trim();
                    if !v.is_empty() {
                        generic = Some(v.to_string());
                    }
                }
            } else if let Some(v) = line.strip_prefix("Keywords=") {
                let kw = v
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        let lower = s.to_lowercase();
                        Keyword {
                            words: tokenize(&lower),
                            lower,
                        }
                    });
                keywords.extend(kw);
            }
        } else if let Some(a) = action.as_mut()
            && let Some(v) = line.strip_prefix("Name=")
            && a.name.is_empty()
        {
            a.name = v.trim().to_string();
            a.name_lower = a.name.to_lowercase();
        }
    }
    actions.extend(action);

    DesktopMeta {
        generic_lower: generic.map(|g| g.to_lowercase()),
        keywords,
        actions: actions.into_iter().filter(|a| !a.name.is_empty()).collect(),
    }
}

/// Locate the `.desktop` file by id through the XDG data dirs and read the
/// plain `GenericName`/`Keywords` keys. gio-rs does not bind GDesktopAppInfo,
/// so this is the only way to reach them.
///
/// Candidates are tried in precedence order and the first file that actually
/// carries one of the keys wins: a hand-written override in `~/.local/share`
/// that only sets `Name=`/`Exec=` must not hide the packaged copy's keywords.
fn desktop_meta(id: &str) -> Option<DesktopMeta> {
    for candidate in system_fs::desktop_file_candidates(id) {
        let Ok(content) = fs::read_to_string(&candidate) else {
            continue;
        };

        let meta = parse_meta(&content);
        if meta.generic_lower.is_some() || !meta.keywords.is_empty() || !meta.actions.is_empty() {
            return Some(meta);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(generic: Option<&str>, keywords: &[&str]) -> DesktopMeta {
        DesktopMeta {
            generic_lower: generic.map(str::to_lowercase),
            keywords: keywords
                .iter()
                .map(|keyword| {
                    let lower = keyword.to_lowercase();
                    Keyword {
                        words: tokenize(&lower),
                        lower,
                    }
                })
                .collect(),
            actions: Vec::new(),
        }
    }

    /// The same precomputation `APPS` does, for one synthetic app.
    fn s(name: &str, comment: Option<&str>, m: Option<&DesktopMeta>, id: &str, q: &str) -> u32 {
        let title_lower = name.to_lowercase();
        let comment_lower = comment.map(str::to_lowercase);
        let query_lower = q.trim().to_lowercase();
        let query_words = tokenize(&query_lower);
        let query_chars: Vec<char> = query_lower.chars().collect();
        let app = CachedApp {
            id: id.to_string(),
            icon_spec: None,
            title: name.to_string(),
            title_words: tokenize(&title_lower),
            title_lower,
            comment: comment.map(str::to_string),
            comment_words: comment_lower.as_deref().map(tokenize).unwrap_or_default(),
            comment_lower,
            id_lower: id.to_lowercase().trim_end_matches(".desktop").to_string(),
            meta: m.cloned(),
        };
        score_app(&app, &query_lower, &query_chars, &query_words)
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
    fn action_rows_use_dms_tiers() {
        assert_eq!(
            action_score("open vm manager", "open vm manager"),
            W_ACTION_EXACT
        );
        assert_eq!(action_score("open vm manager", "open"), W_ACTION_PREFIX);
        assert_eq!(action_score("open vm manager", "vm"), W_ACTION_SUBSTRING);
        assert_eq!(action_score("open vm manager", "zzz"), 0);
    }

    #[test]
    fn parse_meta_reads_action_groups() {
        let meta = parse_meta(
            "[Desktop Entry]\n\
             GenericName=Virtualization Software\n\
             Keywords=virtualization;\n\
             Actions=Manager;\n\
             Name[de]=Oracle VirtualBox\n\
             \n\
             [Desktop Action Manager]\n\
             Name=Open VM Manager\n\
             Name[de]=VM Manager oeffnen\n\
             Exec=VirtualBox\n\
             \n\
             [Desktop Action Broken]\n\
             Name[de]=Nur auf Deutsch\n",
        );
        assert_eq!(
            meta.generic_lower.as_deref(),
            Some("virtualization software")
        );
        assert_eq!(meta.keywords.len(), 1);
        assert_eq!(meta.keywords[0].lower, "virtualization");
        // localised-only names are not entries of their own
        assert_eq!(meta.actions.len(), 1);
        assert_eq!(meta.actions[0].id, "Manager");
        assert_eq!(meta.actions[0].name, "Open VM Manager");
        assert_eq!(meta.actions[0].name_lower, "open vm manager");
    }

    #[test]
    fn empty_query_matches_everything() {
        let m = meta(None, &[]);
        assert_eq!(s("Anything", None, Some(&m), "a.desktop", ""), 1);
    }
}
