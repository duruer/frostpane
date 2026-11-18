#!/usr/bin/env python3
"""Regenerate every Frostpane icon from packaging/icons/frostpane.svg.

Needs the resvg CLI (cargo install resvg --locked) and Pillow
(pip install pillow). Run from anywhere:

    python -I -X utf8 packaging/icons/render-icons.py

Writes:
  packaging/icons/png/frostpane-{16,20,24,32,40,48,64,128,256,512,1024}.png
  packaging/windows/frostpane.ico        (16..256, PNG-compressed)
  packaging/macos/Frostpane.icns         (16..1024, PNG payloads)
  packaging/linux/icons/frostpane-{16,32,48,64,128,256,512}.png
"""
import io
import os
import shutil
import struct
import subprocess
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
PKG = os.path.dirname(HERE)
SVG = os.path.join(HERE, "frostpane.svg")
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024)
LINUX = (16, 32, 48, 64, 128, 256, 512)
ICO = (16, 20, 24, 32, 40, 48, 64, 128, 256)
# ICNS type codes holding PNG data: size -> tag.
ICNS = {
    16: b"icp4", 32: b"icp5", 64: b"icp6", 128: b"ic07",
    256: b"ic08", 512: b"ic09", 1024: b"ic10",
}


def resvg():
    exe = shutil.which("resvg")
    if not exe:
        sys.exit("resvg not found: cargo install resvg --locked")
    return exe


def render(exe, size, out):
    subprocess.run([exe, "-w", str(size), "-h", str(size), SVG, out], check=True)


def main():
    exe = resvg()
    png_dir = os.path.join(HERE, "png")
    os.makedirs(png_dir, exist_ok=True)
    imgs = {}
    for s in SIZES:
        p = os.path.join(png_dir, f"frostpane-{s}.png")
        render(exe, s, p)
        imgs[s] = Image.open(p).convert("RGBA")
        imgs[s].save(p, optimize=True)

    lin = os.path.join(PKG, "linux", "icons")
    os.makedirs(lin, exist_ok=True)
    for s in LINUX:
        shutil.copyfile(os.path.join(png_dir, f"frostpane-{s}.png"), os.path.join(lin, f"frostpane-{s}.png"))

    win = os.path.join(PKG, "windows")
    os.makedirs(win, exist_ok=True)
    imgs[256].save(
        os.path.join(win, "frostpane.ico"),
        format="ICO",
        sizes=[(s, s) for s in ICO],
        append_images=[imgs[s] for s in ICO if s != 256],
    )

    chunks = b""
    for s, tag in ICNS.items():
        b = io.BytesIO()
        imgs[s].save(b, "PNG", optimize=True)
        data = b.getvalue()
        chunks += tag + struct.pack(">I", len(data) + 8) + data
    with open(os.path.join(PKG, "macos", "Frostpane.icns"), "wb") as f:
        f.write(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)


if __name__ == "__main__":
    main()
