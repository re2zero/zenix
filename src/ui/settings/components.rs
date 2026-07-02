//! Card components and form helpers extracted from settings.rs.

use std::collections::HashMap;

use crate::api_provider::{ApiFormat, ApiProviderConfig, ThinkingLevel};
use crate::i18n::t;
use gpui::{
    AnyElement, ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::ui::settings::helpers::*;

use super::{
    ApiProviderAction, ApiProviderFormState, McpAction, McpFormState, SharedActionCb, SkillAction,
};

// ── MCP Cards ─────────────────────────────────────────────────────────

pub fn mcp_card(
    s: &crate::mcp::ZenixMcpServer,
    available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_form: Option<&McpFormState>,
    mcp_test_results: &HashMap<String, String>,
) -> AnyElement {
    let name = s.name.clone();
    let enabled_count = s.enabled_agents.len();
    let total = available_agents.len();
    let status = format!("{enabled_count}/{total} agents");
    let sc = if enabled_count > 0 {
        gpui::rgba(0x00DD44FF)
    } else {
        gpui::rgba(0xFF4444FF)
    };
    let type_label = match s.mcp_type {
        crate::mcp::McpType::Sse => "sse",
        crate::mcp::McpType::Stdio => "stdio",
    };
    let type_color = gpui::rgba(0x8888FFFF);

    let env_text = if s.env.is_empty() {
        String::new()
    } else {
        s.env
            .keys()
            .map(|k| format!("{k}=***"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let test_result = mcp_test_results.get(&name);
    let is_editing = mcp_form
        .and_then(|f| f.editing_name.as_ref())
        .map_or(false, |en| en == &s.name);

    // If this server is being edited inline, render form fields
    if is_editing {
        let form = mcp_form.unwrap();
        return mcp_inline_edit_card(form, on_mcp_action).into_any_element();
    }

    let remove_cb = {
        let oma = on_mcp_action.clone();
        let remove_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::RemoveZenix {
                    name: remove_name.clone(),
                },
                w,
                a,
            );
        }
    };
    let edit_cb = {
        let oma = on_mcp_action.clone();
        let edit_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::EditZenix {
                    name: edit_name.clone(),
                },
                w,
                a,
            );
        }
    };
    let test_cb = {
        let oma = on_mcp_action.clone();
        let test_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::TestServer {
                    name: test_name.clone(),
                },
                w,
                a,
            );
        }
    };

    zed_card()
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            txt(px(14.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(name.clone()),
                        )
                        .child(zed_badge(type_label.to_string(), type_color))
                        .child(zed_badge(status, sc)),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(
                            Button::new(format!("edit-zenix-mcp-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.edit"))
                                .on_click(edit_cb),
                        )
                        .child(
                            Button::new(format!("test-zenix-mcp-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.test"))
                                .on_click(test_cb),
                        )
                        .child(
                            Button::new(format!("rm-zenix-mcp-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("mcp.remove_server"))
                                .on_click(remove_cb),
                        ),
                ),
        )
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") {
                gpui::rgba(0x00DD44FF)
            } else {
                gpui::rgba(0xFF4444FF)
            };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
        .child(
            txt(px(11.))
                .text_color(gpui::rgba(0x888888FF))
                .child(format!("{} {}", s.command, s.args.join(" "))),
        )
        .when(!env_text.is_empty(), |el| {
            el.child(
                txt(px(10.))
                    .text_color(gpui::rgba(0x888888FF))
                    .child(env_text),
            )
        })
        .child(
            h_flex()
                .gap(px(6.))
                .flex_wrap()
                .children(available_agents.iter().map(|agent| {
                    let checked = s.enabled_agents.contains(agent);
                    let srv = name.clone();
                    let agent_name = agent.clone();
                    let oma = on_mcp_action.clone();
                    div()
                        .id(format!("mcp-cb-{srv}-{agent_name}"))
                        .cursor_pointer()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .when(checked, |d| {
                            d.bg(gpui::rgba(0x3B82F622))
                                .border_1()
                                .border_color(gpui::rgba(0x3B82F644))
                        })
                        .when(!checked, |d| {
                            d.bg(gpui::rgba(0x00000022))
                                .border_1()
                                .border_color(gpui::rgba(0x45475a44))
                        })
                        .hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
                        .child(
                            txt(px(11.))
                                .text_color(if checked {
                                    gpui::rgba(0x60A5FAFF)
                                } else {
                                    gpui::rgba(0x888888FF)
                                })
                                .child(format!("{} {agent}", if checked { "✓" } else { "○" })),
                        )
                        .on_click(move |_: &ClickEvent, w: &mut Window, app: &mut gpui::App| {
                            oma(
                                McpAction::ToggleZenixAgent {
                                    server_name: srv.clone(),
                                    agent: agent_name.clone(),
                                },
                                w,
                                app,
                            );
                        })
                })),
        )
        .into_any_element()
}

