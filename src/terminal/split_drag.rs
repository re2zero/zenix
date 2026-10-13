//! A drag of the border between panes.
//!
//! Ported in shape from herdr-gpui (Apache-2.0)
//! `crates/herdr-gpui/src/window/mouse.rs` (`SplitDrag`, `split_mouse_*`,
//! `flush_split`). The split is kept as pressed: its area, which the ratio
//! divides, does not move with its own border. `grab` keeps the pointer's place
//! on the border; `want` is the latest ratio not yet sent to the daemon.
//!
//! Two invariants hold it together: only one request travels at a time, and a
//! drag ends as soon as the tab or the pane topology changes, because its path
//! could then name a different border.

use gpui_kit::CursorStyle;
use herdr_client::Method;
use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame, PaneSurfaceSplit};
use serde_json::json;

use super::splits;
use crate::herdr::{Connection, LiveState};

/// What a drag may do at the moment it is flushed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ready {
    /// A request is still travelling, or the state to write against is not in.
    Wait,
    /// Its border can no longer be named: drop the drag.
    Retire,
    /// Send this ratio.
    Send(f32),
}

pub(crate) struct SplitDrag {
    split: PaneSurfaceSplit,
    boot: String,
    tab: String,
    topology: u64,
    grab: f32,
    held: bool,
    want: Option<f32>,
    sent: Option<f32>,
}

impl SplitDrag {
    /// Arm a drag of the border under `(x, y)`, in paint-origin pixels.
    /// `None` where no border is hit, or no tab can be named to resize.
    pub(crate) fn press(
        surface: &PaneSurfaceFrame,
        snapshot: &ClientShellSnapshot,
        x: f32,
        y: f32,
        cell_width: f32,
        cell_height: f32,
    ) -> Option<Self> {
        let split = splits::split_at(surface, x, y, cell_width, cell_height)?.clone();
        let tab = snapshot.focused_tab_id.clone()?;
        let pointer = splits::along(split.direction, x, y);
        let grab = splits::edge(&split, cell_width, cell_height) - pointer;
        Some(Self {
            boot: snapshot.boot_id.clone(),
            tab,
            topology: splits::topology(surface),
            split,
            grab,
            held: true,
            want: None,
            sent: None,
        })
    }

    /// Whether the press is still down.
    pub(crate) fn held(&self) -> bool {
        self.held
    }

    /// The cursor the border under this drag shows.
    pub(crate) fn cursor(&self) -> CursorStyle {
        splits::cursor(self.split.direction)
    }

    /// Follow the pointer, remembering the ratio it asks for unless it is the
    /// one already travelling. Nothing is sent here; [`Self::flush`] decides.
    pub(crate) fn drag(&mut self, x: f32, y: f32, cell_width: f32, cell_height: f32) {
        let pointer = splits::along(self.split.direction, x, y);
        let ratio = splits::ratio(&self.split, pointer + self.grab, cell_width, cell_height);
        if let Some(ratio) = ratio.filter(|ratio| self.sent != Some(*ratio)) {
            self.want = Some(ratio);
        }
    }

    pub(crate) fn release(&mut self) {
        self.held = false;
    }

    /// What a flush may do now, from the state it would write against.
    fn ready(
        &self,
        snapshot: Option<&ClientShellSnapshot>,
        surface: Option<&PaneSurfaceFrame>,
        in_flight: bool,
    ) -> Ready {
        if in_flight {
            return Ready::Wait;
        }
        let Some(ratio) = self.want else {
            // A released drag with nothing left to send is done; a held one
            // keeps waiting for the pointer to move again.
            return if self.held {
                Ready::Wait
            } else {
                Ready::Retire
            };
        };
        // A connection that went away cannot be resized.
        let Some(snapshot) = snapshot else {
            return Ready::Retire;
        };
        if snapshot.boot_id != self.boot {
            return Ready::Retire;
        }
        // A snapshot ahead of its surface is waited out, not taken as a change.
        let Some(surface) = surface else {
            return Ready::Wait;
        };
        if snapshot.focused_tab_id.as_deref() != Some(self.tab.as_str())
            || splits::topology(surface) != self.topology
        {
            return Ready::Retire;
        }
        Ready::Send(ratio)
    }

