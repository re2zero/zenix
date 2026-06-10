//! Terminal emulation module wrapping alacritty_terminal.

use std::{
    io::{Read, Write},
    sync::mpsc::{self, Sender},
    thread,
};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Point, Side},
    selection::{Selection, SelectionType},
    term::{
        cell::Cell,
        viewport_to_point,
        Config, Term, TermMode,
    },
    vte::ansi::{CursorShape, Processor},
};
use anyhow::Context as _;
use gpui::Keystroke;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

// ── Types ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum BackendCommand {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Output { tab_id: String, bytes: Vec<u8> },
    Status { tab_id: String, text: String },
    Closed { tab_id: String, reason: String },
    TitleChanged { tab_id: String, title: String },
    Bell { tab_id: String },
}

#[derive(Clone)]
pub enum BackendTx {
    Local(Sender<BackendCommand>),
}

impl BackendTx {
    pub fn send(&self, command: BackendCommand) {
        let Self::Local(tx) = self;
        let _ = tx.send(command);
    }
}

pub struct TerminalTab {
    pub id: String,
    pub title: String,
    pub connected: bool,
    pub cols: u16,
    pub rows: u16,
    pub status: String,
    processor: Processor,
    term: Term<HerdrListener>,
    pub backend: BackendTx,
}

#[derive(Clone, Copy)]
pub struct CursorState {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
}

#[derive(Clone)]
pub struct RenderCell {
    pub row: i32,
    pub col: i32,
    pub cell: Cell,
}

#[derive(Clone)]
pub struct RenderSnapshot {
    pub cells: Vec<RenderCell>,
    pub cursor: Option<CursorState>,
    pub display_offset: usize,
    pub history_size: usize,
    pub rows: usize,
    pub cols: usize,
}

// ── Custom Dimensions ────────────────────────────────────────────────

struct TerminalSize {
    cols: usize,
    rows: usize,
}

impl TerminalSize {
    fn new(cols: u16, rows: u16) -> Self {
        Self { cols: cols.max(1) as usize, rows: rows.max(1) as usize }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize { self.rows }
    fn screen_lines(&self) -> usize { self.rows }
    fn columns(&self) -> usize { self.cols }
}

struct HerdrListener {
    tab_id: String,
    events: Sender<BackendEvent>,
}

impl EventListener for HerdrListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => {
                let _ = self.events.send(BackendEvent::TitleChanged {
                    tab_id: self.tab_id.clone(),
                    title,
                });
            }
            Event::Bell => {
                let _ = self.events.send(BackendEvent::Bell {
                    tab_id: self.tab_id.clone(),
                });
            }
            _ => {}
        }
    }
}

// ── TerminalTab ──────────────────────────────────────────────────────

fn new_term(cols: u16, rows: u16, tab_id: String, events: Sender<BackendEvent>) -> Term<HerdrListener> {
    Term::new(
        Config { scrolling_history: 2000, ..Config::default() },
        &TerminalSize::new(cols, rows),
        HerdrListener { tab_id, events },
    )
}
impl TerminalTab {
    pub fn new_local(id: String, title: String, backend: BackendTx, events: Sender<BackendEvent>) -> Self {
        let tab_id = id.clone();
        Self {
            id, title, connected: true,
            processor: Processor::new(),
            term: new_term(100, 30, tab_id, events),
            cols: 100, rows: 30,
            status: "starting".into(),
            backend,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.term.resize(TerminalSize::new(cols, rows));
        self.backend.send(BackendCommand::Resize { cols, rows });
    }

    pub fn cursor_state(&self) -> Option<CursorState> {
        let content = self.term.renderable_content();
        if matches!(content.cursor.shape, CursorShape::Hidden) || content.display_offset > 0 {
            return None;
        }
        let row = content.cursor.point.line.0;
        if row < 0 || row >= self.rows as i32 { return None; }
        Some(CursorState {
            row: row as usize,
            col: content.cursor.point.column.0,
            shape: content.cursor.shape,
        })
    }

    pub fn render_snapshot(&self) -> RenderSnapshot {
        let rows = self.rows;
        let cols = self.cols;
        let content = self.term.renderable_content();
        let display_offset = content.display_offset as i32;
        let mut cells = Vec::with_capacity((rows * cols) as usize);
        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            let row = line + display_offset;
            if row < 0 || row >= rows as i32 { continue; }
            let col = indexed.point.column.0 as i32;
            if col >= cols as i32 { continue; }
            cells.push(RenderCell { row, col, cell: indexed.cell.clone() });
        }
        RenderSnapshot {
            cells,
            cursor: self.cursor_state(),
            display_offset: content.display_offset,
            history_size: self.term.grid().history_size(),
            rows: rows as usize,
            cols: cols as usize,
        }
    }

