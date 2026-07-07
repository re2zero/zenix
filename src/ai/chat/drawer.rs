//! Slide-in drawer overlay for History / Agents panels.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Styled as _,
    div, hsla, px,
};
use gpui_component::{Theme, h_flex, scroll::ScrollableElement as _, v_flex};

use crate::ai::session::Session;

use super::state::{AiChatAction, AiChatActionHandler, DrawerKind};

pub(super) fn render_drawer_overlay(
    kind: DrawerKind,
    data: &super::state::AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let bg = th.background;
    let border = th.border;
    let mu = th.muted_foreground;
    let close_action = on_action.clone();
    let on_close_btn = on_action.clone();

    let title = match kind {
        DrawerKind::History => "History",
        DrawerKind::Agents => "Agents",
    };

    let body: AnyElement = match kind {
        DrawerKind::History => render_history_body(data, theme),
        DrawerKind::Agents => super::header::render_agent_monitor(data, theme).into_any_element(),
    };

    let backdrop = div()
        .absolute()
        .inset_0()
        .bg(hsla(0., 0., 0., 0.4))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            close_action(AiChatAction::CloseDrawer, window, cx);
        });

    let panel = v_flex()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(px(320.))
        .h_full()
        .bg(bg)
        .border_l_1()
        .border_color(border)
        .shadow_lg()
        .child(
            div()
                .px(px(16.))
                .py(px(12.))
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            super::txt(px(13.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .id("drawer-close")
                                .cursor_pointer()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded_md()
                                .hover(|st| st.bg(th.muted))
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    on_close_btn(AiChatAction::CloseDrawer, window, cx)
                                })
                                .child(super::txt(px(16.)).text_color(mu).child("×")),
                        ),
                ),
        )
        .child(div().flex_1().overflow_y_scrollbar().child(body));

    div()
        .absolute()
        .inset_0()
        .child(backdrop)
        .child(panel)
        .into_any_element()
}

fn render_history_body(data: &super::state::AiChatPanelData, theme: &Theme) -> AnyElement {
    if data.history_sessions.is_empty() {
        return div()
            .p(px(16.))
            .text_size(px(12.))
            .text_color(theme.muted_foreground)
            .child("No conversations yet")
            .into_any_element();
    }

    let mut groups = group_sessions_by_date(&data.history_sessions);
    let mut container = v_flex().p(px(8.)).gap(px(12.));
    for (label, sessions) in groups.drain(..) {
        if sessions.is_empty() {
            continue;
        }
        let mut group = v_flex().gap(px(4.));
        group = group.child(
            super::txt(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(label.to_uppercase()),
        );
        for sess in sessions {
            group = group.child(render_session_row(sess, theme));
        }
        container = container.child(group);
    }
    container.into_any_element()
}

fn render_session_row(sess: &Session, theme: &Theme) -> impl IntoElement {
    let title = if sess.title.is_empty() {
        "Untitled"
    } else {
        &sess.title
    };
    let time = sess
        .updated_at
        .get(..5)
        .or_else(|| sess.created_at.get(..5))
        .unwrap_or("");
    let provider = if sess.provider_id.is_empty() {
        "—"
    } else {
        &sess.provider_id
    };
    div()
        .w_full()
        .px(px(10.))
        .py(px(8.))
        .rounded_md()
        .hover(|s| s.bg(theme.muted))
        .cursor_pointer()
        .child(
            v_flex()
                .gap(px(2.))
                .child(
                    super::txt(px(12.))
                        .text_color(theme.foreground)
                        .child(title.to_string()),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(
                            super::txt(px(10.))
                                .text_color(theme.accent)
                                .child(provider.to_string()),
                        )
                        .child(
                            super::txt(px(10.))
                                .text_color(theme.muted_foreground)
                                .child(time.to_string()),
                        ),
                ),
        )
}

const DATE_LABELS: &[&str] = &["Today", "Yesterday", "This Week", "Earlier"];

fn group_sessions_by_date(sessions: &[Session]) -> Vec<(&'static str, Vec<&Session>)> {
    let mut buckets: [Vec<&Session>; 4] = Default::default();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let today = date_prefix(now_secs);
    let yesterday = date_prefix(now_secs.saturating_sub(86400));
    let week_ago = date_prefix(now_secs.saturating_sub(7 * 86400));

    for s in sessions {
        let created = s.created_at.get(..10).unwrap_or(&s.created_at);
        let bucket = if created >= today.as_str() {
            0
        } else if created >= yesterday.as_str() {
            1
        } else if created >= week_ago.as_str() {
            2
        } else {
            3
        };
        buckets[bucket].push(s);
    }

    DATE_LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| (*label, std::mem::take(&mut buckets[i])))
        .collect()
}

fn date_prefix(epoch_secs: u64) -> String {
    let secs = epoch_secs;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{min:02}:{sec:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::chat::state::AiChatPanelData;
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn noop_handler() -> AiChatActionHandler {
        std::rc::Rc::new(|_, _, _| {})
    }

    fn mk_session(id: &str, created_at: &str) -> Session {
        Session {
            id: id.into(),
            title: format!("t{id}"),
            created_at: created_at.into(),
            updated_at: String::new(),
            workspace: None,
            provider_id: String::new(),
            model: String::new(),
            thinking_level: String::new(),
        }
    }

    #[test]
    fn test_render_history_drawer_empty() {
        let data = AiChatPanelData::default();
        let _element =
            render_drawer_overlay(DrawerKind::History, &data, &test_theme(), noop_handler());
    }

    #[test]
    fn test_render_agents_drawer() {
        let data = AiChatPanelData::default();
        let _element =
            render_drawer_overlay(DrawerKind::Agents, &data, &test_theme(), noop_handler());
    }

    #[test]
    fn test_date_prefix_round_trip() {
        let p = date_prefix(0);
        assert_eq!(&p[..10], "1970-01-01");
    }

    #[test]
    fn test_civil_from_days_epoch() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }
}
