//! CapsX 的纯逻辑核心。
//!
//! 这个 crate 不依赖任何 GUI 库，方便单测、CLI 与未来其它前端复用。

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod client;
pub mod config_gen;
pub mod constants;
pub mod error;
pub mod kanata;
pub mod logger;
pub mod paths;
pub mod platform;
pub mod settings;

pub use error::{Error, Result};

/// 返回 crate 版本，供关于窗口展示。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
