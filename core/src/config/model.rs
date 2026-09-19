use super::path;

/// Core behaviour loaded from `~/.config/wayrun/config.toml`. Every field
/// defaults to the compiled-in constant, so an absent file changes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub web_search: WebSearch,
    pub font: Font,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebSearch {
    /// `google` or `duckduckgo`; an unknown value falls back to google.
    pub engine: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    /// The family the UI shapes with; missing glyphs fall back to the system, so
    /// a family covering only the scripts you read keeps the rest out of memory.
    pub family: String,
}

impl Default for WebSearch {
    fn default() -> Self {
        Self {
            engine: "google".to_string(),
        }
    }
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "Source Han Sans CN".to_string(),
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    web_search: WebSearchFile,
    #[serde(default)]
    font: FontFile,
}

#[derive(serde::Deserialize, Default)]
struct WebSearchFile {
    engine: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct FontFile {
    family: Option<String>,
}

impl Config {
    pub(super) fn load() -> Self {
        let mut config = Self::default();
        let Some(path) = path() else {
            return config;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return config;
        };
        let Ok(file) = toml::from_str::<ConfigFile>(&text) else {
            return config;
        };
        config.apply(file);
        config
    }

    fn apply(&mut self, file: ConfigFile) {
        if let Some(engine) = file.web_search.engine {
            self.web_search.engine = engine;
        }
        if let Some(family) = file.font.family.filter(|f| !f.trim().is_empty()) {
            self.font.family = family;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Config {
        let mut config = Config::default();
        config.apply(toml::from_str(src).unwrap());
        config
    }

    #[test]
    fn absent_keys_keep_the_defaults() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn present_keys_override_field_by_field() {
        let config = parse(
            r#"
            [web_search]
            engine = "duckduckgo"

            [font]
            family = "Noto Sans"
            "#,
        );
        assert_eq!(config.web_search.engine, "duckduckgo");
        assert_eq!(config.font.family, "Noto Sans");
        // an absent section keeps its default
        assert_eq!(
            parse("[web_search]\nengine = \"google\"").font,
            Font::default()
        );
    }

    #[test]
    fn a_blank_family_keeps_the_default() {
        assert_eq!(parse("[font]\nfamily = \"  \"").font, Font::default());
    }
}
