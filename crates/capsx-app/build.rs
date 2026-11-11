//! Windows 可执行文件资源（图标 + 版本信息）。
//!
//! 不引入 `winres`/`embed-resource`：直接调用 Windows SDK 的 `rc.exe`，
//! 找到编译器时把它生成的 `.res` 作为链接输入；找不到时安静跳过，保证
//! 任何环境都能先编译出功能完整的 exe。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/capsx.rc");
    println!("cargo:rerun-if-changed=assets/capsx.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let Some(rc) = find_rc() else {
        println!("cargo:warning=未找到 rc.exe，跳过 Windows 图标/版本资源");
        return;
    };
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into()));
    let res = out_dir.join("capsx.res");

    let status = Command::new(&rc)
        .current_dir(manifest_dir.join("assets"))
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg("capsx.rc")
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", res.display());
        }
        Ok(status) => {
            println!("cargo:warning=rc.exe 失败（{status}），跳过 Windows 资源");
        }
        Err(error) => {
            println!("cargo:warning=调用 rc.exe 失败（{error}），跳过 Windows 资源");
        }
    }
}

fn find_rc() -> Option<PathBuf> {
    if let Ok(rc) = std::env::var("RC") {
        let path = PathBuf::from(rc);
        if path.is_file() {
            return Some(path);
        }
    }

    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("x64").join("rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    versions.sort();
    versions.pop()
}
