#!/usr/bin/env python3
"""Generate rugif tray icons: default (film frame + R) and recording (stop square)."""

import struct
import zlib
import os

SIZE = 128
DIR = os.path.dirname(__file__)


def make_png(width, height, rgba_data):
    """Create a minimal PNG from RGBA pixel data."""
    def chunk(chunk_type, data):
        c = chunk_type + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    header = b"\x89PNG\r\n\x1a\n"
    ihdr = chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))

    raw = b""
    for y in range(height):
        raw += b"\x00"
        raw += rgba_data[y * width * 4 : (y + 1) * width * 4]

    idat = chunk(b"IDAT", zlib.compress(raw, 9))
    iend = chunk(b"IEND", b"")
    return header + ihdr + idat + iend


def set_pixel(pixels, x, y, r, g, b, a=255):
    if 0 <= x < SIZE and 0 <= y < SIZE:
        i = (y * SIZE + x) * 4
        pixels[i] = r
        pixels[i + 1] = g
        pixels[i + 2] = b
        pixels[i + 3] = a


def fill_rect(pixels, x0, y0, w, h, r, g, b, a=255):
    for y in range(y0, min(y0 + h, SIZE)):
        for x in range(x0, min(x0 + w, SIZE)):
            set_pixel(pixels, x, y, r, g, b, a)


def fill_rounded_rect(pixels, x0, y0, w, h, radius, r, g, b, a=255):
    for y in range(y0, min(y0 + h, SIZE)):
        for x in range(x0, min(x0 + w, SIZE)):
            lx, ly = x - x0, y - y0
            rx, ry = x0 + w - 1 - x, y0 + h - 1 - y
            in_rect = True
            for cx, cy in [(lx, ly), (rx, ly), (lx, ry), (rx, ry)]:
                if cx < radius and cy < radius:
                    dx = radius - cx - 0.5
                    dy = radius - cy - 0.5
                    if dx * dx + dy * dy > radius * radius:
                        in_rect = False
                        break
            if in_rect:
                set_pixel(pixels, x, y, r, g, b, a)


def fill_circle(pixels, cx, cy, radius, r, g, b, a=255):
    for y in range(SIZE):
        for x in range(SIZE):
            dx = x - cx
            dy = y - cy
            if dx * dx + dy * dy <= radius * radius:
                set_pixel(pixels, x, y, r, g, b, a)


def draw_default_icon():
    """Film frame with letter R."""
    pixels = bytearray(SIZE * SIZE * 4)

    fill_rounded_rect(pixels, 4, 4, 120, 120, 16, 200, 40, 40)

    for i in range(5):
        y = 16 + i * 22
        fill_rounded_rect(pixels, 8, y, 10, 14, 3, 140, 25, 25)
        fill_rounded_rect(pixels, 110, y, 10, 14, 3, 140, 25, 25)

    fill_rounded_rect(pixels, 24, 14, 80, 100, 8, 160, 30, 30)

    fill_rect(pixels, 42, 28, 10, 60, 255, 255, 255)
    fill_rect(pixels, 42, 28, 30, 10, 255, 255, 255)
    fill_rect(pixels, 42, 52, 30, 10, 255, 255, 255)
    fill_rect(pixels, 72, 28, 10, 34, 255, 255, 255)
    for i in range(28):
        fill_rect(pixels, 52 + i, 62 + i, 10, 3, 255, 255, 255)

    return bytes(pixels)


def draw_recording_icon():
    """Film frame with stop square — indicates active recording."""
    pixels = bytearray(SIZE * SIZE * 4)

    # Same film frame background as default
    fill_rounded_rect(pixels, 4, 4, 120, 120, 16, 200, 40, 40)

    for i in range(5):
        y = 16 + i * 22
        fill_rounded_rect(pixels, 8, y, 10, 14, 3, 140, 25, 25)
        fill_rounded_rect(pixels, 110, y, 10, 14, 3, 140, 25, 25)

    # Inner area — brighter to indicate active state
    fill_rounded_rect(pixels, 24, 14, 80, 100, 8, 180, 35, 35)

    # White stop square in center
    fill_rounded_rect(pixels, 40, 32, 48, 48, 6, 255, 255, 255)

    return bytes(pixels)


# Generate both icons
for name, draw_fn in [("rugif.png", draw_default_icon), ("rugif_recording.png", draw_recording_icon)]:
    pixels = draw_fn()
    png_data = make_png(SIZE, SIZE, pixels)
    path = os.path.join(DIR, name)
    with open(path, "wb") as f:
        f.write(png_data)
    print(f"Generated {path} ({len(png_data)} bytes)")
