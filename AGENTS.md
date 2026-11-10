# AGENTS.md — CapsX 实现说明（交给编码 Agent）

> 本文件是这个仓库的**唯一技术契约**。动手前先读完，尤其是第 5、6、13 节。
> 目标读者：一个没有历史上下文的编码 Agent。所有"为什么这么做"的决定都已经写在下面，不要自行"优化"成相反的做法。

---

## 1. 一句话定义

CapsX 是一个 Windows 桌面应用：把 [kanata](https://github.com/jtroo/kanata) 这个键盘重映射器**封装成可安装、可设置、可卸载的产品**。

核心卖点只有一句话：**按住 CapsLock + HJKL = 方向键，轻点 CapsLock 仍是大小写锁定**。

## 2. 本期范围（M1 必须交付）

| # | 功能 | 说明 |
|---|---|---|
| 1 | 安装 / 卸载 | 单用户安装，不需要管理员；出现在「设置 → 应用」里 |
| 2 | 开机自启 | 可开关，写 `HKCU\...\Run` |
| 3 | 托盘常驻 | 可隐藏 / 显示托盘图标，状态可读 |
| 4 | 设置窗口 | `CapsLock + ,` 或托盘菜单打开 |
| 5 | 键位生效 | CapsLock 导航层：h j k l / y o / u i / [ ] / g / t |
| 6 | 和弦 | `h`+`l` 选词、`k`+`j` 选行（可关闭） |
| 7 | 设置即时生效 | 保存后 1 秒内生效，失败自动回滚并提示 |
| 8 | 设置持久化 | `%APPDATA%\CapsX\settings.json`，升级不丢 |

**非目标（本期不做）**：按应用切层、多平台、云同步、主题皮肤、自动更新、账号体系。

## 3. 技术栈

| 层 | 选型 | 备注 |
|---|---|---|
| UI | C# / .NET 8 **WinForms** | 单文件自包含发布，`win-x64`，目标 `net8.0-windows` |
| 内核 | kanata **v1.12.0**，`winIOv2` + `tty` 变体 | 不需要驱动、不需要管理员 |
| IPC | kanata 内置 **TCP JSON** 服务 | 见第 6 节，不要自己发明协议 |
| 安装包 | Inno Setup 6 | `PrivilegesRequired=lowest` |
| 单测 | xUnit | 配置生成必须有快照测试 |
| 图标 | `assets/logo/` | 已有成品，不要重画 |

**禁止引入**：AutoHotkey、Electron、Tauri、任何需要额外运行时安装的框架（自包含发布除外）、任何全局键盘钩子库。

## 4. 目录结构（约定）

```
capsx/
├─ assets/logo/                 # 品牌资产（已生成，见第 12 节）
├─ runtime/                     # 构建时拉取，打包进安装包
│   ├─ kanata.exe               # 由 kanata_windows_tty_winIOv2_x64.exe 重命名而来
│   ├─ kanata.version           # 文本：1.12.0
│   └─ default.kbd              # 首次启动拷贝到用户目录的默认配置
├─ src/CapsX/
│   ├─ CapsX.csproj
│   ├─ Program.cs               # 入口：--tray / --uninstall / --show 开关
│   ├─ App/TrayApp.cs           # NotifyIcon、单实例、子进程生命周期
│   ├─ Kanata/
│   │   ├─ KanataProcess.cs     # 启动/停止/监控 kanata.exe
│   │   ├─ KanataClient.cs      # TCP JSON 客户端（异步、断线重连）
│   │   └─ ConfigGenerator.cs   # settings.json → keymap.kbd
│   ├─ Settings/
│   │   ├─ SettingsModel.cs     # 纯数据 + 默认值 + 版本迁移
│   │   ├─ SettingsWindow.cs    # WinForms 设置窗口
│   │   └─ SettingsStore.cs     # 原子读写 settings.json
│   └─ Infra/
│       ├─ Paths.cs             # %APPDATA%\CapsX / %LOCALAPPDATA%\CapsX\logs
│       ├─ Logger.cs            # 滚动日志
│       └─ Autostart.cs         # HKCU Run 读写
├─ installer/capsx.iss
├─ tests/CapsX.Tests/
├─ tools/make_brand_assets.py   # 品牌资产生成脚本（Python + Pillow）
├─ README.md
└─ AGENTS.md
```

## 5. 运行时架构

```
              ┌──────────────────────────────┐
  登录/托盘 → │ CapsX.exe (WinForms, 单实例) │
              │  ├ NotifyIcon                │
              │  ├ SettingsWindow            │
              │  ├ KanataProcess (子进程)     │
              │  └ KanataClient (TCP)        │
              └───────────┬──────────────────┘
                          │ TCP 127.0.0.1:<port>  JSON 行
              ┌───────────▼──────────────────┐
              │ kanata.exe (tty, 隐藏控制台)  │ ← 唯一持有键盘钩子的进程
              └──────────────────────────────┘
```

### 5.1 启动顺序（必须严格）

1. 取得单实例互斥量 `Local\CapsX.SingleInstance`；已有实例 → 通过命名管道或 `WM_COPYDATA`（简单起见：命名事件 + 第二实例自杀并把已有实例唤到前台）→ 退出。
2. 加载 `settings.json`；不存在则从 `runtime\default.kbd` 附近的默认值生成。
3. 选一个空闲端口：`new TcpListener(IPAddress.Loopback, 0)` → 启动 → 取端口 → 停止。
4. 用 `ConfigGenerator` 生成/校验配置（第 7 节）。
5. 启动 `kanata.exe`（第 6 节参数）。
6. 连 TCP，发 `{"Hello":{}}`，等 `HelloOk`（超时 5s；失败按第 6.4 节处理）。
7. 建立托盘图标；若 `settings.tray.visible == false` 则不显示图标。
8. 若以 `--tray` 启动（登录自启），不打开设置窗口。

### 5.2 关闭顺序

1. 标记"正在退出"，阻止自动重启。
2. 断开 TCP。
3. 结束 kanata 子进程（`Kill(entireProcessTree: true)`，等待 ≤2s）。
4. 移除托盘图标，释放互斥量。

**任何退出路径（包括任务管理器结束进程）都必须保证 kanata 被带走**：注册 `ProcessExit`/`AppDomain.UnhandledException`，并在安装器卸载前调用 `CapsX.exe --uninstall`。

## 6. kanata 集成契约

### 6.1 二进制

- 来源：`kanata` release **v1.12.0** 的 `windows-binaries-x64.zip`
  （SHA256 `13947ed78cfa3284bfef854e3c542c74ab366236b72fd9f7e039f8638deead9d`）
- 使用其中的 **`kanata_windows_tty_winIOv2_x64.exe`**，重命名为 `runtime\kanata.exe`。
- **不要**使用 `gui` 变体：它会自己驻留托盘图标，与 CapsX 的托盘冲突。
- **不要**使用 `wintercept` 变体：需要装 Interception 驱动，且有"蓝牙键盘反复断连后失灵"的已知问题。
- **不要**修改二进制、不要自己编译 kanata。

### 6.2 启动参数

```
kanata.exe -c "%APPDATA%\CapsX\keymap.kbd" -p 127.0.0.1:<port> -q
```

- `-q`：只输出错误。
- 不加 `-n`：保留 kanata 默认的 2 秒启动安全延迟（防呆，别优化掉）。
- 进程参数：`UseShellExecute=false`、`CreateNoWindow=true`、`RedirectStandardOutput/Error=true`、`WorkingDirectory=%APPDATA%\CapsX`。
- 把 stderr 每行写入日志（前缀 `[kanata]`）。
- 紧急退出：物理 `LCtrl + Space + Esc` 会让 kanata 立即退出（用于键盘被配置搞死时）。

### 6.3 TCP 协议（换行分隔 JSON）

**客户端 → kanata**

| 消息 | 用途 |
|---|---|
| `{"Hello":{}}` | 握手，返回 `HelloOk`（含 version / protocol） |
| `{"Reload":{"wait":true,"timeout_ms":5000}}` | 重载配置，返回 `ReloadResult` |
| `{"RequestCurrentLayerName":{}}` | 查询当前层 |
| `{"ChangeLayer":{"new":"nav"}}` | 切层（二期按应用切层时用） |
| `{"ActOnFakeKey":{"name":"x","action":"Tap"}}` | 触发 `defvirtualkeys` 里定义的虚键 |

**kanata → 客户端（事件）**

| 消息 | 用途 |
|---|---|
| `{"MessagePush":{"message":"capsx:settings"}}` | 配置里 `(push-msg "capsx:settings")` 触发；CapsX 收到后打开设置窗口 |
| `{"LayerChange":{"new":"nav"}}` | 刷新托盘状态（可选） |
| `{"ConfigFileReload":{"new":"..."}}` | 配置重载成功 |
| `{"Error":{"msg":"..."}}` | 记录日志并提示用户 |
| `{"ReloadResult":{"ok":true}}` | 配置应用结果（关键路径） |
| `{"HoldActivated":{"key":"caps"}}` / `{"TapActivated":{...}}` | 可选：调试用 |

要求：单行 JSON、`\n` 结尾、UTF-8；用 `StreamReader.ReadLineAsync` 循环读取；解析失败只记日志不能崩。

### 6.4 子进程监督

- kanata 退出码非 0 或意外退出 → 自动重启，退避 1s / 2s / 4s / 8s / 15s / 30s（上限 30s）。
- 连续 3 次在 10 秒内退出 → **停止自动重启**，托盘图标变红，弹一次提示并提供「查看日志 / 重试 / 打开配置文件」。绝不允许无限快速重启。
- 用户主动退出（菜单「退出」）时不要重启。
- 启动后 10 秒内若 TCP 连不上，视为启动失败，按上一条处理。

## 7. 配置生成（关键路径）

### 7.1 `settings.json`（完整 schema）

```json
{
  "version": 1,
  "tray": { "visible": true },
  "autostart": true,
  "layer": {
    "tapRepressMs": 200,
    "holdMs": 200,
    "chordMs": 80,
    "chordsEnabled": true
  },
  "mappings": {
    "h": "left",  "j": "down",  "k": "up",    "l": "right",
    "y": "home",  "o": "end",   "u": "pgdn",  "i": "pgup",
    "[": "S-tab", "]": "tab",   "g": "ret",   "t": "del"
  },
  "settingsKey": ","
}
```

- `version` 用于未来迁移；读到更高版本时保留文件、提示"设置来自更新版本"。
- 未知字段必须原样保留（前向兼容）。
- `mappings` 的键是**物理键名**（kanata `defsrc` 名），值是 kanata 动作名。设置窗口只允许从白名单里选。

### 7.2 生成的 `keymap.kbd`（模板，`{{...}}` 为占位）

```
;; 本文件由 CapsX 自动生成，请勿手工编辑。
;; 生成时间：{{timestamp}}  设置版本：{{version}}

(defcfg
  process-unmapped-keys yes
)

(defsrc
  caps  {{keys}}   {{settingsKey}}
)

(deflayer base
  (tap-hold-press {{tapRepressMs}} {{holdMs}} caps (layer-while-held nav))
  {{baseKeys}}   {{settingsKey}}
)

(deflayer nav
  caps  {{navActions}}   {{settingsKeyAction}}
)

;; 仅当 chordsEnabled 且 h/l/k/j 都在 mappings 里时输出下面这段
(defchords navchords {{chordMs}}
  (h) left
  (l) right
  (k) up
  (j) down
  (h l) (macro C-left C-S-right)
  (k j) (macro home S-end)
)

(defalias
  ch-h (chord navchords h)
  ch-l (chord navchords l)
  ch-k (chord navchords k)
  ch-j (chord navchords j)
)
```

> 上例中 `navActions` 在开启和弦时用 `@ch-h @ch-l @ch-k @ch-j`，关闭时用 `left right up down`。
> `settingsKeyAction` 恒为 `(push-msg "capsx:settings")`。
> 注意：`defsrc` / `deflayer` 的列必须一一对应，**顺序错位会导致整层错乱**——生成器必须按键顺序数组拼接，而不是靠字典遍历。

**两个已经踩过的坑（务必遵守）：**

1. **kanata 的注释必须用 `;;`（双分号）。** 单 `;` 会被当成内容，直接报 `Everything must be in a list` 并拒绝启动。生成的 `.kbd` 里所有注释、以及行尾注释都必须是 `;;`。
2. `runtime/default.kbd` 是已经用 `kanata --check` 验证通过的默认配置，可以直接作为生成器的基准模板；改模板后必须重新跑 `--check`。

### 7.3 应用流程（必须原子）

1. 生成候选文本 → 写 `%TEMP%\capsx-<guid>.kbd`。
2. `kanata.exe --check -c <候选文件>`（隐藏窗口，等待退出，超时 5s）；退出码非 0 → 把 stderr 展示给用户，**不落盘**。
3. `File.Replace(候选, keymap.kbd, 备份)`（或 `MoveFileEx` 语义）；保留上一版为 `keymap.kbd.bak`。
4. 发 `{"Reload":{"wait":true,"timeout_ms":5000}}`，等 `ReloadResult`。
5. `ok == false` 或超时 → 用 `.bak` 还原 + 再次 `Reload`，提示失败原因。
6. 成功后刷新 `settings.json`（同样原子写）。

## 8. UI 规格（M1）

### 8.1 托盘菜单（中文）

```
打开设置…
——
状态：运行中 / 已停止 / 启动失败
启用导航层            [勾选]
显示托盘图标          [勾选]   ← 取消后图标消失
开机自启              [勾选]
——
重启 kanata
打开配置文件夹
查看日志
——
退出
```

> 勾选「显示托盘图标」取消后，**必须**仍然可以用 `CapsLock + ,` 和 `Ctrl+Alt+K` 唤出设置。

### 8.2 设置窗口（M1 字段）

| 控件 | 作用 |
|---|---|
| 开关：启用导航层 | 对应 `layer.enabled`（M1 可先只做整体启停） |
| 开关：启用和弦 | `layer.chordsEnabled` |
| 数值：长按判定（150–400ms） | `layer.holdMs` |
| 数值：和弦窗口（50–200ms） | `layer.chordMs` |
| 列表：键位映射 | 只读展示 + 「恢复默认」（M2 再加捕获改键） |
| 复选：显示托盘图标 | `tray.visible` |
| 复选：开机自启 | `autostart` |
| 按钮：应用 / 关闭 / 打开配置文件夹 / 查看日志 | |

要求：

- 设置窗口 96 DPI 与 200% 缩放都要正常（`Application.SetHighDpiMode(HighDpiMode.PerMonitorV2)`）。
- 「应用」必须走第 7.3 节流程；失败时窗口不关闭、显示错误。
- 窗口单例；再次唤出时激活已有窗口而不是开第二个。

### 8.3 唤出路径（三条，缺一不可）

1. `CapsLock + ,`（kanata `push-msg`）
2. 托盘菜单
3. `Ctrl + Alt + K`（`RegisterHotKey`，**不要**用键盘钩子）

## 9. 安装 / 卸载

### 9.1 安装器（Inno Setup）

- `PrivilegesRequired=lowest`，`DefaultDirName={localappdata}\Programs\CapsX`
- 打包 `CapsX.exe`（含运行时自包含）、`runtime\*`
- 桌面/开始菜单快捷方式可选；**默认只建开始菜单**
- 安装结束：可选「立即启动 CapsX」+「开机自启」默认勾选
- 卸载前执行 `{app}\CapsX.exe --uninstall --quiet`（等待退出，超时 10s 后强杀）
- 卸载时询问「是否保留设置」（默认保留 `%APPDATA%\CapsX`）

### 9.2 自启

- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值名 `CapsX`，内容 `"<安装路径>\CapsX.exe" --tray`
- 不要用启动文件夹快捷方式（不便于卸载时清理）
- **提权模式**（M2）：可选创建登录计划任务（`RunLevel=HighestAvailable`），安装时需要管理员；M1 不做

### 9.3 升级

- 安装器覆盖安装；`%APPDATA%\CapsX\settings.json` 必须保留并做 schema 迁移

## 10. 可靠性与日志

- 所有 JSON / 配置文件写入：写 `.tmp` → `File.Replace`，禁止原地覆盖。
- 日志：`%LOCALAPPDATA%\CapsX\logs\capsx-YYYYMMDD.log`，保留 7 天；单文件 >5MB 滚动。
- 未捕获异常：写日志 + 托盘气泡提示，不要静默崩溃。
- 结束时必须释放：托盘图标、互斥量、TCP、子进程。

## 11. 测试要求

### 11.1 自动化（必须在 CI 里跑）

- `ConfigGenerator` 快照测试：默认设置、关闭和弦、自定义映射、极端超时值 → 生成的 `.kbd` 与快照一致。
- 生成的 `.kbd` 逐个跑 `kanata.exe --check`，断言退出码 0。
- `KanataClient` 协议测试：用本地 TCP mock 断言 JSON 序列化/反序列化、超时、断线重连。
- 设置迁移测试：`version 0 → 1`。

### 11.2 手动测试矩阵（写进 PR 描述，逐项打勾）

| # | 场景 | 期望 |
|---|---|---|
| 1 | 记事本里 `CapsLock + h/j/k/l` | 光标移动，且不输入字母 |
| 2 | 轻点 CapsLock | 大小写锁定切换，指示灯变化 |
| 3 | `CapsLock + Ctrl + h` | 按词左移 |
| 4 | `CapsLock + Shift + l` | 向右选中 |
| 5 | `h`+`l` 几乎同时按 | 选中一个词 |
| 6 | `CapsLock + ,` | 弹出设置窗口，且不输入逗号 |
| 7 | 取消「显示托盘图标」→ `CapsLock + ,` | 设置仍能打开 |
| 8 | 改一个映射并应用 | 1 秒内生效 |
| 9 | 故意写错配置（手工改 `keymap.kbd` 后重启） | 回滚到上一版并提示，键盘仍可用 |
| 10 | 任务管理器结束 kanata.exe | 3 秒内自动重启 |
| 11 | 卸载（运行中） | 无残留进程，自启项被删 |
| 12 | 重新安装 | 设置保留 |
| 13 | 中文输入法激活时 | 上面 1–6 仍成立 |

> **重要**：kanata 只处理**物理按键**，对 `SendInput` 等合成输入不做重映射（防冲突设计）。因此上面这些**无法用自动化脚本验证**，必须人工执行。不要为了"自动化"去给 kanata 加补丁。

## 12. 品牌资产（已就绪，直接用）

- `assets/logo/capsx.ico` — 应用图标 / 安装包图标（多尺寸）
- `assets/logo/capsx-icon-{16..1024}.png` — 托盘、关于页
- `assets/logo/capsx-icon.svg` — 矢量源
- `assets/logo/capsx-wordmark.png` — README 头图
- `assets/logo/alt/` — 琥珀色变体（备用，不要混用）

配色（代码里集中为常量，不要散落）：

| 名称 | 值 |
|---|---|
| 强调色 Mint | `#3BE3AE` |
| 深底 | `#1B2231` → `#0B0F16` |
| 键帽 | `#242D3E` → `#1A2130` |
| 键帽描边 | `#33405A` |

重新生成：`python tools/make_brand_assets.py`（需要 Pillow；Windows 上使用系统字体 Segoe UI / 微软雅黑）。

## 13. 禁止事项（违反即返工）

1. **不要新增任何全局键盘钩子**。键盘只由 kanata 一个进程接管；CapsX 抓 `CapsLock+,` 会与 kanata 的 tap-hold 冲突（kanata 会吞掉 CapsLock，钩子链顺序不可靠）。只用 `push-msg` + TCP。
2. **不要用 kanata 的 `gui` 变体**（双托盘）。
3. **不要修改 / 重新编译 kanata**，不要改 `runtime\kanata.exe`。
4. **不要用 AutoHotkey**，哪怕只是一小段。
5. **不要写需要管理员权限的路径**（`Program Files`、`HKLM`）——M1 是纯单用户应用。
6. **不要把绝对路径写死**（`D:\Tools\...` 之类），全部走 `Environment.GetFolderPath`。
7. **不要在 UI 线程同步等待 TCP**（`async/await` + 超时）。
8. **不要静默吞掉 kanata 的 stderr** 或 `ReloadResult.ok == false`。
9. **不要删除 `%APPDATA%\CapsX` 里的用户设置**做"修复"手段。
10. **不要改 `keymap.kbd` 的手工编辑支持之外的生成逻辑**（生成器是唯一写入方）。

## 14. 代码风格

- `nullable enable`、文件级命名空间、`var` 谨慎使用、公共 API 有 XML 注释。
- 异步 IO 一律 `async/await` + `CancellationToken`，超时显式传入。
- 常量（端口超时、退避序列、默认热键）集中在 `Constants.cs`。
- 字符串 UI 全中文；日志中文/英文均可，但同一行不要混。
- **PowerShell 脚本必须保存为 UTF-8 with BOM**：Windows PowerShell 5.1 读无 BOM 的 UTF-8 会乱码，中文串甚至会让脚本解析失败（已实测）。C# 源文件用 UTF-8（无 BOM 即可），`.kbd` 也用 UTF-8。
- 提交信息：`feat: ...` / `fix: ...` / `docs: ...`（Conventional Commits），正文说明"为什么"。

## 15. 参考资料

- kanata 仓库：<https://github.com/jtroo/kanata>
- 配置语法：<https://github.com/jtroo/kanata/blob/main/docs/config.adoc>
- TCP 协议：同文档 "args-port / TCP Protocol" 一节
- `push-msg`：同文档 `push-msg` 一节
- 本仓库的键位设计来源：CapsLockX 的「编辑增强」模块（h/j/k/l 等），已在 `README.md` 的键位表里列全

## 16. 验收清单（M1 Definition of Done）

- [ ] 安装包可在一台干净的 Windows 11 上安装成功，无需管理员
- [ ] 安装后 `CapsLock + h/j/k/l` 立即生效
- [ ] `CapsLock + ,` 打开设置；不输入逗号
- [ ] 设置改完点「应用」1 秒内生效；失败有提示且可回滚
- [ ] 托盘显示/隐藏双向可用，隐藏后热键仍能唤出设置
- [ ] 开机自启开关生效（重启一次验证）
- [ ] 杀进程后自动重启；连续失败会停止并提示
- [ ] 卸载后无残留进程、无自启项、无开始菜单快捷方式
- [ ] 重新安装后设置保留
- [ ] 日志文件包含 kanata 的 stderr
- [ ] 手动测试矩阵 13 项全过（附截图或录屏）
