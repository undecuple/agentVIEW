"""Raster image checks. No decoding: only the file signature and the header dimensions are read.
SVG is never served. Metadata (EXIF, text chunks) is never read or shown."""
import struct

MAX_IMG_BYTES = 5_000_000
MAX_PIXELS = 16_000_000
MAX_SIDE = 16_384
MAX_SEGMENTS = 1000
EXT = {"png": {".png"}, "jpeg": {".jpg", ".jpeg"}, "gif": {".gif"}, "webp": {".webp"}}
MIME = {"png": "image/png", "jpeg": "image/jpeg", "gif": "image/gif", "webp": "image/webp"}
ALL_EXT = {e for v in EXT.values() for e in v}


class BadImage(Exception):
    pass


def kind(data):
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if data.startswith(b"\xff\xd8\xff"):
        return "jpeg"
    if data[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "webp"
    raise BadImage("not a supported raster image")


def size(data, k):
    try:
        if k == "png":
            if data[12:16] != b"IHDR":
                raise BadImage("bad png")
            return struct.unpack(">II", data[16:24])
        if k == "gif":
            return struct.unpack("<HH", data[6:10])
        if k == "webp":
            c = data[12:16]
            if c == b"VP8X":
                return (int.from_bytes(data[24:27], "little") + 1, int.from_bytes(data[27:30], "little") + 1)
            if c == b"VP8L":
                b = int.from_bytes(data[21:25], "little")
                return ((b & 0x3FFF) + 1, ((b >> 14) & 0x3FFF) + 1)
            if c == b"VP8 ":
                w, h = struct.unpack("<HH", data[26:30])
                return (w & 0x3FFF, h & 0x3FFF)
            raise BadImage("bad webp")
        # jpeg: walk markers to a start-of-frame
        i = 2
        for _ in range(MAX_SEGMENTS):
            if i + 9 >= len(data):
                break
            if data[i] != 0xFF:
                raise BadImage("bad jpeg")
            m = data[i + 1]
            if m in (0xD8, 0x01) or 0xD0 <= m <= 0xD7:
                i += 2
                continue
            ln = struct.unpack(">H", data[i + 2:i + 4])[0]
            if m in (0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF):
                h, w = struct.unpack(">HH", data[i + 5:i + 9])
                return (w, h)
            i += 2 + ln
        raise BadImage("no jpeg frame header")
    except struct.error:
        raise BadImage("truncated header")


def validate(data, name):
    """Return the mime type, or raise BadImage."""
    k = kind(data)
    ext = "." + name.rsplit(".", 1)[-1].lower() if "." in name else ""
    if ext not in EXT[k]:
        raise BadImage("extension does not match file signature")
    w, h = size(data, k)
    if w < 1 or h < 1 or w > MAX_SIDE or h > MAX_SIDE or w * h > MAX_PIXELS:
        raise BadImage("image dimensions out of range")
    return MIME[k]
