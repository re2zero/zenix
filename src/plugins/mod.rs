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
//! * [`host`] — the cache, its worker thread, and the requests the shell makes.
//! * [`panels`] — the sidebar blocks the registry mounts.

pub(crate) mod api;
pub(crate) mod host;
pub(crate) mod model;
pub(crate) mod panels;
