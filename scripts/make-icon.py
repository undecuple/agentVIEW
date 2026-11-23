#!/usr/bin/env python3
"""Deterministic agentVIEW SVG + PNG. Standard library only; no fonts or timestamps.

Run from any directory: python3 scripts/make-icon.py
Use --check to verify committed outputs without writing. PNG uses fixed 4x sampling,
integer box filtering and uncompressed DEFLATE for identical bytes across zlib versions.
"""
import argparse
import binascii
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent.parent
SVG = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><title>agentVIEW</title><rect x="8" y="8" width="240" height="240" rx="52" fill="#19171d"/><rect x="18" y="18" width="220" height="220" rx="44" fill="#242029"/><path d="M55 66H91L128 140L165 66H201L146 177H110Z" fill="#ef8caf"/><path d="M128 184V203M75 217V203H181V217M128 203V217" fill="none" stroke="#94dbba" stroke-width="4"/><g fill="#94dbba"><circle cx="75" cy="219" r="6"/><circle cx="128" cy="219" r="6"/><circle cx="181" cy="219" r="6"/></g></svg>
'''


def rounded(x, y, lo, hi, radius):
    if not (lo <= x <= hi and lo <= y <= hi):
        return False
    cx = min(max(x, lo + radius), hi - radius)
    cy = min(max(y, lo + radius), hi - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2


def polygon(x, y, points):
    inside = False
    for (ax, ay), (bx, by) in zip(points, points[1:] + points[:1]):
        if (ay > y) != (by > y) and x < (bx - ax) * (y - ay) / (by - ay) + ax:
            inside = not inside
    return inside


def pixel(x, y, light=False):
    colors = [(234,234,234,255),(255,255,255,255),(0,87,209,255),(0,140,153,255)] if light else [(27,29,30,255),(39,40,34,255),(240,140,176,255),(102,217,239,255)]
    color = (0, 0, 0, 0)
    if rounded(x, y, 8, 248, 52):
        color = colors[0]
    if rounded(x, y, 18, 238, 44):
        color = colors[1]
    if polygon(x, y, [(55, 66), (91, 66), (128, 140), (165, 66), (201, 66), (146, 177), (110, 177)]):
        color = colors[2]
    line = (126 <= x <= 130 and 184 <= y <= 219) or (73 <= x <= 183 and 201 <= y <= 205) or (73 <= x <= 77 and 203 <= y <= 219) or (179 <= x <= 183 and 203 <= y <= 219)
    if line or any((x - cx) ** 2 + (y - 219) ** 2 <= 36 for cx in [75, 128, 181]):
        color = colors[3]
    return color


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', binascii.crc32(kind + data) & 0xffffffff)


def png(light=False):
    rows = bytearray()
    for y in range(256):
        rows.append(0)
        for x in range(256):
            samples = [pixel(x + (sx + .5) / 4, y + (sy + .5) / 4, light) for sy in range(4) for sx in range(4)]
            alpha = sum(p[3] for p in samples)
            rows.extend([(sum(p[c] * p[3] for p in samples) + alpha // 2) // alpha if alpha else 0 for c in range(3)])
            rows.append((alpha + 8) // 16)
    # Explicit stored blocks avoid compressor implementation/version differences.
    stream = bytearray(b'\x78\x01')
    for offset in range(0, len(rows), 65535):
        block = rows[offset:offset + 65535]
        stream.append(1 if offset + len(block) == len(rows) else 0)
        stream.extend(struct.pack('<HH', len(block), 65535 - len(block)))
        stream.extend(block)
    stream.extend(struct.pack('>I', zlib.adler32(rows) & 0xffffffff))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 256, 256, 8, 6, 0, 0, 0)) + chunk(b'IDAT', stream) + chunk(b'IEND', b'')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    check = parser.parse_args().check
    def svg(light=False):
        text=SVG
        colors=['#eaeaea','#ffffff','#0057d1','#008c99'] if light else ['#1b1d1e','#272822','#f08cb0','#66d9ef']
        for old,new in zip(['#19171d','#242029','#ef8caf','#94dbba'],colors):text=text.replace(old,new)
        return text.encode()
    for relative, content in [('ui/agentview.svg', svg()), ('ui/agentview-light.svg', svg(True)), ('src-tauri/icons/icon.png', png()), ('assets/icon-light.png', png(True))]:
        path = ROOT / relative
        if check:
            if not path.exists() or path.read_bytes() != content:
                raise SystemExit(f'Icon differs: {relative}')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    print('agentVIEW icon: ' + ('verified' if check else 'generated'))


if __name__ == '__main__':
    main()
