//! 应用事件：单实例通知与配置应用工作线程。

use std::path::PathBuf;

use capsx_core::settings::Settings;

/// 主线程事件。
#[derive(Debug)]
pub enum AppEvent {
    /// 第二个实例 / 热键请求打开设置。
    Activate,
    /// 请求退出（卸载器、托盘“退出”）。
    QuitRequested,
    /// 配置应用流程。
    Apply(ApplyEvent),
}

/// 配置应用线程回传。
#[derive(Debug)]
pub enum ApplyEvent {
    /// 文件已校验并替换；等待主线程发 Reload。
    Prepared {
        /// 待保存的新设置。
        settings: Settings,
        /// 旧配置备份。
        backup: Option<PathBuf>,
    },
    /// 在安装前失败（校验/写入）。
    Failed {
        /// 标题。
        title: String,
        /// 详情（kanata 输出等）。
        details: String,
    },
    /// 回滚线程结束。
    RolledBack {
        /// 回滚本身是否失败。
        error: Option<String>,
    },
}
