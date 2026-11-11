//! CapsX 常量集中地。
//!
//! 时间、退避序列、尺寸预算等“魔法数字”只允许出现在这里。

/// CapsX 版本。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 单实例互斥体名称。
pub const INSTANCE_MUTEX: &str = "CapsX.SingleInstance";

/// 激活已运行实例的命名事件。
pub const ACTIVATE_EVENT: &str = "Local\\CapsX.Activate";

/// 请求已运行实例退出的命名事件。
pub const QUIT_EVENT: &str = "Local\\CapsX.Quit";

/// `kanata --check` 超时，毫秒。
pub const CONFIG_CHECK_TIMEOUT_MS: u64 = 5_000;

/// 启动后等待 TCP 连接成功的最长时间，毫秒。
pub const TCP_STARTUP_DEADLINE_MS: u64 = 10_000;

/// 子进程启动后的稳定判定时间，毫秒。
pub const PROCESS_STABLE_MS: u64 = 10_000;

/// 10 秒内连续失败次数达到该值后停止重启。
pub const MAX_FAST_FAILURES: usize = 3;

/// 连续失败计数窗口，毫秒。
pub const FAST_FAILURE_WINDOW_MS: u64 = 10_000;

/// 子进程非预期退出后的退避序列，毫秒（封顶 30 秒）。
pub const RESTART_BACKOFF_MS: [u64; 6] = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000];

/// 子进程停止时等待其退出的上限，毫秒。
pub const CHILD_STOP_TIMEOUT_MS: u64 = 2_000;

/// TCP 重连间隔，毫秒。
pub const TCP_RECONNECT_DELAY_MS: u64 = 3_000;

/// 断线后最多重连次数。
pub const TCP_MAX_RECONNECTS: u32 = 5;

/// TCP 连接超时，毫秒。
pub const TCP_CONNECT_TIMEOUT_MS: u64 = 200;

/// 首次连接（尚未成功过）的重试间隔，毫秒。
pub const TCP_INITIAL_RETRY_DELAY_MS: u64 = 50;

/// 首次连接的最大尝试次数（由 supervisor 的 10 秒启动期限兜底）。
pub const TCP_INITIAL_MAX_ATTEMPTS: u32 = 50;

/// TCP `Hello` 握手超时，毫秒。
pub const TCP_HELLO_TIMEOUT_MS: u64 = 5_000;

/// `Reload` 请求默认超时，毫秒。
pub const TCP_RELOAD_TIMEOUT_MS: u64 = 5_000;

/// TCP 读取轮询间隔，毫秒。
pub const TCP_POLL_INTERVAL_MS: u64 = 100;

/// 设置应用后等待 `ReloadResult` 的宽限时间（在请求超时之外），毫秒。
pub const RELOAD_UI_GRACE_MS: u64 = 1_000;

/// 日志单文件大小上限：5 MB。
pub const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// 日志保留天数。
pub const LOG_RETENTION_DAYS: u64 = 7;

/// 设置窗口默认尺寸。
pub const WINDOW_SIZE: [f32; 2] = [620.0, 900.0];

/// 设置窗口最小尺寸。
pub const WINDOW_MIN_SIZE: [f32; 2] = [400.0, 480.0];

/// 主窗口标题。
pub const WINDOW_TITLE: &str = "CapsX 设置";

/// 默认设置键（`CapsLock + ,`）。
pub const DEFAULT_SETTINGS_KEY: &str = ",";

/// 托盘/窗口使用的应用标识。
pub const APP_ID: &str = "com.capsx.app";

/// 等待旧实例退出的上限（卸载兜底），毫秒。
pub const INSTANCE_EXIT_WAIT_MS: u64 = 6_000;
