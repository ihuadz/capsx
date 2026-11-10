# AGENTS.md — CapsX 实现说明（Rust + egui）

> 本文件是这个仓库的**唯一技术契约**。动手前先读完，尤其是第 5、6、11、14 节。
> 目标读者：一个没有历史上下文的编码 Agent。所有"为什么这么做"的决定都已写在下面，不要自行"优化"成相反的做法。

---

## 1. 一句话定义

CapsX 是一个**键盘导航层工具**：把 [kanata](https://github.com/jtroo/kanata) 封装成可安装、可设置、可卸载的桌面应用。

核心卖点：**按住 CapsLock + HJKL = 方向键，轻点 CapsLock 仍是大小写锁定**。

设计约束（按优先级）：**体积小、内存低、跨平台可移植**。功能刻意保持很少。

## 2. 本期范围（M1，仅 Windows）

| # | 功能 | 说明 |
|---|---|---|
| 1 | 安装 / 卸载 | 单用户，不需要管理员；出现在「设置 → 应用」 |
| 2 | 开机自启 | 可开关，Windows 写 `HKCU\...\Run` |
| 3 | 托盘常驻 | 可隐藏 / 显示，状态可读 |
| 4 | 设置窗口 | `CapsLock + ,` / 托盘菜单 / `Ctrl+Alt+K` 三条路径 |
| 5 | 键位生效 | CapsLock 导航层：h j k l / y o / u i / [ ] / g / t |
| 6 | 和弦 | `h`+`l` 选词、`k`+`j` 选行（可关） |
| 7 | 设置即时生效 | 保存后 1 秒内生效，失败自动回滚 |
| 8 | 设置持久化 | `%APPDATA%\CapsX\settings.json`，升级不丢 |

**非目标（本期不做）**：按应用切层、云同步、自动更新、多语言 UI、主题皮肤。

## 3. 技术栈（唯一允许的组合）

| 层 | 选型 |
|---|---|
| 语言 | **Rust**，stable，`edition = "2021"`，MSRV **1.80** |
| GUI | **egui / eframe 0.29+**，**`glow`（OpenGL）后端**，不要 wgpu |
| 托盘 | `tray-icon` |
| 全局热键 | `global-hotkey` |
| 单实例 | `single-instance` |
| JSON | `serde` + `serde_json` |
| 路径 | `directories`（ProjectDirs） |
| 注册表（Win） | `winreg` |
| 错误 | `thiserror`（库层）/ `anyhow`（应用层） |
| 日志 | 自写 100 行滚动日志（不引 `tracing`） |
| 内核 | kanata **v1.12.0**，`winIOv2` + `tty` 变体 |

**依赖白名单**：上面这些 + `log`（可选）。其余 crate 引入前必须在 PR 里说明理由与体积影响。

**明确禁止**：`tokio` / 任何 async 运行时、`reqwest`、`tauri`、`electron`、`qt`、`gtk`（Linux 托盘由 `tray-icon` 内部处理）、`azure`/`webview` 类依赖。

**发布 profile（写进根 `Cargo.toml`）**：

```toml
[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

目标二进制：**≤ 8 MB**（含 egui）。

## 4. 目录结构（workspace）

```
capsx/
├─ Cargo.toml                    # [workspace] + release profile
├─ crates/
│   ├─ capsx-core/               # 纯逻辑，无 GUI 依赖，可单测
│   │   ├─ src/lib.rs
│   │   ├─ src/settings.rs       # SettingsModel + 版本迁移
│   │   ├─ src/config_gen.rs     # settings → keymap.kbd
│   │   ├─ src/kanata.rs         # 子进程启动/监督/退避
│   │   ├─ src/client.rs         # TCP JSON 客户端（std::net + 线程）
│   │   ├─ src/paths.rs
│   │   ├─ src/logger.rs
│   │   └─ src/platform/
│   │       ├─ mod.rs            # trait Autostart / Hotkey 等
│   │       ├─ windows.rs
│   │       ├─ macos.rs          # M2 占位
│   │       └─ linux.rs          # M2 占位
│   └─ capsx-app/                # egui 托盘壳
│       ├─ src/main.rs
│       ├─ src/app.rs            # eframe::App 实现、事件循环
│       ├─ src/ui/settings.rs    # 设置窗口
│       ├─ src/ui/about.rs
│       └─ src/tray.rs
├─ assets/logo/
├─ runtime/                      # kanata 二进制（fetch 脚本产出，不入库）
├─ installer/capsx.iss
├─ tools/fetch-kanata.ps1
├─ tools/make_brand_assets.py
├─ README.md
└─ AGENTS.md
```

> `capsx-core` **不允许**依赖 `eframe`/`egui`/`tray-icon`——它必须能在没有 GUI 的环境里跑（测试、将来的 CLI、其他前端）。

## 5. 运行时架构

```
[主线程] capsx-app (eframe, glow)
   ├─ App::new(): 读设置、算端口、校验配置、启动 kanata、连 TCP
   ├─ 首次 update(): 创建 tray-icon + global-hotkey（必须主线程，见 5.3）
   ├─ update(): 消费 AppEvent 队列 → 更新 UI / 执行动作
   └─ 窗口 visible=false 时仍每 250ms request_repaint_after 轮询
[子线程] kanata 监督：spawn / wait / 退避重启 → 发 AppEvent::KanataExited
[子线程] TCP 读取：BufReader::lines → serde_json → 发 AppEvent::Message
[子线程] 日志写入（可选，或主线程小批量刷）
```

事件用 `std::sync::mpsc::channel` 传递；**不使用 async 运行时**。

### 5.1 启动顺序（严格）

1. 单实例检查（`single-instance`）；第二次启动 → 让已运行实例打开设置窗 → 新进程退出。
2. 加载 `settings.json`（缺失则用默认值生成）。
3. 选空闲端口：`TcpListener::bind("127.0.0.1:0")` → 取端口 → drop。
4. `config_gen` 生成配置 → `kanata --check` 校验 → 原子写入 `keymap.kbd`。
5. 启动 kanata 子进程（第 6 节参数），接管 stdout/stderr。
6. TCP 连接 + `Hello` 握手（5s 超时）。
7. 建托盘图标；`settings.tray.visible == false` 则不建。
8. 若以 `--tray` 启动（登录自启），不显示窗口。

### 5.2 关闭顺序

1. 置"正在退出"标志，停重启逻辑。
2. 断开 TCP。
3. `Child::kill()` + `wait()`（≤2s，超时强杀）。
4. 移除托盘图标，落盘设置。

任何退出路径（含未捕获 panic、任务管理器结束进程）都必须让 kanata 一起退出：
- `panic = "abort"` 下拿不到正常析构，因此**启动前给子进程加 Job Object**（Windows）或 `PR_SET_PDEATHSIG`（Linux），保证父进程死亡时子进程被回收。
- 卸载器还要额外调用 `CapsX.exe --uninstall` 兜底。

### 5.3 托盘与热键的线程约束

- `tray-icon` 与 `global-hotkey` 都需要**消息循环**：在 Windows 上必须在主线程创建，且主线程要有消息泵（winit 的循环满足）。
- 因此：**第一次 `update()` 里创建**它们，保存到 App 状态；不要在 `main()` 里提前创建。
- 窗口隐藏时不产生重绘，所以隐藏态必须 `ctx.request_repaint_after(Duration::from_millis(250))`，否则托盘菜单点击无响应。
- 若实际测试发现托盘菜单不响应（不同 winit 版本行为差异），退路：把托盘放到专用线程并自己跑 `GetMessage` 循环，通过 channel 通知主线程 `request_repaint()`。**只有实测失败才走这条退路**，不要提前复杂化。

## 6. kanata 集成契约

### 6.1 二进制

- 来源：kanata release **v1.12.0** 的 `windows-binaries-x64.zip`
  （SHA256 `13947ed78cfa3284bfef854e3c542c74ab366236b72fd9f7e039f8638deead9d`）
- 用 `tools/fetch-kanata.ps1` 下载并校验，产出 `runtime\kanata.exe`、`runtime\kanata.version`、`runtime\LICENSE.kanata.txt`。
- 取其中 **`kanata_windows_tty_winIOv2_x64.exe`**，重命名为 `runtime\kanata.exe`。
- **不要**用 `gui` 变体（自带托盘图标，与 CapsX 冲突）；**不要**用 `wintercept`（要驱动，有蓝牙键盘失灵已知问题）；**不要**改二进制。
- 跨平台预留：`runtime/` 按平台放不同二进制，用 `cfg(target_os)` 选择文件名。

### 6.2 启动参数

```
kanata.exe -c "<config_dir>/keymap.kbd" -p 127.0.0.1:<port> -q
```

- `-q`：只输出错误。
- 不加 `-n`：保留默认 2 秒启动安全延迟。
- 子进程用 `CreateNoWindow`（Windows）/ `pre_exec`（Linux）确保无控制台窗口。
- stderr 每行写日志，前缀 `[kanata]`。
- 紧急退出：物理 `LCtrl + Space + Esc` 会让 kanata 立即退出（写进 README 排障）。

### 6.3 TCP 协议（换行分隔 JSON，UTF-8）

**客户端 → kanata**

| 消息 | 用途 |
|---|---|
| `{"Hello":{}}` | 握手，回 `HelloOk` |
| `{"Reload":{"wait":true,"timeout_ms":5000}}` | 重载配置，回 `ReloadResult` |
| `{"RequestCurrentLayerName":{}}` | 查当前层（可选，用于托盘状态） |
| `{"ChangeLayer":{"new":"nav"}}` | 切层（M3 按应用切层用） |
| `{"ActOnFakeKey":{"name":"x","action":"Tap"}}` | 触发虚键（可选） |

**kanata → 客户端（事件）**

| 消息 | 处理 |
|---|---|
| `{"MessagePush":{"message":"capsx:settings"}}` | 打开设置窗口 |
| `{"LayerChange":{"new":"nav"}}` | 刷新托盘状态（可选） |
| `{"ConfigFileReload":{"new":"..."}}` | 记日志 |
| `{"Error":{"msg":"..."}}` | 记日志 + 提示 |
| `{"ReloadResult":{"ok":true}}` | 配置应用结果（关键路径） |
| `{"HoldActivated"/"TapActivated":...}` | 仅调试 |

要求：`BufReader::new(stream).lines()` 循环；单行 JSON；解析失败只记日志不崩；断线后 3s 重连，最多 5 次。

### 6.4 子进程监督

- 非预期退出 → 退避重启：1s / 2s / 4s / 8s / 15s / 30s（封顶）。
- **10 秒内连续 3 次失败 → 停止重启**，托盘变红，弹一次提示（查看日志 / 重试 / 打开配置）。
- 用户主动退出时不得重启。
- 启动后 10 秒内 TCP 未连上，视为启动失败，走上面的失败路径。

## 7. 配置生成（关键路径）

### 7.1 `settings.json`

```json
{
  "version": 1,
  "tray": { "visible": true },
  "autostart": true,
  "layer": { "tapRepressMs": 200, "holdMs": 200, "chordMs": 80, "chordsEnabled": true, "enabled": true },
  "mappings": {
    "h": "left", "j": "down", "k": "up", "l": "right",
    "y": "home", "o": "end", "u": "pgdn", "i": "pgup",
    "[": "S-tab", "]": "tab", "g": "ret", "t": "del"
  },
  "settingsKey": ","
}
```

- `version` 用于迁移；读到更高版本 → 保留文件 + 提示，不覆盖。
- 未知字段必须原样保留（`#[serde(flatten)] extra: Map<String, Value>`）。
- `mappings` 的键是 kanata 键名，值必须来自白名单（设置窗只允许选白名单里的动作）。

### 7.2 生成的 `keymap.kbd`

参照 `runtime/default.kbd`（**已用 `kanata --check` 验证通过**），结构：

```
;; 注释必须用双分号
(defchords navchords <chordMs>
  (h) left  (j) down  (k) up  (l) right
  (h l) (macro C-left C-S-right)
  (k j) (macro home S-end)
)
(defalias ch-h (chord navchords h) ... )
(defcfg process-unmapped-keys yes)
(defsrc caps <keys...> <settingsKey>)
(deflayer base (tap-hold-press <tapRepressMs> <holdMs> caps (layer-while-held nav)) <keys...> <settingsKey>)
(deflayer nav caps @ch-h @ch-j @ch-k @ch-l home end pgdn pgup S-tab tab ret del (push-msg "capsx:settings"))
```

**三个必须遵守的规则：**

1. **kanata 注释必须是 `;;`**。单个 `;` 会被当成内容，启动时报 `Everything must be in a list` 并拒绝加载（已实测）。
2. **`defsrc` 与 `deflayer` 的列必须严格对齐**。生成器必须按固定顺序的 `Vec<Key>` 拼接，**禁止遍历 HashMap**（顺序不稳定会导致整层错位）。
3. 关闭和弦时，nav 层直接用 `left right up down`，并**不要输出** `defchords`/`defalias` 段。

### 7.3 应用流程（原子）

1. 生成候选文本 → 写 `<config_dir>/keymap.kbd.new`。
2. 跑 `kanata --check -c <候选>`（隐藏窗口，5s 超时）；失败 → 展示 stderr，**不替换**。
3. 备份旧文件 → `fs::rename(候选, keymap.kbd)`。
4. 发 `{"Reload":{"wait":true,"timeout_ms":5000}}`。
5. `ok=false` 或超时 → 用备份还原 + 再 Reload 一次 → 提示失败原因。
6. 成功后原子写 `settings.json`（tmp + rename）。

## 8. UI 规格（egui）

### 8.1 窗口形态

- 单窗口，标题 `CapsX 设置`，约 520×420，可缩放但不记忆尺寸（M1）。
- 隐藏用 `ViewportCommand::Visible(false)`，**不要销毁窗口**（重建代价高、也会丢托盘线程状态）。
- 关闭按钮 = 隐藏到托盘（`settings.tray.visible == false` 时也要能通过热键唤回）。
- 中文字体：egui 默认字体不含 CJK，**必须内嵌一个中文子集字体**（如思源黑体子集，或加载 `C:\Windows\Fonts\msyh.ttc` 的 fallback）。为保证跨平台，**优先内嵌子集**，体积控制在 ~1.5MB 内；实在不行再用系统字体 + fallback 列表。

### 8.2 界面内容（M1）

```
CapsX                                   [状态: 运行中 ●]
─────────────────────────────────────
[✓] 启用导航层
[✓] 启用和弦（h+l 选词 / k+j 选行）
    长按判定   [ 200 ] ms   (150–400)
    和弦窗口   [  80 ] ms   (50–200)
[✓] 显示托盘图标
[✓] 开机自启
─────────────────────────────────────
键位：  h← j↓ k↑ l→   y=Home o=End   u=PgDn i=PgUp   [=S-Tab ]=Tab  g=Enter t=Del
                                          [恢复默认]
─────────────────────────────────────
[应用]  [关闭]              [打开配置文件夹] [查看日志]
```

- 状态圆点：绿=运行中，黄=重启中，红=启动失败。
- 「应用」走 7.3 流程；失败时窗口不关，顶部显示红色错误条（可展开看 stderr）。
- 设置窗单例；重复唤出 = 置前聚焦。

### 8.3 托盘菜单

```
打开设置…
——
状态：运行中
[✓] 启用导航层
[✓] 显示托盘图标
[✓] 开机自启
——
重启 kanata
打开配置文件夹
查看日志
——
退出
```

### 8.4 唤出路径（三条，缺一不可）

1. `CapsLock + ,`（kanata `push-msg`）
2. 托盘菜单
3. `Ctrl + Alt + K`（`global-hotkey`，**不要**自写键盘钩子）

## 9. 安装 / 打包

### 9.1 Windows（M1）

- Inno Setup 6，`PrivilegesRequired=lowest`，`DefaultDirName={localappdata}\Programs\CapsX`
- 打包 `capsx.exe` + `runtime\*.exe` + `default.kbd` + kanata 的 LICENSE
- 卸载前执行 `{app}\capsx.exe --uninstall`（等待 ≤10s 后强杀）
- 卸载询问是否保留 `%APPDATA%\CapsX`

### 9.2 自启（按平台）

| 平台 | 方式 |
|---|---|
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值 `"<path>" --tray` |
| macOS | `~/Library/LaunchAgents/com.capsx.agent.plist` |
| Linux | `~/.config/autostart/capsx.desktop` |

实现放在 `capsx-core::platform`，用 `cfg(target_os)` 分派；M1 只实现 Windows，另外两个留空函数 + `todo!` 会被 clippy 拦，改用返回 `Err(Unsupported)`。

### 9.3 跨平台路线（M2/M3，不在本期）

- macOS：kanata 需要安装 **Karabiner-Elements 驱动**并要求输入监控/辅助功能授权 → 需要首启引导向导；分发包必须**公证**（Apple 开发者账号）。
- Linux：需要 `/dev/uinput` 权限（udev 规则 + 安装脚本），X11/Wayland 差异；打包 AppImage/deb（注意 AppImage 不要捆绑 WebKit——我们用 egui，无此问题）。
- 这三项的成本远大于 UI 移植，排期时不要低估。

## 10. 可靠性与日志

- 所有落盘写操作：tmp + rename，禁止原地覆盖。
- 日志：`<data_dir>/CapsX/logs/capsx-YYYYMMDD.log`，保留 7 天，>5MB 滚动。
- panic hook：写日志 + 尝试结束子进程 + 托盘提示（`panic=abort` 下用 `std::panic::set_hook`）。
- 释放顺序见 5.2。

## 11. 体积与内存预算（实测基线）

同机实测（Windows 11，空闲 5s 后取样）：

| 组件 | 工作集 | 说明 |
|---|---|---|
| kanata（Rust，LLHOOK） | **17.4 MB** | 固定成本，不可优化 |
| net48 WinForms 托盘壳 | 20.3 MB | 仅作参照 |
| .NET 10 自包含 WinForms | 33.9 MB | 仅作参照（exe 110MB） |
| WebView2 类应用 | 229–302 MB | 反例，说明为什么不用 WebView |

CapsX（egui）预算：

| 指标 | 目标 | 上限 |
|---|---|---|
| 安装包 | ≤ 8 MB | 10 MB |
| 安装后磁盘 | ≤ 12 MB | 15 MB |
| 空闲常驻（两进程合计 WS） | ≤ 45 MB | 60 MB |
| 打开设置窗峰值 | ≤ 65 MB | 90 MB |
| 进程数 | 2（capsx + kanata） | 2 |
| 空闲 CPU | < 0.3% | 1% |
| 冷启动到可用 | ≤ 3 s（含 kanata 2s 安全延迟） | 5 s |

验收时用 `Get-Process capsx,kanata | Measure-Object WorkingSet64 -Sum` 实测并记录在 PR 里。

## 12. 测试

### 12.1 自动化（CI 必须绿）

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- `config_gen` 快照测试：默认设置 / 关心和 / 自定义映射 / 极端超时 → 与 `tests/snapshots/*.kbd` 逐字节一致。
- 每个生成的 `.kbd` 都必须能通过 `kanata --check`（用 `runtime\kanata.exe`，测试里 `#[ignore]` 标记为需二进制的集成测试）。
- `KanataClient`：用本地 `TcpListener` mock，测 JSON 编解码、超时、断线重连。
- `settings`：版本迁移、未知字段保留。

### 12.2 手动测试矩阵（PR 里逐项打勾）

| # | 场景 | 期望 |
|---|---|---|
| 1 | 记事本 `CapsLock + h/j/k/l` | 光标移动，不输入字母 |
| 2 | 轻点 CapsLock | 大小写锁定切换 |
| 3 | `CapsLock + Ctrl + h` | 按词左移 |
| 4 | `CapsLock + Shift + l` | 向右选中 |
| 5 | `h`+`l` 几乎同时按 | 选中一个词 |
| 6 | `CapsLock + ,` | 弹出设置窗，不输入逗号 |
| 7 | 取消「显示托盘图标」后按热键 | 设置窗仍能打开 |
| 8 | 改一个映射并应用 | 1 秒内生效 |
| 9 | 手工写坏 `keymap.kbd` 后启动 | 回滚 + 提示，键盘可用 |
| 10 | 结束 kanata.exe | 3 秒内自动重启 |
| 11 | 卸载（运行中） | 无残留进程/自启项 |
| 12 | 重装 | 设置保留 |
| 13 | 中文输入法激活时 | 1–6 仍成立 |
| 14 | 隐藏窗口后内存 | 回到 ≤ 60 MB |

> **重要**：kanata 只处理**物理按键**，对 `SendInput` 等合成输入不做重映射（防冲突设计）。因此 1–6 无法用脚本自动化，必须人工验证。不要为"自动化"去改 kanata。

## 13. 品牌资产（已就绪）

- `assets/logo/capsx.ico`、`capsx-icon-{16..1024}.png`、`capsx-icon.svg`、`capsx-wordmark.png`、`alt/`（琥珀变体）
- 配色：强调 `#3BE3AE`；底 `#1B2231→#0B0F16`；键帽 `#242D3E→#1A2130`；描边 `#33405A`
- 重新生成：`python tools/make_brand_assets.py`（需要 Pillow）

egui 里把配色写成 `const`，窗口标题栏/控件用强调色点缀，不要另起一套色。

## 14. 禁止事项（违反即返工）

1. **不要新增任何键盘钩子**。键盘只由 kanata 接管；`CapsLock+,` 必须走 `push-msg`。
2. **不要用 kanata 的 `gui` 变体**（双托盘），不要改 kanata 二进制。
3. **不要引入 async 运行时（tokio 等）**。std 线程 + channel 足够，体积和复杂度都更小。
4. **不要引入 `tauri` / `electron` / `webview` / `qt` / `gtk` 系依赖**。
5. **不要让 `capsx-core` 依赖 GUI 库**（`eframe`/`egui`/`tray-icon` 只能在 `capsx-app`）。
6. **不要遍历 HashMap 生成 `defsrc`/`deflayer`**（顺序错位 = 整层错乱）。
7. **不要写需要管理员权限的路径**（`Program Files`、`HKLM`）。
8. **不要写死绝对路径**（`D:\...`），一律走 `directories`/`Environment`。
9. **不要在 UI 线程做阻塞 IO**（TCP 等待、`kanata --check` 都要在子线程，结果用 channel 回传）。
10. **不要吞掉 `ReloadResult.ok == false` 或 kanata 的 stderr**。
11. **不要删除用户的 `settings.json` 当作"修复"手段**。
12. **不要提交 `runtime/*.exe`**（体积大，且应可复现）；`.gitignore` 已排除。

## 15. 代码风格

- `cargo fmt` + `clippy -D warnings`；公共 API 加文档注释（`///`）。
- 错误分类型：`thiserror` 定义 `Error` 枚举；应用层 `anyhow::Result`。
- 禁止 `unwrap()`/`expect()`（测试除外）；用 `?` 或显式处理。
- 日志：`info!` 记生命周期事件，`debug!` 记协议帧，错误必须带上下文。
- 常量集中 `capsx-core::constants`（端口超时、退避序列、默认热键、尺寸预算）。
- 提交信息：`feat:` / `fix:` / `refactor:` / `docs:` / `test:`（Conventional Commits），正文写"为什么"。
- **PowerShell 脚本必须保存为 UTF-8 with BOM**（PS 5.1 读无 BOM UTF-8 会乱码甚至解析失败，已实测）。

## 16. 参考资料

- kanata 仓库：<https://github.com/jtroo/kanata>
- 配置语法：<https://github.com/jtroo/kanata/blob/main/docs/config.adoc>（`push-msg`、`args-port`、TCP 协议章节）
- egui：<https://github.com/emilk/egui>（`eframe` 示例、`ViewportBuilder::with_visible`）
- tray-icon：<https://github.com/tauri-apps/tray-icon>
- 本仓库默认键位：`runtime/default.kbd`（已验证）

## 17. 验收清单（M1 Definition of Done）

- [ ] `cargo fmt --check` / `clippy -D warnings` / `cargo test` 全绿
- [ ] `cargo build --release` 产物 ≤ 8 MB
- [ ] 安装包在干净 Windows 11 上安装成功，无需管理员
- [ ] 安装后 `CapsLock + h/j/k/l` 立即生效
- [ ] `CapsLock + ,` 打开设置且不输入逗号
- [ ] 设置应用 1 秒内生效；故意写坏配置能回滚并提示
- [ ] 托盘显示/隐藏双向可用，隐藏后热键仍能唤出设置
- [ ] 开机自启开关生效（重启验证一次）
- [ ] 杀 kanata 进程后自动重启；连续失败会停止并提示
- [ ] 卸载后无残留进程、自启项、快捷方式
- [ ] 重装后设置保留
- [ ] 内存：空闲 ≤ 60 MB，开设置窗 ≤ 90 MB（PR 附 `Get-Process` 实测数据）
- [ ] 手动测试矩阵 14 项全过（附截图/录屏）
