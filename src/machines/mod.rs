//! The machines cockpit's data: one read-only endpoint connection per machine
//! the daemon's catalog names, beside the window's own.
//!
//! Every other surface of this client answers "what is this daemon doing";
//! this one answers "what is everything running" — the question the saved-host
//! catalog exists for. The roster is this window's machine first, then the
//! catalog's enabled SSH hosts in the catalog's own order (the ordering the
//! old D15 default recorded: group by machine, daemon order within a group),
//! minus a catalog entry that *is* this window's machine, whose live state the
//! window's own connection already carries.
//!
//! Each remote machine gets one [`Connection`] — the same reader a shell
//! window runs — but nothing is ever sent through it: the cockpit reads
//! pushed snapshots and draws a roster. That keeps the slice honest about the
//! lanes a remote target has (W7: endpoint yes, JSON API no) — a roster needs
//! only the endpoint. The connections live exactly as long as the sheet is
//! open: closing it sets the stop flag their readers check, and the window's
//! end sets it a last time through `Drop`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::Sender;
use herdr_client::{ConnectTarget, SavedHost};

use crate::herdr::Connection;

/// One row of the roster: what to call the machine and how to reach it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Machine {
    pub(crate) label: String,
    pub(crate) target: ConnectTarget,
    /// This window's own machine: its live state comes from the shell's
    /// connection, and no second reader is started for it.
    pub(crate) own: bool,
}

/// The roster, in draw order: this machine first, then the catalog's enabled
/// hosts, with a catalog entry that is this window's own target dropped.
pub(crate) fn build_roster(target: &ConnectTarget, hosts: &[SavedHost]) -> Vec<Machine> {
    let own_ssh = match target {
        ConnectTarget::Ssh { target, .. } => Some(target.as_str()),
        _ => None,
    };
    let mut roster = vec![Machine {
        label: machine_label(target),
        target: target.clone(),
        own: true,
    }];
    for host in hosts
        .iter()
        .filter(|host| host.enabled)
        .filter(|host| own_ssh != Some(host.target.as_str()))
    {
        roster.push(Machine {
            label: host.label.clone(),
            target: ConnectTarget::Ssh {
                target: host.target.clone(),
                session: host.session.clone(),
            },
            own: false,
        });
    }
    roster
}

