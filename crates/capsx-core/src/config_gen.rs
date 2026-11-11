//! 从设置生成 `keymap.kbd`，以及带校验/备份/回滚的安装流程。
//!
//! 三条硬规则：
//! 1. kanata 注释只用 `;;`；
//! 2. `defsrc` / `deflayer` 的列严格按 [`MAPPING_KEYS`] 的固定顺序拼接；
//! 3. 关闭和弦时完全不生成 `defchords` / `defalias`。

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::constants::CONFIG_CHECK_TIMEOUT_MS;
use crate::error::{Error, Result};
use crate::platform;
use crate::settings::Settings;

const TAP_MIN_MS: u32 = 50;
const TAP_MAX_MS: u32 = 1_000;
const HOLD_MIN_MS: u32 = 50;
const HOLD_MAX_MS: u32 = 1_000;
const CHORD_MIN_MS: u32 = 10;
const CHORD_MAX_MS: u32 = 1_000;

/// 生成的配置文本。
pub fn generate(settings: &Settings) -> Result<String> {
    settings.validate()?;
    let mappings = settings.complete_mappings();
    let tap = settings.layer.tap_repress_ms.clamp(TAP_MIN_MS, TAP_MAX_MS);
    let hold = settings.layer.hold_ms.clamp(HOLD_MIN_MS, HOLD_MAX_MS);
    let chord = settings.layer.chord_ms.clamp(CHORD_MIN_MS, CHORD_MAX_MS);

    let mut out = String::new();
    out.push_str(";; CapsX 自动生成的 kanata 配置\n");
    out.push_str(";; 在 CapsX 设置里点击「应用」会覆盖本文件。\n");
    out.push_str(";; 语法参考：https://github.com/jtroo/kanata/blob/main/docs/config.adoc\n");
    out.push('\n');

    if settings.layer.enabled && settings.layer.chords_enabled {
        out.push_str(";; 输入和弦：同时按下两个导航键。\n");
        out.push_str(";; 注释必须使用双分号，单个分号会让 kanata 拒绝加载。\n");
        out.push_str(&format!("(defchords navchords {chord}\n"));
        for key in ["h", "j", "k", "l"] {
            out.push_str(&format!("  ({key}) {}\n", settings.action_for(key)));
        }
        out.push_str("  (h l) (macro C-left C-S-right)\n");
        out.push_str("  (k j) (macro home S-end)\n");
        out.push_str(")\n\n");

        out.push_str("(defalias\n");
        for key in ["h", "j", "k", "l"] {
            out.push_str(&format!("  ch-{key} (chord navchords {key})\n"));
        }
        out.push_str(")\n\n");
    }

    out.push_str("(defcfg\n");
    out.push_str("  ;; 只接管 defsrc 里的键，其余按键原样透传\n");
    out.push_str("  process-unmapped-keys yes\n");
    out.push_str(")\n\n");

    out.push_str("(defsrc\n  caps");
    for (key, _) in &mappings {
        out.push_str(&format!("  {key}"));
    }
    out.push_str(&format!("  {}\n", settings.settings_key));
    out.push_str(")\n\n");

    out.push_str("(deflayer base\n  ");
    if settings.layer.enabled {
        out.push_str(&format!(
            "(tap-hold-press {tap} {hold} caps (layer-while-held nav))"
        ));
    } else {
        out.push_str("caps");
    }
    for (key, _) in &mappings {
        out.push_str(&format!("  {key}"));
    }
    out.push_str(&format!("  {}\n", settings.settings_key));
    out.push_str(")\n");

    if settings.layer.enabled {
        out.push('\n');
        out.push_str("(deflayer nav\n");
        out.push_str("  caps");
        for (key, action) in &mappings {
            if settings.layer.chords_enabled && matches!(*key, "h" | "j" | "k" | "l") {
                out.push_str(&format!("  @ch-{key}"));
            } else {
                out.push_str(&format!("  {action}"));
            }
        }
        out.push_str("  (push-msg \"capsx:settings\")\n)\n");
    }

    Ok(out)
}

