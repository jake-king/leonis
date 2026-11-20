#!/usr/bin/env python3
"""Compare P4 renders: exact equality, then how far apart they are.

usage: compare.py BASE.png OTHER.png [OTHER.png ...]
       compare.py --selftest BASE.png

For each OTHER prints one line: identical, or the count of differing pixels, the largest channel
difference, and the fraction of 8x8 blocks that differ. Exit 0 if every OTHER is byte-identical in
pixels, 1 if any differs, 2 on unreadable input.

--selftest plants a one-pixel, one-unit change in BASE and must report exactly one differing pixel
with a max difference of 1 (exit 0 if the comparison caught it, 1 if not).

Standard library only, so it runs unchanged on every CI OS.
"""

import struct
import sys
import zlib


def read_png(path):
    """Returns (width, height, bytes RGBA) for an 8-bit RGBA or RGB, non-interlaced PNG."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos, idat, header = 8, b"", None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
    if header is None:
        raise ValueError(f"{path}: no IHDR chunk")
    width, height, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f"{path}: unsupported PNG (depth {depth}, colour {colour}, interlace {interlace})")
    bpp = 4 if colour == 6 else 3
    raw = zlib.decompress(idat)
    stride = width * bpp
    prev = bytearray(stride)
    out = bytearray()
    for row in range(height):
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
            elif kind != 0:
                raise ValueError(f"{path}: bad filter {kind}")
        if bpp == 3:
            out += b"".join(bytes(line[i:i + 3]) + b"\xff" for i in range(0, stride, 3))
        else:
            out += line
        prev = line
    return width, height, bytes(out)


def compare(base, other):
    """Returns (differing pixels, max channel difference, differing 8x8 blocks, total blocks)."""
    (w, h, a), (w2, h2, b) = base, other
    if (w, h) != (w2, h2):
        raise ValueError(f"size {w}x{h} vs {w2}x{h2}")
    pixels, worst, blocks = 0, 0, set()
    for i in range(0, len(a), 4):
        d = max(abs(a[i + k] - b[i + k]) for k in range(4))
        if d:
            pixels += 1
            worst = max(worst, d)
            x, y = (i // 4) % w, (i // 4) // w
            blocks.add((x // 8, y // 8))
    return pixels, worst, len(blocks), ((w + 7) // 8) * ((h + 7) // 8)


def main(argv):
    try:
        if len(argv) == 2 and argv[0] == "--selftest":
            w, h, px = read_png(argv[1])
            planted = bytearray(px)
            i = (h // 2 * w + w // 2) * 4
            planted[i] = planted[i] + 1 if planted[i] < 255 else planted[i] - 1
            result = compare((w, h, px), (w, h, bytes(planted)))
            ok = result[:3] == (1, 1, 1)
            print(f"selftest planted 1 px -> pixels={result[0]} max_diff={result[1]} blocks={result[2]} "
                  f"{'caught' if ok else 'MISSED'}")
            return 0 if ok else 1
        if len(argv) < 2:
            print("usage: compare.py BASE.png OTHER.png [...] | --selftest BASE.png", file=sys.stderr)
            return 2
        base = read_png(argv[0])
        differ = False
        for path in argv[1:]:
            pixels, worst, blocks, total = compare(base, read_png(path))
            if pixels == 0:
                print(f"{path}: identical to {argv[0]}")
            else:
                differ = True
                print(f"{path}: differs from {argv[0]}: pixels={pixels}/{base[0] * base[1]} "
                      f"max_diff={worst} blocks={blocks}/{total} ({blocks / total:.4f})")
        return 1 if differ else 0
    except (OSError, ValueError, zlib.error) as e:
        print(f"compare: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
