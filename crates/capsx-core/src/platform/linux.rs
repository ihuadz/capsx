//! Linux 平台占位实现（M2）。
//!
//! 后续需要：`~/.config/autostart/capsx.desktop`、`PR_SET_PDEATHSIG`，
//! 以及 `/dev/uinput` 权限检测。

use std::process::Child;

use crate::error::{Error, Result};
use crate::platform::{Autostart, ChildCleanup, LocalNow};

struct UnsupportedAutostart;

impl Autostart for UnsupportedAutostart {
    fn is_enabled(&self) -> Result<bool> {
        Err(Error::Unsupported("Linux 自启（M2）"))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<()> {
        Err(Error::Unsupported("Linux 自启（M2）"))
    }
}

pub(super) fn autostart() -> Box<dyn Autostart> {
    Box::new(UnsupportedAutostart)
}

pub(super) fn install_instance_listeners(
    _on_activate: Box<dyn Fn() + Send>,
    _on_quit: Box<dyn Fn() + Send>,
) -> Result<()> {
    log::warn!("Linux 单实例通知尚未实现（M2）");
    Ok(())
}

pub(super) fn signal_event(_name: &str) -> Result<()> {
    Err(Error::Unsupported("Linux 实例事件（M2）"))
}

pub(super) fn attach_child_cleanup(_child: &Child) -> Result<ChildCleanup> {
    Err(Error::Unsupported(
        "Linux 子进程回收（M2，需 PR_SET_PDEATHSIG）",
    ))
}

pub(super) fn hide_console(_command: &mut std::process::Command) {}

pub(super) fn local_now() -> LocalNow {
    super::utc_now()
}
