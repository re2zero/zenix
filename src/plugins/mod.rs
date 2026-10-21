//! Hosting herdr plugins: what is installed, what can be run, and where its
//! output and panes go.
//!
//! Who owns what (design: `.trellis/tasks/10-12-ui-plugin-architecture/`): the
//! daemon owns plugin sources, installation, manifest validation, the
//! invocation context, the log ring and the panes; this client owns the
//! surface — what is offered where, in what order, and how a click becomes an
//! invocation. zenix adds no plugin format, no installer and no permission
//! system of its own.
//!
//! The daemon reaches a plugin's own state through one lane here: `plugin.*`
//! lives on the JSON API socket, never on the endpoint lane the shell paints
//! through (see [`api`]). A plugin *pane*, once opened, is ordinary daemon
//! state and comes back on the endpoint lane as any other pane does.
//!
//! * [`model`] — the daemon's plugin vocabulary.
//! * [`api`] — the `plugin.*` methods, one call each.
//! * [`actions`] — one action as the palette, the keymap and the MCP bridge see it.
//! * [`host`] — the cache, its worker thread, and the requests the shell makes.
//! * [`ui_manifest`] — `zenix-ui.toml`, the UI a plugin declares for this host.
//! * [`ui_data`] — the JSON the declarations' commands print, read off-thread.
//! * [`views`] — how a declaration is drawn: panel, sidebar line, status item.
//! * [`manager`] — one plugin's page: its declaration, its slots, its actions.
//! * [`panels`] — the sidebar blocks the registry mounts.

pub(crate) mod actions;
pub(crate) mod api;
pub(crate) mod host;
pub(crate) mod manager;
pub(crate) mod model;
pub(crate) mod panels;
pub(crate) mod trust;
pub(crate) mod trust_view;
pub(crate) mod ui_data;
pub(crate) mod ui_manifest;
pub(crate) mod views;
