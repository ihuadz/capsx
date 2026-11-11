//! macOS 平台占位实现（M2）。
//!
//! M1 的 Windows 安装包不包含本文件；保留接口以便后续实现
//! LaunchAgents 自启与激活事件。

use std::process::Child;

use crate::error::{Error, Result};
use crate::platform::{Autostart, ChildCleanup, LocalNow};

struct UnsupportedAutostart;

impl Autostart for UnsupportedAutostart {
    fn is_enabled(&self) -> Result<bool> {
        Err(Error::Unsupported("macOS 自启（M2）"))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<()> {
        Err(Error::Unsupported("macOS 自启（M2）"))
    }
}

pub(super) fn autostart() -> Box<dyn Autostart> {
    Box::new(UnsupportedAutostart)
}

pub(super) fn install_instance_listeners(
    _on_activate: Box<dyn Fn() + Send>,
    _on_quit: Box<dyn Fn() + Send>,
) -> Result<()> {
    log::warn!("macOS 单实例通知尚未实现（M2）");
    Ok(())
}

pub(super) fn signal_event(_name: &str) -> Result<()> {
    Err(Error::Unsupported("macOS 实例事件（M2）"))
}

pub(super) fn attach_child_cleanup(_child: &Child) -> Result<ChildCleanup> {
    Err(Error::Unsupported(
        "macOS 子进程回收（M2，需 PDEATHSIG 等价实现）",
    ))
}

pub(super) fn hide_console(_command: &mut std::process::Command) {}

pub(super) fn local_now() -> LocalNow {
    super::utc_now()
}
