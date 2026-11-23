#!/usr/bin/env python3
"""Generate the agentVIEW banner, hierarchy divider and social preview.

The double rules, checker dither and rectangular framing adapt the code-native
art approach in undecuple/me's assets/banner.svg and assets/divider.svg at
ce194c20037b15ab9fa03e38f1f2d36c476417b1. That repository has no retained banner
generator; this is a new reusable implementation, not an invocation of a missing
tool. The lettering is an original, embedded pixel alphabet (no font files),
with a two-row descender on the lowercase g.
The V/tree motif follows agentVIEW's original icon. No downloads or AI imagery.

Only Python's standard library is needed. SVG and PNG share integer geometry;
the indexed PNG uses stored DEFLATE blocks for bytes stable across zlib versions.
Run with --check to compare all outputs without writing. No timestamps or seed.
"""
import argparse
import binascii
import html
import json
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent.parent
DARK_COLORS = ["#1b1d1e", "#232526", "#465457", "#f08cb0", "#66d9ef", "#f8f8f2", "#8f9ca0"]
LIGHT_COLORS = ["#ffffff", "#eaeaea", "#acacac", "#0057d1", "#008c99", "#16181a", "#596274"]
COLORS = DARK_COLORS
BG, PANEL, RULE, PINK, MINT, WHITE, MUTED = range(len(COLORS))
GLYPHS = {
    "A": "01110/10001/10001/11111/10001/10001/10001",
    "B": "11110/10001/10001/11110/10001/10001/11110",
    "C": "01111/10000/10000/10000/10000/10000/01111",
    "D": "11110/10001/10001/10001/10001/10001/11110",
    "E": "11111/10000/10000/11110/10000/10000/11111",
    "F": "11111/10000/10000/11110/10000/10000/10000",
    "G": "01111/10000/10000/10111/10001/10001/01111",
    "H": "10001/10001/10001/11111/10001/10001/10001",
    "I": "11111/00100/00100/00100/00100/00100/11111",
    "J": "00111/00010/00010/00010/10010/10010/01100",
    "K": "10001/10010/10100/11000/10100/10010/10001",
    "L": "10000/10000/10000/10000/10000/10000/11111",
    "M": "10001/11011/10101/10101/10001/10001/10001",
    "N": "10001/11001/11001/10101/10011/10011/10001",
    "O": "01110/10001/10001/10001/10001/10001/01110",
    "P": "11110/10001/10001/11110/10000/10000/10000",
    "Q": "01110/10001/10001/10001/10101/10010/01101",
    "R": "11110/10001/10001/11110/10100/10010/10001",
    "S": "01111/10000/10000/01110/00001/00001/11110",
    "T": "11111/00100/00100/00100/00100/00100/00100",
    "U": "10001/10001/10001/10001/10001/10001/01110",
    "V": "10001/10001/10001/10001/10001/01010/00100",
    "W": "10001/10001/10001/10101/10101/10101/01010",
    "X": "10001/10001/01010/00100/01010/10001/10001",
    "Y": "10001/10001/01010/00100/00100/00100/00100",
    "Z": "11111/00001/00010/00100/01000/10000/11111",
    "a": "00000/00000/01110/00001/01111/10001/01111",
    "g": "00000/00000/01111/10001/10001/10001/01111/00001/01110",
    "e": "00000/00000/01110/10001/11111/10000/01111",
    "n": "00000/00000/11110/10001/10001/10001/10001",
    "t": "00100/00100/11111/00100/00100/00101/00010",
    "/": "00001/00001/00010/00100/01000/10000/10000",
    ".": "00000/00000/00000/00000/00000/00110/00110",
    "-": "00000/00000/00000/11111/00000/00000/00000",
    " ": "00000/00000/00000/00000/00000/00000/00000",
}