pub fn mcp_inline_edit_card(
    form: &McpFormState,
    on_mcp_action: &SharedActionCb<McpAction>,
) -> impl IntoElement {
    let cancel_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::CancelAddForm, w, a);
        }
    };
    let submit_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::SubmitAddForm, w, a);
        }
    };
    let is_sse = form.mcp_type == "sse";

    v_flex()
        .gap(px(6.))
        .py(px(8.))
        .px(px(10.))
        .border_1()
        .border_color(gpui::rgba(0x44AAFFFF))
        .rounded_md()
        .overflow_y_scrollbar()
        .max_h(px(500.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.name")))
                .child(
                    form.name
                        .as_ref()
                        .map(|e| Input::new(e).into_any_element())
                        .unwrap_or_else(|| {
                            txt(px(12.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("_")
                                .into_any_element()
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.type")))
                .child(
                    Button::new("inline-mcp-type-stdio")
                        .cursor_pointer()
                        .when(!is_sse, |b| b.primary())
                        .when(is_sse, |b| b.ghost())
                        .compact()
                        .label("stdio")
                        .on_click({
                            let oma = on_mcp_action.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oma(
                                    McpAction::SetFormType {
                                        mcp_type: "stdio".into(),
                                    },
                                    w,
                                    a,
                                );
                            }
                        }),
                )
                .child(
                    Button::new("inline-mcp-type-sse")
                        .cursor_pointer()
                        .when(is_sse, |b| b.primary())
                        .when(!is_sse, |b| b.ghost())
                        .compact()
                        .label("sse")
                        .on_click({
                            let oma = on_mcp_action.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oma(
                                    McpAction::SetFormType {
                                        mcp_type: "sse".into(),
                                    },
                                    w,
                                    a,
                                );
                            }
                        }),
                ),
        )
        .when(!is_sse, |el| {
            el.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        txt(px(12.))
                            .w(px(100.))
                            .child(t("settings.mcp.form.command")),
                    )
                    .child(
                        form.command
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.args")))
                    .child(
                        form.args
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
        })
        .when(is_sse, |el| {
            el.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child("URL"))
                    .child(
                        form.url
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
        })
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.env")))
                .child(
                    form.env
                        .as_ref()
                        .map(|e| Input::new(e).into_any_element())
                        .unwrap_or_else(|| {
                            txt(px(12.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("_")
                                .into_any_element()
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(
                    Button::new("inline-mcp-form-cancel")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.cancel"))
                        .on_click(cancel_cb),
                )
                .child(
                    Button::new("inline-mcp-form-submit")
                        .cursor_pointer()
                        .primary()
                        .label(t("general.update"))
                        .on_click(submit_cb),
                ),
        )
}

pub fn mcp_add_form(
    form: &McpFormState,
    on_mcp_action: &SharedActionCb<McpAction>,
) -> impl IntoElement {
    let cancel_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::CancelAddForm, w, a);
        }
    };
    let submit_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::SubmitAddForm, w, a);
        }
    };

    let is_stdio = form.mcp_type == "stdio" || form.mcp_type.is_empty();
    let submit_label = if form.editing_name.is_some() {
        t("general.update")
    } else {
        t("general.add")
    };
    let form_title = if form.editing_name.is_some() {
        t("settings.mcp.form.title_edit")
    } else {
        t("settings.mcp.form.title_add")
    };

    v_flex()
        .gap(px(8.))
        .py(px(10.))
        .px(px(12.))
        .border_1()
        .border_color(gpui::rgba(0x45475a66))
        .rounded_md()
        .child(
            txt(px(13.))
                .font_weight(gpui::FontWeight::BOLD)
                .child(form_title),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.name")))
                .child(
                    form.name
                        .as_ref()
                        .map(|e| Input::new(e).into_any_element())
                        .unwrap_or_else(|| {
                            txt(px(12.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("_")
                                .into_any_element()
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.type")))
                .child(
                    Button::new("mcp-type-stdio")
                        .cursor_pointer()
                        .when(is_stdio, |b| b.primary())
                        .when(!is_stdio, |b| b.ghost())
                        .compact()
                        .label("stdio")
                        .on_click({
                            let oma = on_mcp_action.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oma(
                                    McpAction::SetFormType {
                                        mcp_type: "stdio".into(),
                                    },
                                    w,
                                    a,
                                );
                            }
                        }),
                )
                .child(
                    Button::new("mcp-type-sse")
                        .cursor_pointer()
                        .when(!is_stdio, |b| b.primary())
                        .when(is_stdio, |b| b.ghost())
                        .compact()
                        .label("sse")
                        .on_click({
                            let oma = on_mcp_action.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oma(
                                    McpAction::SetFormType {
                                        mcp_type: "sse".into(),
                                    },
                                    w,
                                    a,
                                );
                            }
                        }),
                ),
        )
        .when(is_stdio, |el| {
            el.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        txt(px(12.))
                            .w(px(100.))
                            .child(t("settings.mcp.form.command")),
                    )
                    .child(
                        form.command
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.args")))
                    .child(
                        form.args
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
        })
        .when(!is_stdio, |el| {
            el.child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child("URL"))
                    .child(
                        form.url
                            .as_ref()
                            .map(|e| Input::new(e).into_any_element())
                            .unwrap_or_else(|| {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child("_")
                                    .into_any_element()
                            }),
                    ),
            )
        })
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.mcp.form.env")))
                .child(
                    form.env
                        .as_ref()
                        .map(|e| Input::new(e).into_any_element())
                        .unwrap_or_else(|| {
                            txt(px(12.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("_")
                                .into_any_element()
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(
                    Button::new("mcp-form-cancel")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.cancel"))
                        .on_click(cancel_cb),
                )
                .child(
                    Button::new("mcp-form-submit")
                        .cursor_pointer()
                        .primary()
                        .label(submit_label)
                        .on_click(submit_cb),
                ),
        )
}

pub fn mcp_agent_entry_with_test(
    agent: &str,
    entry: &crate::mcp::McpServerEntry,
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_test_results: &HashMap<String, String>,
) -> impl IntoElement {
    let name = entry.name.clone();
    let cmd = format!("{} {}", entry.command, entry.args.join(" "));
    let disabled = entry.disabled;
    let test_result = mcp_test_results.get(&format!("{name}@{agent}"));

    let toggle_cb = {
        let oma = on_mcp_action.clone();
        let toggle_name = name.clone();
        let toggle_agent = agent.to_string();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::ToggleForAgent {
                    name: toggle_name.clone(),
                    agent: toggle_agent.clone(),
                },
                w,
                a,
            );
        }
    };
    let remove_cb = {
        let oma = on_mcp_action.clone();
        let remove_name = name.clone();
        let remove_agent = agent.to_string();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::RemoveFromAgent {
                    name: remove_name.clone(),
                    agent: remove_agent.clone(),
                },
                w,
                a,
            );
        }
    };
    let test_cb = {
        let oma = on_mcp_action.clone();
        let test_name = name.clone();
        let test_agent = agent.to_string();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(
                McpAction::TestAgentServer {
                    name: test_name.clone(),
                    agent: test_agent.clone(),
                },
                w,
                a,
            );
        }
    };

    let status_text = if disabled {
        t("mcp.disabled")
    } else {
        t("mcp.enabled")
    };
    let status_color = if disabled {
        gpui::rgba(0xFF4444FF)
    } else {
        gpui::rgba(0x00DD44FF)
    };

    v_flex()
        .gap(px(4.))
        .py(px(6.))
        .px(px(10.))
        .border_1()
        .border_color(gpui::rgba(0x45475a44))
        .rounded_md()
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            txt(px(12.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(name.clone()),
                        )
                        .child(txt(px(10.)).text_color(status_color).child(status_text)),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(
                            Button::new(format!("toggle-agent-mcp-{agent}-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(if disabled {
                                    t("mcp.enable")
                                } else {
                                    t("mcp.disable")
                                })
                                .on_click(toggle_cb),
                        )
                        .child(
                            Button::new(format!("test-agent-mcp-{agent}-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.test"))
                                .on_click(test_cb),
                        )
                        .child(
                            Button::new(format!("remove-agent-mcp-{agent}-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("mcp.remove_server"))
                                .on_click(remove_cb),
                        ),
                ),
        )
        .child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child(cmd))
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") {
                gpui::rgba(0x00DD44FF)
            } else {
                gpui::rgba(0xFF4444FF)
            };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
}

// ── Skill Cards ───────────────────────────────────────────────────────

pub fn zenix_skill_card(
    s: &crate::skills::SkillInfo,
    on_skill_action: &SharedActionCb<SkillAction>,
) -> impl IntoElement {
    let name = s.name.clone();
    let link_name = name.clone();
    let remove_name = name.clone();
    let btn_id_link = format!("link-{name}");
    let btn_id_rm = format!("rm-{name}");
    let link_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(
                SkillAction::Link {
                    skill_name: link_name.clone(),
                },
                w,
                a,
            );
        }
    };
    let remove_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(
                SkillAction::Remove {
                    skill_name: remove_name.clone(),
                },
                w,
                a,
            );
        }
    };
    zed_card()
        .flex_1()
        .min_w_0()
        .max_w(px(320.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .max_h(px(72.))
                .overflow_hidden()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .min_w_0()
                        .child(
                            txt(px(13.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(name),
                        )
                        .child(zed_badge("link", gpui::rgba(0x44DD88FF))),
                ),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .justify_end()
                .min_w_0()
                .flex_wrap()
                .child(
                    Button::new(btn_id_link)
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.link"))
                        .on_click(link_cb),
                )
                .child(
                    Button::new(btn_id_rm)
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.remove"))
                        .on_click(remove_cb),
                ),
        )
}

pub fn api_skill_card(
    s: &crate::skills::ApiSkill,
    installed_names: &std::collections::HashSet<String>,
    on_skill_action: &SharedActionCb<SkillAction>,
) -> impl IntoElement {
    let is_installed = installed_names.contains(&s.slug);
    let skill_id = s.id.clone();
    let install_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(
                SkillAction::InstallApi {
                    skill_id: skill_id.clone(),
                },
                w,
                a,
            );
        }
    };
    let action = if is_installed {
        txt(px(10.))
            .text_color(gpui::rgba(0x00DD44FF))
            .child(t("skills.installed"))
            .into_any_element()
    } else {
        Button::new(format!("install-api-{}", s.slug))
            .cursor_pointer()
            .ghost()
            .compact()
            .label(t("agent.install"))
            .on_click(install_cb)
            .into_any_element()
    };
    let installs_str = if s.installs > 1000 {
        format!("{:.1}k", s.installs as f64 / 1000.0)
    } else {
        s.installs.to_string()
    };
    zed_card()
        .flex_1()
        .min_w_0()
        .max_w(px(320.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .max_h(px(72.))
                .overflow_hidden()
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_ellipsis()
                        .child(s.name.clone()),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .min_w_0()
                        .child(zed_badge(s.source_type.clone(), gpui::rgba(0x8888AAFF)))
                        .child(
                            txt(px(10.))
                                .text_color(gpui::rgba(0x888888FF))
                                .text_ellipsis()
                                .child(format!("⬇ {}", installs_str)),
                        ),
                )
                .child(
                    txt(px(10.))
                        .text_color(gpui::rgba(0x888888FF))
                        .text_ellipsis()
                        .child(s.source.clone()),
                ),
        )
        .child(v_flex().gap(px(2.)).items_center().child(action))
}

