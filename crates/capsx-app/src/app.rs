//! CapsX 应用主状态与事件循环。

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use capsx_core::client::{ClientEvent, KanataClient};
use capsx_core::config_gen::{
    self, default_check_timeout, generate, install_checked, restore_backup,
};
use capsx_core::constants::{APP_VERSION, RELOAD_UI_GRACE_MS, TCP_RELOAD_TIMEOUT_MS};
use capsx_core::kanata::{KanataSupervisor, SupervisorConfig, SupervisorEvent};
use capsx_core::paths::AppPaths;
use capsx_core::platform;
use capsx_core::settings::Settings;
use eframe::egui::{self, Pos2, TextureHandle, ViewportCommand, WindowLevel};
use single_instance::SingleInstance;

use crate::assets;
use crate::event::{AppEvent, ApplyEvent};
use crate::status::{Banner, ServiceState, Toast};
use crate::tray::{HotkeyController, TrayAction, TrayController, TrayState};
use crate::ui;

/// 应用主状态。
pub struct CapsxApp {
    paths: AppPaths,
    settings: Settings,
    draft: Settings,
    settings_editable: bool,
    start_hidden: bool,
    window_visible: bool,
    saved_window_pos: Option<Pos2>,
    quitting: bool,

    service: ServiceState,
    connected: bool,
    port: u16,
    supervisor: Option<KanataSupervisor>,
    supervisor_events: Option<Receiver<SupervisorEvent>>,
    client: Option<KanataClient>,
    kanata_version: Option<String>,
    pending_window_activation: bool,

    event_tx: Sender<AppEvent>,
    event_rx: Receiver<AppEvent>,

    integrations_ready: bool,
    tray: Option<TrayController>,
    tray_failed: bool,
    hotkey: Option<HotkeyController>,

    banner: Option<Banner>,
    toast: Option<Toast>,
    about_open: bool,
    logo: Option<TextureHandle>,

    applying: bool,
    rolling_back: bool,
    pending_apply: Option<PendingApply>,
    rollback_reason: String,

    _instance: SingleInstance,
}

struct PendingApply {
    settings: Settings,
    backup: Option<PathBuf>,
    deadline: Instant,
}