class Art:
    def __init__(self, width, height, title):
        self.width, self.height, self.title = width, height, title
        self.items = []
        self.groups = {}  # name -> (first item, end item), for animated SVG output only
        self.animation = None  # (duration seconds, [(start fraction, visible group name)])
        self.rect(0, 0, width, height, BG)

    def rect(self, x, y, w, h, color, dither=False):
        self.items.append(("rect", x, y, w, h, color, dither))

    def frame(self, x, y, w, h, color, thickness=2):
        for a, b, c, d in [(x, y, w, thickness), (x, y + h - thickness, w, thickness),
                           (x, y, thickness, h), (x + w - thickness, y, thickness, h)]:
            self.rect(a, b, c, d, color)

    def poly(self, points, color):
        self.items.append(("poly", points, color))

    def text(self, text, x, y, scale, color, dither=False):
        for letter in text:
            for row, bits in enumerate(GLYPHS[letter].split("/")):
                for col, bit in enumerate(bits):
                    if bit == "1":
                        self.rect(x + col * scale, y + row * scale, scale, scale, color, dither)
            x += scale * 6

    def begin(self, name):
        self._open = (name, len(self.items))

    def end(self):
        name, start = self._open
        self.groups[name] = (start, len(self.items))

    def eye(self, x, y, unit, rows, name):
        # '#' lid outline (pink), 'M' iris (mint), 'W' highlight (white)
        self.begin(name)
        for r, line in enumerate(rows):
            for c, ch in enumerate(line):
                if ch != ".":
                    self.rect(x + c * unit, y + r * unit, unit, unit, {"#": PINK, "M": MINT, "W": WHITE}[ch])
        self.end()

    def svg(self):
        out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.width}" height="{self.height}" '
               f'viewBox="0 0 {self.width} {self.height}" role="img" aria-labelledby="title">',
               f'<title id="title">{html.escape(self.title)}</title>',
               '<defs><pattern id="dither" width="4" height="4" patternUnits="userSpaceOnUse">'
               f'<path d="M0 0h2v2H0zM2 2h2v2H2z" fill="{COLORS[RULE]}"/>'
               '</pattern></defs><g shape-rendering="crispEdges">']
        starts = {a: n for n, (a, b) in self.groups.items()}
        ends = {b for a, b in self.groups.values()}
        for index, item in enumerate(self.items + [None]):
            if index in ends:
                out.append("</g>")
            if index in starts and self.animation:
                duration, steps = self.animation
                name = starts[index]
                values = ";".join("1" if step == name else "0" for _, step in steps)
                times = ";".join(f"{start:.3f}" for start, _ in steps)
                out.append(f'<g id="{name}" opacity="{1 if steps[0][1] == name else 0}"><animate attributeName="opacity" calcMode="discrete" '
                           f'dur="{duration}s" repeatCount="indefinite" keyTimes="{times}" values="{values}"/>')
            if item is None:
                continue
            if item[0] == "rect":
                _, x, y, w, h, color, dither = item
                fill = "url(#dither)" if dither else COLORS[color]
                out.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"/>')
            else:
                _, points, color = item
                coordinates = " ".join(f"{x},{y}" for x, y in points)
                out.append(f'<polygon points="{coordinates}" fill="{COLORS[color]}"/>')
        return ("\n".join(out) + "</g></svg>\n").encode()

    def png(self):
        pixels = bytearray(self.width * self.height)
        for item in self.items:
            if item[0] == "rect":
                _, x, y, w, h, color, dither = item
                for py in range(max(0, y), min(self.height, y + h)):
                    for px in range(max(0, x), min(self.width, x + w)):
                        if not dither or (px % 4 < 2) == (py % 4 < 2):
                            pixels[py * self.width + px] = RULE if dither else color
            else:
                _, points, color = item
                for py in range(max(0, min(y for _, y in points)), min(self.height, max(y for _, y in points))):
                    for px in range(max(0, min(x for x, _ in points)), min(self.width, max(x for x, _ in points))):
                        x, y, inside = px + .5, py + .5, False
                        for (ax, ay), (bx, by) in zip(points, points[1:] + points[:1]):
                            if (ay > y) != (by > y) and x < (bx - ax) * (y - ay) / (by - ay) + ax:
                                inside = not inside
                        if inside:
                            pixels[py * self.width + px] = color
        # Four-bit palette rows: < 1 MiB even without compression, deterministic.
        rows = bytearray()
        for y in range(self.height):
            rows.append(0)
            row = pixels[y * self.width:(y + 1) * self.width]
            rows.extend((row[x] << 4) | (row[x + 1] if x + 1 < self.width else 0)
                        for x in range(0, self.width, 2))
        stream = bytearray(b"\x78\x01")
        for offset in range(0, len(rows), 65535):
            block = rows[offset:offset + 65535]
            stream.append(1 if offset + len(block) == len(rows) else 0)
            stream.extend(struct.pack("<HH", len(block), 65535 - len(block)))
            stream.extend(block)
        stream.extend(struct.pack(">I", zlib.adler32(rows) & 0xffffffff))
        def chunk(kind, data):
            return (struct.pack(">I", len(data)) + kind + data +
                    struct.pack(">I", binascii.crc32(kind + data) & 0xffffffff))
        palette = b"".join(bytes.fromhex(color[1:]) for color in COLORS)
        return (b"\x89PNG\r\n\x1a\n" +
                chunk(b"IHDR", struct.pack(">IIBBBBB", self.width, self.height, 4, 3, 0, 0, 0)) +
                chunk(b"PLTE", palette) + chunk(b"IDAT", stream) + chunk(b"IEND", b""))


