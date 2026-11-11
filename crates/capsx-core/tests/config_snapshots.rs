//! 配置生成快照与真实 `kanata --check` 集成测试。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use capsx_core::config_gen::{check_config, generate};
use capsx_core::settings::Settings;

struct Case {
    name: &'static str,
    settings: Settings,
}

fn cases() -> Vec<Case> {
    let default = Settings::default();

    let mut no_chords = Settings::default();
    no_chords.layer.chords_enabled = false;

    let mut custom = Settings::default();
    custom
        .mappings
        .insert("h".into(), "esc".into())
        .expect("insert h");
    custom
        .mappings
        .insert("t".into(), "ret".into())
        .expect("insert t");
    custom
        .mappings
        .insert("y".into(), "C-home".into())
        .expect("insert y");

    let mut extreme = Settings::default();
    extreme.layer.tap_repress_ms = 0;
    extreme.layer.hold_ms = 99_999;
    extreme.layer.chord_ms = 0;

    let mut disabled = Settings::default();
    disabled.layer.enabled = false;

    vec![
        Case {
            name: "default",
            settings: default,
        },
        Case {
            name: "chords_disabled",
            settings: no_chords,
        },
        Case {
            name: "custom_mapping",
            settings: custom,
        },
        Case {
            name: "extreme_timeouts",
            settings: extreme,
        },
        Case {
            name: "layer_disabled",
            settings: disabled,
        },
    ]
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

#[test]
fn generated_configs_match_snapshots() {
    let update = std::env::var("CAPSX_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");
    fs::create_dir_all(snapshot_dir()).expect("创建快照目录");

    for case in cases() {
        let actual = generate(&case.settings)
            .unwrap_or_else(|error| panic!("生成 {} 配置失败：{error}", case.name));
        let path = snapshot_dir().join(format!("{}.kbd", case.name));
        if update {
            fs::write(&path, actual.as_bytes())
                .unwrap_or_else(|error| panic!("写入快照 {} 失败：{error}", path.display()));
        }
        let expected = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读取快照 {} 失败：{error}", path.display()));
        assert_eq!(
            expected, actual,
            "快照不一致：{}（可设置 CAPSX_UPDATE_SNAPSHOTS=1 更新）",
            case.name
        );
        assert!(
            !actual.lines().any(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with(';') && !trimmed.starts_with(";;")
            }),
            "{} 使用了单分号注释",
            case.name
        );
    }
}

#[test]
#[ignore = "需要 runtime/kanata.exe；执行 cargo test -- --ignored"]
fn generated_configs_pass_real_kanata_check() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace 根");
    let kanata = workspace.join("runtime").join("kanata.exe");
    assert!(
        kanata.is_file(),
        "缺少 {}，先运行 tools/fetch-kanata.ps1",
        kanata.display()
    );

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temp =
        std::env::temp_dir().join(format!("capsx-kanata-check-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&temp).expect("创建临时目录");

    for case in cases() {
        let text = generate(&case.settings)
            .unwrap_or_else(|error| panic!("生成 {} 配置失败：{error}", case.name));
        let path = temp.join(format!("{}.kbd", case.name));
        fs::write(&path, text).expect("写配置");
        let output = check_config(
            &kanata,
            &path,
            capsx_core::config_gen::default_check_timeout(),
        )
        .unwrap_or_else(|error| panic!("{} 未通过 kanata --check：{error}", case.name));
        assert!(
            !output.contains("ERROR"),
            "{} 的 kanata 输出含 ERROR：\n{output}",
            case.name
        );
    }

    let _ = fs::remove_dir_all(temp);
}
