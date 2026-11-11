//! CapsX 入口：日志、单实例、卸载兜底、eframe 启动。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod event;
mod status;
mod tray;
mod ui;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use capsx_core::constants::{
    APP_ID, APP_VERSION, INSTANCE_EXIT_WAIT_MS, INSTANCE_MUTEX, WINDOW_MIN_SIZE, WINDOW_SIZE,
    WINDOW_TITLE,
};
use capsx_core::logger;
use capsx_core::paths::AppPaths;
use capsx_core::platform;
use eframe::egui;
use log::LevelFilter;
use single_instance::SingleInstance;

use crate::app::CapsxApp;
use crate::event::AppEvent;

fn main() {
    if let Err(error) = run() {
        log::error!("CapsX 退出：{error:#}");
        logger::flush();
        // release 是 windows_subsystem，无控制台；保留 eprintln 便于开发构建排障。
        eprintln!("CapsX: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("CapsX {APP_VERSION}");
        return Ok(());
    }

    let paths = AppPaths::discover().context("解析应用路径失败")?;
    paths.ensure_dirs().context("创建应用目录失败")?;

    let level = match std::env::var("CAPSX_LOG").as_deref() {
        Ok("trace") => LevelFilter::Trace,
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        Ok("error") => LevelFilter::Error,
        _ => {
            if cfg!(debug_assertions) {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            }
        }
    };
    logger::init(&paths.logs_dir, level).context("初始化日志失败")?;
    logger::install_panic_hook();
    log::info!(
        "CapsX {} 启动（args={args:?}，data={}）",
        APP_VERSION,
        paths.data_dir.display()
    );

    if args.iter().any(|arg| arg == "--uninstall") {
        return run_uninstall(&paths);
    }

    let start_hidden = args.iter().any(|arg| arg == "--tray");
    let instance = SingleInstance::new(INSTANCE_MUTEX).context("创建单实例互斥体失败")?;
    if !instance.is_single() {
        log::info!("已有实例在运行，通知它打开设置窗口");
        if let Err(error) = platform::signal_activate() {
            log::warn!("通知已有实例失败：{error}");
        }
        return Ok(());
    }

    let (event_tx, event_rx) = mpsc::channel::<AppEvent>();
    let activate_tx = event_tx.clone();
    let quit_tx = event_tx.clone();
    platform::install_instance_listeners(
        Box::new(move || {
            log::info!("收到第二实例的激活请求");
            let _ = activate_tx.send(AppEvent::Activate);
        }),
        Box::new(move || {
            log::info!("收到第二实例的退出请求");
            let _ = quit_tx.send(AppEvent::QuitRequested);
        }),
    )
    .context("安装单实例事件监听失败")?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(WINDOW_MIN_SIZE)
        .with_icon(std::sync::Arc::new(assets::window_icon()))
        .with_visible(true)
        .with_taskbar(false)
        .with_app_id(APP_ID);
    if start_hidden {
        // 隐藏启动时窗口仍然“可见”，只是放在屏幕外。这样 Windows 会继续
        // 派发 redraw，eframe 的 update 才能在托盘/热键/单实例事件到来时运行。
        viewport = viewport.with_position([-20_000.0, -20_000.0]);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        vsync: true,
        centered: !start_hidden,
        persist_window: false,
        ..Default::default()
    };

    let app_creator = move |cc: &eframe::CreationContext<'_>| {
        let app = CapsxApp::new(cc, paths, start_hidden, instance, event_tx, event_rx);
        Ok(Box::new(app) as Box<dyn eframe::App>)
    };

    eframe::run_native(WINDOW_TITLE, options, Box::new(app_creator))
        .map_err(|error| anyhow::anyhow!("eframe 运行失败：{error}"))?;
    log::info!("CapsX 主事件循环结束");
    logger::flush();
    Ok(())
}

fn run_uninstall(paths: &AppPaths) -> Result<()> {
    log::info!("运行卸载兜底流程");

    let running = {
        let probe = SingleInstance::new(INSTANCE_MUTEX).context("检查单实例失败")?;
        !probe.is_single()
    };

    if running {
        if let Err(error) = platform::signal_quit() {
            log::warn!("通知运行中的 CapsX 退出失败：{error}");
        }
        let deadline = Instant::now() + Duration::from_millis(INSTANCE_EXIT_WAIT_MS);
        let mut exited = false;
        while Instant::now() < deadline {
            let probe = SingleInstance::new(INSTANCE_MUTEX).context("检查单实例失败")?;
            if probe.is_single() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        if !exited {
            log::warn!("等待 CapsX 退出超时，尝试强制结束其它实例");
            force_kill_other_instances();
            std::thread::sleep(Duration::from_millis(800));
        }
    }

    platform::autostart()
        .set_enabled(false)
        .context("删除开机自启项失败")?;
    log::info!("卸载兜底完成（设置目录保留：{}）", paths.data_dir.display());
    logger::flush();
    Ok(())
}

#[cfg(target_os = "windows")]
fn force_kill_other_instances() {
    let pid = std::process::id();
    let filter = format!("PID ne {pid}");
    match std::process::Command::new("taskkill")
        .args(["/F", "/IM", "capsx.exe", "/FI", &filter])
        .output()
    {
        Ok(output) => {
            log::info!("taskkill 完成：status={:?}", output.status.code());
        }
        Err(error) => log::warn!("taskkill 失败：{error}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn force_kill_other_instances() {}