impl CapsxApp {
    /// 构造应用并完成启动检查（配置生成、进程和 TCP 线程）。
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: AppPaths,
        start_hidden: bool,
        instance: SingleInstance,
        event_tx: Sender<AppEvent>,
        event_rx: Receiver<AppEvent>,
    ) -> Self {
        ui::theme::install(&cc.egui_ctx);
        let logo = Some(assets::load_logo(&cc.egui_ctx));

        let (settings, settings_editable, banner) = load_settings(&paths);
        let mut app = Self {
            paths,
            settings: settings.clone(),
            draft: settings,
            settings_editable,
            start_hidden,
            window_visible: !start_hidden,
            saved_window_pos: None,
            quitting: false,
            service: ServiceState::Starting,
            connected: false,
            port: 0,
            supervisor: None,
            supervisor_events: None,
            client: None,
            kanata_version: None,
            pending_window_activation: false,
            event_tx,
            event_rx,
            integrations_ready: false,
            tray: None,
            tray_failed: false,
            hotkey: None,
            banner,
            toast: None,
            about_open: false,
            logo,
            applying: false,
            rolling_back: false,
            pending_apply: None,
            rollback_reason: String::new(),
            _instance: instance,
        };
        app.draft = app.settings.clone();
        app.bootstrap();
        // 即使以 --tray 启动（viewport 初始不可见），也保证第一帧会执行，
        // 用于在主线程创建托盘和全局热键。
        cc.egui_ctx.request_repaint();
        app
    }

    fn bootstrap(&mut self) {
        if self.settings_editable {
            self.sync_autostart_at_startup();
        }

        match self.prepare_initial_config() {
            Ok(()) => {
                if let Err(error) = self.start_runtime() {
                    self.fail_service(format!("启动 kanata 失败：{error:#}"));
                }
            }
            Err(error) => {
                self.fail_service(format!("初始配置不可用：{error:#}"));
            }
        }
    }

    fn sync_autostart_at_startup(&mut self) {
        let manager = platform::autostart();
        match manager.is_enabled() {
            Ok(current) if current == self.settings.autostart => {}
            Ok(_) => {
                if let Err(error) = manager.set_enabled(self.settings.autostart) {
                    log::warn!("同步开机自启失败：{error}");
                    self.set_banner_if_none(Banner::warning("同步开机自启失败", error.to_string()));
                }
            }
            Err(error) => {
                log::warn!("读取开机自启状态失败：{error}");
                self.set_banner_if_none(Banner::warning("读取开机自启状态失败", error.to_string()));
            }
        }
    }

    fn prepare_initial_config(&mut self) -> Result<()> {
        if !self.paths.kanata_file.is_file() {
            return Err(anyhow!(
                "找不到 kanata：{}\n请先运行 tools/fetch-kanata.ps1，或重新安装 CapsX。",
                self.paths.kanata_file.display()
            ));
        }

        self.port = free_port().context("选择本地 TCP 端口失败")?;

        // 如果旧文件被手工改坏，先给出提示；无论旧文件是否有效，
        // 都按 AGENTS.md 5.1 从设置重新生成（可手工编辑的文件会在下次启动被覆盖）。
        if self.paths.keymap_file.is_file() {
            if let Err(error) = config_gen::check_config(
                &self.paths.kanata_file,
                &self.paths.keymap_file,
                default_check_timeout(),
            ) {
                log::warn!("现有 keymap.kbd 未通过校验，将按设置重建：{error}");
                self.set_banner_if_none(Banner::warning(
                    "检测到 keymap.kbd 无效，已按当前设置重建",
                    error.to_string(),
                ));
            }
        }

        let first_error = match generate(&self.settings).and_then(|text| {
            install_checked(
                &text,
                &self.paths.keymap_file,
                &self.paths.kanata_file,
                default_check_timeout(),
            )
        }) {
            Ok(_) => return Ok(()),
            Err(error) => error,
        };
        log::error!("生成当前设置对应的配置失败，尝试默认配置：{first_error}");

        let fallback = Settings::default();
        let fallback_result = generate(&fallback).and_then(|text| {
            install_checked(
                &text,
                &self.paths.keymap_file,
                &self.paths.kanata_file,
                default_check_timeout(),
            )
        });
        match fallback_result {
            Ok(_) => {
                self.set_banner_if_none(Banner::warning(
                    "当前设置无法生成有效配置，已回退默认键位",
                    first_error.to_string(),
                ));
                Ok(())
            }
            Err(second_error) => Err(anyhow!(
                "当前设置失败：{first_error}\n默认配置也失败：{second_error}"
            )),
        }
    }

    fn start_runtime(&mut self) -> Result<()> {
        self.stop_processes();
        if self.port == 0 {
            self.port = free_port().context("选择本地 TCP 端口失败")?;
        }
        let address = SocketAddr::from(([127, 0, 0, 1], self.port));

        let mut client = KanataClient::start(address).context("启动 TCP 客户端失败")?;
        let connected_flag = client.connected_flag();
        let (supervisor_tx, supervisor_rx) = mpsc::channel();
        let supervisor_config = SupervisorConfig {
            kanata: self.paths.kanata_file.clone(),
            config: self.paths.keymap_file.clone(),
            port: self.port,
        };
        match KanataSupervisor::start(supervisor_config, connected_flag, supervisor_tx) {
            Ok(supervisor) => {
                self.client = Some(client);
                self.supervisor = Some(supervisor);
                self.supervisor_events = Some(supervisor_rx);
                self.connected = false;
                self.service = ServiceState::Starting;
                log::info!("运行时已启动：127.0.0.1:{}", self.port);
                Ok(())
            }
            Err(error) => {
                client.shutdown();
                Err(error.into())
            }
        }
    }

    fn stop_processes(&mut self) {
        // 先断开 TCP，再结束子进程（AGENTS.md 5.2）。
        if let Some(mut client) = self.client.take() {
            client.shutdown();
        }
        if let Some(mut supervisor) = self.supervisor.take() {
            supervisor.stop();
        }
        self.supervisor_events = None;
        self.connected = false;
    }

    fn fail_service(&mut self, reason: String) {
        log::error!("服务失败：{reason}");
        self.stop_processes();
        self.service = ServiceState::Failed {
            reason: reason.clone(),
        };
        self.banner = Some(Banner::error("CapsX 无法启动键盘导航层", reason));
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if !self.integrations_ready {
            self.init_integrations(ctx);
        }

        self.drain_app_events(ctx);
        if self.quitting {
            return;
        }

        if ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }

        self.poll_supervisor();
        self.poll_client();
        if self.pending_window_activation {
            self.pending_window_activation = false;
            self.activate_window(ctx);
        }
        self.poll_tray(ctx);
        if self.quitting {
            return;
        }
        self.check_apply_timeout();
        self.refresh_tray();
        self.expire_toast();

        if self.quitting {
            return;
        }

        let repaint_interval = if self.applying {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        };

        if self.window_visible {
            if let Some(position) = visible_window_position(ctx) {
                self.saved_window_pos = Some(position);
            }
            let toast = self
                .toast
                .as_ref()
                .filter(|toast| toast.alive())
                .map(|t| t.text.as_str());
            let dirty = self.draft != self.settings;
            let action = ui::settings::show(
                ctx,
                ui::settings::SettingsView {
                    draft: &mut self.draft,
                    state: &self.service,
                    busy: self.applying,
                    read_only: !self.settings_editable,
                    dirty,
                    banner: self.banner.as_ref(),
                    toast,
                    logo: self.logo.as_ref(),
                },
            );
            self.handle_ui_action(action, ctx);

            if self.about_open {
                ui::about::show(
                    ctx,
                    &mut self.about_open,
                    ui::about::AboutInfo {
                        app_version: APP_VERSION,
                        kanata_version: self.kanata_version.as_deref(),
                        config_dir: &self.paths.data_dir,
                        logs_dir: &self.paths.logs_dir,
                    },
                );
            }
        }

        ctx.request_repaint_after(repaint_interval);
    }

    fn init_integrations(&mut self, ctx: &egui::Context) {
        self.integrations_ready = true;

        match HotkeyController::new() {
            Ok(hotkey) => self.hotkey = Some(hotkey),
            Err(error) => {
                log::warn!("全局热键不可用：{error:#}");
                self.set_banner_if_none(Banner::warning(
                    "Ctrl + Alt + K 热键不可用",
                    format!("{error:#}\n仍可用 CapsLock + , 或托盘菜单打开设置。"),
                ));
            }
        }

        match TrayController::new(&self.tray_state()) {
            Ok(tray) => self.tray = Some(tray),
            Err(error) => {
                self.tray_failed = true;
                log::warn!("托盘不可用：{error:#}");
                self.set_banner_if_none(Banner::warning(
                    "托盘图标创建失败",
                    format!("{error:#}\n你仍可用热键或 CapsLock + , 打开设置。"),
                ));
            }
        }

        if self.start_hidden && self.settings.tray.visible && self.tray.is_none() {
            // 隐藏启动且托盘失败时，不要把用户锁在看不见的窗口里。
            self.activate_window(ctx);
        }
    }

    fn drain_app_events(&mut self, ctx: &egui::Context) {
        loop {
            match self.event_rx.try_recv() {
                Ok(AppEvent::Activate) => self.activate_window(ctx),
                Ok(AppEvent::QuitRequested) => {
                    self.begin_quit(ctx);
                    return;
                }
                Ok(AppEvent::Apply(event)) => {
                    self.handle_apply_event(event);
                    if self.quitting {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn handle_apply_event(&mut self, event: ApplyEvent) {
        match event {
            ApplyEvent::Prepared { settings, backup } => {
                if !self.applying {
                    log::warn!("收到过期的配置准备事件，开始回滚");
                    self.rollback_with_backup(backup, "收到过期配置事件".into());
                    return;
                }
                self.pending_apply = Some(PendingApply {
                    settings,
                    backup,
                    deadline: Instant::now()
                        + Duration::from_millis(TCP_RELOAD_TIMEOUT_MS + RELOAD_UI_GRACE_MS),
                });

                let Some(client) = self.client.as_ref() else {
                    self.begin_rollback("kanata 未运行，无法发送 Reload".into());
                    return;
                };
                if let Err(error) = client.reload(true, TCP_RELOAD_TIMEOUT_MS) {
                    self.begin_rollback(format!("发送 Reload 失败：{error}"));
                }
            }
            ApplyEvent::Failed { title, details } => {
                self.applying = false;
                self.pending_apply = None;
                self.banner = Some(Banner::error(title, details));
            }
            ApplyEvent::RolledBack { error } => {
                self.rolling_back = false;
                self.applying = false;
                let mut details = std::mem::take(&mut self.rollback_reason);
                if let Some(error) = error {
                    details.push_str("\n\n回滚失败：");
                    details.push_str(&error);
                } else if let Some(client) = self.client.as_ref() {
                    if let Err(error) = client.reload(true, TCP_RELOAD_TIMEOUT_MS) {
                        details.push_str(&format!("\n\n回滚后 Reload 发送失败：{error}"));
                    }
                }
                self.banner = Some(Banner::error("设置未生效，已回滚到上一版配置", details));
            }
        }
    }

    fn begin_apply(&mut self) {
        if self.applying || !self.settings_editable {
            return;
        }
        if self.client.is_none() || self.supervisor.is_none() {
            self.banner = Some(Banner::error(
                "kanata 当前未运行",
                "请先点击「重试启动」恢复服务，再应用设置。",
            ));
            return;
        }

        let settings = self.draft.clone();
        self.applying = true;
        self.rolling_back = false;
        self.pending_apply = None;
        self.rollback_reason.clear();
        self.banner = None;

        let paths = self.paths.clone();
        let tx = self.event_tx.clone();
        let spawned = thread::Builder::new()
            .name("capsx-apply".into())
            .spawn(move || {
                let kanata = paths.kanata_file.clone();
                let destination = paths.keymap_file.clone();
                let outcome = generate(&settings).and_then(|text| {
                    install_checked(&text, &destination, &kanata, default_check_timeout())
                });
                match outcome {
                    Ok(installed) => {
                        let _ = tx.send(AppEvent::Apply(ApplyEvent::Prepared {
                            settings,
                            backup: installed.backup,
                        }));
                    }
                    Err(error) => {
                        let _ = tx.send(AppEvent::Apply(ApplyEvent::Failed {
                            title: "配置未通过校验，未替换现有文件".into(),
                            details: truncate_detail(&error.to_string()),
                        }));
                    }
                }
            });

        if let Err(error) = spawned {
            self.applying = false;
            self.banner = Some(Banner::error("无法启动配置线程", format!("{error}")));
        }
    }

    fn begin_rollback(&mut self, reason: String) {
        let Some(pending) = self.pending_apply.take() else {
            self.applying = false;
            self.banner = Some(Banner::error("应用配置失败", reason));
            return;
        };
        self.rollback_with_backup(pending.backup, reason);
    }

    fn rollback_with_backup(&mut self, backup: Option<PathBuf>, reason: String) {
        self.rollback_reason = reason.clone();
        self.rolling_back = true;
        self.applying = true;
        self.banner = Some(Banner::error("新配置未生效，正在回滚", reason.clone()));

        let destination = self.paths.keymap_file.clone();
        let tx = self.event_tx.clone();
        let spawned = thread::Builder::new()
            .name("capsx-rollback".into())
            .spawn(move || {
                let error = match backup {
                    Some(backup) => restore_backup(&backup, &destination)
                        .err()
                        .map(|error| error.to_string()),
                    None => Some("没有可用的上一版配置备份".into()),
                };
                let _ = tx.send(AppEvent::Apply(ApplyEvent::RolledBack { error }));
            });
        if let Err(error) = spawned {
            self.rolling_back = false;
            self.applying = false;
            self.banner = Some(Banner::error(
                "无法启动回滚线程",
                format!("{error}\n{reason}"),
            ));
        }
    }

    fn finish_apply(&mut self) {
        let Some(pending) = self.pending_apply.take() else {
            return;
        };
        if let Err(error) = pending.settings.save_atomic(&self.paths.settings_file) {
            log::error!("保存设置失败：{error}");
            self.pending_apply = Some(pending);
            self.begin_rollback(format!("Reload 成功，但保存设置失败：{error}"));
            return;
        }

        self.settings = pending.settings;
        self.draft = self.settings.clone();
        self.applying = false;
        self.rolling_back = false;
        self.banner = None;
        self.toast = Some(Toast::new("设置已应用，约 1 秒内生效"));
        log::info!("设置已应用并持久化");
        self.apply_runtime_preferences();
    }

    fn apply_runtime_preferences(&mut self) {
        if self.settings_editable {
            if let Err(error) = platform::autostart().set_enabled(self.settings.autostart) {
                log::warn!("写入开机自启失败：{error}");
                self.set_banner_if_none(Banner::warning(
                    "设置已生效，但开机自启写入失败",
                    error.to_string(),
                ));
            }
        }
        // 允许托盘在从隐藏切换回显示时重新尝试创建。
        self.tray_failed = false;
        self.refresh_tray();
    }

    fn check_apply_timeout(&mut self) {
        let timed_out = self
            .pending_apply
            .as_ref()
            .map(|pending| Instant::now() >= pending.deadline)
            .unwrap_or(false);
        if timed_out {
            self.begin_rollback("等待 kanata 的 ReloadResult 超时".into());
        }
    }

    fn handle_ui_action(
        &mut self,
        action: Option<ui::settings::SettingsAction>,
        ctx: &egui::Context,
    ) {
        let Some(action) = action else {
            return;
        };
        match action {
            ui::settings::SettingsAction::Apply => self.begin_apply(),
            ui::settings::SettingsAction::Close => self.hide_window(ctx),
            ui::settings::SettingsAction::RestoreDefaults => {
                if !self.applying && self.settings_editable {
                    let extra = std::mem::take(&mut self.draft.extra);
                    self.draft = Settings::default();
                    self.draft.extra = extra;
                }
            }
            ui::settings::SettingsAction::OpenConfig => {
                if let Err(error) = open_path(&self.paths.data_dir) {
                    self.banner = Some(Banner::error("打开配置文件夹失败", error));
                }
            }
            ui::settings::SettingsAction::OpenLogs => {
                if let Err(error) = open_path(&self.paths.logs_dir) {
                    self.banner = Some(Banner::error("打开日志文件夹失败", error));
                }
            }
            ui::settings::SettingsAction::About => self.about_open = true,
            ui::settings::SettingsAction::RetryService => self.retry_service(),
        }
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        if let Some(hotkey) = &self.hotkey {
            if hotkey.pressed() {
                self.activate_window(ctx);
            }
        }

        let actions = self
            .tray
            .as_ref()
            .map(TrayController::poll)
            .unwrap_or_default();
        for action in actions {
            match action {
                TrayAction::OpenSettings => self.activate_window(ctx),
                TrayAction::ToggleLayer => {
                    if !self.applying && self.settings_editable {
                        self.draft.layer.enabled = !self.draft.layer.enabled;
                        self.begin_apply();
                    }
                }
                TrayAction::ToggleTray => self.toggle_tray_visible(),
                TrayAction::ToggleAutostart => self.toggle_autostart(),
                TrayAction::RestartKanata => self.retry_service(),
                TrayAction::OpenConfig => {
                    if let Err(error) = open_path(&self.paths.data_dir) {
                        self.banner = Some(Banner::error("打开配置文件夹失败", error));
                    }
                }
                TrayAction::OpenLogs => {
                    if let Err(error) = open_path(&self.paths.logs_dir) {
                        self.banner = Some(Banner::error("打开日志文件夹失败", error));
                    }
                }
                TrayAction::Quit => self.begin_quit(ctx),
            }
        }
    }

    fn toggle_tray_visible(&mut self) {
        if self.applying || !self.settings_editable {
            return;
        }
        let mut next = self.settings.clone();
        next.tray.visible = !next.tray.visible;
        if let Err(error) = next.save_atomic(&self.paths.settings_file) {
            self.banner = Some(Banner::error("保存托盘设置失败", error.to_string()));
            return;
        }
        self.settings = next;
        self.draft.tray.visible = self.settings.tray.visible;
        self.tray_failed = false;
        self.toast = Some(Toast::new(if self.settings.tray.visible {
            "托盘图标已显示"
        } else {
            "托盘图标已隐藏；仍可用热键打开设置"
        }));
        self.refresh_tray();
    }

    fn toggle_autostart(&mut self) {
        if self.applying || !self.settings_editable {
            return;
        }
        let desired = !self.settings.autostart;
        let manager = platform::autostart();
        if let Err(error) = manager.set_enabled(desired) {
            self.banner = Some(Banner::error("写入开机自启失败", error.to_string()));
            return;
        }
        let mut next = self.settings.clone();
        next.autostart = desired;
        if let Err(error) = next.save_atomic(&self.paths.settings_file) {
            let _ = manager.set_enabled(!desired);
            self.banner = Some(Banner::error("保存设置失败", error.to_string()));
            return;
        }
        self.settings = next;
        self.draft.autostart = desired;
        self.toast = Some(Toast::new(if desired {
            "已开启开机自启"
        } else {
            "已关闭开机自启"
        }));
    }

    fn refresh_tray(&mut self) {
        if self.quitting || !self.integrations_ready {
            return;
        }
        if self.tray_failed {
            return;
        }
        let state = self.tray_state();
        if let Some(tray) = self.tray.as_mut() {
            if let Err(error) = tray.sync(&state) {
                self.tray_failed = true;
                log::warn!("刷新托盘失败：{error:#}");
                self.set_banner_if_none(Banner::warning("刷新托盘图标失败", format!("{error:#}")));
            }
        } else if self.settings.tray.visible {
            match TrayController::new(&state) {
                Ok(tray) => self.tray = Some(tray),
                Err(error) => {
                    self.tray_failed = true;
                    log::warn!("创建托盘失败：{error:#}");
                    self.set_banner_if_none(Banner::warning(
                        "创建托盘图标失败",
                        format!("{error:#}"),
                    ));
                }
            }
        }
    }

    fn tray_state(&self) -> TrayState {
        let status = if self.rolling_back {
            "回滚中".to_string()
        } else if self.applying {
            "应用中".to_string()
        } else {
            self.service.label().to_string()
        };
        TrayState {
            status,
            layer_enabled: self.settings.layer.enabled,
            tray_visible: self.settings.tray.visible,
            autostart: self.settings.autostart,
            editable: self.settings_editable && !self.applying,
            restart_enabled: !self.applying,
            alert: self.service.is_failed(),
        }
    }

    fn poll_supervisor(&mut self) {
        let events: Vec<SupervisorEvent> = {
            let Some(receiver) = self.supervisor_events.as_ref() else {
                return;
            };
            let mut events = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
            events
        };

        for event in events {
            match event {
                SupervisorEvent::Started { pid } => {
                    log::info!("supervisor：kanata 启动（pid {pid}）");
                    self.connected = false;
                    if !matches!(self.service, ServiceState::Failed { .. }) {
                        self.service = ServiceState::Starting;
                    }
                    if let Some(client) = &self.client {
                        if let Err(error) = client.reconnect() {
                            log::debug!("请求 TCP 重连失败：{error}");
                        }
                    }
                }
                SupervisorEvent::Exited { code, expected } => {
                    log::info!("supervisor：kanata 退出（code={code:?}, expected={expected}）");
                    self.connected = false;
                }
                SupervisorEvent::Restarting { attempt, delay_ms } => {
                    log::warn!("supervisor：{delay_ms} ms 后重启（近期失败 {attempt} 次）");
                    self.service = ServiceState::Restarting { attempt };
                    if let Some(client) = &self.client {
                        let _ = client.reconnect();
                    }
                }
                SupervisorEvent::Failed { reason } => {
                    log::error!("supervisor 熔断：{reason}");
                    self.stop_processes();
                    self.service = ServiceState::Failed {
                        reason: reason.clone(),
                    };
                    self.banner = Some(Banner::error("kanata 连续启动失败", reason));
                    // “弹一次提示”：即使窗口隐藏在托盘，也把设置窗显示出来，
                    // 让用户能直接看到错误并重试。
                    self.pending_window_activation = true;
                }
                SupervisorEvent::Stopped => {
                    if !matches!(self.service, ServiceState::Failed { .. }) {
                        self.service = ServiceState::Stopped;
                    }
                }
            }
        }
    }

    fn poll_client(&mut self) {
        let events: Vec<ClientEvent> = {
            let Some(client) = self.client.as_ref() else {
                return;
            };
            let mut events = Vec::new();
            while let Some(event) = client.poll_event() {
                events.push(event);
            }
            events
        };

        for event in events {
            match event {
                ClientEvent::Connected { version } => {
                    log::info!("kanata TCP 已连接：{version}");
                    self.kanata_version = Some(version);
                    self.connected = true;
                    self.service = ServiceState::Running;
                }
                ClientEvent::Disconnected { reason } => {
                    log::warn!("kanata TCP 断开：{reason}");
                    self.connected = false;
                    if !matches!(self.service, ServiceState::Failed { .. }) {
                        self.service = ServiceState::Reconnecting;
                    }
                }
                ClientEvent::Reconnecting {
                    attempt,
                    max,
                    delay_ms,
                } => {
                    log::info!("TCP 重连 {attempt}/{max}，{delay_ms} ms 后重试");
                    if !matches!(self.service, ServiceState::Failed { .. }) {
                        self.service = ServiceState::Reconnecting;
                    }
                }
                ClientEvent::ReconnectExhausted { reason } => {
                    log::error!("TCP 重连次数耗尽：{reason}");
                    self.service = ServiceState::Failed {
                        reason: reason.clone(),
                    };
                    self.banner = Some(Banner::error(
                        "与 kanata 的连接已断开",
                        format!("{reason}\n可以点击「重试启动」或托盘「重启 kanata」。"),
                    ));
                    self.pending_window_activation = true;
                }
                ClientEvent::HelloOk { version, .. } => {
                    self.kanata_version = Some(version);
                }
                ClientEvent::MessagePush { message, .. } => {
                    log::info!("收到 kanata MessagePush：{message}");
                    if message.trim() == "capsx:settings" {
                        // 这是三条唤出路径中唯一由 kanata 触发的路径。
                        // 用不依赖 Context 的延迟标记在本帧稍后显示窗口。
                        self.pending_window_activation = true;
                    } else {
                        log::debug!("忽略 kanata 消息：{message}");
                    }
                }
                ClientEvent::LayerChange { new, .. } => {
                    log::debug!("kanata 层切换：{new}");
                }
                ClientEvent::ConfigFileReload { new, .. } => {
                    log::info!("kanata 已重载配置：{new}");
                }
                ClientEvent::KanataError { message, .. } => {
                    log::error!("kanata 错误：{message}");
                    self.banner = Some(Banner::warning("kanata 报告错误", message));
                }
                ClientEvent::ReloadResult { ok, raw } => {
                    if self.pending_apply.is_some() {
                        if ok {
                            self.finish_apply();
                        } else {
                            log::error!("ReloadResult ok=false：{raw}");
                            self.begin_rollback(
                                "kanata 拒绝了新配置（ReloadResult.ok=false）".into(),
                            );
                        }
                    } else if self.rolling_back {
                        log::info!("回滚后的 ReloadResult：{raw}");
                    } else {
                        log::debug!("收到无待处理请求的 ReloadResult：{raw}");
                    }
                }
                ClientEvent::CurrentLayerName { name, .. } => {
                    log::debug!("当前层：{name}");
                }
                ClientEvent::Status { .. }
                | ClientEvent::HoldActivated { .. }
                | ClientEvent::TapActivated { .. }
                | ClientEvent::Unknown { .. }
                | ClientEvent::ParseError { .. }
                | ClientEvent::CommandDropped { .. } => {
                    log::debug!("kanata 事件：{event:?}");
                }
            }
        }
    }

    fn retry_service(&mut self) {
        if self.applying {
            return;
        }
        self.banner = None;
        if self.supervisor.is_some() {
            if let Some(supervisor) = &self.supervisor {
                supervisor.restart();
            }
            if let Some(client) = &self.client {
                let _ = client.reconnect();
            }
            self.service = ServiceState::Restarting { attempt: 0 };
            self.toast = Some(Toast::new("正在重启 kanata…"));
            return;
        }

        if !self.paths.keymap_file.is_file() {
            self.banner = Some(Banner::error(
                "无法重启 kanata",
                format!(
                    "配置文件不存在：{}\n请先应用一次设置，或重新安装 CapsX。",
                    self.paths.keymap_file.display()
                ),
            ));
            return;
        }
        match self.start_runtime() {
            Ok(()) => {
                self.toast = Some(Toast::new("正在重新启动 kanata…"));
            }
            Err(error) => {
                self.fail_service(format!("重启 kanata 失败：{error:#}"));
            }
        }
    }

    fn activate_window(&mut self, ctx: &egui::Context) {
        if self.quitting {
            return;
        }
        log::info!("显示 CapsX 设置窗口");
        self.window_visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        if let Some(position) = self
            .saved_window_pos
            .or_else(|| default_window_position(ctx))
        {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        }
        // 从托盘菜单唤出时，Windows 的前台锁可能让 Focus 失效，窗口会停在
        // 其它窗口后面。设置窗是短生命周期的工具窗，直接置顶最可靠。
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        if self.quitting {
            return;
        }
        if let Some(position) = visible_window_position(ctx) {
            self.saved_window_pos = Some(position);
        }
        // 关闭 = 放弃本次未应用的草稿。下次打开时回到已持久化的设置，
        // 避免“恢复默认”后关闭再打开仍显示未保存状态。
        if !self.applying {
            self.draft = self.settings.clone();
        }
        self.window_visible = false;
        // 不使用 Visible(false)：Windows/winit 在隐藏窗口上不会派发
        // RedrawRequested，eframe 的 App::update 会停摆，托盘、热键和
        // 单实例事件就全部失效。把窗口移到屏幕外可保持事件循环运转。
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
            OFFSCREEN_X,
            OFFSCREEN_Y,
        )));
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn begin_quit(&mut self, ctx: &egui::Context) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        log::info!("CapsX 正在退出");
        if self.settings_editable {
            if let Err(error) = self.settings.save_atomic(&self.paths.settings_file) {
                log::warn!("退出时保存设置失败：{error}");
            }
        }
        self.stop_processes();
        self.tray = None;
        self.hotkey = None;
        capsx_core::logger::flush();
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn expire_toast(&mut self) {
        if self
            .toast
            .as_ref()
            .map(|toast| !toast.alive())
            .unwrap_or(false)
        {
            self.toast = None;
        }
    }

    fn set_banner_if_none(&mut self, banner: Banner) {
        if self.banner.is_none() {
            self.banner = Some(banner);
        }
    }
}

impl Drop for CapsxApp {
    fn drop(&mut self) {
        if !self.quitting {
            log::info!("CapsX 窗口销毁，停止运行时");
            self.stop_processes();
        }
        capsx_core::logger::flush();
    }
}

impl eframe::App for CapsxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let color = crate::ui::theme::BG_BOTTOM;
        [
            color.r() as f32 / 255.0,
            color.g() as f32 / 255.0,
            color.b() as f32 / 255.0,
            1.0,
        ]
    }
}

