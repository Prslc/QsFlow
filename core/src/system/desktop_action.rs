//! Desktop actions: the `[Desktop Action <id>]` groups of a `.desktop` file.
//!
//! gio binds no desktop-action launcher, so the entry is read here and its argv
//! is handed to the detached runner with no shell in between.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use freedesktop_desktop_entry::{DesktopEntry, get_languages_from_env};

/// Every path a desktop id may live at, in XDG precedence order: the user's own
/// data dir first, then `$XDG_DATA_DIRS` (which `fs::ensure_flatpak_data_dirs`
/// has already padded with the flatpak exports).
fn candidates(id: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(data) = dirs::data_dir() {
        roots.push(data);
    }
    if let Ok(list) = std::env::var("XDG_DATA_DIRS") {
        roots.extend(list.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    } else {
        roots.push(PathBuf::from("/usr/local/share"));
        roots.push(PathBuf::from("/usr/share"));
    }
    let names: Vec<String> = if id.ends_with(".desktop") {
        vec![id.to_owned()]
    } else {
        vec![format!("{id}.desktop")]
    };
    roots
        .into_iter()
        .flat_map(|root| {
            names
                .iter()
                .map(move |name| root.join("applications").join(name))
        })
        .collect()
}

/// The most preferred `.desktop` file for `id`, user overrides included.
pub fn find(id: &str) -> Option<PathBuf> {
    candidates(id).into_iter().find(|path| path.is_file())
}

/// What this entry's field codes stand for. `%i` names the action group's own
/// `Icon=` when it has one, as the spec's action groups allow.
struct Codes<'a> {
    name: Option<Cow<'a, str>>,
    icon: Option<&'a str>,
    path: String,
}

impl<'a> Codes<'a> {
    fn of(entry: &'a DesktopEntry, action_id: &str, path: &Path, locales: &[String]) -> Self {
        Self {
            name: entry.name(locales),
            icon: entry
                .action_entry(action_id, "Icon")
                .or_else(|| entry.icon()),
            path: path.display().to_string(),
        }
    }

    /// One argument of the parsed `Exec=`, expanded into zero, one or two
    /// arguments of the argv. `%i` is the only code that becomes two.
    fn push(&self, arg: &str, argv: &mut Vec<String>) {
        if arg == "%i" {
            if let Some(icon) = self.icon {
                argv.push("--icon".to_owned());
                argv.push(icon.to_owned());
            }
            return;
        }
        let mut out = String::with_capacity(arg.len());
        let mut chars = arg.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(self.name.as_deref().unwrap_or("")),
                Some('k') => out.push_str(&self.path),
                // `%f`/`%u`/`%F`/`%U` stand for a file or URI this row does not
                // carry, `%i` was handled above, and anything else is unknown:
                // all of them drop out.
                Some(_) | None => {}
            }
        }
        // An argument that was nothing but field codes is gone, not empty.
        if !out.is_empty() {
            argv.push(out);
        }
    }
}

/// The argv of one `Exec=`, with its field codes expanded. The split is
/// `g_shell_parse_argv` — the same function gio parses `Exec=` with on the
/// `launch:` path — because the spec's argument quoting is not the shell's:
/// `Exec=foo "a;b"` is two arguments with a literal semicolon, and nothing in
/// the line is ever syntax.
fn expand(exec: &str, entry: &DesktopEntry, action_id: &str, path: &Path) -> Vec<String> {
    let Ok(parsed) = gio::glib::shell_parse_argv(exec) else {
        return Vec::new();
    };
    let locales = get_languages_from_env();
    let codes = Codes::of(entry, action_id, path, &locales);
    let mut argv = Vec::with_capacity(parsed.len());
    for arg in parsed {
        codes.push(&arg.to_string_lossy(), &mut argv);
    }
    argv
}

/// The argv of `[Desktop Action <action_id>]`, or `None` when the entry carries
/// no such group or its `Exec=` expands to nothing.
pub fn action_argv(desktop_id: &str, action_id: &str) -> Option<Vec<String>> {
    let path = find(desktop_id)?;
    let entry = DesktopEntry::from_path(&path, None::<&[String]>).ok()?;
    let argv = expand(entry.action_exec(action_id)?, &entry, action_id, &path);
    (!argv.is_empty()).then_some(argv)
}

/// Runs one action of one desktop entry, returning the argv it ran.
pub fn launch(desktop_id: &str, action_id: &str) -> Option<Vec<String>> {
    let argv = action_argv(desktop_id, action_id)?;
    crate::system::executor::execute_argv(&argv);
    Some(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "\
[Desktop Entry]
Name=Nautilus
Icon=org.gnome.Nautilus
Exec=nautilus --new-window
Actions=new-window;tab;icon;

[Desktop Action new-window]
Name=New Window
Exec=nautilus --new-window %U

[Desktop Action tab]
Name=New Tab
Exec=nautilus --tab \"a b;c\"

[Desktop Action icon]
Name=Icon
Exec=nautilus %i --profile \"100%% sure\"
";

    fn argv(action_id: &str) -> Vec<String> {
        let path = Path::new("/usr/share/applications/org.gnome.Nautilus.desktop");
        let entry = DesktopEntry::from_str(path, ENTRY, None::<&[String]>).unwrap();
        expand(
            entry.action_exec(action_id).unwrap_or(""),
            &entry,
            action_id,
            path,
        )
    }

    #[test]
    fn an_action_takes_its_own_exec_and_loses_the_path_codes() {
        assert_eq!(
            argv("new-window"),
            ["nautilus", "--new-window"],
            "%U names a URI the row does not carry"
        );
        assert_eq!(argv("tab"), ["nautilus", "--tab", "a b;c"]);
    }

    #[test]
    fn the_plain_entry_is_not_an_action() {
        assert!(argv("").is_empty());
        assert!(argv("does-not-exist").is_empty());
    }

    #[test]
    fn a_quoted_argument_is_one_argument_and_a_percent_stays_literal() {
        assert_eq!(
            argv("icon"),
            [
                "nautilus",
                "--icon",
                "org.gnome.Nautilus",
                "--profile",
                "100% sure"
            ]
        );
    }

    #[test]
    fn a_semicolon_is_not_a_command_separator() {
        // The whole point of the argv runner: `;` is an argument, not syntax.
        assert_eq!(argv("tab")[2], "a b;c");
    }
}
