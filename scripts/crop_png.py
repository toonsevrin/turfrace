#!/usr/bin/env python3
"""Crop an 8-bit RGBA PNG without requiring Pillow or ImageMagick."""

from __future__ import annotations

import argparse
import struct
import zlib


PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def paeth(a: int, b: int, c: int) -> int:
    estimate = a + b - c
    pa = abs(estimate - a)
    pb = abs(estimate - b)
    pc = abs(estimate - c)
    return a if pa <= pb and pa <= pc else b if pb <= pc else c


def read_png(path: str) -> tuple[int, int, int, bytearray]:
    data = open(path, "rb").read()
    if not data.startswith(PNG_SIGNATURE):
        raise ValueError(f"not a PNG: {path}")
    offset = len(PNG_SIGNATURE)
    width = height = bit_depth = color_type = interlace = None
    compressed = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        chunk = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, _, _, interlace = struct.unpack(
                ">IIBBBBB", chunk
            )
        elif kind == b"IDAT":
            compressed.extend(chunk)
        elif kind == b"IEND":
            break
    if bit_depth != 8 or color_type not in (2, 6) or interlace != 0:
        raise ValueError("crop_png.py only supports non-interlaced 8-bit RGB/RGBA PNGs")

    channels = 4 if color_type == 6 else 3
    stride = width * channels
    raw = zlib.decompress(compressed)
    pixels = bytearray(height * stride)
    cursor = 0
    previous = bytearray(stride)
    for row in range(height):
        filter_type = raw[cursor]
        cursor += 1
        encoded = raw[cursor : cursor + stride]
        cursor += stride
        decoded = bytearray(stride)
        for index, value in enumerate(encoded):
            left = decoded[index - channels] if index >= channels else 0
            up = previous[index]
            upper_left = previous[index - channels] if index >= channels else 0
            if filter_type == 0:
                decoded[index] = value
            elif filter_type == 1:
                decoded[index] = (value + left) & 0xFF
            elif filter_type == 2:
                decoded[index] = (value + up) & 0xFF
            elif filter_type == 3:
                decoded[index] = (value + (left + up) // 2) & 0xFF
            elif filter_type == 4:
                decoded[index] = (value + paeth(left, up, upper_left)) & 0xFF
            else:
                raise ValueError(f"unsupported PNG filter {filter_type}")
        pixels[row * stride : (row + 1) * stride] = decoded
        previous = decoded
    return width, height, channels, pixels


def chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def write_crop(path: str, width: int, height: int, channels: int, pixels: bytearray, args: argparse.Namespace) -> None:
    left = max(0, min(args.x, width))
    top = max(0, min(args.y, height))
    right = max(left, min(args.x + args.width, width))
    bottom = max(top, min(args.y + args.height, height))
    crop_width = right - left
    crop_height = bottom - top
    rows = []
    for row in range(top, bottom):
        start = (row * width + left) * channels
        rows.append(b"\x00" + bytes(pixels[start : start + crop_width * channels]))
    header = struct.pack(">IIBBBBB", crop_width, crop_height, 8, 6 if channels == 4 else 2, 0, 0, 0)
    output = PNG_SIGNATURE + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(output)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source")
    parser.add_argument("destination")
    parser.add_argument("--x", type=int, default=1000)
    parser.add_argument("--y", type=int, default=0)
    parser.add_argument("--width", type=int, default=280)
    parser.add_argument("--height", type=int, default=110)
    args = parser.parse_args()
    width, height, channels, pixels = read_png(args.source)
    write_crop(args.destination, width, height, channels, pixels, args)


if __name__ == "__main__":
    main()
