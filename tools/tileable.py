"""Make a photograph tile without seams: shift it by half (the old edges meet in the middle, the new edges are
the old middle, which already wraps), then cross-fade the unshifted image back in everywhere except near the new
edges, so the middle keeps the original and the seams are feathered away.

    uv run --with pillow tools/tileable.py in.png out.png [--size 1024]
"""

import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

src, out = sys.argv[1], sys.argv[2]
size = int(sys.argv[sys.argv.index("--size") + 1]) if "--size" in sys.argv else 1024
img = Image.open(src).convert("RGB").resize((size, size), Image.LANCZOS)
shifted = ImageChops.offset(img, size // 2, size // 2)
# Mask: 255 (original) in the middle, 0 (shifted, which wraps) near the borders, soft in between.
mask = Image.new("L", (size, size), 0)
margin = size // 6
ImageDraw.Draw(mask).rectangle([margin, margin, size - margin, size - margin], fill=255)
mask = mask.filter(ImageFilter.GaussianBlur(size / 14))
Image.composite(img, shifted, mask).save(out)
print("tileable", out, size)