/// What this machine is called in the sheet. The window title's words without
/// the `zenix` prefix: the sheet is a list of machines, not of windows.
pub(crate) fn machine_label(target: &ConnectTarget) -> String {
    match target {
        ConnectTarget::Local => String::from("local"),
        ConnectTarget::Session { name, .. } => format!("session {name}"),
        ConnectTarget::Socket(path) => {
            let tail = path
                .parent()
                .and_then(|dir| dir.file_name())
                .map(|dir| dir.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            format!("socket {tail}")
        }
        ConnectTarget::Ssh { target, .. } => format!("ssh {target}"),
        ConnectTarget::Wsl { distro, .. } => format!("wsl {distro}"),
    }
}

/// One machine's connection and the flag that ends it.
struct Slot {
    machine: Machine,
    /// `None` for the window's own machine, whose [`Connection`] the shell
    /// already runs.
    connection: Option<Connection>,
}

/// The cockpit's connections, held by the shell while the sheet is up.
#[derive(Default)]
pub(crate) struct Fleet {
    stop: Option<Arc<AtomicBool>>,
    slots: Vec<Slot>,
}

impl Fleet {
    /// Start a reader for every remote machine of `roster`. `pulse` is the
    /// shell's repaint channel; a reader stops when the shell asks (the sheet
    /// closed) or the pulse's window is gone, whichever it meets first.
    pub(crate) fn attach(&mut self, roster: Vec<Machine>, pulse: Sender<()>) {
        self.detach();
        let stop = Arc::new(AtomicBool::new(false));
        self.slots = roster
            .into_iter()
            .map(|machine| {
                let connection = (!machine.own).then(|| {
                    let pulse = pulse.clone();
                    let stop = stop.clone();
                    Connection::start(machine.target.clone(), move || {
                        pulse.try_send(()).is_ok() && !stop.load(Ordering::Acquire)
                    })
                });
                Slot {
                    machine,
                    connection,
                }
            })
            .collect();
        self.stop = Some(stop);
    }

    /// The sheet closed (or the window did): the readers stop at the next
    /// thing they hear — a parked one at its next pushed event, a dialling
    /// one at its next attempt. A machine idle on a quiet daemon keeps its
    /// reader a little longer, the same contract the events and fleet
    /// streams have lived under since U8.
    pub(crate) fn detach(&mut self) {
        if let Some(stop) = self.stop.as_ref() {
            stop.store(true, Ordering::Release);
        }
        self.stop = None;
        self.slots.clear();
    }

    /// The roster in draw order. Empty before the catalog read lands.
    pub(crate) fn roster(&self) -> Vec<Machine> {
        self.slots.iter().map(|slot| slot.machine.clone()).collect()
    }

    /// The connection one machine's live state is read from. The window's own
    /// machine answers `None` — the shell passes its own [`LiveState`] for it.
    pub(crate) fn connection(&self, label: &str) -> Option<Connection> {
        self.slots
            .iter()
            .find(|slot| slot.machine.label == label)
            .and_then(|slot| slot.connection.clone())
    }

    /// Whether the roster is up and every own-machine entry is accounted for.
    pub(crate) fn attached(&self) -> bool {
        self.stop.is_some()
    }
}

impl Drop for Fleet {
    fn drop(&mut self) {
        self.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn host(id: &str, label: &str, target: &str, enabled: bool) -> SavedHost {
        SavedHost {
            id: id.to_owned(),
            label: label.to_owned(),
            target: target.to_owned(),
            session: "default".to_owned(),
            enabled,
        }
    }

    /// The roster is this machine first, then the catalog's enabled hosts in
    /// the catalog's own order; a disabled host is offered nowhere.
    #[test]
    fn the_roster_keeps_this_machine_first_and_the_catalogs_order() {
        let hosts = vec![
            host(&"a".repeat(32), "alpha", "alpha@example.invalid", true),
            host(&"b".repeat(32), "beta", "beta@example.invalid", false),
            host(&"c".repeat(32), "gamma", "gamma@example.invalid", true),
        ];
        let roster = build_roster(&ConnectTarget::Local, &hosts);
        assert_eq!(roster.len(), 3, "own + two enabled, disabled dropped");
        assert!(roster[0].own);
        assert_eq!(roster[0].label, "local");
        assert_eq!(roster[1].label, "alpha");
        assert_eq!(roster[2].label, "gamma");
        assert!(!roster[1].own);
    }

    /// A catalog entry that *is* this window's machine is dropped: its live
    /// state already arrives on the shell's own connection, and a second
    /// reader would dial the same host twice for one roster row.
    #[test]
    fn a_catalog_host_that_is_this_window_is_dropped() {
        let own = ConnectTarget::Ssh {
            target: "drive@example.invalid".to_owned(),
            session: "default".to_owned(),
        };
        let hosts = vec![
            host(&"a".repeat(32), "this one", "drive@example.invalid", true),
            host(&"b".repeat(32), "other", "other@example.invalid", true),
        ];
        let roster = build_roster(&own, &hosts);
        assert_eq!(roster.len(), 2, "own + the other host only");
        assert!(roster[0].own);
        assert_eq!(roster[1].label, "other");
    }

    /// The label is the machine's own word — the target for a remote, the
    /// session or socket tail for a named local daemon.
    #[test]
    fn machine_labels_name_the_target() {
        assert_eq!(machine_label(&ConnectTarget::Local), "local");
        assert_eq!(
            machine_label(&ConnectTarget::Session {
                name: "drive".to_owned(),
                development: false,
            }),
            "session drive"
        );
        assert_eq!(
            machine_label(&ConnectTarget::Ssh {
                target: "drive@example.invalid".to_owned(),
                session: "default".to_owned(),
            }),
            "ssh drive@example.invalid"
        );
    }
}
