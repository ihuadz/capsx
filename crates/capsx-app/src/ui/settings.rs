//! 设置窗口主体：顶部标题栏 + 中部设置区 + 底部操作栏。

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, Color32, ComboBox, Frame, Label, Layout, Margin, RichText, ScrollArea, Stroke,
    TextureHandle, TopBottomPanel, Ui,
};

use capsx_core::settings::{default_action, Settings, MAPPING_ACTIONS, MAPPING_KEYS};

use crate::status::{Banner, BannerKind, ServiceState};
use crate::ui::theme;

const SIDE_PADDING: f32 = 20.0;
const HEADER_HEIGHT: f32 = 64.0;
const FOOTER_HEIGHT: f32 = 58.0;
const MAPPING_TWO_COLUMN_WIDTH: f32 = 440.0;

/// 本帧 UI 产生的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    /// 应用设置。
    Apply,
    /// 关闭（隐藏到托盘）。
    Close,
    /// 恢复默认设置。
    RestoreDefaults,
    /// 打开配置目录。
    OpenConfig,
    /// 打开日志目录。
    OpenLogs,
    /// 打开关于窗口。
    About,
    /// 失败后重试启动 kanata。
    RetryService,
}

/// 设置窗口的只读视图数据。
pub struct SettingsView<'a> {
    /// 可编辑的设置副本。
    pub draft: &'a mut Settings,
    /// 服务状态。
    pub state: &'a ServiceState,
    /// 正在应用/回滚。
    pub busy: bool,
    /// 设置文件不可写（未来版本或解析失败）。
    pub read_only: bool,
    /// 是否有未应用修改。
    pub dirty: bool,
    /// 顶部提示。
    pub banner: Option<&'a Banner>,
    /// 成功 toast。
    pub toast: Option<&'a str>,
    /// 左上角 logo。
    pub logo: Option<&'a TextureHandle>,
}

/// 绘制设置窗口，返回本帧动作。
pub fn show(ctx: &egui::Context, view: SettingsView<'_>) -> Option<SettingsAction> {
    let SettingsView {
        draft,
        state,
        busy,
        read_only,
        dirty,
        banner,
        toast,
        logo,
    } = view;
    let mut action = None;

    header_panel(ctx, state, logo);
    footer_panel(ctx, state, busy, read_only, dirty, &mut action);
    settings_panel(
        ctx,
        draft,
        busy,
        read_only,
        dirty,
        banner,
        toast,
        &mut action,
    );

    action
}

fn header_panel(ctx: &egui::Context, state: &ServiceState, logo: Option<&TextureHandle>) {
    TopBottomPanel::top("capsx-header")
        .exact_height(HEADER_HEIGHT)
        .frame(
            Frame::none()
                .fill(theme::BG_TOP)
                .inner_margin(Margin::symmetric(SIDE_PADDING, 12.0)),
        )
        .show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 40.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    if let Some(logo) = logo {
                        ui.image((logo.id(), egui::vec2(32.0, 32.0)))
                            .on_hover_text("CapsX");
                        ui.add_space(8.0);
                    }

                    let title_width = (ui.available_width() - 98.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(title_width, 40.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(
                                RichText::new("CapsX")
                                    .size(20.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add(
                                Label::new(
                                    RichText::new("CapsLock 导航层 · 轻点切换大小写")
                                        .size(12.0)
                                        .color(theme::MUTED),
                                )
                                .truncate(),
                            );
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let response = theme::status_pill(ui, state);
                        if let Some(reason) = state.reason() {
                            response.on_hover_text(reason);
                        }
                    });
                },
            );
        });
}