fn load_settings(paths: &AppPaths) -> (Settings, bool, Option<Banner>) {
    match Settings::load_optional(&paths.settings_file) {
        Ok(Some(settings)) => (settings, true, None),
        Ok(None) => {
            let settings = Settings::default();
            let banner = match settings.save_atomic(&paths.settings_file) {
                Ok(()) => None,
                Err(error) => {
                    log::warn!("写入默认设置失败：{error}");
                    Some(Banner::warning(
                        "无法写入默认设置",
                        format!("{error}\n设置窗口将只读。"),
                    ))
                }
            };
            (settings, true, banner)
        }
        Err(error) => {
            log::error!("读取设置失败：{error}");
            (
                Settings::default(),
                false,
                Some(Banner::error(
                    "设置文件无法读取，已保留原文件",
                    format!(
                        "{error}\n\n为了防止覆盖，本次运行禁用「应用」。\
                         请打开配置文件夹修复 settings.json，或移走它后重启。"
                    ),
                )),
            )
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

const OFFSCREEN_X: f32 = -20_000.0;
const OFFSCREEN_Y: f32 = -20_000.0;

fn visible_window_position(ctx: &egui::Context) -> Option<Pos2> {
    ctx.input(|input| input.viewport().outer_rect.map(|rect| rect.min))
        .filter(|position| position.x > OFFSCREEN_X + 1_000.0 && position.y > OFFSCREEN_Y + 1_000.0)
}

fn default_window_position(ctx: &egui::Context) -> Option<Pos2> {
    let size = ctx
        .input(|input| input.viewport().outer_rect.map(|rect| rect.size()))
        .unwrap_or(egui::vec2(600.0, 720.0));
    ctx.input(|input| input.viewport().monitor_size)
        .map(|monitor| {
            Pos2::new(
                ((monitor.x - size.x) / 2.0).max(20.0),
                ((monitor.y - size.y) / 2.0).max(20.0),
            )
        })
}

fn truncate_detail(detail: &str) -> String {
    const MAX: usize = 8_000;
    if detail.chars().count() <= MAX {
        return detail.to_string();
    }
    let mut text: String = detail.chars().take(MAX).collect();
    text.push_str("\n…（输出已截断，完整内容见日志）");
    text
}

fn open_path(path: &Path) -> std::result::Result<(), String> {
    if !path.exists() {
        return Err(format!("路径不存在：{}", path.display()));
    }
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return Err("当前平台不支持打开文件夹".into());

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("启动文件管理器失败：{error}"))
}
