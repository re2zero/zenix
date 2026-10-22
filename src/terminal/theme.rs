//! Terminal colors: built-in palettes as packed 24-bit RGB.
//!
//! Ported from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/config/theme.rs`.
//! Theme *file* loading (Ghostty themes, `light:`/`dark:` pairs, `available_themes`)
//! stays upstream. The chrome's theme families live in `crate::config::theme`;
//! the entries named after them pair a terminal palette with a family by hand.

/// Colors are packed 24-bit RGB, without an alpha channel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Theme {
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    pub bold: Option<u32>,
    pub surface: u32,
    pub active: u32,
    pub muted: u32,
    pub palette: [u32; 256],
}

impl Default for Theme {
    fn default() -> Self {
        let mut palette = [0; 256];
        palette[..16].copy_from_slice(&[
            0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0,
            0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
        ]);
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
            background: 0x101419,
            foreground: 0xd8dee9,
            cursor: 0xd8dee9,
            bold: None,
            surface: 0x1c1c22,
            active: 0x2b2933,
            muted: 0x827e91,
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

impl Theme {
    pub(crate) const BUILTIN_NAMES: &'static [&'static str] = &[
        "Default",
        "Nord",
        "Dracula",
        "Catppuccin Mocha",
        "Catppuccin Latte",
        "Gruvbox",
        "Gruvbox Light",
        "Solarized",
        "Solarized Light",
        "Tokyo Night",
        "Matrix",
    ];

    fn derive_chrome(&mut self) {
        let blend = |percent| mix(self.background, self.foreground, percent);
        self.surface = blend(5);
        self.active = blend(12);
        self.muted = blend(55);
    }

    pub(crate) fn builtin(name: &str) -> Option<Self> {
        // Small hand-authored palettes; no external theme assets are bundled.
        let (background, foreground, ansi) = match name {
            "Default" => return Some(Self::default()),
            "Nord" => (
                0x2e3440,
                0xd8dee9,
                [
                    0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                    0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
                ],
            ),
            "Dracula" => (
                0x282a36,
                0xf8f8f2,
                [
                    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
                ],
            ),
            "Catppuccin Mocha" => (
                0x1e1e2e,
                0xcdd6f4,
                [
                    0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
                    0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
                ],
            ),
            "Catppuccin Latte" => (
                0xeff1f5,
                0x4c4f69,
                [
                    0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
                    0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
                ],
            ),
            "Gruvbox" => (
                0x282828,
                0xebdbb2,
                [
                    0x3c3836, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                    0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xfbf1c7,
                ],
            ),
            "Gruvbox Light" => (
                0xfbf1c7,
                0x3c3836,
                [
                    0x3c3836, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                    0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0xd5c4a1,
                ],
            ),
            // Solarized runs one accent set over both appearances; only the
            // background and foreground swap.
            "Solarized" => (
                0x002b36,
                0x839496,
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            ),
            "Solarized Light" => (
                0xfdf6e3,
                0x657b83,
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            ),
            "Tokyo Night" => (
                0x1a1b26,
                0xc0caf5,
                [
                    0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                    0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
                ],
            ),
            // The chrome's Matrix set in terminal form: a green ramp all the
            // way through, matching the family's fiction.
            "Matrix" => (
                0x020d02,
                0x88ff88,
                [
                    0x113311, 0x00b32d, 0x00ff41, 0x7dff9a, 0x00cc41, 0x00ff99, 0x99ffcc, 0x88ff88,
                    0x1a4d1a, 0x66ff66, 0xaaffaa, 0xd2ffd2, 0x4dff7d, 0x66ffbf, 0xccffee, 0xccffcc,
                ],
            ),
            _ => return None,
        };
        let mut theme = Self {
            background,
            foreground,
            cursor: foreground,
            ..Self::default()
        };
        theme.palette[..16].copy_from_slice(&ansi);
        theme.derive_chrome();
        Some(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name the config's Choice field offers must resolve, and a light
    /// palette must actually be light — the mode picker hands it the light
    /// mode's panes, and a dark canvas there would break the pair. Latte is
    /// the one light entry the set's naming does not mark.
    #[test]
    fn every_builtin_name_resolves_and_light_means_light() {
        let light_names = ["Gruvbox Light", "Solarized Light", "Catppuccin Latte"];
        for name in Theme::BUILTIN_NAMES {
            let theme = Theme::builtin(name).unwrap_or_else(|| panic!("{name} must resolve"));
            let channel = |shift: u32| (theme.background >> shift) & 255;
            let light = channel(16) + channel(8) + channel(0);
            let expected_light = light_names.contains(name);
            assert_eq!(
                light > 384,
                expected_light,
                "{name}: background 0x{:06x} {}",
                theme.background,
                if expected_light {
                    "is not light"
                } else {
                    "is not dark"
                }
            );
            assert_ne!(theme.surface, theme.background);
            assert_ne!(theme.muted, theme.foreground);
        }
    }
}
