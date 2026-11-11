//! 托盘图标、托盘菜单与全局热键。
//!
//! 托盘菜单由独立线程和独立 Win32 消息泵运行。原因是 `TrackPopupMenu` 是
//! 模态循环：放在 eframe 主线程时，菜单关闭后 winit 的重绘调度可能停摆，
//! 表现为托盘所有菜单项都无响应。独立线程只负责托盘 UI，动作通过 channel
//! 回到主线程，主线程的 250ms 轮询保持完全独立。

use anyhow::{anyhow, Context, Result};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
    PM_NOREMOVE, WM_APP, WM_USER,
};

use crate::assets;

const MENU_OPEN: &str = "capsx.open";
const MENU_STATUS: &str = "capsx.status";
const MENU_LAYER: &str = "capsx.layer";
const MENU_TRAY: &str = "capsx.tray";
const MENU_AUTOSTART: &str = "capsx.autostart";
const MENU_RESTART: &str = "capsx.restart";
const MENU_CONFIG: &str = "capsx.config";
const MENU_LOGS: &str = "capsx.logs";
const MENU_QUIT: &str = "capsx.quit";

/// 托盘菜单需要的状态快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayState {
    /// 状态文案，例如“运行中”。
    pub status: String,
    /// 当前设置里的导航层开关。
    pub layer_enabled: bool,
    /// 当前设置里的托盘开关。
    pub tray_visible: bool,
    /// 当前设置里的自启开关。
    pub autostart: bool,
    /// 设置是否可修改（未来版本/解析失败时禁用）。
    pub editable: bool,
    /// 是否有可重启的服务。
    pub restart_enabled: bool,
    /// 是否处于失败态（托盘图标叠加红点）。
    pub alert: bool,
}

/// 托盘/热键触发的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// 打开设置。
    OpenSettings,
    /// 切换导航层。
    ToggleLayer,
    /// 切换托盘图标。
    ToggleTray,
    /// 切换开机自启。
    ToggleAutostart,
    /// 重启 kanata。
    RestartKanata,
    /// 打开配置目录。
    OpenConfig,
    /// 打开日志目录。
    OpenLogs,
    /// 退出 CapsX。
    Quit,
}

struct MenuHandles {
    status: MenuItem,
    layer: CheckMenuItem,
    tray: CheckMenuItem,
    autostart: CheckMenuItem,
    restart: MenuItem,
}

enum TrayCommand {
    Sync(TrayState),
    Quit,
}

enum TrayNotice {
    Action(TrayAction),
}

/// 托盘图标控制器。真正的 `TrayIcon` 和菜单都在专用线程里。
pub struct TrayController {
    commands: Sender<TrayCommand>,
    notices: Receiver<TrayNotice>,
    thread_id: u32,
    last_state: Option<TrayState>,
    _thread: thread::JoinHandle<()>,
}

impl TrayController {
    /// 创建控制器并在托盘线程里初始化图标。只有 `state.tray_visible` 为真
    /// 时才真正显示图标。
    pub fn new(state: &TrayState) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (notice_tx, notice_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let initial = state.clone();

        let thread = thread::Builder::new()
            .name("capsx-tray".into())
            .spawn(move || run_tray_thread(initial, command_rx, notice_tx, ready_tx))
            .context("创建托盘线程失败")?;

        let (thread_id, ready) = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .context("等待托盘线程初始化超时")?;
        ready.map_err(|message| anyhow!(message))?;

        Ok(Self {
            commands: command_tx,
            notices: notice_rx,
            thread_id,
            last_state: Some(state.clone()),
            _thread: thread,
        })
    }

    /// 根据状态创建/隐藏托盘图标并刷新菜单。该函数只发命令，不阻塞 UI。
    pub fn sync(&mut self, state: &TrayState) -> Result<()> {
        if self.last_state.as_ref() == Some(state) {
            return Ok(());
        }
        self.commands
            .send(TrayCommand::Sync(state.clone()))
            .context("托盘线程已结束")?;
        wake_tray_thread(self.thread_id);
        self.last_state = Some(state.clone());
        Ok(())
    }

