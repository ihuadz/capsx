//! 用户设置模型、校验、版本迁移与原子持久化。

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::constants::DEFAULT_SETTINGS_KEY;
use crate::error::{Error, Result};

/// 当前设置文件版本。
pub const CURRENT_VERSION: u32 = 1;

/// `defsrc` 中固定顺序的 12 个导航键。
pub const MAPPING_KEYS: [&str; 12] = ["h", "j", "k", "l", "y", "o", "u", "i", "[", "]", "g", "t"];

/// 允许映射到的 kanata 动作白名单。
pub const MAPPING_ACTIONS: &[&str] = &[
    "left", "down", "up", "right", "home", "end", "pgup", "pgdn", "S-tab", "tab", "ret", "del",
    "esc", "bspc", "C-left", "C-right", "C-home", "C-end", "S-left", "S-right", "S-home", "S-end",
];

/// 托盘设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TraySettings {
    /// 是否显示托盘图标。
    pub visible: bool,
    /// 保留未识别的未来字段。
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            visible: true,
            extra: BTreeMap::new(),
        }
    }
}

/// 导航层与和弦设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayerSettings {
    /// 轻点 CapsLock 的判定时间（毫秒）。
    pub tap_repress_ms: u32,
    /// 长按 CapsLock 的判定时间（毫秒）。
    pub hold_ms: u32,
    /// 和弦窗口（毫秒）。
    pub chord_ms: u32,
    /// 是否启用 `h+l` / `k+j` 和弦。
    pub chords_enabled: bool,
    /// 是否启用导航层。
    pub enabled: bool,
    /// 保留未识别的未来字段。
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self {
            tap_repress_ms: 200,
            hold_ms: 200,
            chord_ms: 80,
            chords_enabled: true,
            enabled: true,
            extra: BTreeMap::new(),
        }
    }
}

/// CapsX 设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// 设置文件版本。
    pub version: u32,
    /// 托盘设置。
    pub tray: TraySettings,
    /// 是否开机自启。
    pub autostart: bool,
    /// 导航层设置。
    pub layer: LayerSettings,
    /// 导航键到 kanata 动作的映射。
    pub mappings: BTreeMap<String, String>,
    /// `CapsLock + <settingsKey>` 打开设置。
    pub settings_key: String,
    /// 保留未识别的未来字段。
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        let mappings = MAPPING_KEYS
            .iter()
            .map(|key| ((*key).to_string(), default_action(key).to_string()))
            .collect();
        Self {
            version: CURRENT_VERSION,
            tray: TraySettings::default(),
            autostart: true,
            layer: LayerSettings::default(),
            mappings,
            settings_key: DEFAULT_SETTINGS_KEY.to_string(),
            extra: BTreeMap::new(),
        }
    }
}

/// 读取设置文件失败的原因。
#[derive(Debug, Error)]
pub enum SettingsLoadError {
    /// 文件读写失败。
    #[error("读取设置失败：{0}")]
    Io(#[from] std::io::Error),
    /// JSON 解析失败。
    #[error("设置文件不是合法 JSON：{0}")]
    Json(#[from] serde_json::Error),
    /// 文件版本高于当前程序。
    #[error("设置文件版本为 {found}，本程序只支持到 {supported}；已保留原文件")]
    FutureVersion {
        /// 文件版本。
        found: u32,
        /// 支持的版本。
        supported: u32,
    },
}

impl Settings {
    /// 读取设置；文件不存在时返回 `None`。
    pub fn load_optional(path: &Path) -> std::result::Result<Option<Self>, SettingsLoadError> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(SettingsLoadError::Io(error)),
        };

        let value: Value = serde_json::from_slice(&data)?;
        // 先检查 version，绝不因为“文件更新”而覆盖它。
        if let Some(version) = value.get("version").and_then(Value::as_u64) {
            if version > u64::from(CURRENT_VERSION) {
                return Err(SettingsLoadError::FutureVersion {
                    found: version.min(u64::from(u32::MAX)) as u32,
                    supported: CURRENT_VERSION,
                });
            }
        }

        let mut settings: Settings = serde_json::from_value(value)?;
        if settings.version < CURRENT_VERSION {
            settings.version = CURRENT_VERSION;
        }
        Ok(Some(settings))
    }

