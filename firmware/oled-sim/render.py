"""Вивід sim -> GIF на кожен режим + PNG-аркуш з кількома кадрами кожного режиму.

python3 render.py frames.bin outdir [кадр ...]
"""
import os
import sys
from PIL import Image

W, H, SCALE, GAP = 128, 32, 4, 10
NAMES = ["status", "ship", "pet", "eq", "pc", "image"]

def to_img(buf):
    img = Image.new("L", (W, H))
    p = img.load()
    for x in range(W):
        for y in range(H):
            p[x, y] = 235 if buf[x + (y // 8) * W] >> (y % 8) & 1 else 18
    return img.resize((W * SCALE, H * SCALE), Image.NEAREST)

data = open(sys.argv[1], "rb").read()
out = sys.argv[2]
picks = [int(a) for a in sys.argv[3:]] or [60, 140, 220]
os.makedirs(out, exist_ok=True)
step = 1 + 512
screens = {}
for i in range(0, len(data), step):
    screens.setdefault(data[i], []).append(data[i + 1:i + step])

sheet = Image.new("L", (len(picks) * (W * SCALE + GAP), len(screens) * (H * SCALE + GAP)), 60)
for row, (scr, frames) in enumerate(sorted(screens.items())):
    imgs = [to_img(f) for f in frames]
    imgs[0].save(f"{out}/{NAMES[scr]}.gif", save_all=True, append_images=imgs[1:], duration=50, loop=0)
    for col, n in enumerate(picks):
        sheet.paste(imgs[n], (col * (W * SCALE + GAP), row * (H * SCALE + GAP)))
sheet.save(f"{out}/sheet.png")
print("ok:", ", ".join(f"{NAMES[s]}.gif" for s in sorted(screens)))
