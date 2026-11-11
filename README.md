![CapsX](assets/logo/capsx-wordmark.png)

# CapsX

**把 CapsLock 变成导航层。** 按住 `CapsLock` + `HJKL` = 方向键，轻点 `CapsLock` 仍然是大小写锁定。

基于 [kanata](https://github.com/jtroo/kanata) 这一开源重映射引擎，用 **Rust + egui** 封装成可以**安装、设置、卸载**的小工具——不再是"一堆脚本 + 一堆配置 + 一堆用不到的功能"。

![CapsX 设置窗口](docs/screenshots/settings.png)

> 当前状态：M1（Windows）代码已完成，构建、安装、卸载与关键故障路径已在本机验证。
> 验收记录见 [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)。

---

## 解决什么问题

习惯 vim 的人都有一个肌肉记忆：`HJKL` 是方向。但手一离开主键区去够方向键，思路就断了。

市面上的方案各有各的毛病：

- **AutoHotkey 脚本**：要装运行时、写脚本、出问题难排查；
- **CapsLockX 这类全家桶**：功能很全，但窗口管理、OneNote 联动、番茄钟……你一个都用不上，却要一起加载、一起维护，升级还会覆盖你的修改；
- **改键工具**：大多只能做 1:1 重映射，做不出"按住某键成层"的效果。

CapsX 只做一件事：**把 CapsLock 变成一个导航层**，并且把它做成一个正常软件——有安装包、有设置界面、有托盘、能卸载。

## 特性

- **CapsLock 导航层**：`h j k l` → ← ↓ ↑ →，另有 Home/End、翻页、Tab、Enter、Delete
- **和弦手势**：`h`+`l` 选中当前词，`k`+`j` 选中当前行
- **轻点 CapsLock** 仍是大小写锁定，不改变原有习惯
- **Ctrl / Shift 自动组合**：`CapsLock+Ctrl+h` = Ctrl+Left（按词跳），`CapsLock+Shift+l` = 选中到右边
- **`CapsLock + ,` 打开设置**，即时生效
- **托盘显示 / 隐藏**，开机自启可开关
- **配置坏不了**：应用前先校验，失败自动回滚，键盘永远可用
- **单用户安装、无需管理员**，卸载干净

## 体积与内存

这是本项目刻意控制的两项指标。下表 **CapsX 一列是设计目标**，对比列是本机实测（Windows 11，空闲 5 秒后取样）：

| 指标 | CapsX | 对比 |
|---|---|---|
| 安装包 | **≈ 7 MB**（含 kanata 1.5MB） | Tauri 类 5–10MB、.NET 自包含 40MB+ |
| 应用 exe | **≈ 5–8 MB** | Electron 150MB+ |
| 空闲常驻内存 | **≈ 40–50 MB**（含 kanata 的 17MB） | WebView2 类应用 230–300MB |
| 进程数 | **2 个**（capsx + kanata） | WebView2 类应用 5–7 个 |

> 本地 release 实测：exe **2.22 MiB**、安装包约 **3.4 MiB**。内存受显卡 OpenGL
> 驱动映射影响很大；本机（AMD Radeon）仅一个空 eframe 窗口的 WS 就有 107 MiB，
> 详见验收记录中的基线数据，不把驱动开销误算到 CapsX 逻辑上。

之所以能这么小：**egui 是即时模式 GUI，直接画在 GPU 上，不打包浏览器内核**；而 kanata 本身只有 1.5MB。

## 键位

按住 `CapsLock` 进入导航层：

| 按键 | 输出 | 说明 |
|---|---|---|
| `h` `j` `k` `l` | ← ↓ ↑ → | 方向键 |
| `y` `o` | Home / End | 行首 / 行尾 |
| `u` `i` | PageDown / PageUp | 翻页 |
| `[` `]` | Shift+Tab / Tab | 反向 / 正向切换 |
| `g` | Enter | 回车 |
| `t` | Delete | 删除 |
| `h` + `l` 同按 | 选中当前词 | Ctrl+Left → Ctrl+Shift+Right |
| `k` + `j` 同按 | 选中当前行 | Home → Shift+End |

轻点 `CapsLock` 仍然是大小写锁定。

![键位设置](docs/screenshots/settings-mappings.png)

## 安装

1. 下载 `CapsX-Setup-x.y.z.exe`
2. 双击安装（单用户安装，**不需要管理员权限**）
3. 装完自动启动，托盘出现 CapsX 图标
4. 按住 `CapsLock` 按 `h/j/k/l` 试试

卸载：`设置 → 应用 → CapsX → 卸载`，会停掉后台进程、删除自启项；询问是否保留你的设置。

## 使用

| 操作 | 方式 |
|---|---|
| 打开设置 | `CapsLock + ,` / 托盘菜单 / `Ctrl + Alt + K` |
| 显示 / 隐藏托盘图标 | 设置窗口或托盘菜单 |
| 开机自启 | 设置窗口或托盘菜单 |
| 临时停用 | 托盘菜单「重启 kanata」或退出 CapsX |

> 关闭设置窗口 = 放弃未点击「应用」的修改，并隐藏到托盘；已应用设置不受影响。

> 托盘图标隐藏后，`CapsLock + ,` 和 `Ctrl + Alt + K` 仍然能唤出设置——不会把自己锁死。

## 配置文件在哪

| 文件 | 路径 | 说明 |
|---|---|---|
| 应用设置 | `%APPDATA%\CapsX\settings.json` | 键位、超时、托盘、自启 |
| 生成的 kanata 配置 | `%APPDATA%\CapsX\keymap.kbd` | 由设置生成；应用设置或重启时会被覆盖，请勿手工编辑 |
| 日志 | `%LOCALAPPDATA%\CapsX\logs\` | 排查问题用 |

## 从源码构建

需要：

- [Rust](https://rustup.rs/) stable（MSRV 1.80+）
- [Inno Setup 6](https://jrsoftware.org/isinfo.php)（打包安装器）
- Python 3 + Pillow + fontTools（仅重新生成图标或字体子集时需要）

```powershell
# 一键：格式化/ clippy / 测试 / release / Inno 安装包
powershell -ExecutionPolicy Bypass -File tools/build-release.ps1

# 或分步：
# 1. 拉取 kanata 内核（v1.12.0，自动校验 SHA256）
powershell -ExecutionPolicy Bypass -File tools/fetch-kanata.ps1

# 2. 编译（release profile 已配置 opt-level=z / lto / strip）
cargo build --release

# 3. 本地试跑（会拉起 kanata 子进程）
cargo run --release -p capsx-app

# 4. 生成安装包（Inno Setup 6 的 iscc 需要在 PATH，或用完整路径）
iscc installer/capsx.iss

# 重新生成品牌资产 / 中文字体子集（可选）
python tools/make_brand_assets.py
python tools/make_font_subset.py
```

常用验证命令：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -p capsx-core --test config_snapshots -- --ignored
```

> 字体子集来自 Noto Sans SC（SIL OFL 1.1）。仓库只提交子集，不提交 17 MB 的
> 原始可变字体；`cache/` 已加入 `.gitignore`，脚本会重新生成。首次重新生成前，
> 按 `tools/make_font_subset.py` 的提示把 `NotoSansSC-VF.ttf` 下载到 `cache/fonts/`。
>
> MSRV 为 1.80。`Cargo.lock` 对部分传递依赖做了向下锁定（`muda 0.15.2`、
> `image 0.25.6`、`webbrowser 1.0.5`、ICU 1.5 等），以避开新版依赖悄然抬高
> MSRV；若要 `cargo update`，请重新跑 `cargo +1.80.0 check --locked -p capsx-app`。

## 架构

```
capsx-app  (Rust, eframe/egui)
  ├─ 托盘 + 设置窗口（egui 即时模式，无浏览器内核）
  ├─ 事件循环：std 线程 + channel，无 async 运行时
  └─ TCP 127.0.0.1:<port>，JSON 行协议
        │
kanata.exe (tty/winIOv2 变体，隐藏控制台，唯一持有键盘钩子)
```

拆分方式：

- `crates/capsx-core` —— 纯逻辑：配置生成、kanata 子进程监督、TCP 客户端、平台服务（自启/路径/日志）。**不依赖任何 GUI 库**，可单测、可复用。
- `crates/capsx-app` —— 薄壳：托盘、设置窗、事件分发。

几个刻意的设计决定：

- **托盘归应用，不归 kanata**：所以用 `tty` 变体而不是 `gui` 变体，才能做到"隐藏/显示托盘"。
- **`CapsLock + ,` 由 kanata 触发**：配置里写 `(push-msg "capsx:settings")`，应用收到 TCP 消息后弹设置窗。**应用自己不再挂钩子**——kanata 已经接管了 CapsLock，再挂一个钩子只会在钩子链上打架。
- **配置原子生效**：写临时文件 → `kanata --check` 校验 → 替换 → `Reload{wait:true}`，失败回滚到上一版。
- **不用 WebView**：一个轻量设置窗不值得背一个浏览器内核（实测 WebView2 类应用每个 5–7 进程、230–300MB）。

### 实现注记（winit 隐藏窗口）

AGENTS.md 初稿建议用 `ViewportCommand::Visible(false)` 隐藏窗口。但本机实测
（winit 0.30.13 / Windows 11）隐藏后 Windows 不再派发 `RedrawRequested`，
eframe 的 `update()` 会完全停摆，导致托盘菜单、全局热键、单实例唤出全部失效。
当前实现改为：

- 原生窗口保持 `WS_VISIBLE`，但启动 `--tray` 时放在屏幕外；
- 手动关闭时用 `ViewportCommand::OuterPosition` 移到屏幕外；
- 窗口设 `with_taskbar(false)`，不在任务栏/Alt-Tab 里露出；
- 打开设置时移回上次位置并聚焦。

这仍然满足"隐藏到托盘"的用户可见行为，同时保证 250ms 事件轮询能持续消费
托盘、热键与单实例事件。代价是隐藏时窗口仍在离屏渲染（占用很低）。

### kanata Reload 的 5 秒坑

kanata v1.12 的 `Reload{wait:true}` 会轮询主循环的 `is_reload_complete`，但
主循环需要 TCP 线程发 `WakeUp` 才会推进；TCP 线程又卡在轮询里，必须等满
`timeout_ms` 才发 WakeUp。结果是空闲时一次 Reload 固定耗时 5 秒。

CapsX 在发出 `Reload` 后 100ms 用第二条本地 TCP 短连接发一次
`RequestCurrentLayerName` 触发 WakeUp，ReloadResult 因此能在 ~150ms 返回，
满足"设置 1 秒内生效"。这不是改 kanata 二进制，只是利用它既有的多连接行为。

另外，kanata v1.12 文档写 `push-msg` 发送字符串，实际线上格式是数组：

```json
{"MessagePush":{"message":["capsx:settings"]}}
```

CapsX 的客户端同时兼容字符串和数组两种形式，否则 `CapsLock + ,` 会被静默忽略。

### 托盘菜单为什么在独立线程

Windows 上 `tray-icon` 显示菜单时会进入 `TrackPopupMenu` 的嵌套消息循环。菜单关闭后，
如果它和 eframe/winit 共用一个线程，winit 的重绘调度会停摆，表现为打开设置、快捷开关、
退出等所有托盘菜单项都没有反应。CapsX 因此把托盘图标和菜单放到 `capsx-tray` 专用线程，
线程内自己跑 `GetMessageW`/`DispatchMessageW` 消息泵；菜单动作通过 channel 回到主线程。
主线程从未被菜单循环阻塞，250ms 的设置轮询继续运行。

## 跨平台

当前只发 Windows。架构上已经按跨平台预留：

- `capsx-core::platform` 用 `cfg(target_os)` 分派自启/路径/热键；
- kanata 本身支持 Windows / macOS / Linux；
- GUI 用 egui，三端可移植，且**不依赖 WebView**（Linux 上尤其省事）。

真要上其他平台时的主要成本不在 UI，而在权限与分发：

| 平台 | 成本 |
|---|---|
| macOS | kanata 依赖 Karabiner-Elements 驱动 + 输入监控授权 → 需要首启引导；分发要 Apple 公证 |
| Linux | 需要 `/dev/uinput` 权限（udev 规则），X11/Wayland 差异；打包 AppImage/deb |

## 已知限制

1. **只处理物理按键**。其它软件用 `SendInput` 模拟的按键不会被重映射（kanata 的防冲突设计，因此也没法用脚本自动测试按键，只能人工验证）。
2. **对"以管理员身份运行"的程序不生效**（CapsX 默认非提权）。需要覆盖这类窗口时，可选用「最高权限开机启动」模式（规划中）。
3. **未签名 exe**：首次在新电脑运行可能触发 SmartScreen 提示（更多信息 → 仍要运行）。个人使用没问题，要分发需要代码签名证书。
4. **不能与 CapsLockX 等其它改键工具同时运行**——它们都会接管 CapsLock。
5. 启动时有约 2 秒初始化（kanata 的默认安全延迟）。

## 路线图

- [x] 品牌与键位设计
- [ ] **M1**（Windows）：安装/卸载、托盘、`CapsLock + ,` 设置、键位生效、自启、体积/内存达标
  - 实现、构建、安装/卸载和可自动化的故障路径已完成；物理按键矩阵待人工验证，
    内存上限在本机受 OpenGL 驱动基线影响未达标（见验收记录）
- [ ] **M2**：按键捕获改键、逐项开关、日志查看、导入导出
- [ ] **M3**：macOS / Linux 移植；按应用切层（前台窗口 → `ChangeLayer`）

## 致谢

- [kanata](https://github.com/jtroo/kanata) — 键盘重映射引擎，本项目的基石
- [egui / eframe](https://github.com/emilk/egui) — 轻量即时模式 GUI
- [CapsLockX](https://github.com/snolab/CapsLockX) — 键位设计的灵感来源（「编辑增强」模块）

## 许可

CapsX 自身源代码使用 **MIT License**，全文见 [LICENSE](LICENSE)。
随安装包分发的 `runtime/kanata.exe` 遵循 kanata 的 **LGPL-3.0**，
分发时保留其许可证与版权声明；第三方组件与许可证全文见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
