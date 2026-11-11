//! CapsX 配色、字体与 egui 样式。

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Rounding, Stroke,
    TextStyle, Vec2, Visuals,
};

use crate::assets;
use crate::status::ServiceState;

/// 品牌强调色。
pub const ACCENT: Color32 = Color32::from_rgb(0x3B, 0xE3, 0xAE);
/// 强调色的暗调。
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1F, 0x8F, 0x6F);
/// 背景顶部。
pub const BG_TOP: Color32 = Color32::from_rgb(0x11, 0x16, 0x1F);
/// 背景底部。
pub const BG_BOTTOM: Color32 = Color32::from_rgb(0x11, 0x16, 0x1F);
/// 卡片背景。
pub const CARD: Color32 = Color32::from_rgb(0x14, 0x1C, 0x28);
/// 次级卡片。
pub const CARD_SOFT: Color32 = Color32::from_rgb(0x18, 0x21, 0x30);
/// 键帽背景。
pub const KEYCAP: Color32 = Color32::from_rgb(0x24, 0x2D, 0x3E);
/// 键帽/控件描边。
pub const BORDER: Color32 = Color32::from_rgb(0x2A, 0x35, 0x48);
/// 主文字。
pub const TEXT: Color32 = Color32::from_rgb(0xEA, 0xF2, 0xFA);
/// 次要文字。
pub const MUTED: Color32 = Color32::from_rgb(0x91, 0xA1, 0xB8);
/// 错误。
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
/// 警告。
pub const WARNING: Color32 = Color32::from_rgb(0xF5, 0xB8, 0x4C);
/// 成功/运行。
pub const SUCCESS: Color32 = Color32::from_rgb(0x3B, 0xE3, 0xAE);
/// 关闭/中性。
pub const NEUTRAL: Color32 = Color32::from_rgb(0x7A, 0x8A, 0xA3);

const FONT_KEY: &str = "capsx-sans";

/// 安装字体与视觉样式。
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        FONT_KEY.to_owned(),
        FontData::from_static(assets::FONT_REGULAR),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, FONT_KEY.to_owned());
    }
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 9.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.slider_width = 150.0;
    style.spacing.combo_width = 110.0;
    style.spacing.scroll.floating = false;
    style.spacing.scroll.bar_width = 7.0;
    style.spacing.scroll.bar_inner_margin = 6.0;
    style.spacing.scroll.bar_outer_margin = 2.0;
    style.spacing.scroll.foreground_color = false;
    style.interaction.selectable_labels = false;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    ctx.set_style(style);
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG_BOTTOM;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = BG_BOTTOM;
    visuals.faint_bg_color = CARD_SOFT;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_rounding = Rounding::same(8.0);
    visuals.menu_rounding = Rounding::same(8.0);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, BG_BOTTOM);
    visuals.widgets.noninteractive.bg_fill = CARD_SOFT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.rounding = Rounding::same(7.0);
    visuals.widgets.inactive.bg_fill = KEYCAP;
    visuals.widgets.inactive.weak_bg_fill = CARD_SOFT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(7.0);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2C, 0x38, 0x4D);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x25, 0x30, 0x43);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.rounding = Rounding::same(7.0);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, BG_BOTTOM);
    visuals.widgets.active.rounding = Rounding::same(7.0);
    visuals.widgets.open.bg_fill = KEYCAP;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.open.rounding = Rounding::same(7.0);
    visuals
}

/// 画平直深色背景。
pub fn paint_background(ui: &egui::Ui) {
    ui.painter().rect_filled(ui.max_rect(), 0.0, BG_TOP);
}

/// 状态点颜色。
pub fn status_color(state: &ServiceState) -> Color32 {
    match state {
        ServiceState::Running => SUCCESS,
        ServiceState::Starting | ServiceState::Reconnecting | ServiceState::Restarting { .. } => {
            WARNING
        }
        ServiceState::Failed { .. } => DANGER,
        ServiceState::Stopped => NEUTRAL,
    }
}

/// 绘制状态胶囊。
pub fn status_pill(ui: &mut egui::Ui, state: &ServiceState) -> egui::Response {
    let color = status_color(state);
    let galley = ui.painter().layout_no_wrap(
        format!("● {}", state.label()),
        FontId::new(11.5, FontFamily::Proportional),
        color,
    );
    let size = Vec2::new(galley.size().x + 14.0, galley.size().y + 7.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        Rounding::same(14.0),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 24),
        Stroke::new(1.0, color.gamma_multiply(0.5)),
    );
    let text_pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, color);
    response
}

/// 小键帽。固定尺寸，保证键位网格对齐。
pub fn keycap(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, 24.0), egui::Sense::hover());
    ui.painter()
        .rect(rect, Rounding::same(6.0), KEYCAP, Stroke::new(1.0, BORDER));
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(11.5, FontFamily::Monospace),
        TEXT,
    );
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, TEXT);
    response
}

/// 主按钮。
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .size(13.5)
            .color(BG_BOTTOM)
            .strong(),
    )
    .fill(ACCENT)
    .rounding(Rounding::same(7.0))
    .min_size(Vec2::new(80.0, 34.0))
}

/// 次级按钮。
pub fn secondary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).size(13.5).color(TEXT))
        .fill(CARD_SOFT)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(64.0, 34.0))
}

/// 用户文本中的 `\n` 用 Label 显示。
pub fn detail_text(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .monospace()
                .size(11.0)
                .color(Color32::from_rgb(0xD8, 0xDF, 0xEA)),
        )
        .wrap(),
    );
}
