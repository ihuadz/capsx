#!/usr/bin/env python3
"""从 Noto Sans SC 可变量字体生成 CapsX 的 CJK 子集。

用法：
    python tools/make_font_subset.py [--source cache/fonts/NotoSansSC-VF.ttf]

注意：先把新 UI 文案写进 Rust 源码，再运行本脚本；脚本会自动收集
crates/capsx-app 与 crates/capsx-core 里出现的全部字符。
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "crates" / "capsx-app" / "assets" / "fonts" / "NotoSansSC-Regular-subset.ttf"
DEFAULT_SOURCE = ROOT / "cache" / "fonts" / "NotoSansSC-VF.ttf"
EXTRA_CHARS = (
    "←↑→↓●○✓✕×·—…“”‘’《》「」『』、，。！？：；（）【】"
    " CapsX HJKL CapsLock kanata egui"
)


def collect_chars() -> str:
    chars = set(chr(code) for code in range(0x20, 0x7F))
    chars.update(EXTRA_CHARS)
    for directory in (ROOT / "crates" / "capsx-app", ROOT / "crates" / "capsx-core"):
        for path in directory.rglob("*.rs"):
            chars.update(path.read_text(encoding="utf-8"))
    return "".join(sorted(chars))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()

    if not args.source.is_file():
        print(f"找不到源字体：{args.source}", file=sys.stderr)
        print("请先运行：", file=sys.stderr)
        print(
            "  curl -L -o cache/fonts/NotoSansSC-VF.ttf "
            "'https://github.com/google/fonts/raw/main/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf'",
            file=sys.stderr,
        )
        return 1

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    chars = collect_chars()
    print(f"收集到 {len(chars)} 个字符")

    with tempfile.TemporaryDirectory(prefix="capsx-font-") as temp:
        temp = Path(temp)
        text_file = temp / "chars.txt"
        text_file.write_text(chars, encoding="utf-8")

        static_font = temp / "NotoSansSC-Regular.ttf"
        instancer = [
            sys.executable,
            "-m",
            "fontTools.varLib.instancer",
            str(args.source),
            "wght=400",
            "--output",
            str(static_font),
        ]
        print("固定字重 400 ...")
        subprocess.run(instancer, check=True)

        subset = [
            sys.executable,
            "-m",
            "fontTools.subset",
            str(static_font),
            f"--text-file={text_file}",
            f"--output-file={OUTPUT}",
            "--layout-features=*",
            "--glyph-names",
            "--symbol-cmap",
            "--legacy-cmap",
            "--notdef-glyph",
            "--notdef-outline",
            "--recommended-glyphs",
            "--name-IDs=*",
            "--name-legacy",
            "--name-languages=*",
        ]
        print("生成子集 ...")
        subprocess.run(subset, check=True)

    size = OUTPUT.stat().st_size
    print(f"已写入 {OUTPUT}")
    print(f"大小：{size / 1024 / 1024:.2f} MiB（{size} 字节）")
    if size > 1_500_000:
        print("警告：字体子集超过 1.5 MB 目标", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