fn footer_panel(
    ctx: &egui::Context,
    state: &ServiceState,
    busy: bool,
    read_only: bool,
    dirty: bool,
    action: &mut Option<SettingsAction>,
) {
    TopBottomPanel::bottom("capsx-footer")
        .exact_height(FOOTER_HEIGHT)
        .frame(Frame::none().fill(theme::BG_BOTTOM).inner_margin(Margin {
            left: SIDE_PADDING,
            right: SIDE_PADDING,
            top: 11.0,
            bottom: 12.0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.menu_button(RichText::new("… 更多").size(13.0), |ui| {
                    if ui.button("关于 CapsX").clicked() {
                        *action = Some(SettingsAction::About);
                        ui.close_menu();
                    }
                    if ui.button("配置目录").clicked() {
                        *action = Some(SettingsAction::OpenConfig);
                        ui.close_menu();
                    }
                    if ui.button("查看日志").clicked() {
                        *action = Some(SettingsAction::OpenLogs);
                        ui.close_menu();
                    }
                });

                if (*state == ServiceState::Stopped || matches!(state, ServiceState::Failed { .. }))
                    && ui
                        .add(theme::secondary_button("重试启动"))
                        .on_hover_text("重新启动 kanata")
                        .clicked()
                {
                    *action = Some(SettingsAction::RetryService);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let apply_text = if busy { "应用中…" } else { "应用" };
                    if ui
                        .add_enabled(!read_only && !busy, theme::primary_button(apply_text))
                        .on_hover_text("校验配置 → 原子替换 → Reload（失败自动回滚）")
                        .clicked()
                    {
                        *action = Some(SettingsAction::Apply);
                    }
                    if ui
                        .add(theme::secondary_button("关闭"))
                        .on_hover_text("关闭窗口并放弃未应用的修改")
                        .clicked()
                    {
                        *action = Some(SettingsAction::Close);
                    }
                    if dirty && !read_only {
                        ui.label(RichText::new("未应用").size(11.5).color(theme::WARNING));
                    }
                });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn settings_panel(
    ctx: &egui::Context,
    draft: &mut Settings,
    busy: bool,
    read_only: bool,
    dirty: bool,
    banner: Option<&Banner>,
    toast: Option<&str>,
    action: &mut Option<SettingsAction>,
) {
    egui::CentralPanel::default()
        .frame(Frame::none())
        .show(ctx, |ui| {
            theme::paint_background(ui);

            ScrollArea::vertical()
                .id_salt("capsx-settings-scroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    Frame::none()
                        .inner_margin(Margin {
                            left: SIDE_PADDING,
                            right: SIDE_PADDING,
                            top: 16.0,
                            bottom: 20.0,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            if let Some(banner) = banner {
                                banner_ui(ui, banner);
                                ui.add_space(14.0);
                            }

                            navigation_section(ui, draft, read_only, busy);
                            section_divider(ui);
                            behavior_section(ui, draft, read_only, busy);
                            section_divider(ui);
                            mappings_section(ui, draft, read_only, busy, action);

                            ui.add_space(16.0);
                            let hint = if read_only {
                                "设置文件只读：修复配置后重启 CapsX。"
                            } else if busy {
                                "正在写入配置，请稍候…"
                            } else {
                                "点击「应用」后约 1 秒生效，失败自动回滚。"
                            };
                            ui.add(
                                Label::new(RichText::new(hint).size(12.0).color(if read_only {
                                    theme::DANGER
                                } else {
                                    theme::MUTED
                                }))
                                .wrap(),
                            );
                            if let Some(toast) = toast {
                                ui.add_space(4.0);
                                ui.label(RichText::new(toast).size(12.0).color(theme::SUCCESS));
                            }
                            if dirty && !read_only {
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new("有未应用的修改。")
                                        .size(11.5)
                                        .color(theme::WARNING),
                                );
                            }
                        });
                });
        });
}

fn navigation_section(ui: &mut Ui, draft: &mut Settings, read_only: bool, busy: bool) {
    section_heading(ui, "导航与输入");
    let enabled = !read_only && !busy;

    ui.add_enabled_ui(enabled, |ui| {
        setting_checkbox(ui, &mut draft.layer.enabled, "启用导航层");
    });
    ui.add_space(4.0);
    setting_help(ui, "按住 CapsLock + H/J/K/L 移动光标。");
    ui.add_space(14.0);

    ui.add_enabled_ui(enabled && draft.layer.enabled, |ui| {
        setting_checkbox(ui, &mut draft.layer.chords_enabled, "启用和弦");
    });
    ui.add_space(4.0);
    setting_help(ui, "H + L 选中单词 · K + J 选中当前行");
    ui.add_space(18.0);

    ui.add_enabled_ui(enabled && draft.layer.enabled, |ui| {
        timing_row(
            ui,
            "长按判定",
            &mut draft.layer.hold_ms,
            150..=400,
            "数值越小，响应越灵敏。",
        );
    });
    ui.add_enabled_ui(
        enabled && draft.layer.enabled && draft.layer.chords_enabled,
        |ui| {
            timing_row(
                ui,
                "和弦窗口",
                &mut draft.layer.chord_ms,
                50..=200,
                "两个键需在此时间内按下。",
            );
        },
    );
}

fn behavior_section(ui: &mut Ui, draft: &mut Settings, read_only: bool, busy: bool) {
    section_heading(ui, "启动与托盘");
    let enabled = !read_only && !busy;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            setting_checkbox(ui, &mut draft.tray.visible, "显示托盘图标");
            setting_checkbox(ui, &mut draft.autostart, "开机自启");
        });
    });
    ui.add_space(4.0);
    setting_help(ui, "隐藏托盘后，可用 CapsLock + , 或 Ctrl + Alt + K 唤回。");
}

