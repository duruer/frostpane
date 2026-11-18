#!/usr/bin/env python3
"""Regenerate the Frostpane icons from the geometry of docs/assets/logo.svg.

Needs Pillow (pip install pillow). Writes, next to this script's parent:
  packaging/linux/icons/frostpane-256.png, frostpane-512.png
  packaging/macos/Frostpane.icns   (PNG-based ICNS: 256, 512, 1024 px)

Run from anywhere:  python3 packaging/icons/render-icons.py
The shapes below mirror logo.svg (128x128 viewBox); keep them in sync.
"""
import io
import math
import os
import struct

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
PKG = os.path.dirname(HERE)
SS = 4  # supersampling factor


def render(size):
    n = size * SS
    k = n / 128.0
    # Diagonal gradient #4a92ff -> #1c5fd6 over the whole 128 box.
    grad = Image.new("RGB", (n, n))
    px = grad.load()
    c0, c1 = (0x4A, 0x92, 0xFF), (0x1C, 0x5F, 0xD6)
    for y in range(n):
        for x in range(n):
            t = (x + y) / (2.0 * (n - 1))
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(c0, c1))
    bg = Image.new("L", (n, n), 0)
    ImageDraw.Draw(bg).rounded_rectangle([4 * k, 4 * k, 124 * k, 124 * k], radius=30 * k, fill=255)

    fg = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(fg)

    def stroke(points, width):
        pts = [(64 * k + x * k, 64 * k + y * k) for x, y in points]
        d.line(pts, fill=255, width=round(width * k), joint="curve")
        r = width * k / 2
        for x, y in pts:
            d.ellipse([x - r, y - r, x + r, y + r], fill=255)

    def rot(points, deg):
        a = math.radians(deg)
        c, s = math.cos(a), math.sin(a)
        return [(x * c - y * s, x * s + y * c) for x, y in points]

    arm = [[(0, -36), (0, 36)], [(-9, -27), (0, -18), (9, -27)], [(-9, 27), (0, 18), (9, 27)]]
    for deg in (0, 60, -60):
        for part in arm:
            stroke(rot(part, deg), 7)
    # Hub ring: r=9, stroke 5.
    c = 64 * k
    d.ellipse([c - 11.5 * k, c - 11.5 * k, c + 11.5 * k, c + 11.5 * k], fill=255)
    d.ellipse([c - 6.5 * k, c - 6.5 * k, c + 6.5 * k, c + 6.5 * k], fill=0)

    out = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    out.paste(grad, (0, 0), bg)
    out.paste(Image.new("RGB", (n, n), (255, 255, 255)), (0, 0), fg)
    return out.resize((size, size), Image.LANCZOS)


def png_bytes(img):
    b = io.BytesIO()
    img.save(b, "PNG", optimize=True)
    return b.getvalue()


def main():
    os.makedirs(os.path.join(PKG, "linux", "icons"), exist_ok=True)
    big = render(1024)
    sizes = {s: big.resize((s, s), Image.LANCZOS) for s in (256, 512)}
    for s in (256, 512):
        sizes[s].save(os.path.join(PKG, "linux", "icons", f"frostpane-{s}.png"), optimize=True)
    # ICNS: 'ic08' = 256, 'ic09' = 512, 'ic10' = 1024 (PNG payloads).
    chunks = b""
    for tag, img in ((b"ic08", sizes[256]), (b"ic09", sizes[512]), (b"ic10", big)):
        data = png_bytes(img)
        chunks += tag + struct.pack(">I", len(data) + 8) + data
    with open(os.path.join(PKG, "macos", "Frostpane.icns"), "wb") as f:
        f.write(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)


if __name__ == "__main__":
    main()
