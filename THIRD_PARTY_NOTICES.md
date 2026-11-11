# Third-Party Notices

CapsX 自身源代码使用 **MIT License**（见仓库根目录 `LICENSE`）。下列组件随 CapsX 分发，
各自保留原许可证：

## kanata

- 项目：<https://github.com/jtroo/kanata>
- 版本：v1.12.0（`windows-binaries-x64.zip` 中的 `kanata_windows_tty_winIOv2_x64.exe`）
- 许可证：**LGPL-3.0**
- 随安装包附带完整许可证：`runtime/LICENSE.kanata.txt`
- 未修改二进制。用户可以用兼容的 kanata 构建替换安装目录下的
  `runtime/kanata.exe`。
- 对应源码：<https://github.com/jtroo/kanata/tree/v1.12.0>

## Noto Sans SC（字体子集）

- 项目：<https://fonts.google.com/noto/specimen/Noto+Sans+SC>
- 许可证：**SIL Open Font License 1.1**
- CapsX 内嵌的是从 Noto Sans SC 生成的 UI 字符子集，不是完整字体。
- 许可证全文：`crates/capsx-app/assets/fonts/OFL.txt`
- 子集生成脚本：`tools/make_font_subset.py`

## Rust 依赖

以下 crate 通过 Cargo 依赖树引入，均为宽松开源许可证（MIT / Apache-2.0）：

- egui / eframe / epaint / ecolor / emath / egui-winit / egui_glow
- tray-icon / muda / global-hotkey
- single-instance
- serde / serde_json
- directories
- thiserror / anyhow
- winreg
- windows-sys（仅 Windows FFI：Win32 消息循环与线程 ID）
- log

完整版本与许可证文本可由 `cargo tree` 与各自 crate 的发布包查看。

## 构建工具（不随安装包分发）

- Inno Setup 6：安装器编译器，仅构建时使用。
- Rust / cargo：编译器工具链。
- Python + Pillow + fontTools：仅用于生成品牌素材和字体子集。