    /// 取出本帧发生的托盘交互。
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(TrayNotice::Action(action)) = self.notices.try_recv() {
            actions.push(action);
        }
        if !actions.is_empty() {
            log::info!("托盘动作：{actions:?}");
        }
        actions
    }
}

impl Drop for TrayController {
    fn drop(&mut self) {
        let _ = self.commands.send(TrayCommand::Quit);
        wake_tray_thread(self.thread_id);
    }
}

fn action_for_id(id: &str) -> Option<TrayAction> {
    match id {
        MENU_OPEN => Some(TrayAction::OpenSettings),
        MENU_LAYER => Some(TrayAction::ToggleLayer),
        MENU_TRAY => Some(TrayAction::ToggleTray),
        MENU_AUTOSTART => Some(TrayAction::ToggleAutostart),
        MENU_RESTART => Some(TrayAction::RestartKanata),
        MENU_CONFIG => Some(TrayAction::OpenConfig),
        MENU_LOGS => Some(TrayAction::OpenLogs),
        MENU_QUIT => Some(TrayAction::Quit),
        MENU_STATUS => None,
        other => {
            log::debug!("未处理的托盘菜单：{other}");
            None
        }
    }
}

fn build_icon(state: &TrayState) -> Result<(TrayIcon, MenuHandles)> {
    let open = MenuItem::with_id(MENU_OPEN, "打开设置…", true, None);
    let status = MenuItem::with_id(MENU_STATUS, format!("状态：{}", state.status), false, None);
    let layer = CheckMenuItem::with_id(
        MENU_LAYER,
        "启用导航层",
        state.editable,
        state.layer_enabled,
        None,
    );
    let tray_item = CheckMenuItem::with_id(
        MENU_TRAY,
        "显示托盘图标",
        state.editable,
        state.tray_visible,
        None,
    );
    let autostart = CheckMenuItem::with_id(
        MENU_AUTOSTART,
        "开机自启",
        state.editable,
        state.autostart,
        None,
    );
    let restart = MenuItem::with_id(MENU_RESTART, "重启 kanata", state.restart_enabled, None);
    let config = MenuItem::with_id(MENU_CONFIG, "打开配置文件夹", true, None);
    let logs = MenuItem::with_id(MENU_LOGS, "查看日志", true, None);
    let quit = MenuItem::with_id(MENU_QUIT, "退出", true, None);

    let menu = Menu::new();
    menu.append_items(&[
        &open,
        &PredefinedMenuItem::separator(),
        &status,
        &layer,
        &tray_item,
        &autostart,
        &PredefinedMenuItem::separator(),
        &restart,
        &config,
        &logs,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .context("构建托盘菜单失败")?;

    let icon = Icon::from_rgba(
        assets::tray_icon_rgba(state.alert),
        assets::TRAY_ICON_SIZE,
        assets::TRAY_ICON_SIZE,
    )
    .map_err(|error| anyhow!("托盘图标 RGBA 无效：{error}"))?;

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip(state))
        .with_icon(icon)
        .with_menu_on_left_click(true)
        .build()
        .context("创建托盘图标失败（可能与系统托盘状态有关）")?;

    Ok((
        tray_icon,
        MenuHandles {
            status,
            layer,
            tray: tray_item,
            autostart,
            restart,
        },
    ))
}

fn apply_state(
    icon: Option<&TrayIcon>,
    handles: Option<&MenuHandles>,
    alert: &mut bool,
    state: &TrayState,
) {
    let Some(handles) = handles else {
        return;
    };
    handles.status.set_text(format!("状态：{}", state.status));
    handles.layer.set_checked(state.layer_enabled);
    handles.layer.set_enabled(state.editable);
    handles.tray.set_checked(state.tray_visible);
    handles.tray.set_enabled(state.editable);
    handles.autostart.set_checked(state.autostart);
    handles.autostart.set_enabled(state.editable);
    handles.restart.set_enabled(state.restart_enabled);

    let Some(icon) = icon else {
        return;
    };
    if let Err(error) = icon.set_tooltip(Some(tooltip(state))) {
        log::debug!("更新托盘提示失败：{error}");
    }
    if state.alert != *alert {
        match Icon::from_rgba(
            assets::tray_icon_rgba(state.alert),
            assets::TRAY_ICON_SIZE,
            assets::TRAY_ICON_SIZE,
        ) {
            Ok(new_icon) => {
                if let Err(error) = icon.set_icon(Some(new_icon)) {
                    log::warn!("更新托盘失败态图标失败：{error}");
                } else {
                    *alert = state.alert;
                }
            }
            Err(error) => log::warn!("生成托盘失败态图标失败：{error}"),
        }
    }
}

fn tooltip(state: &TrayState) -> String {
    format!("CapsX · {}", state.status)
}

#[cfg(windows)]
type ReadySender = Sender<(u32, Result<(), String>)>;

#[cfg(windows)]
fn run_tray_thread(
    initial: TrayState,
    commands: Receiver<TrayCommand>,
    notices: Sender<TrayNotice>,
    ready: ReadySender,
) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut icon: Option<TrayIcon> = None;
    let mut handles: Option<MenuHandles> = None;
    let mut alert = initial.alert;

    if initial.tray_visible {
        match build_icon(&initial) {
            Ok((new_icon, new_handles)) => {
                icon = Some(new_icon);
                handles = Some(new_handles);
            }
            Err(error) => {
                let _ = ready.send((thread_id, Err(error.to_string())));
                return;
            }
        }
    }
    // 即使初始不显示托盘，也先创建本线程的消息队列。否则主线程紧接着
    // PostThreadMessage 唤醒时会因为没有队列而失败，后续 Sync 永远卡住。
    let mut wake_probe: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        PeekMessageW(
            &mut wake_probe,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        );
    }

    let _ = ready.send((thread_id, Ok(())));
    log::info!("托盘线程已启动");

    loop {
        let mut message: MSG = unsafe { std::mem::zeroed() };
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        while let Ok(command) = commands.try_recv() {
            match command {
                TrayCommand::Sync(state) => {
                    if !state.tray_visible {
                        icon = None;
                        handles = None;
                    } else if icon.is_none() {
                        match build_icon(&state) {
                            Ok((new_icon, new_handles)) => {
                                icon = Some(new_icon);
                                handles = Some(new_handles);
                                alert = state.alert;
                            }
                            Err(error) => log::error!("重新创建托盘图标失败：{error}"),
                        }
                    } else {
                        apply_state(icon.as_ref(), handles.as_ref(), &mut alert, &state);
                    }
                }
                TrayCommand::Quit => return,
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_for_id(event.id.as_ref()) {
                let _ = notices.send(TrayNotice::Action(action));
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                let _ = notices.send(TrayNotice::Action(TrayAction::OpenSettings));
            }
        }
    }
}

