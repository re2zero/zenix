//! The window's short-lived messages: what the client could not say where the
//! action happened.
//!
//! Most failures already have a surface of their own — the trust review, the
//! worktree form, the chat's error card, the settings footer — and those keep
//! their sentence inline. A rejection that arrives after the surface closed,
//! an action taken from a chord or the palette, a request the connection could
//! not even send: those had nowhere to land before U3-5. They land here.
//!
//! The stack itself is the component library's (`Toaster` plus
//! `ToastViewport`): it lowers to the window's bottom-right corner, stacks four
//! at most, holds its clocks while the pointer is over it, and leaves after
//! five seconds. Nothing here polls — a toast is pushed by the pulse that
//! carried the news.
//!
//! The title is this client's and resolves through `locales/zenix.yml`; the
//! body is the daemon's own reason, passed through verbatim the way every
//! daemon sentence is.

use ely_core::feedback::{Toast, ToastViewport, Toaster};
use ely_core::primitives::Severity;
use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, div};

use crate::i18n::t;

/// The stack, drawn with the shell's frame. The library's viewport is its own
/// raised layer above the shell's content, so a message arriving while a dialog
/// is up is still visible; the corner it sits in keeps it off the dialog's own
/// controls.
pub(crate) fn viewport(toaster: &Entity<Toaster>) -> AnyElement {
    div()
        .child(ToastViewport::new("shell-toasts", toaster))
        .into_any_element()
}

/// The daemon answered a request with an error. `reason` is the daemon's
/// message; the title says what kind of event this is.
pub(crate) fn refused(toaster: &Entity<Toaster>, reason: &str, cx: &mut App) {
    push(
        toaster,
        Severity::Warning,
        t("shell.toast.refused"),
        Some(reason.to_owned()),
        cx,
    );
}

/// A command the connection could not send, because there is no daemon to send
/// it to. `what` is the command's own locale label, already resolved.
pub(crate) fn not_sent(toaster: &Entity<Toaster>, what: &str, cx: &mut App) {
    push(
        toaster,
        Severity::Warning,
        rust_i18n::t!("shell.toast.not_sent", what = what).to_string(),
        Some(t("shell.toast.not_sent_body")),
        cx,
    );
}

/// A host request (a worktree or a plugin call) the daemon refused. `title` is
/// the localized family ("Worktree request failed"); the body is the daemon's
/// reason.
pub(crate) fn host_failed(toaster: &Entity<Toaster>, title: String, reason: &str, cx: &mut App) {
    push(
        toaster,
        Severity::Danger,
        title,
        Some(reason.to_owned()),
        cx,
    );
}

fn push(
    toaster: &Entity<Toaster>,
    severity: Severity,
    title: String,
    body: Option<String>,
    cx: &mut App,
) {
    toaster.update(cx, |toaster, cx| {
        let mut toast = Toast::new(title).severity(severity);
        if let Some(body) = body.filter(|body| !body.trim().is_empty()) {
            toast = toast.body(body);
        }
        toaster.push(toast, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// A missing locale key comes back as the key itself, which would read as
    /// `shell.toast.refused` on screen: the test would rather fail.
    #[test]
    fn every_toast_word_resolves() {
        for key in [
            "shell.toast.refused",
            "shell.toast.not_sent",
            "shell.toast.not_sent_body",
            "shell.toast.worktree_failed",
            "shell.toast.plugin_failed",
        ] {
            let copy = t(key);
            assert_ne!(copy, key, "{key} has no translation");
            assert!(!copy.trim().is_empty(), "{key} is blank");
        }
    }

    /// The sentence with a placeholder keeps the command's name in it: the
    /// label is the directory's, already localized, and substituting it is the
    /// whole point of this title.
    #[test]
    fn the_not_sent_title_names_the_command() {
        let title = rust_i18n::t!("shell.toast.not_sent", what = "New Tab").to_string();
        assert!(title.contains("New Tab"), "{title}");
        assert!(!title.contains("%{"), "{title} kept its placeholder");
    }
}
