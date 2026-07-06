#![allow(dead_code)]

use gpui::{Hsla, px};
use gpui_component::Theme;

// ── Shadow Colors ─────────────────────────────────────────────────────────

#[inline(always)]
pub fn shadow_color() -> Hsla {
    gpui::hsla(0.683, 0.30, 0.15, 0.5)
}
#[inline(always)]
pub fn shadow_color_heavy() -> Hsla {
    gpui::hsla(0.683, 0.30, 0.10, 0.65)
}

// ── Divider Colors ────────────────────────────────────────────────────────

#[inline(always)]
pub fn divider_color() -> Hsla {
    gpui::hsla(0.683, 0.15, 0.33, 1.0)
}
#[inline(always)]
pub fn divider_color_subtle() -> Hsla {
    gpui::hsla(0.683, 0.12, 0.35, 0.6)
}

// ── Background Colors ──────────────────────────────────────────────────────

// HTML Design: #1e1e2e, #282838, #313143, #2a2a3c
#[inline(always)]
pub fn bg_primary() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.18, 1.0) // #1e1e2e
}
#[inline(always)]
pub fn bg_secondary() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.22, 1.0) // #282838
}
#[inline(always)]
pub fn bg_tertiary() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.26, 1.0) // #313143
}
#[inline(always)]
pub fn bg_elevated() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.20, 1.0) // #2a2a3c (slightly different)
}
#[inline(always)]
pub fn bg_hover() -> Hsla {
    gpui::hsla(0.0, 0.0, 1.0, 0.08) // #ffffff08
}
#[inline(always)]
pub fn bg_active() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.24, 1.0) // #222230
}
#[inline(always)]
pub fn bg_selected() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.28, 1.0) // #2a2a38
}

// Theme-aware background colors
#[inline(always)]
pub fn bg_primary_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(1.0)
}
#[inline(always)]
pub fn bg_secondary_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.95)
}
#[inline(always)]
pub fn bg_tertiary_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.9)
}
#[inline(always)]
pub fn bg_elevated_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.98)
}

// Theme-aware sidebar and selection colors
#[inline(always)]
pub fn bg_sidebar_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.85)
}
#[inline(always)]
pub fn bg_selected_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.9)
}
#[inline(always)]
pub fn bg_hover_theme(theme: &Theme) -> Hsla {
    theme.background.alpha(0.92)
}

// ── Border Colors ──────────────────────────────────────────────────────────

// HTML Design: #45475a, #45475a66
#[inline(always)]
pub fn border_color() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.35, 1.0) // #45475a
}
#[inline(always)]
pub fn border_color_transparent() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.35, 0.4) // #45475a66 (25% opacity)
}
#[inline(always)]
pub fn border_color_transparent_2() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.35, 0.6) // #45475a99 (35% opacity)
}
#[inline(always)]
pub fn border_color_active() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.75, 1.0) // #bfbeff (blue-ish)
}

// ── Accent Colors ─────────────────────────────────────────────────────────

// HTML Design: #3b82f6, #2563eb, #00dd44, #ff4444
#[inline(always)]
pub fn primary_color() -> Hsla {
    gpui::hsla(0.60, 0.90, 0.62, 1.0) // #3b82f6
}
#[inline(always)]
pub fn primary_color_hover() -> Hsla {
    gpui::hsla(0.60, 0.90, 0.53, 1.0) // #2563eb
}
#[inline(always)]
pub fn primary_color_transparent() -> Hsla {
    gpui::hsla(0.60, 0.90, 0.62, 0.12) // #3b82f6 with 12% opacity
}
#[inline(always)]
pub fn primary_color_border() -> Hsla {
    gpui::hsla(0.60, 0.90, 0.62, 0.30) // #3b82f6 with 30% opacity
}
#[inline(always)]
pub fn success_color() -> Hsla {
    gpui::hsla(0.40, 1.0, 0.43, 1.0) // #00dd44
}
#[inline(always)]
pub fn error_color() -> Hsla {
    gpui::hsla(0.0, 1.0, 0.63, 1.0) // #ff4444
}
#[inline(always)]
pub fn warning_color() -> Hsla {
    gpui::hsla(0.08, 0.65, 0.57, 1.0) // #dd8844
}

// ── Text Colors ───────────────────────────────────────────────────────────

// HTML Design: #ffffff, #888888, #aaaaaa, #666666
#[inline(always)]
pub fn text_color_high() -> Hsla {
    gpui::hsla(0.0, 0.0, 1.0, 1.0) // #ffffff
}
#[inline(always)]
pub fn text_color_medium() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.53, 1.0) // #888888
}
#[inline(always)]
pub fn text_color_muted() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.67, 1.0) // #aaaaaa
}
#[inline(always)]
pub fn text_color_very_muted() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.40, 1.0) // #666666
}
#[inline(always)]
pub fn text_color_link() -> Hsla {
    gpui::hsla(0.60, 0.90, 0.73, 1.0) // #60a5fa
}

// ── Card / Surface Colors ──────────────────────────────────────────────────

// HTML Design: #00000022, #000000ad (backdrop)
#[inline(always)]
pub fn card_bg_transparent() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, 0.12) // #00000022
}
#[inline(always)]
pub fn backdrop_bg() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, 0.67) // #000000ad (backdrop)
}

// ── Badge Colors ──────────────────────────────────────────────────────────