    /// Send the latest wanted ratio once the previous drag request has
    /// answered. `true` when the drag is finished and must be dropped.
    pub(crate) fn flush(&mut self, live: &LiveState, connection: &Connection) -> bool {
        let ready = self.ready(
            live.snapshot.as_deref(),
            live.surface.as_deref(),
            connection.has_in_flight(),
        );
        match ready {
            Ready::Wait => false,
            Ready::Retire => true,
            Ready::Send(ratio) => {
                let params = json!({
                    "tab_id": self.tab,
                    "path": self.split.path,
                    "ratio": ratio,
                });
                match connection.request(Method::LayoutSetSplitRatio, params) {
                    Some(_) => {
                        self.want = None;
                        self.sent = Some(ratio);
                        false
                    }
                    // The connection was not ready, or the daemon refused the
                    // queue: nothing is travelling, so the drag cannot continue.
                    None => true,
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use herdr_client::protocol::{
        FrameData, PaneSurfacePane, PaneSurfaceSplitDirection, SurfaceRect,
    };

    fn rect(x: u16, y: u16, width: u16, height: u16) -> SurfaceRect {
        SurfaceRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two panes side by side across 80 columns, the border at column 40.
    fn border(path: Vec<bool>) -> PaneSurfaceSplit {
        PaneSurfaceSplit {
            direction: PaneSurfaceSplitDirection::Horizontal,
            pos: 40,
            area: rect(0, 0, 80, 24),
            hit_rect: rect(40, 0, 1, 24),
            path,
        }
    }

    fn pane(pane_id: &str) -> PaneSurfacePane {
        PaneSurfacePane {
            pane_id: pane_id.into(),
            content_revision: 1,
            rect: rect(0, 0, 40, 24),
            inner_rect: rect(0, 0, 40, 24),
            scrollbar_rect: None,
            scroll: None,
            focused: false,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 320,
            pixel_height: 480,
        }
    }

    fn surface(splits: Vec<PaneSurfaceSplit>) -> PaneSurfaceFrame {
        PaneSurfaceFrame {
            boot_id: "boot".into(),
            projection_revision: 1,
            surface_revision: 1,
            frame: FrameData {
                cells: vec![],
                width: 80,
                height: 24,
                cursor: None,
                hyperlinks: vec![],
                graphics: vec![],
            },
            panes: vec![pane("w1:p1"), pane("w1:p2")],
            splits,
            popup: None,
            graphics: Default::default(),
        }
    }

    fn snapshot(boot: &str, focused_tab: Option<&str>) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": boot,
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "focused_tab_id": focused_tab,
            "workspaces": [],
            "tabs": [],
            "panes": [],
            "agents": [],
            "commands": [],
        }))
        .expect("a snapshot with no workspaces still deserializes")
    }

    /// The pixel the border sits at, and one just beside it.
    const ON_BORDER: f32 = 40. * 8.;
    const OFF_BORDER: f32 = ON_BORDER - 1.;

    fn pressed() -> SplitDrag {
        let frame = surface(vec![border(vec![])]);
        SplitDrag::press(
            &frame,
            &snapshot("boot", Some("w1:t1")),
            ON_BORDER,
            100.,
            8.,
            20.,
        )
        .expect("the border is hit")
    }

    #[test]
    fn a_press_arms_only_on_a_border_with_a_tab_to_resize() {
        let frame = surface(vec![border(vec![])]);
        let ready = snapshot("boot", Some("w1:t1"));
        assert!(
            SplitDrag::press(&frame, &ready, ON_BORDER, 100., 8., 20.).is_some(),
            "the border is where the daemon says it is"
        );
        assert!(SplitDrag::press(&frame, &ready, OFF_BORDER, 100., 8., 20.).is_none());
        assert!(SplitDrag::press(&surface(vec![]), &ready, ON_BORDER, 100., 8., 20.).is_none());
        assert!(
            SplitDrag::press(&frame, &snapshot("boot", None), ON_BORDER, 100., 8., 20.).is_none(),
            "a resize names the tab it resizes"
        );
    }

    #[test]
    fn the_pointer_asks_for_the_ratio_under_it() {
        let mut drag = pressed();
        // Pressed on the border: the pointer names the ratio directly.
        drag.drag(ON_BORDER, 100., 8., 20.);
        assert_eq!(drag.want, Some(0.5));
        assert_eq!(
            drag.ready(
                Some(&snapshot("boot", Some("w1:t1"))),
                Some(&surface(vec![border(vec![])])),
                false
            ),
            Ready::Send(0.5)
        );

        // Beyond the range Herdr's layout accepts, the ratio is clamped there.
        drag.drag(ON_BORDER + 320., 100., 8., 20.);
        assert_eq!(drag.want, Some(0.9));
        drag.drag(0., 100., 8., 20.);
        assert_eq!(drag.want, Some(0.1));
    }

    #[test]
    fn a_ratio_already_travelling_is_not_asked_for_again() {
        let mut drag = pressed();
        drag.drag(ON_BORDER, 100., 8., 20.);
        // What a successful flush leaves behind.
        drag.want = None;
        drag.sent = Some(0.5);
        drag.drag(ON_BORDER, 100., 8., 20.);
        assert_eq!(drag.want, None);
        assert_eq!(
            drag.ready(
                Some(&snapshot("boot", Some("w1:t1"))),
                Some(&surface(vec![border(vec![])])),
                false
            ),
            Ready::Wait,
            "a held drag with nothing new to send waits for the pointer"
        );
        // A new place still asks for its own ratio.
        drag.drag(ON_BORDER + 64., 100., 8., 20.);
        assert_eq!(drag.want, Some(0.6));
    }

    #[test]
    fn a_drag_waits_for_the_answer_and_retires_once_released() {
        let frame = surface(vec![border(vec![])]);
        let ready = snapshot("boot", Some("w1:t1"));
        let mut drag = pressed();
        drag.drag(ON_BORDER, 100., 8., 20.);
        assert_eq!(
            drag.ready(Some(&ready), Some(&frame), true),
            Ready::Wait,
            "only one request travels at a time"
        );
        drag.release();
        assert_eq!(drag.ready(Some(&ready), Some(&frame), true), Ready::Wait);
        drag.want = None;
        assert_eq!(drag.ready(Some(&ready), Some(&frame), false), Ready::Retire);
    }

    #[test]
    fn a_drag_ends_when_its_boot_tab_or_topology_changes() {
        let mut drag = pressed();
        drag.drag(ON_BORDER, 100., 8., 20.);
        let frame = surface(vec![border(vec![])]);
        assert_eq!(
            drag.ready(
                Some(&snapshot("other-boot", Some("w1:t1"))),
                Some(&frame),
                false
            ),
            Ready::Retire,
            "a reconnect moves the layout"
        );
        assert_eq!(
            drag.ready(Some(&snapshot("boot", Some("w1:t2"))), Some(&frame), false),
            Ready::Retire,
            "another tab's border is not this one"
        );
        // Same tab, but a pane appeared: the path could name another border.
        let mut grown = frame.clone();
        grown.panes.push(pane("w1:p3"));
        grown.splits.push(border(vec![true]));
        assert_eq!(
            drag.ready(Some(&snapshot("boot", Some("w1:t1"))), Some(&grown), false),
            Ready::Retire
        );
        // No connection at all: nothing left to resize.
        assert_eq!(drag.ready(None, Some(&frame), false), Ready::Retire);
        // A snapshot ahead of its surface is waited out, not taken as a change.
        assert_eq!(
            drag.ready(Some(&snapshot("boot", Some("w1:t1"))), None, false),
            Ready::Wait
        );
    }

    #[test]
    fn the_cursor_shows_the_axis_the_border_moves_along() {
        assert_eq!(pressed().cursor(), CursorStyle::ResizeLeftRight);
        assert!(pressed().held());
    }
}