#[cfg(not(windows))]
type ReadySender = Sender<(u32, Result<(), String>)>;

#[cfg(not(windows))]
fn run_tray_thread(
    _initial: TrayState,
    _commands: Receiver<TrayCommand>,
    _notices: Sender<TrayNotice>,
    ready: ReadySender,
) {
    let _ = ready.send((0, Err("托盘线程仅支持 Windows".into())));
}

#[cfg(windows)]
fn wake_tray_thread(thread_id: u32) {
    unsafe {
        PostThreadMessageW(thread_id, WM_APP + 1, 0, 0);
    }
}

#[cfg(not(windows))]
fn wake_tray_thread(_thread_id: u32) {}

/// 全局热键（Ctrl+Alt+K）。
pub struct HotkeyController {
    _manager: GlobalHotKeyManager,
    id: u32,
}

impl HotkeyController {
    /// 注册热键；失败时调用方可降级为托盘/`CapsLock+,`。
    pub fn new() -> Result<Self> {
        let manager = GlobalHotKeyManager::new().context("初始化全局热键失败")?;
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyK);
        let id = hotkey.id;
        manager
            .register(hotkey)
            .context("注册 Ctrl+Alt+K 失败（可能被其它程序占用）")?;
        Ok(Self {
            _manager: manager,
            id,
        })
    }

    /// 本帧是否按下了热键。
    pub fn pressed(&self) -> bool {
        let mut pressed = false;
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id() == self.id && event.state() == HotKeyState::Pressed {
                pressed = true;
            }
        }
        pressed
    }
}