// HTML Design: #00dd44, #8888aa, #ddcc44, #dd8844, #44aadd
#[inline(always)]
pub fn badge_green() -> Hsla {
    gpui::hsla(0.40, 1.0, 0.43, 1.0) // #00dd44
}
#[inline(always)]
pub fn badge_gray_blue() -> Hsla {
    gpui::hsla(0.65, 0.15, 0.60, 1.0) // #8888aa
}
#[inline(always)]
pub fn badge_yellow() -> Hsla {
    gpui::hsla(0.12, 0.55, 0.54, 1.0) // #ddcc44
}
#[inline(always)]
pub fn badge_orange() -> Hsla {
    gpui::hsla(0.08, 0.55, 0.57, 1.0) // #dd8844
}
#[inline(always)]
pub fn badge_blue() -> Hsla {
    gpui::hsla(0.54, 0.55, 0.57, 1.0) // #44aadd
}

// ── Spacing: Gaps ─────────────────────────────────────────────────────────

#[inline(always)]
pub fn gap_xs() -> gpui::Pixels {
    px(2.0)
}
#[inline(always)]
pub fn gap_sm() -> gpui::Pixels {
    px(4.0)
}
#[inline(always)]
pub fn gap_md() -> gpui::Pixels {
    px(8.0)
}
#[inline(always)]
pub fn gap_lg() -> gpui::Pixels {
    px(12.0)
}
#[inline(always)]
pub fn gap_xl() -> gpui::Pixels {
    px(16.0)
}

// ── Spacing: Padding ──────────────────────────────────────────────────────

#[inline(always)]
pub fn pad_xs() -> gpui::Pixels {
    px(2.0)
}
#[inline(always)]
pub fn pad_sm() -> gpui::Pixels {
    px(4.0)
}
#[inline(always)]
pub fn pad_md() -> gpui::Pixels {
    px(8.0)
}
#[inline(always)]
pub fn pad_lg() -> gpui::Pixels {
    px(12.0)
}
#[inline(always)]
pub fn pad_xl() -> gpui::Pixels {
    px(16.0)
}
#[inline(always)]
pub fn pad_2xl() -> gpui::Pixels {
    px(20.0)
}
#[inline(always)]
pub fn pad_3xl() -> gpui::Pixels {
    px(24.0)
}

// ── Component-Specific Dimensions ──────────────────────────────────────────

#[inline(always)]
pub fn badge_pad_x() -> gpui::Pixels {
    px(6.0)
}
#[inline(always)]
pub fn badge_pad_y() -> gpui::Pixels {
    px(2.0)
}
#[inline(always)]
pub fn badge_min_h() -> gpui::Pixels {
    px(18.0)
}
#[inline(always)]
pub fn btn_icon_size() -> gpui::Pixels {
    px(28.0)
}
#[inline(always)]
pub fn form_label_w() -> gpui::Pixels {
    px(100.0)
}
#[inline(always)]
pub fn sidebar_w() -> gpui::Pixels {
    px(210.0)
}
#[inline(always)]
pub fn form_field_w() -> gpui::Pixels {
    px(220.0)
}
#[inline(always)]
pub fn pad_md_half() -> gpui::Pixels {
    px(10.0)
}
#[inline(always)]
pub fn card_max_h() -> gpui::Pixels {
    px(500.0)
}
#[inline(always)]
pub fn agent_card_max_h() -> gpui::Pixels {
    px(72.0)
}
#[inline(always)]
pub fn search_w() -> gpui::Pixels {
    px(320.0)
}
#[inline(always)]
pub fn scrollback_display_w() -> gpui::Pixels {
    px(80.0)
}
#[inline(always)]
pub fn tab_icon_w() -> gpui::Pixels {
    px(44.0)
}
#[inline(always)]
pub fn section_text_size() -> gpui::Pixels {
    px(18.0)
}
#[inline(always)]
pub fn context_menu_w() -> gpui::Pixels {
    px(140.0)
}
#[inline(always)]
pub fn context_menu_offset() -> gpui::Pixels {
    px(24.0)
}
#[inline(always)]
pub fn search_bar_min_h() -> gpui::Pixels {
    px(32.0)
}

// ── Typography ──────────────────────────────────────────────────────────────

#[inline(always)]
pub fn font_size_xs() -> gpui::Pixels {
    px(9.0)
}
#[inline(always)]
pub fn font_size_sm() -> gpui::Pixels {
    px(10.0)
}
#[inline(always)]
pub fn font_size_base() -> gpui::Pixels {
    px(11.0)
}
#[inline(always)]
pub fn font_size_md() -> gpui::Pixels {
    px(12.0)
}
#[inline(always)]
pub fn font_size_lg() -> gpui::Pixels {
    px(13.0)
}
#[inline(always)]
pub fn font_size_xl() -> gpui::Pixels {
    px(14.0)
}
#[inline(always)]
pub fn font_size_2xl() -> gpui::Pixels {
    px(15.0)
}

// ── Border Radius ────────────────────────────────────────────────────────

#[inline(always)]
pub fn border_radius_xs() -> gpui::Pixels {
    px(2.0)
}
#[inline(always)]
pub fn border_radius_sm() -> gpui::Pixels {
    px(4.0)
}
#[inline(always)]
pub fn border_radius_md() -> gpui::Pixels {
    px(6.0)
}
#[inline(always)]
pub fn border_radius_lg() -> gpui::Pixels {
    px(8.0)
}

// ── Dimensions ────────────────────────────────────────────────────────────

#[inline(always)]
pub fn card_min_w() -> gpui::Pixels {
    px(180.0)
}
#[inline(always)]
pub fn card_max_w() -> gpui::Pixels {
    px(240.0)
}
#[inline(always)]
pub fn status_dot_size() -> gpui::Pixels {
    px(6.0)
}
#[inline(always)]
pub fn status_dot_lg() -> gpui::Pixels {
    px(8.0)
}
