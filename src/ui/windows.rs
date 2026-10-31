//! Opening shell windows.
//!
//! Every zenix window is the same shape: geometry remembered for the daemon
//! its launch names, the close guard installed before the first paint, and a
//! [`Root`] over its own endpoint connection. [`crate::run`] opens the first
//! window through here; the `view.new-window` chord and a second launch open
//! further ones. Each window is one more connection face over the daemon its
//! launch names — the daemon owns the workspaces, so the windows on one
//! daemon see the same grid and either can drive it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::*;

use ely_core::shell::guard_close;
use ely_core::theme::ActiveTheme;

use herdr_client::ConnectTarget;

use crate::config;
use crate::platform;
use crate::ui::shell::Root;

/// The name a window's daemon goes by in zenix's own records: the launch's
/// wire word ([`crate::cli::encode_launch`]), which already is the one
/// canonical spelling of a target — the same string a second launch sends
/// over the single-instance socket.
pub(crate) fn target_word(target: &ConnectTarget) -> String {
    crate::cli::encode_launch(target)
}

/// The file the windows' geometries live in, beside `config.json`. A window's
/// frame is remembered state, not a setting — it does not belong in the
/// walked config, and two windows on different daemons must not fight over
/// one `window` section.
fn geometry_path() -> PathBuf {
    platform::zenix_config_dir().join("windows.json")
}

/// One window's remembered frame, the same fields and clamps as the config's
/// `window` section, which stays the fallback for a target with no entry.
type Geometry = config::Window;

/// The geometries on disk, keyed by [`target_word`].
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Saved {
    #[serde(default)]
    windows: BTreeMap<String, Geometry>,
}

fn read_saved(path: &std::path::Path) -> Saved {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_saved(path: &std::path::Path, saved: &Saved) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(saved)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// The frame this daemon's windows closed at last, if one is on record.
pub(crate) fn saved_geometry(target: &ConnectTarget) -> Option<Geometry> {
    read_saved(&geometry_path())
        .windows
        .get(&target_word(target))
        .cloned()
}

/// Remember the frame a window on this daemon closed at.
fn remember_geometry(target: &ConnectTarget, width: f32, height: f32, maximized: bool) {
    let path = geometry_path();
    let mut saved = read_saved(&path);
    let mut geometry = saved
        .windows
        .remove(&target_word(target))
        .unwrap_or_default();
    geometry.set_bounds(width, height, maximized);
    saved.windows.insert(target_word(target), geometry);
    if let Err(error) = write_saved(&path, &saved) {
        tracing::warn!(%error, path = %path.display(), "the window's geometry could not be saved");
    }
}

/// The options a shell window opens with. The frame remembered for this
/// window's daemon decides its size and maximized state, written by the
/// closing window itself; a daemon with no entry on record opens at the
/// config's `window` section. The position is the compositor's to choose.
/// The shell draws the whole frame itself, the window controls included, so
/// the window asks for client-side decorations. Where the platform cannot
/// honor that — on X11, a compositor that does not advertise GTK frame
/// extents (KWin with desktop effects off among them) — gpui falls back to
/// the window manager's own decorations and the chrome hides its controls
/// instead of stacking two title bars. macOS keeps its traffic lights drawn
/// over our strip, at the origin the theme reserves for them.
pub(crate) fn window_options(cx: &App, target: &ConnectTarget) -> WindowOptions {
    let fallback = config::get(cx).window.clone();
    let window_config = saved_geometry(target).unwrap_or(fallback);
    let window_size = size(px(window_config.width), px(window_config.height));
    let window_bounds = if window_config.maximized {
        WindowBounds::Maximized(Bounds {
            origin: Default::default(),
            size: window_size,
        })
    } else {
        WindowBounds::centered(window_size, cx)
    };
    let titlebar = TitlebarOptions {
        title: Some(window_title(target).into()),
        appears_transparent: true,
        traffic_light_position: Some(cx.theme().traffic_light_origin()),
    };
    WindowOptions {
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(640.), px(400.))),
        titlebar: Some(titlebar),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}