/// 校验配置文本，返回 kanata 的合并输出。
pub fn check_config(kanata: &Path, config: &Path, timeout: Duration) -> Result<String> {
    if !kanata.is_file() {
        return Err(Error::MissingKanata(kanata.to_path_buf()));
    }

    let mut command = Command::new(kanata);
    command
        .arg("--check")
        .arg("-c")
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    platform::hide_console(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| Error::Process(format!("启动 kanata --check 失败：{error}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = stdout.map(|mut stream| {
        thread::spawn(move || {
            let mut buffer = String::new();
            let _ = stream.read_to_string(&mut buffer);
            buffer
        })
    });
    let stderr_thread = stderr.map(|mut stream| {
        thread::spawn(move || {
            let mut buffer = String::new();
            let _ = stream.read_to_string(&mut buffer);
            buffer
        })
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Process(format!("等待 kanata --check 失败：{error}")));
            }
        }
    };

    let mut output = String::new();
    if let Some(handle) = stdout_thread {
        if let Ok(text) = handle.join() {
            output.push_str(&text);
        }
    }
    if let Some(handle) = stderr_thread {
        if let Ok(text) = handle.join() {
            output.push_str(&text);
        }
    }
    let output = strip_ansi(output.trim());

    match status {
        Some(status) if status.success() => {
            log::debug!("kanata --check 通过：{}", config.display());
            Ok(output)
        }
        Some(status) => Err(Error::ConfigCheck {
            status: status.code(),
            output: if output.is_empty() {
                format!("kanata --check 退出码 {:?}", status.code())
            } else {
                output
            },
        }),
        None => Err(Error::ConfigCheckTimeout(timeout.as_millis() as u64)),
    }
}

/// 安装结果。`backup` 用于 Reload 失败时回滚。
#[derive(Debug, Clone)]
pub struct InstalledConfig {
    /// 旧配置备份路径（之前不存在时为 `None`）。
    pub backup: Option<PathBuf>,
    /// 校验输出（成功时通常为空）。
    pub check_output: String,
}

/// 生成候选、校验、备份旧文件、原子替换。
///
/// 注意：Reload 在调用方完成；install 只负责文件层面。
pub fn install_checked(
    text: &str,
    destination: &Path,
    kanata: &Path,
    timeout: Duration,
) -> Result<InstalledConfig> {
    let candidate = append_suffix(destination, ".new");
    let backup_path = append_suffix(destination, ".bak");

    fs::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| Error::ConfigInstall("配置路径没有父目录".into()))?,
    )?;

    if let Err(error) = fs::write(&candidate, text.as_bytes()) {
        let _ = fs::remove_file(&candidate);
        return Err(Error::ConfigInstall(format!(
            "写入候选配置 {} 失败：{error}",
            candidate.display()
        )));
    }

    let check_output = match check_config(kanata, &candidate, timeout) {
        Ok(output) => output,
        Err(error) => {
            let _ = fs::remove_file(&candidate);
            return Err(error);
        }
    };

    let backup = if destination.exists() {
        if let Err(error) = fs::copy(destination, &backup_path) {
            let _ = fs::remove_file(&candidate);
            return Err(Error::ConfigInstall(format!(
                "备份旧配置 {} 失败：{error}",
                destination.display()
            )));
        }
        Some(backup_path)
    } else {
        None
    };

    if let Err(error) = fs::rename(&candidate, destination) {
        let _ = fs::remove_file(&candidate);
        return Err(Error::ConfigInstall(format!(
            "替换配置 {} 失败（原文件未动）：{error}",
            destination.display()
        )));
    }

    log::info!("已安装新配置：{}", destination.display());
    Ok(InstalledConfig {
        backup,
        check_output,
    })
}

/// 从 `.bak` 回滚，返回是否成功。
pub fn restore_backup(backup: &Path, destination: &Path) -> Result<()> {
    let bytes = fs::read(backup).map_err(|error| {
        Error::ConfigInstall(format!("读取备份 {} 失败：{error}", backup.display()))
    })?;
    crate::settings::atomic_write(destination, &bytes)?;
    log::warn!("已回滚配置：{}", destination.display());
    Ok(())
}

