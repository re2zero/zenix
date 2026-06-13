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
