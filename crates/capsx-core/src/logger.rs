//! 极简滚动日志。
//!
//! 约束：不引入 `tracing`/`env_logger`；只用一个互斥锁、一个文件句柄。
//! 路径：`<cache_dir>/logs/capsx-YYYYMMDD.log`，单文件 5 MB，保留 7 天。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

use crate::constants::{LOG_MAX_BYTES, LOG_RETENTION_DAYS};
use crate::error::{Error, Result};
use crate::platform;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

struct FileLogger {
    dir: PathBuf,
    state: Mutex<LogState>,
}

#[derive(Default)]
struct LogState {
    file: Option<File>,
    date: String,
    bytes: u64,
}

impl FileLogger {
    fn write_line(&self, level: log::Level, target: &str, args: std::fmt::Arguments<'_>) {
        let now = platform::local_now();
        let date = format!("{:04}{:02}{:02}", now.year, now.month, now.day);
        let line = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {:<5} {}{}\n",
            now.year,
            now.month,
            now.day,
            now.hour,
            now.minute,
            now.second,
            now.millis,
            level.as_str(),
            if target.is_empty() {
                String::new()
            } else {
                format!("{target}: ")
            },
            args
        );

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };

        if state.date != date || state.file.is_none() {
            if let Err(error) = state.open(&self.dir, &date) {
                // 日志系统自身失败时不能再写日志，避免递归。
                let _ = error;
                return;
            }
        }

        if state.bytes + line.len() as u64 > LOG_MAX_BYTES {
            if let Err(error) = state.rotate(&self.dir, &date) {
                let _ = error;
            }
            if let Err(error) = state.open(&self.dir, &date) {
                let _ = error;
                return;
            }
        }

        if let Some(file) = state.file.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() {
                let _ = file.flush();
                state.bytes += line.len() as u64;
            }
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        self.write_line(record.level(), record.target(), *record.args());
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

impl LogState {
    fn open(&mut self, dir: &Path, date: &str) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("capsx-{date}.log"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        self.bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        self.file = Some(file);
        self.date = date.to_string();
        Ok(())
    }

    fn rotate(&mut self, dir: &Path, date: &str) -> std::io::Result<()> {
        self.file = None;
        let current = dir.join(format!("capsx-{date}.log"));
        let rotated = dir.join(format!("capsx-{date}.1.log"));
        if rotated.exists() {
            fs::remove_file(&rotated)?;
        }
        if current.exists() {
            fs::rename(&current, &rotated)?;
        }
        self.bytes = 0;
        Ok(())
    }
}

/// 初始化全局日志器。
///
/// 重复调用只会调整日志级别（用于测试或后续 CLI）。
pub fn init(dir: &Path, level: LevelFilter) -> Result<()> {
    fs::create_dir_all(dir)?;
    cleanup_old(dir)?;

    let logger = LOGGER.get_or_init(|| FileLogger {
        dir: dir.to_path_buf(),
        state: Mutex::new(LogState::default()),
    });

    // 如果宿主程序（理论上没有）已经安装过 logger，不应该因此让 CapsX 起不来。
    if log::set_logger(logger).is_err() {
        // 已安装其它 logger：保留它，只调整级别。
    }
    log::set_max_level(level);
    Ok(())
}

/// 刷新日志文件。
pub fn flush() {
    if let Some(logger) = LOGGER.get() {
        logger.flush();
    }
}

/// 记录 panic；`panic = "abort"` 下这是最后一条日志。
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("未捕获的 panic：{info}");
        flush();
        default_hook(info);
    }));
}

fn cleanup_old(dir: &Path) -> Result<()> {
    let now = platform::local_now();
    let today = days_from_civil(now.year, now.month, now.day);
    let cutoff = today - LOG_RETENTION_DAYS as i64;

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::Io(error)),
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(date) = parse_log_date(&name) else {
            continue;
        };
        if date < cutoff {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn parse_log_date(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("capsx-")?;
    if rest.len() < 8 {
        return None;
    }
    let digits = &rest[..8];
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = digits[0..4].parse::<i32>().ok()?;
    let month = digits[4..6].parse::<u32>().ok()?;
    let day = digits[6..8].parse::<u32>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy;
    era as i64 * 146_097 + doe - 719_468
}

/// UTC 时间戳的日期 key，供未来非 Windows 日志使用。
#[allow(dead_code)]
fn today_utc() -> i64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    days_from_civil(year, month, day)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_round_trip() {
        for (year, month, day) in [(1970, 1, 1), (2000, 2, 29), (2026, 11, 10), (2038, 1, 19)] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day));
        }
    }

    #[test]
    fn parses_log_names() {
        assert!(parse_log_date("capsx-20261110.log").is_some());
        assert!(parse_log_date("capsx-20261110.1.log").is_some());
        assert!(parse_log_date("other-20261110.log").is_none());
        assert!(parse_log_date("capsx-20269999.log").is_none());
    }
}