def icon(art, x, y, scale=1):
    def points(coords):
        return [(x + a * scale, y + b * scale) for a, b in coords]
    art.poly(points([(0, 0), (24, 0), (50, 52), (76, 0), (100, 0),
                     (62, 78), (38, 78)]), PINK)
    art.poly(points([(48, 84), (52, 84), (52, 98), (90, 98), (90, 114),
                     (86, 114), (86, 102), (52, 102), (52, 114), (48, 114),
                     (48, 102), (14, 102), (14, 114), (10, 114), (10, 98),
                     (48, 98)]), MINT)
    for a in [8, 46, 84]:
        art.rect(x + a * scale, y + 112 * scale, 8 * scale, 8 * scale, MINT)


def banner(social=False):
    h = 640 if social else 320
    art = Art(1280, h, "agentVIEW — local Linux agent workspace")
    art.frame(16, 16, 1248, h - 32, RULE)
    art.frame(24, 24, 1232, h - 48, RULE)
    art.text("LOCAL // LINUX WORKSPACE", 64, 58, 2, MUTED)
    y = 176 if social else 108
    if social:
        art.text("agentVIEW", 70, y + 6, 14, RULE, True)
    else:
        art.text("agent", 70, y + 6, 12, RULE, True)
    art.text("agent", 64, y, 14 if social else 12, WHITE)
    if social:
        art.text("VIEW", 484, y, 14, PINK)
    else:
        # README banner only: VIEW opens into a pixel eye, which blinks twice and turns back into VIEW.
        art.begin("view")
        art.text("VIEW", 430, y + 6, 12, RULE, True)
        art.text("VIEW", 424, y, 12, PINK)
        art.end()
        open_eye = ["......############......", "....##............##....", "..##....MMMMMMMM....##..", "##....MMMWWMMMMMMM....##",
                    "..##....MMMMMMMM....##..", "....##............##....", "......############......"]
        open_eye = [row.ljust(24, ".")[:24] for row in open_eye]
        half = ["", "", "....################....", "##....MMMWWMMMMMMM....##", "....################....", "", ""]
        half = [row.ljust(24, ".")[:24] if row else "." * 24 for row in half]
        closed = ["." * 24] * 3 + ["#" * 24] + ["." * 24] * 3
        for name, rows in (("eye-open", open_eye), ("eye-half", half), ("eye-closed", closed)):
            art.eye(424, y, 12, rows, name)
        art.animation = (12, [(0, "view"), (.46, "eye-open"), (.60, "eye-half"), (.62, "eye-closed"), (.66, "eye-half"),
                              (.68, "eye-open"), (.72, "eye-half"), (.74, "eye-closed"), (.78, "eye-half"), (.80, "eye-open"), (.88, "view")])
    icon(art, 1020, y - 4, 1)
    art.text("AGENTS. IN VIEW.", 64, y + (144 if social else 128), 3, MINT)
    if social:
        art.rect(64, 414, 1152, 2, RULE)
        art.rect(64, 420, 1152, 2, RULE)
        art.rect(64, 416, 1152, 4, RULE, True)
        labels = [("INTAKE", "COORDINATORS", "SUBAGENTS"), (PINK, MINT, WHITE)]
        for x, label, color in zip([64, 442, 928], *labels):
            art.rect(x, 468, 8, 8, color)
            art.text(label, x + 24, 464, 2, color)
        art.text("HIERARCHY / CONVERSATIONS / DOCUMENTS", 64, 548, 2, MUTED)
    return art