    pub fn scroll_history(&mut self, delta: i32) {
        if delta != 0 { self.term.scroll_display(Scroll::Delta(delta)); }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn has_selection(&self) -> bool {
        self.term.selection_to_string().is_some_and(|t| !t.is_empty())
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|t| !t.is_empty())
    }

    pub fn begin_selection(&mut self, row: usize, col: usize, side: Side, stype: SelectionType) {
        let point = viewport_to_point(self.term.grid().display_offset(), Point::new(row, Column(col)));
        self.term.selection = Some(Selection::new(stype, point, side));
    }

    pub fn update_selection(&mut self, row: usize, col: usize, side: Side) {
        let point = viewport_to_point(self.term.grid().display_offset(), Point::new(row, Column(col)));
        if let Some(sel) = &mut self.term.selection { sel.update(point, side); }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn app_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether the terminal is in any mouse tracking mode.
    pub fn mouse_mode(&self) -> bool {
        let mode = self.term.mode();
        mode.intersects(TermMode::MOUSE_MODE | TermMode::SGR_MOUSE)
    }

    /// Whether mouse motion events should be reported.
    pub fn mouse_motion_mode(&self) -> bool {
        let mode = self.term.mode();
        mode.contains(TermMode::MOUSE_MOTION)
    }

    /// Whether SGR mouse encoding is in use.
    pub fn mouse_sgr_mode(&self) -> bool {
        let mode = self.term.mode();
        mode.contains(TermMode::SGR_MOUSE)
    }
}


// ── Mouse encoding ───────────────────────────────────────────────────

/// Encode a mouse event into an SGR escape sequence.
/// Returns bytes that should be sent to the PTY.
pub fn encode_mouse_event(row: u16, col: u16, button: u8, pressed: bool, ctrl: bool, shift: bool, meta: bool) -> Vec<u8> {
    let mut cb = if pressed { button } else { 3u8 };
    if shift { cb |= 4; }
    if meta  { cb |= 8; }
    if ctrl  { cb |= 16; }

    let motion = 32u8;
    let cb_val = if button >= 32 { cb + motion } else { cb };
    let action = if pressed || button >= 32 { 'M' } else { 'm' };

    format!("\x1b[<{};{};{}{}", cb_val, col + 1, row + 1, action).into_bytes()
}

/// Encode a mouse motion event (for MOUSE_MOTION mode).
/// Encode a mouse motion event for MOUSE_MOTION mode (no button pressed).
/// SGR cb = 32 | 3 = 35 ("motion with no button").
pub fn encode_mouse_motion(row: u16, col: u16, ctrl: bool, shift: bool, meta: bool) -> Vec<u8> {
    let mut cb = 35u8; // motion(32) | release(3)
    if shift { cb |= 4; }
    if meta  { cb |= 8; }
    if ctrl  { cb |= 16; }
    format!("\x1b[<{};{};{}M", cb, col + 1, row + 1).into_bytes()
}

/// Encode a drag event (button held during mouse_motion_mode).
/// SGR cb = motion(32) | button_code.
pub fn encode_mouse_drag(row: u16, col: u16, button: u8, ctrl: bool, shift: bool, meta: bool) -> Vec<u8> {
    let mut cb = 32u8 | button;
    if shift { cb |= 4; }
    if meta  { cb |= 8; }
    if ctrl  { cb |= 16; }
    format!("\x1b[<{};{};{}M", cb, col + 1, row + 1).into_bytes()
}

/// Encode a scroll event.
pub fn encode_mouse_scroll(row: u16, col: u16, up: bool, ctrl: bool, shift: bool, meta: bool) -> Vec<u8> {
    let mut cb = if up { 64u8 } else { 65u8 };
    if shift { cb |= 4; }
    if meta { cb |= 8; }
    if ctrl { cb |= 16; }
    format!("\x1b[<{};{};{}M", cb, col + 1, row + 1).into_bytes()
}

pub fn spawn_command_in_pty(
    tab_id: String, program: &str, args: &[&str], cols: u16, rows: u16, events: Sender<BackendEvent>,
) -> anyhow::Result<BackendTx> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .context("open PTY")?;