fn mappings_section(
    ui: &mut Ui,
    draft: &mut Settings,
    read_only: bool,
    busy: bool,
    action: &mut Option<SettingsAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("键位映射")
                .size(14.5)
                .strong()
                .color(theme::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(
                    !read_only && !busy,
                    egui::Button::new(RichText::new("恢复默认").size(12.0).color(theme::MUTED))
                        .frame(false)
                        .min_size(egui::vec2(0.0, 24.0)),
                )
                .on_hover_text("把键位恢复为默认映射（仍需点击应用）")
                .clicked()
            {
                *action = Some(SettingsAction::RestoreDefaults);
            }
        });
    });
    ui.add_space(8.0);

    let available = ui.available_width();
    let columns = if available < MAPPING_TWO_COLUMN_WIDTH {
        2
    } else {
        4
    };
    let gap = 10.0;
    let cell_width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).max(80.0);

    for row in MAPPING_KEYS.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for key in row {
                ui.allocate_ui_with_layout(
                    egui::vec2(cell_width, 26.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        theme::keycap(ui, key);
                        ui.add_space(8.0);
                        ui.add(
                            Label::new(
                                RichText::new(action_short(draft.action_for(key)))
                                    .size(12.5)
                                    .color(Color32::from_rgb(0xC8, 0xD2, 0xDF)),
                            )
                            .truncate(),
                        );
                    },
                );
            }
        });
        ui.add_space(6.0);
    }

    ui.add_space(2.0);
    egui::CollapsingHeader::new(RichText::new("自定义键位").size(12.5).color(theme::MUTED))
        .id_salt("capsx-mapping-editor")
        .show(ui, |ui| {
            let enabled = !read_only && !busy;
            ui.add_enabled_ui(enabled, |ui| {
                let available = ui.available_width();
                let gap = 10.0;
                let cell_width = ((available - gap) / 2.0).max(120.0);
                for row in MAPPING_KEYS.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for key in row {
                            ui.allocate_ui_with_layout(
                                egui::vec2(cell_width, 30.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    theme::keycap(ui, key);
                                    ui.add_space(6.0);
                                    let combo_width = (cell_width - 38.0).clamp(96.0, 150.0);
                                    let action = draft
                                        .mappings
                                        .entry((*key).to_string())
                                        .or_insert_with(|| default_action(key).to_string());
                                    ComboBox::from_id_salt(format!("capsx-map-{key}"))
                                        .selected_text(action_label(action))
                                        .width(combo_width)
                                        .show_ui(ui, |ui| {
                                            for candidate in MAPPING_ACTIONS {
                                                ui.selectable_value(
                                                    action,
                                                    (*candidate).to_string(),
                                                    action_label(candidate),
                                                );
                                            }
                                        });
                                },
                            );
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        });
}

fn section_heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(14.5).strong().color(theme::TEXT));
    ui.add_space(12.0);
}

fn section_divider(ui: &mut Ui) {
    ui.add_space(10.0);
    let line_y = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        line_y,
        Stroke::new(1.0, theme::BORDER.gamma_multiply(0.7)),
    );
    ui.add_space(10.0);
}

fn setting_help(ui: &mut Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).size(12.0).color(theme::MUTED)).wrap());
}

fn setting_checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> egui::Response {
    ui.scope(|ui| {
        if *checked && ui.is_enabled() {
            let widgets = &mut ui.style_mut().visuals.widgets;
            widgets.inactive.bg_fill = theme::ACCENT;
            widgets.inactive.bg_stroke = Stroke::new(1.0, theme::ACCENT);
            widgets.inactive.fg_stroke = Stroke::new(1.7, Color32::WHITE);
            widgets.inactive.rounding = egui::Rounding::same(4.0);
            widgets.hovered.bg_fill = theme::ACCENT;
            widgets.hovered.bg_stroke = Stroke::new(1.0, theme::ACCENT);
            widgets.hovered.fg_stroke = Stroke::new(1.7, Color32::WHITE);
            widgets.hovered.rounding = egui::Rounding::same(4.0);
            widgets.active.bg_fill = theme::ACCENT_DIM;
            widgets.active.bg_stroke = Stroke::new(1.0, theme::ACCENT);
            widgets.active.fg_stroke = Stroke::new(1.7, Color32::WHITE);
            widgets.active.rounding = egui::Rounding::same(4.0);
        }
        ui.checkbox(checked, RichText::new(label).size(13.5).color(theme::TEXT))
    })
    .inner
}