/// The name the window manager and the taskbar see: the brand, and — when the
/// window's daemon is not the default local one — the target that names it.
pub(crate) fn window_title(target: &ConnectTarget) -> String {
    match target {
        ConnectTarget::Local => String::from("zenix"),
        ConnectTarget::Session { name, .. } => format!("zenix — {name}"),
        ConnectTarget::Socket(path) => {
            let tail = path
                .parent()
                .and_then(|dir| dir.file_name())
                .map(|dir| dir.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            format!("zenix — socket {tail}")
        }
        ConnectTarget::Ssh { target, .. } => format!("zenix — ssh {target}"),
        ConnectTarget::Wsl { distro, .. } => format!("zenix — wsl {distro}"),
    }
}

/// The short identity the chrome draws beside the brand when this window's
/// daemon is not the default local one.
pub(crate) fn target_label(target: &ConnectTarget) -> Option<String> {
    match target {
        ConnectTarget::Local => None,
        ConnectTarget::Session { name, .. } => Some(name.clone()),
        ConnectTarget::Socket(path) => Some(
            path.parent()
                .and_then(|dir| dir.file_name())
                .map(|dir| dir.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
        ),
        ConnectTarget::Ssh { target, .. } => Some(format!("ssh {target}")),
        ConnectTarget::Wsl { distro, .. } => Some(format!("wsl {distro}")),
    }
}

/// Open one more shell window and bring it to the front. Both of the new
/// window's close paths — the strip's own button and the window manager's —
/// save its geometry through the close guard, which goes in before the shell
/// because a close can arrive before the first paint.
pub(crate) fn open_shell_window(
    cx: &mut App,
    target: ConnectTarget,
) -> anyhow::Result<WindowHandle<Root>> {
    let options = window_options(cx, &target);
    cx.open_window(options, |window, cx| {
        window.activate_window();
        install_close_guard(window, cx, target.clone());
        cx.new(|cx| Root::new(window, target.clone(), cx))
    })
}

/// Raise an open window instead of opening one. `Some(target)` names the
/// daemon whose window comes up; `None` brings up the frontmost one. The
/// platform's own stacking order is the preference when it has one — X11
/// reports it, Wayland does not and any window answers. Answers `false` when
/// no window matches, so the ask's caller can open one instead.
pub(crate) fn focus_window(cx: &mut App, target: Option<&ConnectTarget>) -> bool {
    let mut stack = cx.window_stack().unwrap_or_else(|| cx.windows());
    if let Some(target) = target {
        stack.retain(|handle| window_target(cx, *handle).is_some_and(|own| own == *target));
    }
    let Some(handle) = stack.first().copied() else {
        return false;
    };
    handle
        .update(cx, |_, window, _| window.activate_window())
        .is_ok()
}

/// The daemon a window is a face of, if it is still open to ask.
fn window_target(cx: &App, handle: AnyWindowHandle) -> Option<ConnectTarget> {
    handle
        .read(cx, |root: Entity<Root>, cx| {
            root.read(cx).connection_target().clone()
        })
        .ok()
}

/// Put the window's shape on record for the daemon it is connected to, so the
/// next launch of the same target opens where this one closed.
///
/// The size is the *inner* one on purpose. `WindowOptions::window_bounds` is
/// the content area, while X11 (and Wayland) report the outer window from
/// `window_bounds()` — the client decorations' 10px resize margin per side is
/// part of it. Saving the outer size made every restart re-add that margin,
/// so the window grew 20px a launch; `inner_window_bounds()` subtracts the
/// insets and the round trip is stable. Elsewhere the two are the same.
pub(crate) fn remember_window_geometry(window: &Window, target: &ConnectTarget) {
    let size = match window.inner_window_bounds() {
        WindowBounds::Windowed(bounds)
        | WindowBounds::Maximized(bounds)
        | WindowBounds::Fullscreen(bounds) => bounds.size,
    };
    let maximized = window.is_maximized();
    remember_geometry(target, size.width.as_f32(), size.height.as_f32(), maximized);
}

/// Register the close guard the shell's two close paths share, and name the
/// daemon the geometry is remembered for. Called from inside the window's
/// construction, before the shell exists — a close can arrive before the
/// first paint.
pub(crate) fn install_close_guard(window: &mut Window, cx: &mut App, target: ConnectTarget) {
    guard_close(window, cx, move |window, _cx| {
        remember_window_geometry(window, &target);
        true
    });
}

#[cfg(test)]
mod tests {
    // Explicit: a `use super::*` would drag `gpui::test` in (windows.rs globs
    // gpui above) and the built-in `#[test]` would resolve to it instead.
    use super::{Geometry, Saved, read_saved, target_word, write_saved};
    use crate::config;
    use herdr_client::ConnectTarget;

    /// The side-car is keyed by the launch word, so two windows on different
    /// daemons remember their frames apart, and a third target reads as none.
    #[test]
    fn the_side_car_keeps_each_daemons_frame_apart() {
        let path = std::env::temp_dir().join("zenix-windows-test-saved.json");
        let local = ConnectTarget::Local;
        let dev = ConnectTarget::Session {
            name: String::from("dev"),
            development: false,
        };

        let mut saved = Saved::default();
        let mut frame = Geometry::default();
        frame.set_bounds(1400., 900., false);
        saved.windows.insert(target_word(&local), frame.clone());
        let mut other = Geometry::default();
        other.set_bounds(800., 600., true);
        saved.windows.insert(target_word(&dev), other);
        write_saved(&path, &saved).expect("the side-car writes");

        let read = read_saved(&path);
        let remembered_local = read
            .windows
            .get(&target_word(&local))
            .expect("local remembered");
        assert_eq!(
            (remembered_local.width, remembered_local.height),
            (1400., 900.)
        );
        let remembered_dev = read
            .windows
            .get(&target_word(&dev))
            .expect("dev remembered");
        assert!(remembered_dev.maximized);
        assert!(
            !read.windows.contains_key(&target_word(&ConnectTarget::Ssh {
                target: String::from("ops@box"),
                session: String::from("default"),
            }))
        );

        std::fs::remove_file(&path).ok();
    }

    /// A file a hand edit or a crash mangled must not block a window opening:
    /// it reads as empty, and the config's `window` section answers instead.
    #[test]
    fn a_broken_side_car_reads_as_empty() {
        let path = std::env::temp_dir().join("zenix-windows-test-broken.json");
        std::fs::write(&path, "{ not json").expect("the broken file writes");

        assert!(read_saved(&path).windows.is_empty());

        std::fs::remove_file(&path).ok();
    }

    /// A remembered frame is clamped into the window's own minimum, the way
    /// the config's `window` section always was.
    #[test]
    fn a_remembered_frame_is_clamped_to_the_window_minimum() {
        let mut frame = Geometry::default();
        frame.set_bounds(200., 100., false);
        assert_eq!(
            (frame.width, frame.height),
            (config::WINDOW_SIZE_MIN.0, config::WINDOW_SIZE_MIN.1)
        );
    }
}