    /// 读取设置，缺失时使用默认值。
    pub fn load_or_default(path: &Path) -> std::result::Result<Self, SettingsLoadError> {
        Ok(Self::load_optional(path)?.unwrap_or_default())
    }

    /// 校验关键字段（值白名单、设置键安全性、列冲突）。
    pub fn validate(&self) -> Result<()> {
        if !is_valid_key_token(&self.settings_key) {
            return Err(Error::InvalidSettings(format!(
                "设置键 {:?} 不是合法的 kanata 键名",
                self.settings_key
            )));
        }
        if self.settings_key == "caps" {
            return Err(Error::InvalidSettings(
                "设置键不能是 caps（会与导航层前缀冲突）".into(),
            ));
        }
        if MAPPING_KEYS.contains(&self.settings_key.as_str()) {
            return Err(Error::InvalidSettings(format!(
                "设置键 {:?} 与导航键重复，会造成 defsrc 列冲突",
                self.settings_key
            )));
        }
        for key in MAPPING_KEYS {
            if let Some(action) = self.mappings.get(key) {
                if !MAPPING_ACTIONS.contains(&action.as_str()) {
                    return Err(Error::InvalidSettings(format!(
                        "键 {key} 的动作 {action:?} 不在白名单中"
                    )));
                }
            }
        }
        Ok(())
    }

    /// 某个固定导航键对应的动作；缺失时返回内置默认值。
    pub fn action_for(&self, key: &str) -> &str {
        self.mappings
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| default_action(key))
    }

    /// 将设置序列化为 pretty JSON（以换行结尾）。
    pub fn to_json(&self) -> Result<Vec<u8>> {
        let mut data = serde_json::to_vec_pretty(self)?;
        data.push(b'\n');
        Ok(data)
    }

    /// 原子写入设置文件。
    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        atomic_write(path, &self.to_json()?)
    }

    /// 与默认设置合并出完整映射（用于配置生成）。
    pub fn complete_mappings(&self) -> Vec<(&'static str, &str)> {
        MAPPING_KEYS
            .iter()
            .map(|key| (*key, self.action_for(key)))
            .collect()
    }
}

/// 每个导航键的默认动作。
pub fn default_action(key: &str) -> &'static str {
    match key {
        "h" => "left",
        "j" => "down",
        "k" => "up",
        "l" => "right",
        "y" => "home",
        "o" => "end",
        "u" => "pgdn",
        "i" => "pgup",
        "[" => "S-tab",
        "]" => "tab",
        "g" => "ret",
        "t" => "del",
        _ => "left",
    }
}

/// 是否是白名单中的动作。
pub fn is_allowed_action(action: &str) -> bool {
    MAPPING_ACTIONS.contains(&action)
}

