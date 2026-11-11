//! CapsX 核心错误类型。

use std::path::PathBuf;

use thiserror::Error;

/// `capsx-core` 的统一错误类型。
#[derive(Debug, Error)]
pub enum Error {
    /// 文件或操作系统调用失败。
    #[error("I/O 错误：{0}")]
    Io(#[from] std::io::Error),

    /// JSON 编解码失败。
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),

    /// 当前平台未实现该能力。
    #[error("当前平台不支持：{0}")]
    Unsupported(&'static str),

    /// 平台 API（注册表、命名事件等）失败。
    #[error("平台错误：{0}")]
    Platform(String),

    /// 设置文件版本高于本程序。
    #[error("设置文件版本为 {found}，本程序只支持到 {supported}；已保留原文件")]
    FutureSettingsVersion {
        /// 文件中的版本。
        found: u32,
        /// 本程序支持的版本。
        supported: u32,
    },

    /// 设置内容非法。
    #[error("设置内容非法：{0}")]
    InvalidSettings(String),

    /// 配置未通过 `kanata --check`。
    #[error("配置校验失败（退出码 {status:?}）：{output}")]
    ConfigCheck {
        /// 进程退出码；超时时为 `None`。
        status: Option<i32>,
        /// 合并后的 stdout/stderr。
        output: String,
    },

    /// 配置校验超时。
    #[error("配置校验超时（{0} ms）")]
    ConfigCheckTimeout(u64),

    /// 写入配置失败。
    #[error("写入配置失败：{0}")]
    ConfigInstall(String),

    /// 找不到 kanata。
    #[error("找不到 kanata 可执行文件：{0}")]
    MissingKanata(PathBuf),

    /// 找不到默认配置模板。
    #[error("找不到默认配置模板：{0}")]
    MissingDefaultConfig(PathBuf),

    /// 子进程启动失败。
    #[error("kanata 子进程启动失败：{0}")]
    Process(String),

    /// TCP 连接或协议错误。
    #[error("TCP 错误：{0}")]
    Tcp(String),

    /// 操作超时。
    #[error("操作超时：{0}")]
    Timeout(String),
}

/// 核心层便捷结果类型。
pub type Result<T> = std::result::Result<T, Error>;