def divider():
    art = Art(1280, 64, "Intake to coordinators to subagents")
    for x, width in [(0, 536), (744, 536)]:
        art.rect(x, 26, width, 2, RULE)
        art.rect(x, 36, width, 2, RULE)
        art.rect(x, 28, width, 8, RULE, True)
    # An original three-level hierarchy interrupts the double rule.
    art.rect(638, 10, 4, 16, PINK)
    art.rect(594, 24, 92, 2, MINT)
    for x in [594, 684]:
        art.rect(x, 24, 2, 20, MINT)
        art.rect(x - 22, 44, 48, 2, RULE)
        for dx in [-22, 0, 22]:
            art.rect(x + dx, 44, 2, 9, RULE)
            art.rect(x + dx - 2, 52, 6, 6, MINT)
        art.rect(x - 4, 28, 10, 10, MINT)
    art.rect(632, 4, 16, 12, PINK)
    return art


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    global COLORS
    outputs = {}
    for suffix, palette in [("", DARK_COLORS), ("-light", LIGHT_COLORS)]:
        COLORS = palette
        social = banner(True)
        outputs.update({f"banner{suffix}.svg": banner().svg(), f"divider{suffix}.svg": divider().svg(),
                        f"social-preview{suffix}.svg": social.svg(), f"social-preview{suffix}.png": social.png()})
    COLORS = DARK_COLORS
    manifest = {
        "generator": "scripts/make-readme-assets.py", "fonts": [],
        "palette": COLORS, "palettes": {"dark": DARK_COLORS, "light": LIGHT_COLORS},
        "palette_source": {"repository": "https://github.com/scottmckendry/cyberdream.nvim", "revision": "eafadf5712e447bb813f8f7eab8012d1391fdba2", "path": "lua/cyberdream/colors.lua", "adaptation": "Cyberdream base/accent colors; app-specific readable muted text."}, "social_preview": {"width": 1280, "height": 640},
        "inspiration": {
            "repository": "https://github.com/undecuple/me",
            "revision": "ce194c20037b15ab9fa03e38f1f2d36c476417b1",
            "assets": ["assets/banner.svg", "assets/divider.svg"],
            "reuse": "Double-rule, checker-dither and frame approach; new generator and lettering."},
    }
    outputs["art-manifest.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    assert len(outputs["social-preview.png"]) < 1_000_000
    assert len(outputs["social-preview-light.png"]) < 1_000_000
    for name, content in outputs.items():
        path = ROOT / "assets" / name
        if args.check:
            if not path.exists() or path.read_bytes() != content:
                raise SystemExit(f"Art differs: assets/{name}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    print("agentVIEW README art: " + ("verified" if args.check else "generated"))


if __name__ == "__main__":
    main()