/// 把文本写到目标路径（用于启动时的初始配置）。
pub fn write_config_atomic(destination: &Path, text: &str) -> Result<()> {
    crate::settings::atomic_write(destination, text.as_bytes())
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path
        .file_name()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| OsString::from("keymap.kbd"));
    name.push(suffix);
    path.with_file_name(name)
}

/// 去掉 kanata 输出里的 ANSI 颜色码，避免在 egui 里显示 `[0m`。
fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// 常用超时：5 秒。
pub fn default_check_timeout() -> Duration {
    Duration::from_millis(CONFIG_CHECK_TIMEOUT_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn normalize(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    #[test]
    fn default_config_contains_fixed_columns() {
        let text = generate(&Settings::default()).expect("生成配置");
        assert!(text.contains("(defchords navchords 80"));
        assert!(text.contains("(h l) (macro C-left C-S-right)"));
        assert!(text.contains("ch-h (chord navchords h)"));
        assert!(text.contains("(tap-hold-press 200 200 caps (layer-while-held nav))"));
        assert!(text.contains("(push-msg \"capsx:settings\")"));
        assert!(text.contains(";;"), "注释必须是双分号");
        assert!(!text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with(';') && !trimmed.starts_with(";;")
        }));
    }

    #[test]
    fn chords_disabled_omits_sections() {
        let mut settings = Settings::default();
        settings.layer.chords_enabled = false;
        let text = generate(&settings).expect("生成配置");
        assert!(!text.contains("(defchords"));
        assert!(!text.contains("(defalias"));
        assert!(!text.contains("@ch-h"));
        assert!(text.contains("  left  down  up  right"));
    }

    #[test]
    fn layer_disabled_is_passthrough() {
        let mut settings = Settings::default();
        settings.layer.enabled = false;
        let text = generate(&settings).expect("生成配置");
        assert!(!text.contains("(deflayer nav"));
        assert!(!text.contains("tap-hold-press"));
        assert!(!text.contains("push-msg"));
        assert!(text.contains("(deflayer base\n  caps"));
    }

    #[test]
    fn extreme_timeouts_are_clamped() {
        let mut settings = Settings::default();
        settings.layer.tap_repress_ms = 0;
        settings.layer.hold_ms = 99_999;
        settings.layer.chord_ms = 0;
        let text = generate(&settings).expect("生成配置");
        assert!(text.contains("(defchords navchords 10"));
        assert!(text.contains("(tap-hold-press 50 1000 caps"));
    }

    #[test]
    fn custom_mapping_uses_fixed_order() {
        let mut settings = Settings::default();
        settings.mappings.insert("h".into(), "esc".into());
        settings.mappings.insert("t".into(), "ret".into());
        let text = generate(&settings).expect("生成配置");
        let defsrc = text
            .lines()
            .skip_while(|line| !line.starts_with("(defsrc"))
            .nth(1)
            .expect("defsrc 行");
        assert_eq!(
            defsrc.split_whitespace().collect::<Vec<_>>(),
            vec!["caps", "h", "j", "k", "l", "y", "o", "u", "i", "[", "]", "g", "t", ","]
        );
    }

    #[test]
    fn strips_ansi() {
        let text = "\u{1b}[34m[INFO]\u{1b}[0m ok";
        assert_eq!(strip_ansi(text), "[INFO] ok");
    }

    #[test]
    fn output_is_lf_only() {
        let text = generate(&Settings::default()).expect("生成配置");
        assert_eq!(normalize(&text), text);
    }

    #[test]
    fn restores_previous_config() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("capsx-restore-test-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).expect("创建临时目录");
        let destination = dir.join("keymap.kbd");
        let backup = dir.join("keymap.kbd.bak");
        fs::write(&backup, b"old-config\n").expect("写备份");
        fs::write(&destination, b"new-config\n").expect("写新配置");

        restore_backup(&backup, &destination).expect("回滚");
        assert_eq!(fs::read(&destination).expect("读回"), b"old-config\n");
        let _ = fs::remove_dir_all(dir);
    }
}