pub fn add_skill_card(on_skill_action: &SharedActionCb<SkillAction>) -> impl IntoElement {
    let git_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::InstallGit { url: String::new() }, w, a);
        }
    };
    let local_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(
                SkillAction::InstallLocal {
                    path: String::new(),
                },
                w,
                a,
            );
        }
    };
    v_flex()
        .gap(px(4.))
        .py(px(6.))
        .px(px(8.))
        .min_w(px(180.))
        .max_w(px(240.))
        .border_1()
        .border_color(gpui::rgba(0x45475a66))
        .rounded_md()
        .child(
            txt(px(12.))
                .text_color(gpui::rgba(0x888888FF))
                .child(t("skills.add_skill")),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .flex_wrap()
                .child(
                    Button::new("add-git")
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.add_from_git"))
                        .on_click(git_cb),
                )
                .child(
                    Button::new("add-local")
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.add_from_local"))
                        .on_click(local_cb),
                ),
        )
}

pub fn per_agent_skill_card(
    agent: &str,
    s: &crate::skills::SkillInfo,
    on_skill_action: &SharedActionCb<SkillAction>,
) -> impl IntoElement {
    let name = s.name.clone();
    let is_source = s.source_agents.contains(&agent.to_string());
    let unlink_name = name.clone();
    let unlink_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(
                SkillAction::Unlink {
                    skill_name: unlink_name.clone(),
                },
                w,
                a,
            );
        }
    };
    let tag_color = if is_source {
        gpui::rgba(0xDDCC44FF)
    } else {
        gpui::rgba(0x44DD88FF)
    };
    let tag_text = if is_source { "bind" } else { "link" };
    h_flex()
        .gap(px(8.))
        .py(px(6.))
        .px(px(8.))
        .min_w(px(180.))
        .max_w(px(240.))
        .border_1()
        .border_color(gpui::rgba(0x45475a44))
        .rounded_md()
        .child(
            div().flex_1().min_w_0().child(
                txt(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(name.clone()),
            ),
        )
        .child(
            v_flex()
                .gap(px(2.))
                .items_center()
                .child(txt(px(10.)).text_color(tag_color).child(tag_text))
                .child(if is_source {
                    let move_name = name.clone();
                    let move_cb = {
                        let osa = on_skill_action.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            osa(
                                SkillAction::Link {
                                    skill_name: move_name.clone(),
                                },
                                w,
                                a,
                            );
                        }
                    };
                    Button::new(format!("move-{name}"))
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.move_to_zenix"))
                        .on_click(move_cb)
                        .into_any_element()
                } else {
                    Button::new(format!("unlink-{name}"))
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("skills.unlink"))
                        .on_click(unlink_cb)
                        .into_any_element()
                }),
        )
}

