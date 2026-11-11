//! 应用路径解析。
//!
//! 设置放 `%APPDATA%\CapsX`，日志放 `%LOCALAPPDATA%\CapsX\logs`，
//! kanata 与默认配置在安装目录/开发目录的 `runtime` 下。

use std::env;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{Error, Result};

/// CapsX 运行所需的全部路径。
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// 用户设置与生成的配置所在目录。
    pub data_dir: PathBuf,
    /// 日志所在的缓存目录根。
    pub cache_dir: PathBuf,
    /// `settings.json`。
    pub settings_file: PathBuf,
    /// `keymap.kbd`。
    pub keymap_file: PathBuf,
    /// 日志目录。
    pub logs_dir: PathBuf,
    /// 含 kanata 的运行时目录。
    pub runtime_dir: PathBuf,
    /// `kanata.exe`（按平台名称）。
    pub kanata_file: PathBuf,
    /// 安装包附带、只读的 `default.kbd`。
    pub default_config: PathBuf,
}

impl AppPaths {
    /// 解析当前用户的应用路径。
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "CapsX")
            .ok_or_else(|| Error::InvalidSettings("无法确定用户 AppData 目录".into()))?;
        // `directories` 在 Windows 上会追加 `data` / `cache`；本项目的契约是
        // `%APPDATA%\CapsX` 与 `%LOCALAPPDATA%\CapsX`，所以取父目录。
        let data_dir = app_root(dirs.data_dir());
        let cache_dir = app_root(dirs.cache_dir());
        let exe = env::current_exe()?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| Error::InvalidSettings("无法确定程序所在目录".into()))?
            .to_path_buf();

        let runtime_dir = find_runtime_dir(&exe_dir).unwrap_or_else(|| exe_dir.join("runtime"));
        let kanata_file = runtime_dir.join(kanata_file_name());
        let default_config = find_default_config(&exe_dir, &runtime_dir)
            .unwrap_or_else(|| exe_dir.join("default.kbd"));

        Ok(Self {
            settings_file: data_dir.join("settings.json"),
            keymap_file: data_dir.join("keymap.kbd"),
            logs_dir: cache_dir.join("logs"),
            data_dir,
            cache_dir,
            runtime_dir,
            kanata_file,
            default_config,
        })
    }

    /// 确保可写目录存在。
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        Ok(())
    }

    /// 检查 kanata 与默认模板是否存在，返回缺失的路径列表。
    pub fn missing_runtime_files(&self) -> Vec<PathBuf> {
        let mut missing = Vec::new();
        if !self.kanata_file.is_file() {
            missing.push(self.kanata_file.clone());
        }
        if !self.default_config.is_file() {
            missing.push(self.default_config.clone());
        }
        missing
    }
}

/// 把 `ProjectDirs` 的 Windows 子目录折叠为 CapsX 根目录。
fn app_root(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let is_known_subdir = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| matches!(name, "data" | "cache" | "config"))
            .unwrap_or(false);
        if is_known_subdir {
            if let Some(parent) = path.parent() {
                return parent.to_path_buf();
            }
        }
    }
    path.to_path_buf()
}

/// 当前平台的 kanata 文件名。
pub fn kanata_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "kanata.exe"
    }
    #[cfg(target_os = "macos")]
    {
        "kanata"
    }
    #[cfg(target_os = "linux")]
    {
        "kanata"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "kanata"
    }
}

fn find_runtime_dir(exe_dir: &Path) -> Option<PathBuf> {
    let mut candidates = vec![exe_dir.join("runtime")];
    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join("runtime"));
        if let Some(grandparent) = parent.parent() {
            candidates.push(grandparent.join("runtime"));
        }
    }

    // 开发时 `cargo run` 的可执行文件在 target/debug；用编译期清单目录兜底。
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        let manifest = Path::new(manifest);
        if let Some(workspace) = manifest.parent().and_then(Path::parent) {
            candidates.push(workspace.join("runtime"));
        }
    }

    candidates.into_iter().find(|dir| {
        dir.join("kanata.exe").is_file()
            || dir.join("kanata").is_file()
            || dir.join("default.kbd").is_file()
    })
}

fn find_default_config(exe_dir: &Path, runtime_dir: &Path) -> Option<PathBuf> {
    [
        exe_dir.join("default.kbd"),
        runtime_dir.join("default.kbd"),
        exe_dir.join("runtime").join("default.kbd"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// 在临时目录里构造一个隔离的路径集合，供测试使用。
#[doc(hidden)]
pub fn test_paths(root: &Path) -> AppPaths {
    AppPaths {
        data_dir: root.join("data"),
        cache_dir: root.join("cache"),
        settings_file: root.join("data/settings.json"),
        keymap_file: root.join("data/keymap.kbd"),
        logs_dir: root.join("cache/logs"),
        runtime_dir: root.join("runtime"),
        kanata_file: root.join("runtime").join(kanata_file_name()),
        default_config: root.join("runtime/default.kbd"),
    }
}
