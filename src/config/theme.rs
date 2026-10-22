//! The chrome's theme families.
//!
//! Four gpui-component theme sets ride in the binary (`include_str!` over
//! `assets/themes/`; the files are not packaged, nothing reads them from
//! disk). [`register`] hands them to gpui-kit's [`ThemeRegistry`] once;
//! [`apply`] points the global theme's light and dark slots at the family's
//! configs and lets [`Theme::change`] reload whichever mode is current.
//!
//! A family that lacks a mode's variant falls back to gpui-kit's built-in
//! slot for that mode: Matrix ships dark only, and the Tokyo Night set
//! carries three dark variants of which the family's face is Tokyo Night.

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};

/// The theme families a config can name, in picker order. `default` is
/// gpui-kit's own light and dark pair.
pub const NAMES: &[&str] = &["default", "gruvbox", "solarized", "tokyonight", "matrix"];

const SETS: &[&str] = &[
    include_str!("../../assets/themes/gruvbox.json"),
    include_str!("../../assets/themes/solarized.json"),
    include_str!("../../assets/themes/tokyonight.json"),
    include_str!("../../assets/themes/matrix.json"),
];

/// The variant config a family carries for a mode, if it carries one at all.
fn variant(family: &str, dark: bool) -> Option<&'static str> {
    match (family, dark) {
        ("gruvbox", false) => Some("Gruvbox Light"),
        ("gruvbox", true) => Some("Gruvbox Dark"),
        ("solarized", false) => Some("Solarized Light"),
        ("solarized", true) => Some("Solarized Dark"),
        ("tokyonight", true) => Some("Tokyo Night"),
        ("matrix", true) => Some("Matrix"),
        _ => None,
    }
}

/// Hand the embedded sets to the registry. A set that fails to parse logs and
/// leaves the registry untouched, so its families fall back to the built-in
/// slots; the name contract against the files is the tests' job.
pub(crate) fn register(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    for set in SETS {
        if let Err(error) = registry.load_themes_from_str(set) {
            tracing::error!("theme set failed to parse: {error}");
        }
    }
}

/// Point the chrome at `family` in `mode`. Both slots move — a later mode
/// toggle stays inside the family — then the mode's config is applied.
pub(crate) fn apply(family: &str, mode: ThemeMode, cx: &mut App) {
    let (light, dark) = {
        let registry = ThemeRegistry::global(cx);
        let slot = |dark: bool| {
            variant(family, dark)
                .and_then(|name| registry.themes().get(name).cloned())
                .unwrap_or_else(|| {
                    if dark {
                        registry.default_dark_theme().clone()
                    } else {
                        registry.default_light_theme().clone()
                    }
                })
        };
        (slot(false), slot(true))
    };
    {
        let theme = Theme::global_mut(cx);
        theme.light_theme = light;
        theme.dark_theme = dark;
    }
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `variant()` and the embedded files must agree: a family's promised
    /// variant has to exist in the sets under the declared mode, or `apply`
    /// would silently fall back to the built-in slot while the picker still
    /// offers the family.
    #[test]
    fn variants_are_what_the_embedded_sets_carry() {
        let mut carried: Vec<(String, String)> = Vec::new();
        for set in SETS {
            let parsed: serde_json::Value = serde_json::from_str(set).expect("embedded set parses");
            for theme in parsed["themes"].as_array().expect("themes array") {
                carried.push((
                    theme["name"].as_str().expect("name").to_owned(),
                    theme["mode"].as_str().expect("mode").to_owned(),
                ));
            }
        }
        for family in NAMES {
            for dark in [false, true] {
                let mode = if dark { "dark" } else { "light" };
                match variant(family, dark) {
                    Some(name) => assert!(
                        carried.contains(&(name.to_owned(), mode.to_owned())),
                        "{family}/{mode} promises {name} but the sets do not carry it"
                    ),
                    None => continue,
                }
            }
        }
        assert_eq!(
            carried,
            vec![
                ("Gruvbox Light".to_owned(), "light".to_owned()),
                ("Gruvbox Dark".to_owned(), "dark".to_owned()),
                ("Solarized Light".to_owned(), "light".to_owned()),
                ("Solarized Dark".to_owned(), "dark".to_owned()),
                ("Tokyo Night".to_owned(), "dark".to_owned()),
                ("Tokyo Storm".to_owned(), "dark".to_owned()),
                ("Tokyo Moon".to_owned(), "dark".to_owned()),
                ("Matrix".to_owned(), "dark".to_owned()),
            ],
            "the embedded sets drifted; review variant() against them"
        );
    }

    #[test]
    fn families_resolve_per_mode_and_fall_back_honestly() {
        assert_eq!(variant("default", false), None);
        assert_eq!(variant("default", true), None);
        assert_eq!(variant("gruvbox", false), Some("Gruvbox Light"));
        assert_eq!(variant("gruvbox", true), Some("Gruvbox Dark"));
        assert_eq!(variant("solarized", false), Some("Solarized Light"));
        assert_eq!(variant("solarized", true), Some("Solarized Dark"));
        // Dark-only families leave the light slot to the built-in pair.
        assert_eq!(variant("tokyonight", false), None);
        assert_eq!(variant("tokyonight", true), Some("Tokyo Night"));
        assert_eq!(variant("matrix", false), None);
        assert_eq!(variant("matrix", true), Some("Matrix"));
        // An unknown family resolves nothing anywhere.
        assert_eq!(variant("nord", false), None);
        assert_eq!(variant("nord", true), None);
    }
}
