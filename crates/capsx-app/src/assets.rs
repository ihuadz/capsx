//! 内嵌资产：中文字体、窗口/托盘图标。
//!
//! 字体是 Noto Sans SC 的子集（SIL OFL 1.1），见 `assets/fonts/OFL.txt`；
//! 通过 `tools/make_font_subset.py` 可复现。

use eframe::egui::{ColorImage, IconData, TextureHandle, TextureOptions};

/// 中文子集字体（同时包含 Latin 与常用符号）。
pub const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular-subset.ttf");

/// 64×64 RGBA，窗口图标。
pub const WINDOW_ICON_RGBA: &[u8] = include_bytes!("../assets/icon-64.rgba");

/// 32×32 RGBA，托盘图标。
pub const TRAY_ICON_RGBA: &[u8] = include_bytes!("../assets/icon-32.rgba");

/// 窗口图标尺寸。
pub const WINDOW_ICON_SIZE: u32 = 64;

/// 托盘图标尺寸。
pub const TRAY_ICON_SIZE: u32 = 32;

/// 托盘图标 RGBA；失败态在右下角叠加红色圆点。
pub fn tray_icon_rgba(alert: bool) -> Vec<u8> {
    let mut rgba = TRAY_ICON_RGBA.to_vec();
    if !alert {
        return rgba;
    }

    let size = TRAY_ICON_SIZE as usize;
    let center = 22.5_f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > 9.0 {
                continue;
            }
            let index = (y * size + x) * 4;
            if distance > 7.2 {
                rgba[index..index + 4].copy_from_slice(&[0x0B, 0x0F, 0x16, 0xFF]);
            } else {
                rgba[index..index + 4].copy_from_slice(&[0xFF, 0x5A, 0x5A, 0xFF]);
            }
        }
    }
    rgba
}

/// 构造 eframe 窗口图标。
pub fn window_icon() -> IconData {
    IconData {
        rgba: WINDOW_ICON_RGBA.to_vec(),
        width: WINDOW_ICON_SIZE,
        height: WINDOW_ICON_SIZE,
    }
}

/// 为设置窗口创建左上角 logo 纹理。
pub fn load_logo(ctx: &eframe::egui::Context) -> TextureHandle {
    let image = ColorImage::from_rgba_unmultiplied(
        [WINDOW_ICON_SIZE as usize, WINDOW_ICON_SIZE as usize],
        WINDOW_ICON_RGBA,
    );
    ctx.load_texture("capsx-logo", image, TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_alert_icon_differs() {
        let normal = tray_icon_rgba(false);
        let alert = tray_icon_rgba(true);
        assert_eq!(normal.len(), (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize);
        assert_eq!(alert.len(), normal.len());
        assert_ne!(normal, alert);
    }
}
