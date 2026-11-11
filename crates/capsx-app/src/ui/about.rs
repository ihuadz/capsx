//! 关于窗口。

use std::path::Path;

use eframe::egui::{self, RichText};

use crate::ui::theme;

/// 关于窗口展示的信息。
pub struct AboutInfo<'a> {
    /// CapsX 版本。
    pub app_version: &'a str,
    /// 已连接的 kanata 版本。
    pub kanata_version: Option<&'a str>,
    /// 设置目录。
    pub config_dir: &'a Path,
    /// 日志目录。
    pub logs_dir: &'a Path,
}

/// 绘制关于窗口。
pub fn show(ctx: &egui::Context, open: &mut bool, info: AboutInfo<'_>) {
    let mut visible = *open;
    egui::Window::new("关于 CapsX")
        .open(&mut visible)
        .collapsible(false)
        .resizable(false)
        .default_width(390.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new("CapsX")
                    .size(26.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new("把 CapsLock 变成导航层")
                    .size(12.0)
                    .color(theme::ACCENT),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "轻点 CapsLock 仍切换大小写；按住 + HJKL 移动光标。\
                     键盘钩子完全由 kanata 持有，CapsX 不另挂钩子。",
                )
                .size(12.0)
                .color(theme::MUTED),
            );
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            egui::Grid::new("capsx-about-grid")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("CapsX 版本").size(11.5).color(theme::MUTED));
                    ui.label(
                        RichText::new(info.app_version)
                            .size(11.5)
                            .color(theme::TEXT),
                    );
                    ui.end_row();
                    ui.label(RichText::new("kanata").size(11.5).color(theme::MUTED));
                    let kanata = info.kanata_version.unwrap_or("未连接");
                    ui.label(RichText::new(kanata).size(11.5).color(theme::TEXT));
                    ui.end_row();
                    ui.label(RichText::new("配置目录").size(11.5).color(theme::MUTED));
                    ui.label(
                        RichText::new(info.config_dir.display().to_string())
                            .size(10.5)
                            .color(theme::MUTED),
                    );
                    ui.end_row();
                    ui.label(RichText::new("日志目录").size(11.5).color(theme::MUTED));
                    ui.label(
                        RichText::new(info.logs_dir.display().to_string())
                            .size(10.5)
                            .color(theme::MUTED),
                    );
                    ui.end_row();
                });

            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("kanata 项目", "https://github.com/jtroo/kanata");
                ui.label(RichText::new("·").color(theme::MUTED));
                ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                ui.label(RichText::new("·").color(theme::MUTED));
                ui.hyperlink_to(
                    "Noto Sans SC (OFL)",
                    "https://fonts.google.com/noto/specimen/Noto+Sans+SC",
                );
            });
            ui.label(
                RichText::new("kanata 以 LGPL-3.0 分发；CapsX 源代码建议 MIT。")
                    .size(10.0)
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("关闭")).clicked() {
                    *open = false;
                }
            });
        });
    if !visible {
        *open = false;
    }
}
