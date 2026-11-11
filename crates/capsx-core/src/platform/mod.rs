//! 平台能力：自启、单实例通知、子进程 Job、隐藏控制台。
//!
//! M1 只实现 Windows；其它平台返回 `Unsupported`，但保留同一套接口。

use crate::error::Result;

/// 自启管理器。
pub trait Autostart: Send + Sync {
    /// 当前用户是否已启用开机自启。
    fn is_enabled(&self) -> Result<bool>;

    /// 启用/停用开机自启。
    fn set_enabled(&self, enabled: bool) -> Result<()>;
}

/// 获取当前平台的自启实现。
pub fn autostart() -> Box<dyn Autostart> {
    imp::autostart()
}

/// 启动命名单事件监听线程。
///
/// 第一个实例创建事件；后续实例通过 [`signal_activate`] / [`signal_quit`]
/// 通知已运行实例。监听线程随进程结束，无需显式回收。
pub fn install_instance_listeners(
    on_activate: impl Fn() + Send + 'static,
    on_quit: impl Fn() + Send + 'static,
) -> Result<()> {
    imp::install_instance_listeners(Box::new(on_activate), Box::new(on_quit))
}

/// 通知已运行实例打开设置窗口。
pub fn signal_activate() -> Result<()> {
    imp::signal_event(crate::constants::ACTIVATE_EVENT)
}

/// 通知已运行实例退出。
pub fn signal_quit() -> Result<()> {
    imp::signal_event(crate::constants::QUIT_EVENT)
}

/// 让子进程在父进程死亡时被系统回收（Job Object / PDEATHSIG）。
pub fn attach_child_cleanup(child: &std::process::Child) -> Result<ChildCleanup> {
    imp::attach_child_cleanup(child)
}

/// 子进程清理句柄。Drop 时若开启了 kill-on-close，会强制结束子进程。
pub struct ChildCleanup {
    #[cfg(target_os = "windows")]
    _job: imp::ProcessJob,
    #[cfg(not(target_os = "windows"))]
    _private: (),
}

/// 隐藏子进程控制台窗口（Windows），其它平台无操作。
pub fn hide_console(command: &mut std::process::Command) {
    imp::hide_console(command);
}

/// 当前本地时间（写日志用）。
#[derive(Debug, Clone, Copy)]
pub struct LocalNow {
    /// 年。
    pub year: i32,
    /// 月（1–12）。
    pub month: u32,
    /// 日。
    pub day: u32,
    /// 时。
    pub hour: u32,
    /// 分。
    pub minute: u32,
    /// 秒。
    pub second: u32,
    /// 毫秒。
    pub millis: u32,
}

/// 取当前本地时间。
pub fn local_now() -> LocalNow {
    imp::local_now()
}

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod imp;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod imp;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod imp;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
compile_error!("CapsX M1 只支持 Windows/macOS/Linux 中已预留接口的平台");

/// 无平台时间 API 时的 UTC 回退（M2 再替换为各平台本地时区）。
#[cfg(not(target_os = "windows"))]
pub(crate) fn utc_now() -> LocalNow {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    LocalNow {
        year,
        month,
        day,
        hour: (remainder / 3_600) as u32,
        minute: ((remainder % 3_600) / 60) as u32,
        second: (remainder % 60) as u32,
        millis: 0,
    }
}

/// Howard Hinnant 的 `civil_from_days` 算法。
#[cfg(not(target_os = "windows"))]
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
