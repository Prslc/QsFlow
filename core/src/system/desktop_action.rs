//! Desktop actions: the `[Desktop Action <id>]` groups of a `.desktop` file.
//!
//! gio binds no desktop-action launcher, so the lookup and the `Exec=` parse
//! live here. The command itself goes through the same detached runner as every
//! other row, so nothing here spawns a process itself.

use std::path::PathBuf;

/// Every path a desktop id may live at, in XDG precedence order: the user's own
/// data dir first, then `$XDG_DATA_DIRS` (which `fs::ensure_flatpak_data_dirs`
/// has already padded with the flatpak exports).
fn candidates(id: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(data) = dirs::data_dir() {
        roots.push(data);
    }
    match std::env::var("XDG_DATA_DIRS") {
        Ok(list) => roots.extend(list.split(':').filter(|s| !s.is_empty()).map(PathBuf::from)),
        Err(_) => {
            roots.push(PathBuf::from("/usr/local/share"));
            roots.push(PathBuf::from("/usr/share"));
        }
    }
    let names: Vec<String> = if id.ends_with(".desktop") {
        vec![id.to_owned()]
    } else {
        vec![format!("{id}.desktop")]
    };
    roots
        .into_iter()
        .flat_map(|root| names.iter().map(move |name| root.join("applications").join(name)))
        .collect()
}

/// The most preferred `.desktop` file for `id`, user overrides included.
pub fn find(id: &str) -> Option<PathBuf> {
    candidates(id).into_iter().find(|path| path.is_file())
}

/// The `Exec=` line of `[Desktop Action <action_id>]`, with its field codes
/// removed: an action row carries no file or URI to substitute, and the codes
/// that would name one (`%f`, `%u`, `%F`, `%U`) are meaningless here.
pub fn action_exec(contents: &str, action_id: &str) -> Option<String> {
    let wanted = format!("[Desktop Action {action_id}]");
    let mut in_group = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == wanted;
            continue;
        }
        if !in_group {
            continue;
        }
        if let Some(value) = line.strip_prefix("Exec=") {
            let exec = expand_field_codes(value);
            if !exec.is_empty() {
                return Some(exec);
            }
        }
    }
    None
}

fn expand_field_codes(exec: &str) -> String {
    let mut stripped = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            stripped.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => stripped.push('%'),
            // `%i` stands for `--icon <Icon>`; the rest stand for paths.
            Some(_) | None => {}
        }
    }
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Runs one action of one desktop entry. Returns the command it ran.
pub fn launch(desktop_id: &str, action_id: &str) -> Option<String> {
    let path = find(desktop_id)?;
    let contents = std::fs::read_to_string(path).ok()?;
    let exec = action_exec(&contents, action_id)?;
    crate::system::executor::execute_command(&exec);
    Some(exec)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "\
[Desktop Entry]
Name=Nautilus
Exec=nautilus --new-window
Actions=new-window;tab;

[Desktop Action new-window]
Name=New Window
Exec=nautilus --new-window %U

[Desktop Action tab]
Name=New Tab
Exec=nautilus --tab
";

    #[test]
    fn an_action_takes_its_own_exec_and_loses_the_field_codes() {
        assert_eq!(
            action_exec(ENTRY, "new-window").as_deref(),
            Some("nautilus --new-window"),
            "%U names a path the row does not carry"
        );
        assert_eq!(action_exec(ENTRY, "tab").as_deref(), Some("nautilus --tab"));
    }

    #[test]
    fn the_plain_entry_is_not_an_action() {
        assert_eq!(action_exec(ENTRY, "").as_deref(), None);
        assert_eq!(action_exec(ENTRY, "does-not-exist"), None);
    }

    #[test]
    fn a_doubled_percent_stays() {
        assert_eq!(expand_field_codes("sh -c 'echo 100%%'"), "sh -c 'echo 100%'");
    }
}
