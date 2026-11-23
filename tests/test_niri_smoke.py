"""Real-image controls for the nested Niri smoke's rendered-content check.

No compositor is launched. The valid blank fixtures deliberately exceed the old
1000-byte screenshot criterion, so a live but blank webview cannot pass merely
because a screenshot file was produced. ImageMagick must actually decode them.
"""
import binascii
import importlib.util
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib


SPEC = importlib.util.spec_from_file_location(
    "agentview_niri_smoke", Path(__file__).resolve().parents[1] / "scripts/niri-smoke.py")
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


def png_fixture(width, height, pixel, rgba=False):
    """Small PNG with stored compression: blank fixtures stay above 1 KB."""
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel(x, y))

    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data +
                struct.pack(">I", binascii.crc32(kind + data) & 0xffffffff))

    return (b"\x89PNG\r\n\x1a\n" +
            chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6 if rgba else 2, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress(rows, level=0)) + chunk(b"IEND", b""))


@unittest.skipUnless(shutil.which("magick"),
                     "ImageMagick is unavailable; run real-pixel smoke controls inside nix develop")
class RenderControls(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="agentview-render-control-")
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)

    def fixture(self, name, pixel, rgba=False):
        path = self.directory / name
        path.write_bytes(png_fixture(192, 128, pixel, rgba))
        self.assertGreater(path.stat().st_size, 1000,
                           "Control must satisfy the obsolete screenshot-size criterion")
        return path

    def test_dark_blank_window_is_rejected(self):
        blank = self.fixture("dark.png", lambda x, y: (16, 16, 18))
        with self.assertRaises(RuntimeError):
            SMOKE.check_render(blank)

    def test_white_blank_webview_is_rejected(self):
        blank = self.fixture("white.png", lambda x, y: (255, 255, 255))
        with self.assertRaises(RuntimeError):
            SMOKE.check_render(blank)

    def test_opaque_rgba_dark_blank_is_rejected(self):
        blank = self.fixture("rgba-dark.png", lambda x, y: (16, 16, 18, 255), rgba=True)
        with self.assertRaises(RuntimeError):
            SMOKE.check_render(blank)

    def test_opaque_rgba_white_blank_is_rejected(self):
        blank = self.fixture("rgba-white.png", lambda x, y: (255, 255, 255, 255), rgba=True)
        with self.assertRaises(RuntimeError):
            SMOKE.check_render(blank)

    def test_invisible_blue_pixels_do_not_count_as_rendered_content(self):
        invisible = self.fixture("invisible-blue.png", lambda x, y: (149, 98, 245, 0), rgba=True)
        with self.assertRaises(RuntimeError):
            SMOKE.check_render(invisible)

    def test_workspace_accent_frame_is_accepted(self):
        def workspace(x, y):
            # A Cyberdream blue panel header and border against the dark workspace surface.
            if 24 <= x < 168 and (24 <= y < 32 or 96 <= y < 100):
                return (149, 98, 245)
            if 24 <= y < 100 and (24 <= x < 28 or 164 <= x < 168):
                return (149, 98, 245)
            return (30, 33, 36)

        frame = self.fixture("workspace-frame.png", workspace)
        self.assertGreater(SMOKE.check_render(frame), 0)
        rgba_frame = self.fixture("rgba-workspace-frame.png",
                                  lambda x, y: (*workspace(x, y), 255), rgba=True)
        self.assertGreater(SMOKE.check_render(rgba_frame), 0)

    def test_light_cyberdream_accent_is_accepted(self):
        frame=self.fixture("light-frame.png",lambda x,y:(0,87,209) if 24<=x<168 and 24<=y<32 else (255,255,255))
        self.assertGreater(SMOKE.check_render(frame,theme="light"),0)
        with self.assertRaises(RuntimeError):SMOKE.check_render(frame,theme="dark")

    def test_legacy_pink_is_not_the_new_palette_oracle(self):
        frame=self.fixture("old-pink.png",lambda x,y:(233,129,165) if 24<=x<168 and 24<=y<32 else (30,33,36))
        with self.assertRaises(RuntimeError):SMOKE.check_render(frame)

    def test_neutral_compositor_focus_ring_does_not_pass_a_blank_webview(self):
        frame=self.fixture("focus-ring.png",lambda x,y:(123,132,150) if x<4 or x>=188 or y<4 or y>=124 else (19,24,31))
        with self.assertRaises(RuntimeError):SMOKE.check_render(frame)

    def test_solid_accent_blank_window_is_rejected(self):
        blank=self.fixture("blue-blank.png",lambda x,y:(149,98,245))
        with self.assertRaises(RuntimeError):SMOKE.check_render(blank)

    def test_malformed_image_is_rejected_by_decoder(self):
        malformed = self.directory / "malformed.png"
        malformed.write_bytes(b"This is not a PNG image.\n" * 64)
        self.assertGreater(malformed.stat().st_size, 1000)
        with self.assertRaises(subprocess.CalledProcessError):
            SMOKE.check_render(malformed)


if __name__ == "__main__":
    unittest.main()
