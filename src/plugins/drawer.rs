//! The plugin drawer: one plugin's page, docked to the window's right edge.
//!
//! The manager dialog is where a plugin is governed; the drawer is where one
//! is *looked at*. The prototype draws the same detail twice — as the
//! manager's second column and as a `.drawer` beside the shell — and so does
//! this client: the body is [`super::manager::body`], rendered at the drawer's
//! width. A row in the plugins view and a status segment in the status bar
//! both open it, because both are a glance at one plugin that deserves the
//! whole page a glance can open in place.
//!
//! Like the other overlays it takes the keyboard while it is open (Escape is
//! its only keystroke) and a press outside it closes it. It wears no scrim:
//! the shell stays fully visible beside it, which is what makes the drawer a
//! glance rather than a mode.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

use std::rc::Rc;

use super::host::PluginState;
use super::manager::{self, KeyHandler};
use super::model::{Availability, InstalledPlugin};
use super::panels::button;
use crate::i18n::t;
use crate::shell::registry::{Dispatch, ShellRequest};

/// The drawer as the shell holds it: whether it is up, and which plugin it
/// shows.
#[derive(Default)]
pub(crate) struct PluginDrawer {
    open: bool,
    plugin_id: Option<String>,
}

impl PluginDrawer {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open on one plugin. Unlike the manager there is no "first one" to fall
    /// back to: the drawer exists because something named this plugin, so a
    /// plugin that is not installed any more shows nothing (the shell skips
    /// the overlay) rather than surprising whoever clicked.
    pub(crate) fn open(&mut self, plugin_id: &str) {
        self.open = true;
        self.plugin_id = Some(plugin_id.to_owned());
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.plugin_id = None;
    }

    /// The plugin the drawer is about, or `None` when it is not installed —
    /// the registry, not this state, decides what exists.
    fn selected<'a>(&self, plugins: &'a [InstalledPlugin]) -> Option<&'a InstalledPlugin> {
        let id = self.plugin_id.as_deref()?;
        plugins.iter().find(|plugin| plugin.plugin_id == id)
    }
}

/// The drawer's keyboard, as the shell holds it: the focus it tracks and the
/// one keystroke handler it answers — the same bundle the manager rides in
/// with.
pub(crate) struct DrawerKeys<'a> {
    pub(crate) focus: &'a FocusHandle,
    pub(crate) key: KeyHandler,
}