// ── API Provider Cards ────────────────────────────────────────────────

pub fn api_provider_card(
    p: &ApiProviderConfig,
    available_agents: &[String],
    on_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
    api_form: Option<&ApiProviderFormState>,
) -> impl IntoElement {
    let name = p.name.clone();
    let format_label = p.format.label();
    let format_color = match p.format {
        ApiFormat::OpenAI => gpui::rgba(0x44AADDFF),
        ApiFormat::Anthropic => gpui::rgba(0xDD8844FF),
    };
    let endpoint = format!("{}{}", p.base_url.trim_end_matches('/'), p.url_suffix);
    let think = format!("think: {}", p.thinking_level.label());
    let masked_key = if p.api_key.len() > 8 {
        format!(
            "{}...{}",
            &p.api_key[..4],
            &p.api_key[p.api_key.len() - 4..]
        )
    } else {
        "****".to_string()
    };
    let test_result = api_test_results.get(&name);
    let is_editing = api_form
        .and_then(|f| f.editing_name.as_ref())
        .map_or(false, |en| en == &p.name);

    // If this provider is being edited inline, render form fields
    if is_editing {
        let form = api_form.unwrap();
        return api_inline_edit_card(form, on_action).into_any_element();
    }

    let edit_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::Edit { name: n.clone() }, w, a);
        }
    };
    let test_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::TestConnection { name: n.clone() }, w, a);
        }
    };
    let remove_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::Remove { name: n.clone() }, w, a);
        }
    };
    let fetch_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::FetchModels { name: n.clone() }, w, a);
        }
    };

    v_flex()
        .gap(px(6.))
        .py(px(8.))
        .px(px(10.))
        .border_1()
        .border_color(gpui::rgba(0x45475a44))
        .rounded_md()
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            txt(px(13.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(name.clone()),
                        )
                        .child(txt(px(10.)).text_color(format_color).child(format_label)),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(
                            Button::new(format!("edit-api-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.edit"))
                                .on_click(edit_cb),
                        )
                        .child(
                            Button::new(format!("test-api-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.test"))
                                .on_click(test_cb),
                        )
                        .child(
                            Button::new(format!("fetch-api-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.models"))
                                .on_click(fetch_cb),
                        )
                        .child(
                            Button::new(format!("rm-api-{name}"))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(t("general.remove"))
                                .on_click(remove_cb),
                        ),
                ),
        )
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") {
                gpui::rgba(0x00DD44FF)
            } else {
                gpui::rgba(0xFF4444FF)
            };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
        .child(
            txt(px(11.))
                .text_color(gpui::rgba(0x888888FF))
                .child(endpoint),
        )
        .child(
            h_flex()
                .gap(px(12.))
                .child(
                    txt(px(11.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(format!("model: {}", p.model)),
                )
                .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(think))
                .child(
                    txt(px(11.))
                        .text_color(gpui::rgba(0x666666FF))
                        .child(format!("key: {masked_key}")),
                ),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .flex_wrap()
                .children(available_agents.iter().map(|agent| {
                    let checked = p.enabled_agents.contains(agent);
                    let srv = name.clone();
                    let agent_name = agent.clone();
                    let on = on_action.clone();
                    Button::new(format!("api-agent-{srv}-{agent_name}"))
                        .cursor_pointer()
                        .when(checked, |b| b.primary())
                        .when(!checked, |b| b.ghost())
                        .compact()
                        .label(format!(
                            "{} {}",
                            if checked { "\u{2611}" } else { "\u{2610}" },
                            agent_name
                        ))
                        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            on(
                                ApiProviderAction::ToggleAgent {
                                    name: srv.clone(),
                                    agent: agent_name.clone(),
                                },
                                w,
                                a,
                            );
                        })
                })),
        )
        .into_any_element()
}

