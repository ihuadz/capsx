//! 运行状态与提示条。

use std::time::Instant;

/// 服务状态（决定状态点颜色与文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    /// 子进程已启动，等待 TCP。
    Starting,
    /// TCP 已连接。
    Running,
    /// 断线重连中。
    Reconnecting,
    /// 子进程重启中。
    Restarting {
        /// 第几次。
        attempt: usize,
    },
    /// 熔断或连接彻底失败。
    Failed {
        /// 展示给用户的说明。
        reason: String,
    },
    /// 已停止。
    Stopped,
}

impl ServiceState {
    /// 中文短标签。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Starting => "启动中",
            Self::Running => "运行中",
            Self::Reconnecting => "重连中",
            Self::Restarting { .. } => "重启中",
            Self::Failed { .. } => "启动失败",
            Self::Stopped => "已停止",
        }
    }

    /// 是否失败/红色。
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// 失败原因。
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Failed { reason } => Some(reason),
            _ => None,
        }
    }
}

/// 提示条类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    /// 红色错误。
    Error,
    /// 琥珀警告。
    Warning,
}

/// 顶部提示条。
#[derive(Debug, Clone)]
pub struct Banner {
    /// 类型。
    pub kind: BannerKind,
    /// 标题。
    pub title: String,
    /// 详情（可折叠）。
    pub details: String,
}

impl Banner {
    /// 错误提示。
    pub fn error(title: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            kind: BannerKind::Error,
            title: title.into(),
            details: details.into(),
        }
    }

    /// 警告提示。
    pub fn warning(title: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            kind: BannerKind::Warning,
            title: title.into(),
            details: details.into(),
        }
    }
}

/// 自动消失的 toast。
#[derive(Debug, Clone)]
pub struct Toast {
    /// 内容。
    pub text: String,
    /// 截止时间。
    pub until: Instant,
}

impl Toast {
    /// 保留 4 秒。
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            until: Instant::now() + std::time::Duration::from_secs(4),
        }
    }

    /// 是否仍应显示。
    pub fn alive(&self) -> bool {
        Instant::now() < self.until
    }
}
