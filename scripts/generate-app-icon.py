#!/usr/bin/env python3
# DEVWIKI: dev_wiki 应用图标生成器 —— 黑底「星辰大海」银河主题（2026-06-10 换肤）。
# 纯 PIL 程序化绘制，确定性 seed，可随时重跑再生。
# 用法：python3 scripts/generate-app-icon.py
#   产物: build/app-icon-1024.png  (圆角方块, 喂给 `npm run tauri icon`)
#         build/app-icon-square.png (无圆角全幅, 派生 sidebar logo / favicon)
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
rng = random.Random(20260610)

OUT_DIR = Path(__file__).resolve().parent.parent / "build"
OUT_DIR.mkdir(exist_ok=True)


def screen_blend(base: Image.Image, layer: Image.Image) -> Image.Image:
    """screen 混合：1-(1-a)(1-b)，叠加发光层不丢暗部。"""
    from PIL import ImageChops
    return ImageChops.screen(base, layer)


def radial_glow(size: int, cx: float, cy: float, radius: float,
                color: tuple, peak: float) -> Image.Image:
    """单个柔和径向光晕层（RGB，黑底）。"""
    s = size // 4  # 低分辨率画再放大模糊，省时且更柔
    im = Image.new("RGB", (s, s), (0, 0, 0))
    d = ImageDraw.Draw(im)
    cx_, cy_, r_ = cx / 4, cy / 4, radius / 4
    steps = 48
    for i in range(steps, 0, -1):
        t = i / steps
        a = peak * (1 - t) ** 2.2
        col = tuple(int(c * a) for c in color)
        d.ellipse([cx_ - r_ * t, cy_ - r_ * t, cx_ + r_ * t, cy_ + r_ * t], fill=col)
    im = im.resize((size, size), Image.LANCZOS)
    return im.filter(ImageFilter.GaussianBlur(size // 40))


def draw_star(d: ImageDraw.ImageDraw, x: float, y: float, r: float,
              color: tuple, spikes: bool = False, img_size: int = SIZE):
    """一颗星：核心圆点 + 可选衍射光芒（interstellar 感）。"""
    if spikes:
        L = r * 14
        w = max(1, int(r * 0.7))
        for ang in (0, 90):
            a = math.radians(ang)
            dx, dy = math.cos(a) * L, math.sin(a) * L
            # 渐隐光芒：分段画，远端更暗
            segs = 10
            for s in range(segs):
                t0, t1 = s / segs, (s + 1) / segs
                fade = (1 - t0) ** 2
                col = tuple(int(c * fade) for c in color)
                d.line([x - dx * t1, y - dy * t1, x - dx * t0, y - dy * t0], fill=col, width=w)
                d.line([x + dx * t0, y + dy * t0, x + dx * t1, y + dy * t1], fill=col, width=w)
    d.ellipse([x - r, y - r, x + r, y + r], fill=color)


def build_galaxy() -> Image.Image:
    """全幅 1024×1024 黑底银河（无圆角）。"""
    img = Image.new("RGB", (SIZE, SIZE), (2, 2, 6))  # 近黑微蓝的深空底

    cx, cy = SIZE * 0.46, SIZE * 0.52  # 银河中心略偏左下，构图更生动

    # ---- 1. 深空星尘底噪：远景微弱小星 ----
    d = ImageDraw.Draw(img)
    for _ in range(900):
        x, y = rng.uniform(0, SIZE), rng.uniform(0, SIZE)
        v = rng.randint(18, 70)
        tint = rng.choice([(v, v, v), (v, v, min(255, v + 18)), (min(255, v + 12), v, v)])
        r = rng.uniform(0.4, 1.1)
        d.ellipse([x - r, y - r, x + r, y + r], fill=tint)
    img = img.filter(ImageFilter.GaussianBlur(0.6))

    # ---- 2. 星云：几团彼此呼应的冷色光晕 ----
    nebulae = [
        (SIZE * 0.78, SIZE * 0.22, SIZE * 0.55, (80, 80, 200), 0.85),   # 右上 紫蓝
        (SIZE * 0.20, SIZE * 0.78, SIZE * 0.50, (40, 120, 180), 0.80),  # 左下 青蓝
        (SIZE * 0.85, SIZE * 0.80, SIZE * 0.40, (130, 60, 180), 0.65),  # 右下 紫
        (cx, cy, SIZE * 0.80, (55, 60, 120), 0.70),                     # 中心冷雾托底
    ]
    for nx, ny, nr, ncol, npk in nebulae:
        img = screen_blend(img, radial_glow(SIZE, nx, ny, nr, ncol, npk))

    # ---- 3. 旋臂：对数螺线上撒星，密度沿臂衰减 ----
    arm_layer = Image.new("RGB", (SIZE, SIZE), (0, 0, 0))
    ad = ImageDraw.Draw(arm_layer)
    arms = 2
    a0, b = SIZE * 0.045, 0.30  # r = a0 * e^(b*theta)
    for arm in range(arms):
        phase = arm * math.pi
        for i in range(5200):
            t = rng.uniform(0, 3.4 * math.pi)
            r_spiral = a0 * math.exp(b * t)
            if r_spiral > SIZE * 0.52:
                continue
            # 臂宽随半径增大，靠核处致密（收紧让螺旋纹理可辨）
            spread = r_spiral * 0.085 + SIZE * 0.005
            theta = t + phase
            x = cx + r_spiral * math.cos(theta) + rng.gauss(0, spread)
            y = cy + r_spiral * 0.78 * math.sin(theta) + rng.gauss(0, spread * 0.85)  # 0.78 椭圆倾角
            if not (0 <= x < SIZE and 0 <= y < SIZE):
                continue
            # 颜色：核区暖白 → 臂中蓝白 → 臂梢偏蓝紫
            frac = min(1.0, r_spiral / (SIZE * 0.5))
            warm = (255, 235, 200)
            cool = (150, 180, 255)
            col = tuple(int(warm[k] * (1 - frac) + cool[k] * frac) for k in range(3))
            bright = rng.uniform(0.45, 1.0) * (1 - frac * 0.30)
            col = tuple(int(c * bright) for c in col)
            sr = rng.uniform(0.7, 2.4) * (1.25 - frac * 0.5)
            ad.ellipse([x - sr, y - sr, x + sr, y + sr], fill=col)
    # 旋臂柔化成「星河」而非散点
    arm_glow = arm_layer.filter(ImageFilter.GaussianBlur(5))
    arm_layer = screen_blend(arm_layer.filter(ImageFilter.GaussianBlur(1.0)), arm_glow)
    img = screen_blend(img, arm_layer)

    # ---- 4. 银核：暖白炽亮中心 ----
    img = screen_blend(img, radial_glow(SIZE, cx, cy, SIZE * 0.36, (255, 215, 150), 1.0))
    img = screen_blend(img, radial_glow(SIZE, cx, cy, SIZE * 0.16, (255, 245, 225), 1.0))
    img = screen_blend(img, radial_glow(SIZE, cx, cy, SIZE * 0.06, (255, 255, 250), 1.0))

    # ---- 5. 前景亮星：少量大星，2 颗带衍射光芒 ----
    fg = Image.new("RGB", (SIZE, SIZE), (0, 0, 0))
    fd = ImageDraw.Draw(fg)
    bright_stars = [
        (SIZE * 0.80, SIZE * 0.18, 4.5, (220, 230, 255), True),
        (SIZE * 0.17, SIZE * 0.30, 3.2, (255, 240, 220), True),
        (SIZE * 0.88, SIZE * 0.62, 2.6, (200, 220, 255), False),
        (SIZE * 0.30, SIZE * 0.86, 2.8, (255, 250, 240), False),
        (SIZE * 0.62, SIZE * 0.10, 2.2, (210, 225, 255), False),
    ]
    for x, y, r, col, sp in bright_stars:
        draw_star(fd, x, y, r, col, spikes=sp)
    for _ in range(70):  # 中景中亮星
        x, y = rng.uniform(0, SIZE), rng.uniform(0, SIZE)
        v = rng.randint(120, 230)
        col = rng.choice([(v, v, v), (v, v, min(255, v + 25)), (min(255, v + 20), int(v * 0.97), int(v * 0.9))])
        draw_star(fd, x, y, rng.uniform(1.0, 2.2), col)
    fg_glow = fg.filter(ImageFilter.GaussianBlur(3))
    img = screen_blend(img, screen_blend(fg, fg_glow))

    # ---- 6. 轻 vignette：中心保亮、四角微暗收束 ----
    vig = Image.new("L", (SIZE, SIZE), 110)
    vd = ImageDraw.Draw(vig)
    vd.ellipse([-SIZE * 0.30, -SIZE * 0.30, SIZE * 1.30, SIZE * 1.30], fill=255)
    vig = vig.filter(ImageFilter.GaussianBlur(SIZE // 6))
    black = Image.new("RGB", (SIZE, SIZE), (0, 0, 0))
    img = Image.composite(img, black, vig)

    return img


def rounded_mask(size: int, radius_frac: float = 0.225) -> Image.Image:
    m = Image.new("L", (size * 4, size * 4), 0)
    d = ImageDraw.Draw(m)
    r = int(size * 4 * radius_frac)
    d.rounded_rectangle([0, 0, size * 4 - 1, size * 4 - 1], radius=r, fill=255)
    return m.resize((size, size), Image.LANCZOS)


def main():
    art = build_galaxy()

    square_path = OUT_DIR / "app-icon-square.png"
    art.save(square_path)

    icon = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    icon.paste(art, (0, 0))
    icon.putalpha(rounded_mask(SIZE))
    icon_path = OUT_DIR / "app-icon-1024.png"
    icon.save(icon_path)

    print(f"wrote {square_path}\nwrote {icon_path}")


if __name__ == "__main__":
    main()