pub fn api_inline_edit_card(
    form: &super::ApiProviderFormState,
    on_action: &SharedActionCb<ApiProviderAction>,
) -> impl IntoElement {
    let cancel_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::CancelForm, w, a);
        }
    };
    let submit_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::SubmitForm, w, a);
        }
    };

    let presets = form.format.presets();

    v_flex()
        .gap(px(6.))
        .py(px(8.))
        .px(px(10.))
        .border_1()
        .border_color(gpui::rgba(0x44AAFFFF))
        .rounded_md()
        .overflow_y_scrollbar()
        .max_h(px(500.))
        .child(
            txt(px(12.))
                .font_weight(gpui::FontWeight::BOLD)
                .child(t("settings.api.form.title_edit")),
        )
        .child(form_field(t("settings.api.form.name"), form.name.as_ref()))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.format")),
                )
                .child(api_format_btn(ApiFormat::OpenAI, form.format, on_action))
                .child(api_format_btn(ApiFormat::Anthropic, form.format, on_action)),
        )
        // Preset selector
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.preset")),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .flex_wrap()
                        .children(presets.iter().map(|preset| {
                            let p = preset.clone();
                            let on = on_action.clone();
                            Button::new(format!(
                                "preset-{}",
                                p.name.to_lowercase().replace(' ', "-")
                            ))
                            .cursor_pointer()
                            .ghost()
                            .compact()
                            .label(p.name.to_string())
                            .on_click(
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    on(ApiProviderAction::ApplyPreset { preset: p.clone() }, w, a);
                                },
                            )
                        })),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.suffix")),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .children(form.format.suffix_options().iter().map(|suffix| {
                            let s = suffix.to_string();
                            let on = on_action.clone();
                            Button::new(format!("suffix-{}", s.replace('/', "-")))
                                .cursor_pointer()
                                .ghost()
                                .compact()
                                .label(s.clone())
                                .on_click(
                                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                        on(
                                            ApiProviderAction::SetSuffix { suffix: s.clone() },
                                            w,
                                            a,
                                        );
                                    },
                                )
                        })),
                ),
        )
        .child(form_field(
            t("settings.api.form.base_url"),
            form.base_url.as_ref(),
        ))
        .child(form_field(
            t("settings.api.form.api_key"),
            form.api_key.as_ref(),
        ))
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.thinking")),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .children(ThinkingLevel::all().iter().map(|level| {
                            let l = *level;
                            let on = on_action.clone();
                            Button::new(format!("think-{}", format!("{:?}", l).to_lowercase()))
                                .cursor_pointer()
                                .when(form.thinking_level == l, |b| b.primary())
                                .when(form.thinking_level != l, |b| b.ghost())
                                .compact()
                                .label(l.label())
                                .on_click(
                                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                        on(ApiProviderAction::SetThinkingLevel { level: l }, w, a);
                                    },
                                )
                        })),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.selected_model")),
                )
                .child(
                    Button::new("fetch-models-inline")
                        .cursor_pointer()
                        .ghost()
                        .compact()
                        .label(t("general.fetch"))
                        .on_click({
                            let on = on_action.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                on(
                                    ApiProviderAction::FetchModels {
                                        name: String::new(),
                                    },
                                    w,
                                    a,
                                );
                            }
                        }),
                ),
        )
        .when(!form.available_models.is_empty(), |el| {
            el.child(
                h_flex()
                    .gap(px(4.))
                    .flex_wrap()
                    .children(form.available_models.iter().map(|model| {
                        let m = model.clone();
                        let on = on_action.clone();
                        Button::new(format!("select-model-inline-{}", m))
                            .cursor_pointer()
                            .ghost()
                            .compact()
                            .label(m.clone())
                            .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                on(ApiProviderAction::SelectModel { model: m.clone() }, w, a);
                            })
                    })),
            )
        })
        .child(
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(
                    Button::new("inline-api-cancel")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.cancel"))
                        .on_click(cancel_cb),
                )
                .child(
                    Button::new("inline-api-submit")
                        .cursor_pointer()
                        .primary()
                        .label(t("general.update"))
                        .on_click(submit_cb),
                ),
        )
        .into_any_element()
}