/// 校验一个键名可以安全地放进 S 表达式。
///
/// 这里的限制比 kanata 语法更保守，避免用户从 JSON 注入括号/引号/注释。
pub fn is_valid_key_token(token: &str) -> bool {
    if token.is_empty() || token.len() > 16 || token.starts_with('@') {
        return false;
    }
    token.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(ch, '[' | ']' | ',' | '.' | '/' | '-' | '=' | '`' | '\'')
    })
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 原子写文件：同目录临时文件 + rename。
///
/// Windows 上 `std::fs::rename` 使用可替换目标的语义；若目标被占用会失败，
/// 此时原文件保持不变。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidSettings(format!("路径没有父目录：{}", path.display())))?;
    fs::create_dir_all(parent)?;

    let temp = temp_path(path);
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "capsx".into());
    path.with_file_name(format!(".{file_name}.tmp-{}-{counter}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "capsx-settings-test-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("创建测试目录");
        dir
    }

    #[test]
    fn default_matches_contract() {
        let settings = Settings::default();
        assert_eq!(settings.version, 1);
        assert!(settings.tray.visible);
        assert!(settings.autostart);
        assert!(settings.layer.enabled);
        assert!(settings.layer.chords_enabled);
        assert_eq!(settings.layer.tap_repress_ms, 200);
        assert_eq!(settings.layer.hold_ms, 200);
        assert_eq!(settings.layer.chord_ms, 80);
        assert_eq!(settings.settings_key, ",");
        assert_eq!(settings.action_for("h"), "left");
        assert_eq!(settings.action_for("t"), "del");
        assert_eq!(settings.complete_mappings().len(), 12);
    }

    #[test]
    fn unknown_fields_survive_round_trip() {
        let source = r#"{
            "version": 1,
            "tray": { "visible": false, "futureTray": 1 },
            "autostart": false,
            "layer": { "holdMs": 250, "futureLayer": { "x": true } },
            "mappings": { "h": "left" },
            "settingsKey": ",",
            "futureTop": [1, 2, 3]
        }"#;
        let settings: Settings = serde_json::from_str(source).expect("解析设置");
        assert_eq!(settings.tray.extra.get("futureTray"), Some(&Value::from(1)));
        assert!(settings.layer.extra.contains_key("futureLayer"));
        assert!(settings.extra.contains_key("futureTop"));
        assert_eq!(settings.layer.hold_ms, 250);
        assert_eq!(settings.action_for("j"), "down");

        let json = serde_json::to_value(&settings).expect("序列化");
        assert_eq!(json["futureTop"][2], 3);
        assert_eq!(json["tray"]["futureTray"], 1);
        assert_eq!(json["layer"]["futureLayer"]["x"], true);
    }

    #[test]
    fn future_version_is_preserved() {
        let dir = temp_dir("future");
        let path = dir.join("settings.json");
        let source = r#"{"version":99,"future":"keep me"}"#;
        fs::write(&path, source).expect("写设置");

        let error = Settings::load_optional(&path).expect_err("未来版本应被拒绝");
        assert!(matches!(
            error,
            SettingsLoadError::FutureVersion {
                found: 99,
                supported: 1
            }
        ));
        assert_eq!(fs::read_to_string(&path).expect("读回"), source);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_version_migrates_to_current() {
        let settings: Settings = serde_json::from_str(r#"{"autostart":false}"#).expect("解析");
        // 反序列化时 `#[serde(default)]` 会填当前版本。
        assert_eq!(settings.version, CURRENT_VERSION);
        assert!(!settings.autostart);
    }

    #[test]
    fn old_version_is_migrated_on_load() {
        let dir = temp_dir("migrate");
        let path = dir.join("settings.json");
        fs::write(&path, br#"{"version":0,"autostart":false}"#).expect("写设置");
        let loaded = Settings::load_optional(&path)
            .expect("加载")
            .expect("文件存在");
        assert_eq!(loaded.version, CURRENT_VERSION);
        assert!(!loaded.autostart);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unsafe_settings_key() {
        let mut settings = Settings {
            settings_key: "h".into(),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        settings.settings_key = "x)".into();
        assert!(settings.validate().is_err());
        settings.settings_key = "f1".into();
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn rejects_unknown_action() {
        let mut settings = Settings::default();
        settings.mappings.insert("h".into(), "(macro bad)".into());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn atomic_write_replaces_existing() {
        let dir = temp_dir("atomic");
        let path = dir.join("settings.json");
        fs::write(&path, b"old").expect("写旧文件");
        atomic_write(&path, b"new").expect("原子写");
        assert_eq!(fs::read(&path).expect("读"), b"new");
        let leftovers = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_json_is_error_not_panic() {
        let dir = temp_dir("invalid");
        let path = dir.join("settings.json");
        fs::write(&path, b"{ nope").expect("写坏文件");
        assert!(matches!(
            Settings::load_optional(&path),
            Err(SettingsLoadError::Json(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }
}
