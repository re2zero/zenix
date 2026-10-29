//! The window's content: the shell being rebuilt on the component library.
//!
//! U2-1 replaced the U0 placeholder with the real frame. [`Root`] (in
//! [`shell`]) owns the one daemon connection and the last pushed state;
//! [`chrome`] composes the identity strip, the activity rail, the sidebar's
//! column, the focused workspace's tabs and the status bar from that state and
//! the library's own shell pieces. The window asks for client-side
//! decorations and falls back to the window manager's own where the platform
//! cannot honor them. [`sidebar`] draws the five views' rows (U2-2).
//!
//! U2-3 returned the panes: `crate::terminal::view::TerminalView` paints the
//! focused tab's daemon surface through `crate::terminal::painter` (one card
//! per pane, the focused one on the accent), maps pointer and keyboard input
//! onto the daemon's semantic events, and drags a split into
//! `layout.set_split_ratio`. It is a component of [`Root`], not a second
//! connection owner: every pulse hands it the state `Root` pulled, and the
//! shell's config/appearance observers hand it the theme and metrics.
//!
//! U2-4 gives the shell a command surface: [`commands`] is the one directory
//! the chords, the palette's rows and the chrome's own triggers read;
//! [`keymap`] resolves the directory's chords for the host and matches them
//! ahead of focus dispatch (the prefix must be able to claim a key inside a
//! pane); [`palette`] draws the directory, the session's workspaces and the
//! plugins' actions over the library's `CommandPalette`. The title strip's
//! middle is now the search trigger that opens it, and the status bar always
//! names the prefix (lighting while it is armed). The terminal's find and copy
//! mode, until U2-3 reachable only by mouse, have directory entries here.
//!
//! What is still to come, by PLAN.md's U3 split: the plugin/event/layout
//! surfaces U3-4 returns and the empty/confirm/notification pass U3-5. No
//! control sits here without the surface it opens.
//!
//! U3-1 adds [`settings`]: one dialog over the whole window with seven pages —
//! appearance, terminal, AI, keys, plugins, MCP and about. The pages that own
//! client settings write them through `config::update` as each control
//! changes; the keys, plugins, MCP and about pages state facts their owners
//! hold (the command directory and keymap, the plugin registry, the
//! orchestrator socket, the build) and carry no control they could not act on.
//! The rail's gear and `secondary-,` open it, and its entry in the command
//! directory gives the palette the same row every other command has.
//!
//! U3-2 mounts the aside's conversation: [`aside`] is the panel's frame — a
//! tab strip, a body, and the 34px rail a closed panel folds into — and
//! [`chat`] is the body itself: `crate::ai`'s streamed turn drawn as one
//! message list, with the reasoning block, a card per tool call, the approval
//! gate, the failure retry and the session history. `layout.aside_visible` is
//! the panel's switch (the strip's button and `secondary-shift-a`), and the
//! conversation's unread count badges the rail while the panel is closed.
//!
//! U3-3 appends the aside's second tab, [`fleet`]: the snapshot's agents as
//! cards (status dot, name, held age, the pane line and a strip of the
//! transitions this client watched), the stream's recent transitions, and the
//! TAIL comparison — two panes read on demand, side by side, with the lines
//! they do not share highlighted. A card's eye focuses its pane; its terminal
//! button opens the comparison. The tails are read off the UI thread when the
//! reader asks (a card's button, a mode switch, the refresh button); nothing
//! here polls, and a pane that left the fleet keeps its last column with the
//! read's own age and a line saying so.
//!
//! U3-4 adds the management surfaces, each docked or raised without taking the
//! grid away: [`manager`] is one plugin's 480px page (contract, graded argv,
//! actions, panes, logs, and the switch/unlink footer), [`trust`] is the
//! pre-link review where a directory's commands are read and acknowledged
//! before it is linked, [`inspector`] is one streamed event's line beside the
//! 21-name contract, [`layout_sheet`] is `.zenix/layout.toml` with its split
//! tree and the daemon's export/apply, and [`confirm`] is the one card a
//! worktree removal or a plugin unlink passes through first.
//!
//! U3-5 draws what a new install sees and what a dropped connection says:
//! [`empty`] is the first-run block over the grid — one sentence, four entries
//! that each run a command from the directory, and the daemon card that starts
//! the daemon in place or shows its last output — and [`toast`] is the one
//! short-lived surface for news that had nowhere else to land (a refusal that
//! arrived after its form closed, a command the connection could not send, a
//! host call the daemon answered with an error). The sidebar, the status bar
//! and the title strip's search trigger now say when they are waiting on the
//! daemon instead of drawing a connected shell over nothing.
//!
//! The appearance is [`theme`]'s: the config's four layers become the
//! library's mode and palettes before the first window opens.
//!
//! ## Primitive vocabulary (U1-4)
//!
//! The retired kit's helpers have one replacement each, imported from the
//! pinned library by its own path. Nothing here re-exports them: the library is
//! the authority on its API, and U2/U3 name the modules below directly.
//!
//! | Retired kit | Replacement at the pin |
//! |---|---|
//! | `h_flex` / `v_flex` | `ely_core::layout::{h_stack, v_stack}`; a plain `div().flex()` for a layout neither covers |
//! | `Theme` / `ActiveTheme` | `ely_core::theme::{ActiveTheme, Theme, Palette, Syntax, Mode}`, with `ControlSize` / `IconSize` / `TextSize` / `Radius` / `Elevation` / `Density` from the same module |
//! | `Icon` / `IconName` | `ely_core::primitives::{Icon, IconName}` — Lucide's bundled set, or an app SVG through `Icon::from_path` |
//! | `Button` / `ButtonVariants` | `ely_core::buttons::{Button, IconButton, ButtonVariant, SegmentedControl}` |
//! | `Input` / `InputState` | `ely_core::forms::{TextInput, Input}`: an `Entity<TextInput>` is the state, `Input` its frame |
//! | `Switch` | `ely_core::forms::Switch` |
//! | `Select` / `SelectState` | `ely_core::forms::{Select, Choice}` |
//!
//! Where they land: U2-1 (chrome), U2-2 (sidebar rows) and U2-3 (the grid's
//! find field) take the stacks, tokens, icons and `TextInput`; U2-4 (palette
//! and search fields) takes `TextInput` again; U3-1 (settings) takes `Button`,
//! `Switch` and `Select`; U3-2 (the conversation) takes `Button`, `IconButton`,
//! `Select`, `InlineEdit` and `ConfirmationCard`; U3-3 (the fleet) takes
//! `Badge`, `SimpleGrid` and `ScrollArea`. The kit's re-exported GPUI
//! names (`App`, `Window`, `px`, `point`, …) are `gpui`'s own — import them
//! from `gpui`, which is the `gpui-pre` package.

pub mod theme;

mod aside;
mod chat;
mod chrome;
mod commands;
mod confirm;
mod empty;
mod fleet;
mod inspector;
mod keymap;
mod layout_sheet;
mod manager;
mod palette;
mod review;
mod settings;
mod shell;
mod sidebar;
mod toast;
mod trust;
mod worktree;

pub use shell::Root;

pub(crate) use shell::install_close_guard;