    let mut cmd = CommandBuilder::new(program);
    for arg in args { cmd.arg(arg); }
    if let Ok(path) = std::env::var("PATH") { cmd.env("PATH", path); }
    if let Ok(home) = std::env::var("HOME") { cmd.env("HOME", home); }
    if let Ok(shell) = std::env::var("SHELL") { cmd.env("SHELL", &shell); }
    cmd.env("TERM", "xterm-256color");
    cmd.env("TERM_PROGRAM", "deepin-herdr");

    let mut child = pair.slave.spawn_command(cmd).context("spawn command")?;
    drop(pair.slave);
    let master = pair.master;
    let mut reader = master.try_clone_reader().context("clone reader")?;
    let mut writer = master.take_writer().context("take writer")?;
    let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCommand>();

    let read_id = tab_id.clone();
    let read_events = events.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => { let _ = read_events.send(BackendEvent::Output { tab_id: read_id.clone(), bytes: buf[..n].to_vec() }); }
                Err(err) => { let _ = read_events.send(BackendEvent::Closed { tab_id: read_id.clone(), reason: format!("read error: {err}") }); return; }
            }
        }
        let _ = read_events.send(BackendEvent::Closed { tab_id: read_id, reason: "process exited".into() });
    });

    let write_id = tab_id.clone();
    let write_events = events.clone();
    thread::spawn(move || {
        while let Ok(command) = cmd_rx.recv() {
            match command {
                BackendCommand::Input(bytes) => {
                    if let Err(err) = writer.write_all(&bytes) {
                        let _ = write_events.send(BackendEvent::Closed { tab_id: write_id.clone(), reason: format!("write error: {err}") });
                        break;
                    }
                    let _ = writer.flush();
                }
                BackendCommand::Resize { cols, rows } => {
                    let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                }
                BackendCommand::Close => break,
            }
        }
        let _ = child.kill();
    });

    let _ = events.send(BackendEvent::Status { tab_id, text: "ready".into() });
    Ok(BackendTx::Local(cmd_tx))
}

// ── Key encoding ─────────────────────────────────────────────────────