pub fn api_provider_form(
    form: &super::ApiProviderFormState,
    on_action: &SharedActionCb<ApiProviderAction>,
) -> impl IntoElement {
    let is_locked = form.locked_name.clone();
    let cancel_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::CancelForm, w, a);
        }
    };
    let submit_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::SubmitForm, w, a);
        }
    };

    let form_title = if form.editing_name.is_some() {
        t("settings.api.form.title_edit")
    } else {
        t("settings.api.form.title_add")
    };
    let submit_label = if form.editing_name.is_some() {
        t("general.update")
    } else {
        t("general.add")
    };
    let presets = form.format.presets();

    v_flex()
        .gap(px(8.))
        .py(px(10.))
        .px(px(12.))
        .border_1()
        .border_color(gpui::rgba(0x45475a66))
        .rounded_md()
        .child(
            txt(px(13.))
                .font_weight(gpui::FontWeight::BOLD)
                .child(form_title),
        )
        .child(form_field(t("settings.api.form.name"), form.name.as_ref()))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.format")),
                )
                .child(api_format_btn(ApiFormat::OpenAI, form.format, on_action))
                .child(api_format_btn(ApiFormat::Anthropic, form.format, on_action)),
        )
        // Preset selector
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    txt(px(12.))
                        .w(px(100.))
                        .child(t("settings.api.form.preset")),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .flex_wrap()
                        .children(presets.iter().map(|preset| {
                            let p = preset.clone();
                            let on = on_action.clone();
                            Button::new(format!(
                                "preset-{}",
                                p.name.to_lowercase().replace(' ', "-")
                            ))
                            .cursor_pointer()
                            .ghost()
                            .compact()
                            .label(p.name.to_string())
                            .on_click(
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    on(ApiProviderAction::ApplyPreset { preset: p.clone() }, w, a);
                                },
                            )
                        })),
                ),
        )
        // Suffix field: show as locked text if a built-in preset is active
        .child({
            let label = t("settings.api.form.suffix");
            if is_locked.is_some() {
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child(label.to_string()))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .items_center()
                            .child(txt(px(12.)).child("/v1/chat/completions".to_string()))
                            .child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child("🔒")),
                    )
                    .into_any_element()
            } else {
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child(label.to_string()))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .children(form.format.suffix_options().iter().map(|suffix| {
                                let s = suffix.to_string();
                                let on = on_action.clone();
                                Button::new(format!("suffix-{}", s.replace('/', "-")))
                                    .cursor_pointer()
                                    .ghost()
                                    .compact()
                                    .label(s.clone())
                                    .on_click(
                                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                            on(
                                                ApiProviderAction::SetSuffix { suffix: s.clone() },
                                                w,
                                                a,
                                            );
                                        },
                                    )
                            })),
                    )
                    .into_any_element()
            }
        })
        .child(form_field(
            t("settings.api.form.custom"),
            form.url_suffix.as_ref(),
        ))
        // Base URL field: show as locked text if a built-in preset is active
        .child({
            let label = t("settings.api.form.base_url");
            if is_locked.is_some() {
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(txt(px(12.)).w(px(100.)).child(label.to_string()))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .items_center()
                            .child(txt(px(12.)).child("https://ai.zenix.dev/v1".to_string()))
                            .child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child("🔒")),
                    )
                    .into_any_element()
            } else {
                form_field(label, form.base_url.as_ref()).into_any_element()
            }
        })
        .child(form_field(
            t("settings.api.form.api_key"),
            form.api_key.as_ref(),
        ))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(txt(px(12.)).w(px(100.)).child(t("settings.api.form.think")))
                .child(
                    h_flex()
                        .gap(px(4.))
                        .children(ThinkingLevel::all().iter().map(|level| {
                            let lev = *level;
                            let selected = lev == form.thinking_level;
                            let on = on_action.clone();
                            Button::new(format!("think-{}", lev.label().to_lowercase()))
                                .cursor_pointer()
                                .when(selected, |b| b.primary())
                                .when(!selected, |b| b.ghost())
                                .compact()
                                .label(lev.label().to_string())
                                .on_click(
                                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                        on(
                                            ApiProviderAction::SetThinkingLevel { level: lev },
                                            w,
                                            a,
                                        );
                                    },
                                )
                        })),
                ),
        )
        .child(if !form.available_models.is_empty() {
            let selected = form.selected_model.clone();
            let search_query = form.model_search.to_lowercase();
            let filtered_models: Vec<&String> = form
                .available_models
                .iter()
                .filter(|m| search_query.is_empty() || m.to_lowercase().contains(&search_query))
                .collect();
            v_flex()
                .gap(px(4.))
                .child(txt(px(12.)).child(t("settings.api.form.model")))
                .child(
                    txt(px(11.)).text_color(gpui::rgba(0x44DD88FF)).child(
                        t("settings.api.form.models_available")
                            .replace("{count}", &form.available_models.len().to_string()),
                    ),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .child(
                            txt(px(16.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("\u{1F50D}"),
                        )
                        .child(
                            txt(px(11.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child(t("general.search")),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .flex_wrap()
                        .children(filtered_models.iter().map(|m| {
                            let m_clone: String = (*m).clone();
                            let sel = m_clone == selected;
                            let on = on_action.clone();
                            Button::new(format!("model-{}", m_clone))
                                .cursor_pointer()
                                .when(sel, |b| b.primary())
                                .when(!sel, |b| b.ghost())
                                .compact()
                                .label(m_clone.clone())
                                .on_click(
                                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                        on(
                                            ApiProviderAction::SelectModel {
                                                model: m_clone.clone(),
                                            },
                                            w,
                                            a,
                                        );
                                    },
                                )
                        })),
                )
                .into_any_element()
        } else {
            txt(px(12.))
                .text_color(gpui::rgba(0x888888FF))
                .child(t("settings.api.form.model_fetch_hint"))
                .into_any_element()
        })
        .child(
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(
                    Button::new("api-form-cancel")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.cancel"))
                        .on_click(cancel_cb),
                )
                .child(
                    Button::new("api-form-submit")
                        .cursor_pointer()
                        .primary()
                        .label(submit_label)
                        .on_click(submit_cb),
                ),
        )
}

pub fn api_format_btn(
    fmt: ApiFormat,
    current: ApiFormat,
    on_action: &SharedActionCb<ApiProviderAction>,
) -> impl IntoElement {
    let selected = fmt == current;
    let on = on_action.clone();
    Button::new(format!("api-fmt-{}", fmt.label().to_lowercase()))
        .cursor_pointer()
        .when(selected, |b| b.primary())
        .when(!selected, |b| b.ghost())
        .compact()
        .label(fmt.label().to_string())
        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::SetFormat { format: fmt }, w, a);
        })
}

pub fn form_field(
    label: impl Into<String>,
    input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(txt(px(12.)).w(px(100.)).child(label.into()))
        .child(
            input
                .map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| {
                    txt(px(12.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child("_")
                        .into_any_element()
                }),
        )
}
