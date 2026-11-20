#!/usr/bin/env python3
"""Checks that a screenshot's centre pixel is the expected colour (within 1 per channel).

usage: screen_pixel.py SCREENSHOT.png R,G,B
exit:  0 match, 1 different colour, 2 unreadable input. Standard library only.
"""

import struct
import sys
import zlib


def centre_pixel(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat, header = 8, b"", None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", data[pos + 8:pos + 8 + n])
        elif kind == b"IDAT":
            idat += data[pos + 8:pos + 8 + n]
        pos += 12 + n
    if header is None:
        raise ValueError("no IHDR")
    width, height, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f"unsupported PNG (depth {depth}, colour {colour}, interlace {interlace})")
    bpp = 4 if colour == 6 else 3
    raw = zlib.decompress(idat)
    stride = width * bpp
    prev = bytearray(stride)
    for row in range(height // 2 + 1):
        start = row * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if kind == 1:
                line[i] = (line[i] + a) & 255
            elif kind == 2:
                line[i] = (line[i] + b) & 255
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        prev = line
    x = width // 2 * bpp
    return width, height, tuple(prev[x:x + 3])


def main(argv):
    try:
        path, want = argv
        expected = tuple(int(v) for v in want.split(","))
        width, height, got = centre_pixel(path)
    except (OSError, ValueError, zlib.error) as e:
        print(f"screen_pixel: {e}", file=sys.stderr)
        return 2
    ok = all(abs(g - e) <= 1 for g, e in zip(got, expected))
    print(f"screen {width}x{height} centre={got} expected={expected} {'match' if ok else 'DIFFERENT'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