/// Encode a keyboard keystroke into a terminal escape sequence / byte string.
pub fn encode_key(keystroke: &Keystroke, app_cursor_mode: bool, option_as_meta: bool) -> Option<Vec<u8>> {
    zed_like_to_esc_str(keystroke, app_cursor_mode, option_as_meta)
        .map(|text| text.as_bytes().to_vec())
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalModifiers { None, Alt, Ctrl, Shift, CtrlShift, Other }

impl TerminalModifiers {
    fn new(ks: &Keystroke) -> Self {
        match (ks.modifiers.alt, ks.modifiers.control, ks.modifiers.shift, ks.modifiers.platform) {
            (false, false, false, false) => Self::None,
            (true, false, false, false) => Self::Alt,
            (false, true, false, false) => Self::Ctrl,
            (false, false, true, false) => Self::Shift,
            (false, true, true, false) => Self::CtrlShift,
            _ => Self::Other,
        }
    }
    fn any(&self) -> bool { !matches!(self, Self::None) }
}

fn zed_like_to_esc_str(
    keystroke: &Keystroke, app_cursor_mode: bool, _option_as_meta: bool,
) -> Option<std::borrow::Cow<'static, str>> {
    let modifiers = TerminalModifiers::new(keystroke);
    let key = keystroke.key.to_ascii_lowercase();

    let manual = match (key.as_str(), &modifiers) {
        ("tab", TerminalModifiers::None) => Some("\x09"),
        ("tab", TerminalModifiers::Shift) => Some("\x1b[Z"),
        ("escape", TerminalModifiers::None) => Some("\x1b"),
        ("enter", TerminalModifiers::None) => Some("\x0d"),
        ("enter", TerminalModifiers::Shift) => Some("\x0a"),
        ("enter", TerminalModifiers::Alt) => Some("\x1b\x0d"),
        ("backspace", TerminalModifiers::None) => Some("\x7f"),
        ("backspace", TerminalModifiers::Ctrl) => Some("\x08"),
        ("backspace", TerminalModifiers::Alt) => Some("\x1b\x7f"),
        ("backspace", TerminalModifiers::Shift) => Some("\x7f"),
        ("space", TerminalModifiers::Ctrl) => Some("\x00"),
        ("home", _) if app_cursor_mode => Some("\x1bOH"),
        ("home", _) => Some("\x1b[H"),
        ("end", _) if app_cursor_mode => Some("\x1bOF"),
        ("end", _) => Some("\x1b[F"),
        ("up", _) if app_cursor_mode => Some("\x1bOA"),
        ("up", _) => Some("\x1b[A"),
        ("down", _) if app_cursor_mode => Some("\x1bOB"),
        ("down", _) => Some("\x1b[B"),
        ("right", _) if app_cursor_mode => Some("\x1bOC"),
        ("right", _) => Some("\x1b[C"),
        ("left", _) if app_cursor_mode => Some("\x1bOD"),
        ("left", _) => Some("\x1b[D"),
        ("insert", TerminalModifiers::None) => Some("\x1b[2~"),
        ("delete", TerminalModifiers::None) => Some("\x1b[3~"),
        ("pageup", TerminalModifiers::None) => Some("\x1b[5~"),
        ("pagedown", TerminalModifiers::None) => Some("\x1b[6~"),
        ("a", TerminalModifiers::Ctrl) | ("A", TerminalModifiers::CtrlShift) => Some("\x01"),
        ("b", TerminalModifiers::Ctrl) | ("B", TerminalModifiers::CtrlShift) => Some("\x02"),
        ("c", TerminalModifiers::Ctrl) | ("C", TerminalModifiers::CtrlShift) => Some("\x03"),
        ("d", TerminalModifiers::Ctrl) | ("D", TerminalModifiers::CtrlShift) => Some("\x04"),
        ("e", TerminalModifiers::Ctrl) | ("E", TerminalModifiers::CtrlShift) => Some("\x05"),
        ("f", TerminalModifiers::Ctrl) | ("F", TerminalModifiers::CtrlShift) => Some("\x06"),
        ("g", TerminalModifiers::Ctrl) | ("G", TerminalModifiers::CtrlShift) => Some("\x07"),
        ("h", TerminalModifiers::Ctrl) | ("H", TerminalModifiers::CtrlShift) => Some("\x08"),
        ("i", TerminalModifiers::Ctrl) | ("I", TerminalModifiers::CtrlShift) => Some("\x09"),
        ("j", TerminalModifiers::Ctrl) | ("J", TerminalModifiers::CtrlShift) => Some("\x0a"),
        ("k", TerminalModifiers::Ctrl) | ("K", TerminalModifiers::CtrlShift) => Some("\x0b"),
        ("l", TerminalModifiers::Ctrl) | ("L", TerminalModifiers::CtrlShift) => Some("\x0c"),
        ("m", TerminalModifiers::Ctrl) | ("M", TerminalModifiers::CtrlShift) => Some("\x0d"),
        ("n", TerminalModifiers::Ctrl) | ("N", TerminalModifiers::CtrlShift) => Some("\x0e"),
        ("o", TerminalModifiers::Ctrl) | ("O", TerminalModifiers::CtrlShift) => Some("\x0f"),
        ("p", TerminalModifiers::Ctrl) | ("P", TerminalModifiers::CtrlShift) => Some("\x10"),
        ("q", TerminalModifiers::Ctrl) | ("Q", TerminalModifiers::CtrlShift) => Some("\x11"),
        ("r", TerminalModifiers::Ctrl) | ("R", TerminalModifiers::CtrlShift) => Some("\x12"),
        ("s", TerminalModifiers::Ctrl) | ("S", TerminalModifiers::CtrlShift) => Some("\x13"),
        ("t", TerminalModifiers::Ctrl) | ("T", TerminalModifiers::CtrlShift) => Some("\x14"),
        ("u", TerminalModifiers::Ctrl) | ("U", TerminalModifiers::CtrlShift) => Some("\x15"),
        ("v", TerminalModifiers::Ctrl) | ("V", TerminalModifiers::CtrlShift) => Some("\x16"),
        ("w", TerminalModifiers::Ctrl) | ("W", TerminalModifiers::CtrlShift) => Some("\x17"),
        ("x", TerminalModifiers::Ctrl) | ("X", TerminalModifiers::CtrlShift) => Some("\x18"),
        ("y", TerminalModifiers::Ctrl) | ("Y", TerminalModifiers::CtrlShift) => Some("\x19"),
        ("z", TerminalModifiers::Ctrl) | ("Z", TerminalModifiers::CtrlShift) => Some("\x1a"),
        ("@", TerminalModifiers::Ctrl) => Some("\x00"),
        ("[", TerminalModifiers::Ctrl) => Some("\x1b"),
        ("\\", TerminalModifiers::Ctrl) => Some("\x1c"),
        ("]", TerminalModifiers::Ctrl) => Some("\x1d"),
        ("^", TerminalModifiers::Ctrl) => Some("\x1e"),
        ("_", TerminalModifiers::Ctrl) => Some("\x1f"),
        ("?", TerminalModifiers::Ctrl) => Some("\x7f"),
        _ => None,
    };
    if let Some(esc) = manual { return Some(esc.into()); }

    if modifiers.any() {
        let code = modifier_code(keystroke);
        let modified = match key.as_str() {
            "up" => Some(format!("\x1b[1;{}A", code)),
            "down" => Some(format!("\x1b[1;{}B", code)),
            "right" => Some(format!("\x1b[1;{}C", code)),
            "left" => Some(format!("\x1b[1;{}D", code)),
            "insert" => Some(format!("\x1b[2;{}~", code)),
            "pageup" => Some(format!("\x1b[5;{}~", code)),
            "pagedown" => Some(format!("\x1b[6;{}~", code)),
            "end" => Some(format!("\x1b[1;{}F", code)),
            "home" => Some(format!("\x1b[1;{}H", code)),
            _ => None,
        };
        if let Some(esc) = modified { return Some(esc.into()); }
    }

    if let Some(text) = &keystroke.key_char {
        return Some(text.clone().into());
    }
    if keystroke.key.len() == 1 {
        return Some(keystroke.key.clone().into());
    }
    None
}

fn modifier_code(keystroke: &Keystroke) -> u32 {
    let mut code = 0;
    if keystroke.modifiers.shift { code |= 1; }
    if keystroke.modifiers.alt { code |= 1 << 1; }
    if keystroke.modifiers.control { code |= 1 << 2; }
    code + 1
}