/// The drawer, docked to the right edge over the shell. The keyboard rides in
/// with [`DrawerKeys`], so a keystroke means the same here as it does in the
/// manager. `None` when the drawer's plugin is not installed: the shell
/// renders no overlay at all.
pub(crate) fn overlay(
    drawer: &PluginDrawer,
    state: &PluginState,
    availability: Availability,
    dispatch: &Dispatch,
    keys: DrawerKeys<'_>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let theme = cx.theme().clone();
    let plugin = drawer.selected(&state.plugins)?;
    let close = dispatch.clone();
    // The prototype's own numbers: min(460px, 94vw) wide, the window's full
    // height, against the window's right edge.
    let viewport = window.viewport_size();
    let width = px(460.).min(viewport.width * 0.94);
    let plugin_id = plugin.plugin_id.clone();
    let enabled = plugin.enabled;
    let root = plugin.plugin_root.clone();
    let boxed = v_flex()
        .id("plugin-drawer")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(width)
        .max_w_full()
        .overflow_hidden()
        .bg(theme.background)
        .text_color(theme.foreground)
        .border_l_1()
        .border_color(theme.border)
        .shadow(vec![
            BoxShadow::new(px(-18.), px(0.), black().opacity(0.35)).blur_radius(px(48.)),
        ])
        // A press anywhere outside the drawer closes it, exactly as Escape
        // does; the drawer occludes, so the press lands on this overlay and
        // the shell underneath reads nothing.
        .on_mouse_down_out({
            let close = close.clone();
            move |event, window, cx| {
                if event.button == MouseButton::Left {
                    close(ShellRequest::ClosePluginDrawer, window, cx);
                }
            }
        })
        .track_focus(keys.focus)
        .on_key_down({
            let key = Rc::clone(&keys.key);
            move |event, window, cx| key(event, window, cx)
        })
        .child(
            h_flex()
                .w_full()
                .flex_none()
                .h(px(40.))
                .px(px(12.))
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_none()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t("shell.drawer.title")),
                )
                .child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_size(px(10.5))
                        .text_color(theme.muted_foreground)
                        .child(plugin.plugin_id.clone()),
                )
                .child(div().flex_1())
                .child(
                    button("plugin-drawer-manage", &t("shell.drawer.manage"), &theme).on_click({
                        let open = dispatch.clone();
                        let plugin_id = plugin_id.clone();
                        move |_, window, cx| {
                            open(
                                ShellRequest::OpenPluginManager(Some(plugin_id.clone())),
                                window,
                                cx,
                            )
                        }
                    }),
                )
                .child(
                    button("plugin-drawer-close", &t("settings.close"), &theme).on_click({
                        let close = close.clone();
                        move |_, window, cx| close(ShellRequest::ClosePluginDrawer, window, cx)
                    }),
                ),
        )
        // One plugin's page, at drawer width — the same body the manager's
        // second column shows, so what a glance opened is what the dialog
        // would say.
        .child(manager::body(plugin, state, availability, &theme, dispatch))
        .child(
            h_flex()
                .w_full()
                .flex_none()
                .px(px(12.))
                .py(px(6.))
                .gap_2()
                .items_center()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    button(
                        "plugin-drawer-toggle",
                        &if enabled {
                            t("shell.manager.disable")
                        } else {
                            t("shell.manager.enable")
                        },
                        &theme,
                    )
                    .on_click({
                        let run = dispatch.clone();
                        let plugin_id = plugin_id.clone();
                        move |_, window, cx| {
                            run(
                                ShellRequest::SetPluginEnabled {
                                    plugin_id: plugin_id.clone(),
                                    enabled: !enabled,
                                },
                                window,
                                cx,
                            )
                        }
                    }),
                )
                .child(
                    button("plugin-drawer-review", &t("shell.manager.argv"), &theme).on_click({
                        let review = dispatch.clone();
                        move |_, window, cx| {
                            review(ShellRequest::OpenPluginTrustAt(root.clone()), window, cx)
                        }
                    }),
                )
                .child(
                    button("plugin-drawer-unlink", &t("shell.manager.unlink"), &theme).on_click({
                        let run = dispatch.clone();
                        move |_, window, cx| {
                            run(ShellRequest::UnlinkPlugin(plugin_id.clone()), window, cx)
                        }
                    }),
                ),
        );
    // The overlay is the drawer itself plus the occluding plane that closes
    // it: no scrim, so the shell beside it stays as readable as it was.
    Some(
        v_flex()
            .id("plugin-drawer-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .size_full()
            .child(boxed)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// The drawer is about one named plugin: opening it twice retargets it,
    /// and a plugin that has been unlinked leaves nothing to show rather than
    /// falling back to some other plugin's page. Closing forgets the plugin —
    /// the next open names its own.
    #[test]
    fn the_drawer_shows_the_plugin_it_was_opened_on_or_nothing() {
        let mut drawer = PluginDrawer::default();
        assert!(drawer.selected(&[]).is_none());

        drawer.open("a");
        assert!(drawer.is_open());
        let installed = vec![InstalledPlugin::fixture("a"), InstalledPlugin::fixture("b")];
        assert_eq!(
            drawer.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("a")
        );

        drawer.open("b");
        assert_eq!(
            drawer.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("b")
        );

        drawer.close();
        assert!(!drawer.is_open());
        assert!(drawer.selected(&installed).is_none());

        // The plugin going away while the drawer is open reads the same way.
        drawer.open("gone");
        assert!(drawer.is_open());
        assert!(drawer.selected(&installed).is_none());
    }
}
