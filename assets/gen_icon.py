#!/usr/bin/env python3
"""Generate the rugif tray icon: a film frame with the letter R."""

import struct
import zlib
import os

SIZE = 128
ICON_PATH = os.path.join(os.path.dirname(__file__), "rugif.png")

def make_png(width, height, rgba_data):
    """Create a minimal PNG from RGBA pixel data."""
    def chunk(chunk_type, data):
        c = chunk_type + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    header = b"\x89PNG\r\n\x1a\n"
    ihdr = chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))

    raw = b""
    for y in range(height):
        raw += b"\x00"  # filter: none
        raw += rgba_data[y * width * 4 : (y + 1) * width * 4]

    idat = chunk(b"IDAT", zlib.compress(raw, 9))
    iend = chunk(b"IEND", b"")
    return header + ihdr + idat + iend


def draw_icon():
    pixels = bytearray(SIZE * SIZE * 4)

    def set_pixel(x, y, r, g, b, a=255):
        if 0 <= x < SIZE and 0 <= y < SIZE:
            i = (y * SIZE + x) * 4
            pixels[i] = r
            pixels[i + 1] = g
            pixels[i + 2] = b
            pixels[i + 3] = a

    def fill_rect(x0, y0, w, h, r, g, b, a=255):
        for y in range(y0, min(y0 + h, SIZE)):
            for x in range(x0, min(x0 + w, SIZE)):
                set_pixel(x, y, r, g, b, a)

    def fill_rounded_rect(x0, y0, w, h, radius, r, g, b, a=255):
        for y in range(y0, min(y0 + h, SIZE)):
            for x in range(x0, min(x0 + w, SIZE)):
                # Check corners
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
                    set_pixel(x, y, r, g, b, a)

    # Background: rounded rectangle, dark red/crimson
    fill_rounded_rect(4, 4, 120, 120, 16, 200, 40, 40)

    # Film sprocket holes on left and right edges
    for i in range(5):
        y = 16 + i * 22
        # Left sprockets
        fill_rounded_rect(8, y, 10, 14, 3, 140, 25, 25)
        # Right sprockets
        fill_rounded_rect(110, y, 10, 14, 3, 140, 25, 25)

    # Inner screen area (darker)
    fill_rounded_rect(24, 14, 80, 100, 8, 160, 30, 30)

    # Letter "R" - white, bold
    # Vertical stroke
    fill_rect(42, 28, 10, 60, 255, 255, 255)

    # Top horizontal
    fill_rect(42, 28, 30, 10, 255, 255, 255)

    # Middle horizontal
    fill_rect(42, 52, 30, 10, 255, 255, 255)

    # Top-right curve (approximated with rectangles)
    fill_rect(72, 28, 10, 34, 255, 255, 255)

    # Diagonal leg of R
    for i in range(28):
        x = 52 + i
        y = 62 + i
        fill_rect(x, y, 10, 3, 255, 255, 255)

    return bytes(pixels)


pixels = draw_icon()
png_data = make_png(SIZE, SIZE, pixels)

with open(ICON_PATH, "wb") as f:
    f.write(png_data)

print(f"Generated {ICON_PATH} ({len(png_data)} bytes)")
