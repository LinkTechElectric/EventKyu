"""Regenerate SignalFit's original application icon (Pillow required)."""

from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 512
OUTPUT = Path(__file__).resolve().parents[1] / "src-tauri" / "icons" / "icon.png"

image = Image.new("RGBA", (SIZE, SIZE), (10, 15, 27, 255))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((28, 28, 484, 484), radius=112, fill=(20, 31, 51, 255), outline=(56, 227, 190, 255), width=8)
for index, height in enumerate((112, 208, 288, 176)):
    left = 118 + index * 70
    draw.rounded_rectangle(
        (left, 256 - height // 2, left + 34, 256 + height // 2),
        radius=17,
        fill=(56, 227, 190, 255),
    )
draw.polygon(((350, 188), (424, 256), (350, 324)), fill=(151, 139, 255, 255))
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
image.save(OUTPUT, optimize=True)
print(OUTPUT)
