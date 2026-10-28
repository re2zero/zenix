//! Terminal colors: the chrome's sixteen ANSI entries, packed for the grid,
//! plus the xterm cube and ramp a terminal derives from them.
//!
//! The terminal has no palette of its own (PLAN §3, decision 06): the entries
//! are the component palette's, chosen from the chrome's mode by
//! [`crate::ui::theme`], and this module only packs them into the 24-bit
//! values the daemon's cells carry and the painter paints. Entries 16..=255
//! are the standard xterm cube and grayscale ramp, which no theme names.

/// Colors are packed 24-bit RGB, without an alpha channel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Theme {
    /// The ground a cell with no background of its own is drawn on.
    pub background: u32,
    /// The ink a cell with no foreground of its own is drawn in.
    pub foreground: u32,
    /// The cursor, and the stronger tint a search's current hit takes.
    pub cursor: u32,
    /// What bold text is drawn in when it asks for the default foreground;
    /// `None` keeps the foreground itself.
    pub bold: Option<u32>,
    /// The tone the grid's own chrome — the scrollbar thumb, a popup's
    /// outline — is drawn in.
    pub muted: u32,
    /// The sixteen ANSI entries, then the xterm cube and ramp.
    pub palette: [u32; 256],
}

impl Theme {
    /// A grid in the chrome's colors: `ansi` is the sixteen entries, in the
    /// order the daemon's named colors address them; `background`, `ink` and
    /// `muted` are the tones the grid draws around the cells. The cursor takes
    /// the ink, as it has since the grid was first drawn.
    pub(crate) fn from_ansi(ansi: [u32; 16], background: u32, ink: u32, muted: u32) -> Self {
        let mut palette = [0; 256];
        palette[..16].copy_from_slice(&ansi);
        for (index, color) in palette.iter_mut().enumerate().skip(16) {
            let n = index as u32;
            *color = if n < 232 {
                let n = n - 16;
                let level = |v| if v == 0 { 0 } else { 55 + v * 40 };
                (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
            } else {
                (8 + (n - 232) * 10) * 0x010101
            };
        }
        Self {
            background,
            foreground: ink,
            cursor: ink,
            bold: None,
            muted,
            palette,
        }
    }
}

/// `percent` of `over` blended onto `base`, per channel.
pub(crate) fn mix(base: u32, over: u32, percent: u32) -> u32 {
    let channel = |shift: u32| {
        let base = (base >> shift) & 255;
        let over = (over >> shift) & 255;
        (base * (100 - percent) + over * percent) / 100
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The light set, as `ui::theme` packs it — the order the daemon's named
    /// colors address: black, red, green, yellow, blue, magenta, cyan, white,
    /// then the eight bright ones.
    const LIGHT: [u32; 16] = [
        0x181613, 0xa34942, 0x287c42, 0x8c5f00, 0x346aac, 0x875093, 0x007c84, 0xacaaa8, 0x605d5a,
        0xbd6158, 0x439458, 0xa57710, 0x4c82c6, 0xa067ac, 0x00959c, 0xcccac8,
    ];

    #[test]
    fn the_sixteen_entries_are_taken_in_order() {
        let theme = Theme::from_ansi(LIGHT, 0xfcfaf7, 0x181613, 0x696764);
        assert_eq!(theme.palette[..16], LIGHT);
        assert_eq!(theme.background, 0xfcfaf7);
        assert_eq!(theme.foreground, 0x181613);
        assert_eq!(theme.cursor, theme.foreground);
        assert_eq!(theme.bold, None);
        assert_eq!(theme.muted, 0x696764);
    }

    /// 16..=255 is the xterm cube and ramp: each channel of the six-by-six-by-
    /// six cube steps 55 + 40n, and the ramp adds 8 + 10n of gray. Every
    /// terminal derives these from the first sixteen, so they are not a
    /// theme's to name.
    #[test]
    fn the_rest_of_the_palette_is_the_xterm_cube_and_ramp() {
        let theme = Theme::from_ansi([0; 16], 0, 0, 0);
        assert_eq!(theme.palette[16], 0x000000);
        assert_eq!(theme.palette[17], 0x00005f);
        assert_eq!(theme.palette[21], 0x0000ff);
        assert_eq!(theme.palette[196], 0xff0000);
        assert_eq!(theme.palette[231], 0xffffff);
        assert_eq!(theme.palette[232], 0x080808);
        assert_eq!(theme.palette[255], 0xeeeeee);
    }
}
