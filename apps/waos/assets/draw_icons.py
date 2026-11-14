"""Draws WaOS's icons with Pillow, supersampled: the macOS tile, the Android foreground, and the default program icon."""
import sys
from PIL import Image, ImageDraw, ImageFilter

SS = 4  # supersampling
WAVES = [  # (opacity, start, [(c1, c2, end), ...]) in the site's 120-unit space
    (0.25, (0, 52), [((24, 30), (46, 70), (70, 46)), ((94, 22), (104, 26), (120, 36))]),
    (0.50, (0, 68), [((26, 44), (46, 86), (72, 60)), ((98, 34), (106, 40), (120, 50))]),
    (1.00, (0, 84), [((26, 60), (48, 104), (74, 78)), ((100, 52), (108, 58), (120, 66))]),
]
STOPS = [(0.0, (0x3B, 0x82, 0xF6)), (0.55, (0xD9, 0x46, 0xEF)), (1.0, (0xFB, 0x92, 0x3C))]


def cubic(p0, p1, p2, p3, steps=200):
    for i in range(steps + 1):
        t = i / steps
        u = 1 - t
        yield (u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
               u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1])


def gradient(width, height):
    row = Image.new("RGB", (width, 1))
    for x in range(width):
        f = x / (width - 1)
        for (a, ca), (b, cb) in zip(STOPS, STOPS[1:]):
            if a <= f <= b:
                t = (f - a) / (b - a)
                row.putpixel((x, 0), tuple(round(ca[i] + (cb[i] - ca[i]) * t) for i in range(3)))
                break
    return row.resize((width, height))


def waves(size, origin, scale):
    """The three waves on a transparent square of `size`, the 120-unit space placed at `origin` and scaled by `scale`."""
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    fill = gradient(size, size)
    for opacity, start, curves in WAVES:
        points = [start]
        for c1, c2, end in curves:
            points.extend(cubic(points[-1], c1, c2, end))
        points += [(120, 120), (0, 120)]
        placed = [(origin[0] + x * scale, origin[1] + y * scale) for x, y in points]
        placed += [(origin[0] + 120 * scale, size * 2), (origin[0], size * 2)]
        mask = Image.new("L", (size, size), 0)
        ImageDraw.Draw(mask).polygon(placed, fill=round(255 * opacity))
        layer = fill.convert("RGBA")
        layer.putalpha(mask)
        out = Image.alpha_composite(out, layer)
    return out


def macos_tile(px=1024):
    size = px * SS
    inset, side, radius = 100 * SS, 824 * SS, 185 * SS
    tile = Image.new("L", (size, size), 0)
    ImageDraw.Draw(tile).rounded_rectangle([inset, inset, inset + side, inset + side], radius, fill=255)
    shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    shadow_mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(shadow_mask).rounded_rectangle([inset, inset + 10 * SS, inset + side, inset + side + 10 * SS], radius, fill=round(255 * 0.28))
    shadow.putalpha(shadow_mask.filter(ImageFilter.GaussianBlur(14 * SS)))
    face = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    face = Image.alpha_composite(face, waves(size, (inset, inset), side / 120))
    face.putalpha(tile)
    out = Image.alpha_composite(shadow, face)
    return out.resize((px, px), Image.LANCZOS)


def android_foreground(px=432):
    """Android's adaptive-icon foreground: 108 dp, the waves edge to edge across it."""
    size = px * SS
    out = waves(size, (0, 0), size / 120)
    return out.resize((px, px), Image.LANCZOS)


def blueprint(px=1024):
    """The default program icon (B1): Apple's icon-template lines on blueprint blue, edge to edge."""
    size = px * SS
    k = size / 120
    out = Image.new("RGBA", (size, size), (0x1F, 0x5F, 0xAE, 255))
    lines = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(lines)
    faint = (255, 255, 255, round(255 * 0.16))
    for v in range(24, 108, 12):
        draw.line([(12 * k, v * k), (108 * k, v * k)], fill=faint, width=round(0.6 * k))
        draw.line([(v * k, 12 * k), (v * k, 108 * k)], fill=faint, width=round(0.6 * k))
    out = Image.alpha_composite(out, lines)
    lines = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(lines)
    bright = (255, 255, 255, round(255 * 0.8))
    w = round(1.0 * k)
    for r in (38, 22):
        draw.ellipse([(60 - r) * k, (60 - r) * k, (60 + r) * k, (60 + r) * k], outline=bright, width=w)
    draw.rounded_rectangle([22 * k, 22 * k, 98 * k, 98 * k], 3 * k, outline=bright, width=w)
    for a, b in (((14, 14), (106, 106)), ((106, 14), (14, 106)), ((60, 12), (60, 108)), ((12, 60), (108, 60))):
        draw.line([(a[0] * k, a[1] * k), (b[0] * k, b[1] * k)], fill=bright, width=w)
    out = Image.alpha_composite(out, lines)
    return out.resize((px, px), Image.LANCZOS)


if __name__ == "__main__":
    folder = sys.argv[1]
    macos_tile().save(f"{folder}/icon.png")
    android_foreground().save(f"{folder}/android-foreground.png")
    blueprint().save(f"{folder}/program-default.png")
