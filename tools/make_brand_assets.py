"""CapsX brand asset generator.

Renders the app icon (PNG/ICO) and the README wordmark from a single
geometry definition, supersampled 2x for clean anti-aliasing.

Usage: python tools/make_brand_assets.py
"""
from PIL import Image, ImageDraw, ImageFont
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "logo")
os.makedirs(os.path.join(OUT, "alt"), exist_ok=True)

# ---------------------------------------------------------------- palette
BG_TOP = (27, 34, 49)
BG_BOTTOM = (11, 15, 22)
CAP_TOP = (36, 45, 62)
CAP_BOTTOM = (26, 33, 48)
CAP_STROKE = (51, 64, 90)
DEPTH = (9, 12, 18)
ACCENT = (59, 227, 174)      # mint
ACCENT_AMBER = (255, 194, 75)

S = 2048          # master canvas (2x of 1024)
K = S / 1024.0    # scale factor from the 1024 design grid


def sc(v):
    return v * K


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def vertical_gradient(width, height, top, bottom):
    img = Image.new("RGB", (1, height))
    px = img.load()
    for y in range(height):
        px[0, y] = lerp(top, bottom, y / max(1, height - 1))
    return img.resize((width, height))


def rounded_mask(size, box, radius):
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).rounded_rectangle(box, radius=radius, fill=255)
    return m


def chevron(draw, apex, a1, a2, width, color):
    """A 45-degree chevron with round caps and a round joint."""
    w = int(round(width))
    r = w / 2.0
    draw.line([apex, a1], fill=color, width=w)
    draw.line([apex, a2], fill=color, width=w)
    for p in (apex, a1, a2):
        draw.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=color)


def render_icon(accent=ACCENT, nested=True, arrow_scale=1.28, stroke=50):
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    # tile
    tile_mask = rounded_mask(S, [0, 0, S - 1, S - 1], sc(228))
    img.paste(vertical_gradient(S, S, BG_TOP, BG_BOTTOM), (0, 0), tile_mask)

    if nested:
        cap = [sc(188), sc(188), sc(836), sc(836)]
        radius = sc(132)

        # keycap depth
        depth_layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        ImageDraw.Draw(depth_layer).rounded_rectangle(
            [cap[0], cap[1] + sc(18), cap[2], cap[3] + sc(18)], radius=radius, fill=DEPTH + (255,)
        )
        img.alpha_composite(depth_layer)

        # keycap face
        face_layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        cap_mask = rounded_mask(S, cap, radius)
        gradient = vertical_gradient(S, S, CAP_TOP, CAP_BOTTOM).convert("RGBA")
        gradient.putalpha(cap_mask)
        face_layer.alpha_composite(gradient)
        ImageDraw.Draw(face_layer).rounded_rectangle(
            cap, radius=radius, outline=CAP_STROKE + (255,), width=max(2, int(sc(4)))
        )
        img.alpha_composite(face_layer)
    else:
        # flat variant: subtle top highlight on the tile itself
        hl = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        ImageDraw.Draw(hl).rounded_rectangle(
            [sc(3), sc(3), S - sc(3), S - sc(3)], radius=sc(226),
            outline=(255, 255, 255, 18), width=max(2, int(sc(3)))
        )
        img.alpha_composite(hl)

    # arrow cluster (inverted-T nav cluster)
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    w = sc(stroke)

    def sxy(p):
        return (512 + (p[0] - 512) * arrow_scale, 512 + (p[1] - 512) * arrow_scale)

    arrows = [
        ((512, 388), (452, 448), (572, 448)),   # up
        ((330, 582), (390, 522), (390, 642)),   # left
        ((512, 642), (452, 582), (572, 582)),   # down
        ((694, 582), (634, 522), (634, 642)),   # right
    ]
    for apex, a1, a2 in arrows:
        apex, a1, a2 = sxy(apex), sxy(a1), sxy(a2)
        chevron(d, (sc(apex[0]), sc(apex[1])), (sc(a1[0]), sc(a1[1])), (sc(a2[0]), sc(a2[1])), w, accent + (255,))
    img.alpha_composite(layer)

    return img.resize((1024, 1024), Image.LANCZOS)


def save_icon_pngs(master, prefix="capsx-icon"):
    for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        master.resize((size, size), Image.LANCZOS).save(
            os.path.join(OUT, f"{prefix}-{size}.png")
        )
    master.resize((512, 512), Image.LANCZOS).save(
        os.path.join(OUT, f"{prefix}.png")
    )
    master.save(
        os.path.join(OUT, "capsx.ico"),
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


def render_wordmark():
    W, H = 1280, 400
    card = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    mask = Image.new("L", (W, H), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, H - 1], radius=52, fill=255)
    card.paste(vertical_gradient(W, H, (18, 23, 34), (10, 13, 20)).convert("RGBA"), (0, 0), mask)

    icon_size = 236
    card.alpha_composite(
        render_icon().resize((icon_size, icon_size), Image.LANCZOS),
        (56, (H - icon_size) // 2),
    )

    d = ImageDraw.Draw(card)
    title_font = ImageFont.truetype(r"C:\Windows\Fonts\segoeuib.ttf", 116)
    sub_font = ImageFont.truetype(r"C:\Windows\Fonts\msyhbd.ttc", 40)
    small_font = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 25)

    title_a, title_b = "Caps", "X"
    sub_text = "把 CapsLock 变成导航层"
    small_text = "CapsLock as a navigation layer  ·  powered by kanata"

    a_bbox = d.textbbox((0, 0), title_a, font=title_font)
    b_bbox = d.textbbox((0, 0), title_b, font=title_font)
    sub_bbox = d.textbbox((0, 0), sub_text, font=sub_font)
    small_bbox = d.textbbox((0, 0), small_text, font=small_font)

    a_h = a_bbox[3] - a_bbox[1]
    sub_h = sub_bbox[3] - sub_bbox[1]
    small_h = small_bbox[3] - small_bbox[1]
    gap1, gap2 = 20, 14
    total = a_h + gap1 + sub_h + gap2 + small_h
    y = (H - total) // 2
    x = 334

    d.text((x - a_bbox[0], y - a_bbox[1]), title_a, font=title_font, fill=(242, 245, 250))
    d.text((x - a_bbox[0] + (a_bbox[2] - a_bbox[0]), y - b_bbox[1]), title_b,
           font=title_font, fill=ACCENT)
    y += a_h + gap1
    d.text((x - sub_bbox[0], y - sub_bbox[1]), sub_text, font=sub_font, fill=(158, 168, 188))
    y += sub_h + gap2
    d.text((x - small_bbox[0], y - small_bbox[1]), small_text, font=small_font, fill=(104, 114, 134))

    card.save(os.path.join(OUT, "capsx-wordmark.png"))
    return card


if __name__ == "__main__":
    mint = render_icon()
    save_icon_pngs(mint)
    amber = render_icon(ACCENT_AMBER)
    amber.resize((1024, 1024), Image.LANCZOS).save(
        os.path.join(OUT, "alt", "capsx-icon-1024-amber.png")
    )
    amber.resize((512, 512), Image.LANCZOS).save(
        os.path.join(OUT, "alt", "capsx-icon-512-amber.png")
    )
    render_wordmark()
    print("done:", OUT)

