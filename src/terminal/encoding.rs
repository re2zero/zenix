//! Terminal key and mouse event encoding.

use gpui::Keystroke;

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
    if meta  { cb |= 8; }
    if ctrl  { cb |= 16; }
    format!("\x1b[<{};{};{}M", cb, col + 1, row + 1).into_bytes()
}
