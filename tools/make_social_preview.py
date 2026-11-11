"""CapsX GitHub social preview (Open Graph card) generator.

Composes a 1280x640 card from the wordmark and a real screenshot: dark
backdrop, mint glow behind the wordmark, rounded screenshot with a hairline
border. Everything is rendered at 2x and downscaled for clean edges.

Usage: python tools/make_social_preview.py
Output: assets/branding/social-preview.png

GitHub 的社交预览图只能从仓库 Settings -> Social preview 手动上传，
API 不支持设置，所以这里只负责产出文件。
"""
import os

from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORDMARK = os.path.join(ROOT, "assets", "logo", "capsx-wordmark.png")
SCREENSHOT = os.path.join(ROOT, "docs", "screenshots", "settings.png")
OUT_DIR = os.path.join(ROOT, "assets", "branding")
OUT = os.path.join(OUT_DIR, "social-preview.png")

# ---------------------------------------------------------------- palette
BG_TOP = (23, 30, 43)          # #171E2B，比 #11161F 略亮，避免死黑
BG_BOTTOM = (11, 15, 22)       # #0B0F16
ACCENT = (59, 227, 174)        # #3BE3AE
STROKE = (42, 53, 72)          # #2A3548

W, H = 1280, 640
SS = 2                         # supersample factor


def vertical_gradient(width, height, top, bottom):
    strip = Image.new("RGB", (1, height))
    px = strip.load()
    for y in range(height):
        t = y / max(height - 1, 1)
        px[0, y] = tuple(int(round(top[i] + (bottom[i] - top[i]) * t)) for i in range(3))
    return strip.resize((width, height), Image.BILINEAR)


def rounded_mask(size, radius):
    mask = Image.new("L", size, 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, size[0] - 1, size[1] - 1), radius=radius, fill=255
    )
    return mask


def glow(size_xy, center, radius, color, alpha):
    layer = Image.new("RGBA", size_xy, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    draw.ellipse(
        (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius),
        fill=color + (alpha,),
    )
    return layer.filter(ImageFilter.GaussianBlur(radius * 0.45))


def fit_width(image, width):
    scale = width / image.width
    return image.resize((int(width), max(1, int(image.height * scale))), Image.LANCZOS)


def fit_height(image, height):
    scale = height / image.height
    return image.resize((max(1, int(image.width * scale)), int(height)), Image.LANCZOS)


def main():
    canvas = vertical_gradient(W * SS, H * SS, BG_TOP, BG_BOTTOM).convert("RGBA")

    # 左侧品牌区后面的薄荷色光晕，让画面不是一整块纯色
    canvas.alpha_composite(glow((W * SS, H * SS), (300 * SS, 330 * SS), 340 * SS, ACCENT, 30))

    # 左侧：wordmark 卡片
    mark = fit_width(Image.open(WORDMARK).convert("RGBA"), 600 * SS)
    mark_x, mark_y = 64 * SS, (H * SS - mark.height) // 2
    canvas.alpha_composite(mark, (mark_x, mark_y))

    # 右侧：真实设置窗口截图，带圆角、投影和 1px 描边
    shot = fit_height(Image.open(SCREENSHOT).convert("RGBA"), 560 * SS)
    shot_x = W * SS - 64 * SS - shot.width
    shot_y = (H * SS - shot.height) // 2
    radius = 14 * SS
    mask = rounded_mask(shot.size, radius)

    shadow = Image.new("RGBA", (W * SS, H * SS), (0, 0, 0, 0))
    shadow.paste(
        (0, 0, 0, 150),
        (shot_x, shot_y + 14 * SS),
        mask,
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(18 * SS)))

    canvas.paste(shot, (shot_x, shot_y), mask)
    ImageDraw.Draw(canvas).rounded_rectangle(
        (shot_x, shot_y, shot_x + shot.width - 1, shot_y + shot.height - 1),
        radius=radius,
        outline=STROKE + (255,),
        width=SS,
    )

    out = canvas.resize((W, H), Image.LANCZOS).convert("RGB")

    os.makedirs(OUT_DIR, exist_ok=True)
    out.save(OUT, format="PNG", optimize=True)
    print(f"wrote {OUT} ({os.path.getsize(OUT):,} bytes, {out.width}x{out.height})")


if __name__ == "__main__":
    main()
