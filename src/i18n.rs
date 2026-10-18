//! Internationalization support using rust-i18n.
//!
//! Call `set_locale` early (from saved config or LANG env var)
//! to switch the active locale. All user-visible strings should
//! use `t("key")` so they are resolved through the locale YAML.

/// Switch the active locale at runtime.
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

/// Return the current active locale (e.g. "en", "zh-CN").
pub fn locale() -> String {
    rust_i18n::locale().to_string()
}

/// Resolve a dot-separated i18n key from the active locale YAML.
///
/// Returns the key verbatim when no translation is found so
/// missing strings are obvious rather than silently empty.
pub fn t(key: &str) -> String {
    rust_i18n::t!(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// Measured against rust-i18n 4.2.4: `%{name}` is the only form it
    /// substitutes. A bare `{name}` is passed through verbatim, so a string
    /// written in the older form shows its braces to the user. A key called
    /// without its argument keeps the marker as well.
    #[test]
    fn a_placeholder_is_substituted_only_in_the_percent_form() {
        assert_eq!(
            rust_i18n::t!("settings.config.problems_short", count = 2).to_string(),
            "config: 2 problems"
        );
        assert_eq!(
            rust_i18n::t!("status.disconnected", reason = "socket closed").to_string(),
            "Disconnected: socket closed"
        );
        assert_eq!(t("status.disconnected"), "Disconnected: %{reason}");
        assert_eq!(
            rust_i18n::replace_patterns("count is {count}", &["count"], &["3".to_string()]),
            "count is {count}"
        );
    }
}