fn timing_row(ui: &mut Ui, label: &str, value: &mut u32, range: RangeInclusive<u32>, help: &str) {
    let row_height = 40.0;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row_height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let input_width = 96.0;
            let gap = 16.0;
            let left_width = (ui.available_width() - input_width - gap).max(120.0);

            ui.allocate_ui_with_layout(
                egui::vec2(left_width, row_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(left_width);
                    ui.set_height(row_height);
                    ui.label(RichText::new(label).size(13.5).color(theme::TEXT));
                    ui.add_space(3.0);
                    setting_help(ui, help);
                },
            );
            ui.add_space(gap);
            ui.allocate_ui_with_layout(
                egui::vec2(input_width, row_height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    ui.set_width(input_width);
                    ui.set_height(row_height);
                    ui.add_sized(
                        [96.0, 30.0],
                        egui::DragValue::new(value)
                            .range(range)
                            .speed(1.0)
                            .suffix(" ms"),
                    );
                },
            );
        },
    );
    ui.add_space(10.0);
}

fn banner_ui(ui: &mut Ui, banner: &Banner) {
    let (fill, stroke, icon) = match banner.kind {
        BannerKind::Error => (
            Color32::from_rgba_unmultiplied(0xFF, 0x6B, 0x6B, 20),
            theme::DANGER,
            "!",
        ),
        BannerKind::Warning => (
            Color32::from_rgba_unmultiplied(0xF5, 0xB8, 0x4C, 18),
            theme::WARNING,
            "!",
        ),
    };

    Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke.gamma_multiply(0.6)))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(13.0).strong().color(stroke));
                ui.add(
                    Label::new(RichText::new(&banner.title).size(12.5).color(theme::TEXT)).wrap(),
                );
            });
            if !banner.details.trim().is_empty() {
                egui::CollapsingHeader::new(
                    RichText::new("查看详情").size(11.5).color(theme::MUTED),
                )
                .id_salt("capsx-banner-details")
                .show(ui, |ui| {
                    ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                        theme::detail_text(ui, &banner.details);
                    });
                });
            }
        });
}

fn action_short(action: &str) -> &'static str {
    match action {
        "left" => "←",
        "down" => "↓",
        "up" => "↑",
        "right" => "→",
        "home" => "Home",
        "end" => "End",
        "pgup" => "PgUp",
        "pgdn" => "PgDn",
        "S-tab" => "S-Tab",
        "tab" => "Tab",
        "ret" => "Enter",
        "del" => "Del",
        "esc" => "Esc",
        "bspc" => "Backspace",
        "C-left" => "Ctrl+←",
        "C-right" => "Ctrl+→",
        "C-home" => "Ctrl+Home",
        "C-end" => "Ctrl+End",
        "S-left" => "Shift+←",
        "S-right" => "Shift+→",
        "S-home" => "Shift+Home",
        "S-end" => "Shift+End",
        _ => "?",
    }
}

fn action_label(action: &str) -> &'static str {
    match action {
        "left" => "← 左移",
        "down" => "↓ 下移",
        "up" => "↑ 上移",
        "right" => "→ 右移",
        "home" => "Home 行首",
        "end" => "End 行尾",
        "pgup" => "PageUp",
        "pgdn" => "PageDown",
        "S-tab" => "Shift+Tab",
        "tab" => "Tab",
        "ret" => "Enter",
        "del" => "Delete",
        "esc" => "Esc",
        "bspc" => "Backspace",
        "C-left" => "Ctrl+←",
        "C-right" => "Ctrl+→",
        "C-home" => "Ctrl+Home",
        "C-end" => "Ctrl+End",
        "S-left" => "Shift+←",
        "S-right" => "Shift+→",
        "S-home" => "Shift+Home",
        "S-end" => "Shift+End",
        _ => "未知",
    }
}
