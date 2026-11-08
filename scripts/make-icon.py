"""生成 JToolsBox 应用图标：黄底黑字「JT」。

与 JToolsBoxMobile 使用同一套图标规范（亮黄 #FFC800 背景 + 近黑 #111 文字），
两端保持一致。

用法：
    python scripts/make-icon.py

源图产出 src-tauri/icons/app-icon.png，之后用
`npx tauri icon src-tauri/icons/app-icon.png` 生成全平台图标
（Windows .ico、macOS .icns、各尺寸 png）。
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZE = 1024
BG = (255, 200, 0)      # 亮黄
FG = (17, 17, 17)       # 近黑
TEXT = "JT"
TRACK_RATIO = -0.061    # 字间距占字号比例（负值收紧）
FONT_CANDIDATES = [
    "C:/Windows/Fonts/ariblk.ttf",   # Arial Black
    "C:/Windows/Fonts/seguibl.ttf",  # Segoe UI Black
    "C:/Windows/Fonts/arialbd.ttf",
]


def load_font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONT_CANDIDATES:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit("找不到可用的粗体字体")


def measure(font: ImageFont.FreeTypeFont, draw: ImageDraw.ImageDraw):
    glyphs = [draw.textbbox((0, 0), ch, font=font) for ch in TEXT]
    widths = [g[2] - g[0] for g in glyphs]
    tracking = TRACK_RATIO * font.size
    total_w = sum(widths) + tracking * (len(TEXT) - 1)
    top = min(g[1] for g in glyphs)
    bottom = max(g[3] for g in glyphs)
    return glyphs, widths, total_w, top, bottom, tracking


def draw_word(
    img: Image.Image,
    width_ratio: float,
    color: tuple[int, ...],
    optical_shift: float = -0.012,
) -> None:
    """在画布上居中绘制 JT，宽度占画布 width_ratio。"""
    size = img.width
    draw = ImageDraw.Draw(img)

    # 先按基准字号测量，再按目标宽度反推精确字号
    base_size = int(size * 0.48)
    _, _, total_w, _, _, _ = measure(load_font(base_size), draw)
    font = load_font(max(1, int(base_size * (size * width_ratio) / total_w)))

    glyphs, widths, total_w, top, bottom, tracking = measure(font, draw)
    total_h = bottom - top
    x = (size - total_w) / 2
    # J 的圆弧下伸会压低视觉重心，整体上移做光学居中
    y = (size - total_h) / 2 - top - size * optical_shift

    for ch, g, w in zip(TEXT, glyphs, widths):
        draw.text((x - g[0], y), ch, font=font, fill=color)
        x += w + tracking


def render(size: int = SIZE) -> Image.Image:
    img = Image.new("RGB", (size, size), BG)
    draw_word(img, width_ratio=0.615, color=FG)
    return img


def main() -> None:
    out = Path("src-tauri/icons/app-icon.png")
    out.parent.mkdir(parents=True, exist_ok=True)
    render().save(out, "PNG")
    print(f"已生成 {out.resolve()}")

    # 桌面端在浏览器模式（npm run dev）下用到的 favicon
    favicon = Path("public/icon.png")
    favicon.parent.mkdir(parents=True, exist_ok=True)
    render(64).save(favicon, "PNG")
    print(f"已生成 {favicon.resolve()}")


if __name__ == "__main__":
    main()
