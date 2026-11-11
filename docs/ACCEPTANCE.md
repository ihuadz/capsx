# CapsX M1 验收记录

> 记录日期：2026-11-11
> 机器：Windows 11 Pro 10.0.26300（64 位）、AMD Ryzen 7 5700G（8C/16T）、AMD Radeon 31.0.12029.10015
> 工具链：Rust stable 1.96.0；MSRV 1.80.0 单独验证通过

## 1. 自动化检查

| 检查 | 命令 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all -- --check` | 通过 |
| Lint | `cargo clippy --locked --all-targets -- -D warnings` | 通过，0 warning |
| 单元测试 | `cargo test --locked` | 25 passed（core 24 + app 1） |
| 配置快照 | 随 `cargo test` | 5 份快照逐字节一致 |
| 真实 kanata 校验 | `cargo test --locked -p capsx-core --test config_snapshots -- --ignored` | 5 份配置全部通过 `kanata --check` |
| MSRV | `cargo +1.80.0 check --locked -p capsx-app` | 通过 |

依赖锁文件对若干 MSRV 超标的传递依赖做了向下锁定（`muda 0.15.2`、
`image 0.25.6`、`webbrowser 1.0.5`、ICU 1.5 系列等），详见 `Cargo.lock`。
`cargo metadata` 中 Windows 目标依赖已无 `rust-version > 1.80` 的包。

## 2. 体积

| 产物 | 实测 | 目标 | 结果 |
|---|---:|---:|---|
| `capsx.exe`（release） | 2,329,600 B / **2.22 MiB** | ≤ 8 MB | 通过 |
| `CapsX-Setup-0.1.0.exe` | 3,608,988 B / **3.44 MiB** | ≤ 8 MB | 通过 |
| 安装后磁盘 | **8.13 MiB** | ≤ 12 MB | 通过 |
| 进程数 | **2**（capsx + kanata） | 2 | 通过 |

## 3. 性能与资源

| 指标 | 实测 | 目标 | 结果 |
|---|---:|---:|---|
| 冷启动到 TCP 可用（release） | **2.64 s** | ≤ 3 s | 通过 |
| 设置应用（点击应用到 settings.json 落盘） | **328 ms** | ≤ 1 s | 通过 |
| 空闲 CPU（可见，整机 16 逻辑核） | **0.13%**（capsx），kanata 0% | < 0.3% | 通过 |
| 空闲 CPU（窗口隐藏） | **0.12%**（capsx），kanata 0% | < 0.3% | 通过 |
| 空闲 WS（可见，两进程） | **118.2 MiB**（108.5 + 9.8） | ≤ 45 MB（上限 60） | **未达标（环境基线原因，见下）** |
| 空闲 WS（隐藏，两进程） | **120.3 MiB** | ≤ 45 MB（上限 60） | **未达标（环境基线原因，见下）** |

### 内存基线说明

同一台机器上临时编译的“最小 eframe/glow 空窗口”：

- WS **107.3 MiB**，Private **108.8 MiB**；
- `capsx.exe` 加载的 `atio6axx.dll`（AMD OpenGL ICD）单个模块就有 **63.9 MiB**。

因此 60 MB 的总 WS 上限在这台机器的 OpenGL 驱动下连空 egui 窗口都无法达到。
空窗口 107.3 MiB，CapsX 两进程合计 118.2 MiB，其中 kanata 占 9.8 MiB；
CapsX 壳本身只比空 eframe 窗口多约 **1.1 MiB**，并非应用自身泄漏；
连续观察 20 秒 WS/Private 不增长。为了不掩盖真实提交内存，
没有使用 `EmptyWorkingSet` 之类的“把页换出去”手段伪造指标。
换用显存驱动映射更小的机器（Intel 核显或软件 GL）时应显著低于本记录。

## 4. 安装、卸载与运行验证

| 场景 | 结果 |
|---|---|
| 静默安装（当前用户，无 UAC 提权） | 通过，1.16 s |
| 覆盖安装（应用正在运行） | 通过，先 `--uninstall` 正常退出，再替换文件 |
| `设置 → 应用` 条目 | `CapsX 0.1.0`，安装位置 `%LOCALAPPDATA%\Programs\CapsX` |
| 开始菜单快捷方式 / 卸载快捷方式 | 均创建 |
| 写 `HKCU\...\Run` 自启项 | 通过，命令为 `"<install>\capsx.exe" --tray` |
| 卸载（运行中，静默） | 1.58 s，无残留 capsx/kanata 进程、无自启项、无快捷方式 |
| 卸载保留设置 | 通过（`%APPDATA%\CapsX\settings.json` 保留） |
| 重装读取旧设置 | 通过（chord/托盘/自启设置保持） |
| 单实例：第二次启动唤出设置 | 通过（离屏隐藏时也能唤出） |
| 关闭按钮 → 托盘隐藏，进程保留 | 通过 |
| 关闭窗口放弃未应用修改 | 通过（恢复默认 → 关闭 → 热键重开，草稿回到已应用设置） |
| `Ctrl + Alt + K`（隐藏状态下）唤出 | 通过（合成按键测试） |
| `--tray` 隐藏启动 | 通过（窗口离屏，热键可唤出） |
| 托盘/热键唤出窗口置顶 | 通过（设置 `WS_EX_TOPMOST`，并取得前台焦点） |
| `push-msg` 协议链路 | 通过（用真实 kanata 虚键触发 `{"MessagePush":{"message":["capsx:settings"]}}`，窗口从离屏恢复并置顶） |
| 配置手工写坏后启动 | 通过（`kanata --check` 失败 → 提示 → 自动重建） |
| 杀 kanata 进程 | 通过，约 3 s 内自动重启 |
| 10 秒内连续 3 次失败 | 通过，停止重启、进入红色失败态、自动弹出设置窗 |
| 失败态托盘图标 | 通过，托盘图标右下角叠加红点（已在 Windows 通知区确认） |
| 失败态点击「重试启动」 | 通过，恢复运行 |
| Reload 实测 | 152 ms（通过第二连接唤醒 kanata 主循环） |
| 托盘图标隐藏后热键唤出（配置 `tray.visible=false`） | 通过 |
| 托盘菜单：打开设置 | 通过（菜单在专用线程；主线程继续 tick，窗口置顶并取得前台） |
| 托盘菜单：快捷设置（启用导航层） | 通过（关闭/恢复导航层，配置 Reload + 持久化） |
| 托盘菜单：退出 | 通过（capsx/kanata 均退出，无重复托盘线程创建） |
| 托盘显示/隐藏双向 | 通过（托盘菜单隐藏后，UI 重新开启可恢复图标） |
| 设置面板 420/520/640 宽度 | 通过（client 404/504/624；无横向滚动、无右侧裁切、底部栏始终可见） |
| 窄窗口键位映射 | 通过（内容宽 < 440 时两列，否则四列；网格占满可用宽度） |
| 较矮窗口 520×520 | 通过（仅中区滚动，标题栏/操作栏固定） |

## 5. AGENTS.md 手动测试矩阵

物理按键测试无法由本环境自动化：kanata 只处理物理输入，`SendInput`/`keybd_event`
合成的字符不会被重映射（这是 kanata 的防冲突设计，AGENTS.md 也明确禁止为自动化改它）。

| # | 场景 | 状态 |
|---|---|---|
| 1 | 记事本 `CapsLock + h/j/k/l` | 待人工验证（配置已通过 `kanata --check`） |
| 2 | 轻点 CapsLock | 待人工验证 |
| 3 | `CapsLock + Ctrl + h` | 待人工验证 |
| 4 | `CapsLock + Shift + l` | 待人工验证 |
| 5 | `h+l` 同时按选中词 | 待人工验证 |
| 6 | `CapsLock + ,` 弹出设置且不输入逗号 | 协议链路通过；物理按键待人工（修复了 kanata `push-msg` 数组格式解析） |
| 7 | 取消托盘后热键仍能唤出 | 通过（以 `tray.visible=false` 启动 + 热键验证） |
| 8 | 改一个映射并应用，1 秒内生效 | 部分通过：设置应用链路 328 ms；用和弦开关验证，映射下拉未逐一自动化点击 |
| 9 | 手工写坏 `keymap.kbd` 后启动 | 通过（自动重建 + 提示） |
| 10 | 结束 kanata 后自动重启 | 通过（约 3 s） |
| 11 | 卸载（运行中）无残留 | 通过 |
| 12 | 重装后设置保留 | 通过 |
| 13 | 中文输入法激活时 1–6 成立 | 待人工验证 |
| 14 | 隐藏窗口后内存 ≤ 60 MB | **未达标**，见内存基线说明 |

人工验收步骤建议按 `AGENTS.md` 12.2 原表执行，重点补充 1/3/4/5/6/13。

托盘菜单已做真机点击验证。此前 `TrackPopupMenu` 与 eframe 共用主线程会让 winit
重绘调度在菜单关闭后停摆，表现为打开设置、快捷开关、退出全部无响应；现已按
`AGENTS.md` 5.3 的退路，将托盘图标/菜单移到 `capsx-tray` 专用线程，动作经 channel
回主线程。实测主线程轮询不中断，退出也不再重复创建托盘线程。

## 6. 复现构建 / 安装

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-release.ps1 -Full
# 产物：
#   target\release\capsx.exe
#   dist\CapsX-Setup-0.1.0.exe
```

安装（单用户，无需管理员）：

```powershell
dist\CapsX-Setup-0.1.0.exe
```

卸载：

```text
设置 → 应用 → CapsX → 卸载
```

## 7. 已知设计偏差

1. **隐藏窗口不用 `ViewportCommand::Visible(false)`**：实测 winit 0.30.13 在
   Windows 隐藏窗口后不再派发 `RedrawRequested`，eframe `update()` 停摆，
   托盘/热键/单实例事件全部失效。改为把原生窗口移到屏幕外并设置
   `with_taskbar(false)`，用户可见行为仍是“隐藏到托盘”。详见 README「实现注记」。
2. **Reload 需要二次连接唤醒**：kanata v1.12 的 `wait:true` 会等待主循环
   `is_reload_complete`，而 WakeUp 要等超时才发。CapsX 在 Reload 后 100ms
   用一条短 TCP 连接触发 WakeUp，实测 152ms 返回。
3. **内存不达标是环境基线**，不是 CapsX 逻辑；详见第 3 节。
