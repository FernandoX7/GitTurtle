"""make-images.py DIR: deterministic test images. before/after.png 1600x1000 RGB (gradient, grid,
circles; after shifts and recolours them); before/after.gif 640x400, 24 frames of 50 ms (a moving
disc over stripes; after moves the other way, other colours)."""
import sys, math
from PIL import Image, ImageDraw
out = sys.argv[1]
def photo(shift, hue):
    im = Image.new('RGB', (1600, 1000)); px = im.load()
    for y in range(1000):
        for x in range(1600):
            px[x, y] = ((x * 255 // 1600 + hue) % 256, (y * 255 // 1000) % 256, ((x + y) * 3 + hue) % 256)
    d = ImageDraw.Draw(im)
    for x in range(0, 1600, 50): d.line([(x, 0), (x, 999)], fill=(20, 20, 20))
    for y in range(0, 1000, 50): d.line([(0, y), (1599, y)], fill=(20, 20, 20))
    for i in range(24):
        cx, cy = (i * 157 + shift) % 1600, (i * 97 + shift // 2) % 1000
        d.ellipse([cx - 60, cy - 60, cx + 60, cy + 60], outline=(255, 255, 255), width=6)
    return im
photo(0, 0).save(f'{out}/before.png'); photo(90, 70).save(f'{out}/after.png')
def anim(direction, colours):
    frames = []
    for f in range(24):
        im = Image.new('RGB', (640, 400), colours[0]); d = ImageDraw.Draw(im)
        for x in range(0, 640, 40): d.rectangle([x, 0, x + 19, 399], fill=colours[1])
        cx = 320 + direction * int(250 * math.sin(2 * math.pi * f / 24))
        d.ellipse([cx - 60, 140, cx + 60, 260], fill=colours[2])
        frames.append(im.convert('P', palette=Image.ADAPTIVE, colors=32))
    frames[0].save(f'{out}/{"before" if direction > 0 else "after"}.gif', save_all=True, append_images=frames[1:], duration=50, loop=0)
anim(1, ((30, 40, 60), (50, 60, 90), (240, 180, 40))); anim(-1, ((60, 30, 40), (90, 50, 60), (40, 200, 240)))
