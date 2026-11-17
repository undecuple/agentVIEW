import os
import sys
import tempfile
import struct
import threading
import zlib
import unittest
import urllib.request
import urllib.error

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "viewer"))
import gate  # noqa: E402
import images  # noqa: E402
import paths  # noqa: E402
import server  # noqa: E402
from http.server import ThreadingHTTPServer  # noqa: E402


class Base(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        def w(n, t):
            os.makedirs(os.path.dirname(os.path.join(self.d, n)), exist_ok=True)
            with open(os.path.join(self.d, n), "w") as fh:
                fh.write(t)
        w("docs/a.md", "line1\nline2\n")
        w("docs/b.md", "line1\nline3\n")
        w("docs/work/secret.md", "nope")
        w("docs/key.pem", "nope")
        w("outside.md", "outside")
        os.symlink(os.path.join(self.d, "outside.md"), os.path.join(self.d, "docs", "link.md"))
        os.symlink(os.path.join(self.d, "docs", "work", "secret.md"), os.path.join(self.d, "docs", "ok-name.md"))
        self.cfg = {"allow": [os.path.realpath(os.path.join(self.d, "docs"))],
                    "deny": ["work", "*.pem"]}


class PathTests(Base):
    def test_allowed(self):
        paths.check(os.path.join(self.d, "docs/a.md"), self.cfg)

    def test_excluded_folder_refused(self):
        with self.assertRaises(paths.Refused):
            paths.check(os.path.join(self.d, "docs/work/secret.md"), self.cfg)

    def test_denied_extension_refused_case_insensitive(self):
        with self.assertRaises(paths.Refused):
            paths.check(os.path.join(self.d, "docs/key.pem"), self.cfg)

    def test_symlink_out_of_allow_list_refused(self):
        with self.assertRaises(paths.Refused):
            paths.check(os.path.join(self.d, "docs/link.md"), self.cfg)

    def test_symlink_into_denied_folder_refused(self):
        with self.assertRaises(paths.Refused):
            paths.check(os.path.join(self.d, "docs/ok-name.md"), self.cfg)

    def test_dotdot_refused(self):
        with self.assertRaises(paths.Refused):
            paths.check(os.path.join(self.d, "docs/../outside.md"), self.cfg)

    def test_listing_excludes_denied(self):
        names = [os.path.basename(p) for p in paths.listing(self.cfg)]
        self.assertEqual(sorted(names), ["a.md", "b.md"])


class ConfigTests(unittest.TestCase):
    def write(self, text):
        d = tempfile.mkdtemp()
        p = os.path.join(d, "c.toml")
        with open(p, "w") as fh:
            fh.write(text)
        return p

    def test_missing_deny_names_uses_defaults(self):
        cfg = paths.load_config(self.write('allow = ["/tmp"]\n'))
        self.assertIn("work", cfg["deny"])
        self.assertIn("*.pem", cfg["deny"])

    def test_file_can_only_extend_deny(self):
        cfg = paths.load_config(self.write('allow = ["/tmp"]\ndeny_names = ["extra"]\n'))
        self.assertIn("extra", cfg["deny"])
        self.assertIn("work", cfg["deny"])

    def check_with(self, text, path):
        cfg = paths.load_config(self.write(text))
        return paths.check(path, cfg)

    def test_defaults_apply_at_check_level(self):
        d = tempfile.mkdtemp()
        os.makedirs(os.path.join(d, ".ssh"))
        for n in [".ssh/id_x", ".SSH/x", "KEY.PEM", ".aws", ".git-credentials", "id_rsa.pub", "a.kdbx"]:
            path = os.path.join(d, n)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as fh:
                fh.write("x")
            with self.assertRaises(paths.Refused, msg=n):
                self.check_with(f'allow = ["{d}"]\n', path)

    def test_file_cannot_shrink_defaults(self):
        d = tempfile.mkdtemp()
        os.makedirs(os.path.join(d, "work"))
        with open(os.path.join(d, "work", "x.md"), "w") as fh:
            fh.write("x")
        with self.assertRaises(paths.Refused):
            self.check_with(f'allow = ["{d}"]\ndeny_names = []\n', os.path.join(d, "work", "x.md"))

    def test_bad_allow_entries(self):
        for bad in ["./x", "x/../y", "~user/x", "/", "/etc", "/proc/self", "~", os.path.expanduser("~"), "/home"]:
            with self.assertRaises(ValueError, msg=bad):
                paths.load_config(self.write(f'allow = ["{bad}"]\n'))
        for text in ['allow = [1]\n', 'allow = ["/tmp"]\ndeny_names = "work"\n']:
            with self.assertRaises(ValueError):
                paths.load_config(self.write(text))

    def test_relative_allow_refused(self):
        with self.assertRaises(ValueError):
            paths.load_config(self.write('allow = ["docs"]\n'))


class GateTests(unittest.TestCase):
    def test_pending(self):
        t = ("| d | x | **t: posted on abc1234** |\n"
             "| d | x | **t: PASS on abc1234** |\n"
             "| d | x | **u: PASS on def5678** awaiting approval |\n")
        rows = gate.pending(t)
        self.assertEqual(len(rows), 1)
        self.assertIn("def5678", rows[0])


class GateEdgeTests(unittest.TestCase):
    def test_self_closing_row_stays_pending(self):
        t = "| d | x | **t: PASS on abc1234, approved later** |\n"
        self.assertEqual(len(gate.pending(t)), 1)

    def test_not_approved_does_not_close(self):
        t = ("| d | x | **t: not approved yet, abc1234** |\n"
             "| d | x | **t: PASS on abc1234** |\n")
        self.assertEqual(len(gate.pending(t)), 1)

    def test_other_hash_does_not_close(self):
        t = ("| d | x | **t: approved abc9999** |\n"
             "| d | x | **t: PASS on abc1234** |\n")
        self.assertEqual(len(gate.pending(t)), 1)

    def test_same_hash_closes(self):
        t = ("| d | x | **t: approved abc1234** |\n"
             "| d | x | **t: PASS on abc1234** |\n")
        self.assertEqual(gate.pending(t), [])

    def test_plain_words_are_not_hashes(self):
        t = "| d | x | **t: PASS on deadbeef, decaded** |\n"
        self.assertEqual(len(gate.pending(t)), 1)


class ReadTests(Base):
    def test_symlink_swap_refused(self):
        real = os.path.join(self.d, "docs", "a.md")
        orig = paths.check

        def swapped(path, cfg):
            r = orig(path, cfg)
            os.rename(r, r + ".bak")
            os.symlink(os.path.join(self.d, "outside.md"), r)
            return r
        paths.check = swapped
        try:
            with self.assertRaises(paths.Refused):
                paths.read_checked(real, self.cfg, 1000)
        finally:
            paths.check = orig

    def test_redact(self):
        fake = "gh" + "p_" + "abcdefghijklmnopqrstuvwxyz01"
        self.assertNotIn(fake, paths.redact("x " + fake + " y"))
        line = "tok" + "en = " + chr(34) + "abcdefghijklmnop" + chr(34)
        self.assertIn("[redacted]", paths.redact(line))


class HttpTests(Base):
    def setUp(self):
        super().setUp()
        self.srv = ThreadingHTTPServer(("127.0.0.1", 0), server.make_handler(self.cfg, None))
        self.port = self.srv.server_address[1]
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()

    def get(self, p, host=None):
        r = urllib.request.Request(f"http://127.0.0.1:{self.port}{p}")
        if host:
            r.add_header("Host", host)
        try:
            with urllib.request.urlopen(r) as x:
                return x.status, x.read().decode()
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode()

    def test_lists_docs(self):
        s, b = self.get("/")
        self.assertEqual(s, 200)
        self.assertIn("a.md", b)
        self.assertNotIn("secret.md", b)

    def test_excluded_path_refused(self):
        s, _ = self.get("/view?p=" + os.path.join(self.d, "docs/work/secret.md"))
        self.assertEqual(s, 403)

    def test_diff_synthetic(self):
        a, b = (os.path.join(self.d, f"docs/{n}.md") for n in "ab")
        s, body = self.get(f"/diff?a={a}&b={b}")
        self.assertEqual(s, 200)
        self.assertIn("-line2", body)
        self.assertIn("+line3", body)

    def test_diff_refuses_excluded(self):
        a = os.path.join(self.d, "docs/a.md")
        s, _ = self.get(f"/diff?a={a}&b={os.path.join(self.d, 'docs/work/secret.md')}")
        self.assertEqual(s, 403)

    def test_bad_host_refused(self):
        s, _ = self.get("/", host="evil.example")
        self.assertEqual(s, 403)

    def test_host_variants(self):
        for h, code in [("localhost:1234", 200), ("127.0.0.1", 200), ("127.0.0.1.evil.com", 403),
                        ("evil.com:80", 403), ("[::1]", 403)]:
            self.assertEqual(self.get("/", host=h)[0], code, h)

    def test_missing_host_refused(self):
        import socket
        with socket.create_connection(("127.0.0.1", self.port)) as c:
            c.sendall(b"GET / HTTP/1.0\r\n\r\n")
            self.assertIn(b" 403 ", c.recv(200))

    def test_nul_and_double_encoding(self):
        self.assertEqual(self.get("/view?p=" + os.path.join(self.d, "docs/a.md") + "%00.pem")[0], 403)
        self.assertEqual(self.get("/view?p=%252e%252e/outside.md")[0], 403)
        self.assertEqual(self.get("/view?p=" + os.path.join(self.d, "docs/..%2foutside.md"))[0], 403)

    def test_truncated_banner(self):
        p = os.path.join(self.d, "docs/big.md")
        with open(p, "w") as fh:
            fh.write("x" * (server.MAX_BYTES + 10))
        s, body = self.get("/view?p=" + p)
        self.assertIn("Truncated", body)

    def test_error_page_has_no_full_path(self):
        _, body = self.get("/view?p=" + os.path.join(self.d, "docs/work/secret.md"))
        self.assertNotIn(self.d, body)

    def test_headers(self):
        r = urllib.request.urlopen(f"http://127.0.0.1:{self.port}/")
        self.assertEqual(r.headers["Cache-Control"], "no-store")
        self.assertEqual(r.headers["Referrer-Policy"], "no-referrer")

    def test_post_refused(self):
        r = urllib.request.Request(f"http://127.0.0.1:{self.port}/", data=b"x", method="POST")
        with self.assertRaises(urllib.error.HTTPError) as c:
            urllib.request.urlopen(r)
        self.assertEqual(c.exception.code, 405)

    def test_html_escaped(self):
        p = os.path.join(self.d, "docs/x.md")
        with open(p, "w") as fh:
            fh.write("<script>alert(1)</script>")
        _, body = self.get("/view?p=" + p)
        self.assertNotIn("<script>alert", body)


def png(w=2, h=2):
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    raw = b"".join(b"\x00" + b"\x00\x00\x00" * w for _ in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def gif(w=3, h=4):
    return b"GIF89a" + struct.pack("<HH", w, h) + b"\x00\x00\x00;"


def jpeg(w=5, h=6):
    return (b"\xff\xd8\xff\xe0\x00\x04\x00\x00" + b"\xff\xc0\x00\x0b\x08" + struct.pack(">HH", h, w)
            + b"\x01\x01\x11\x00" + b"\xff\xd9")


def webp_vp8x(w=7, h=8):
    body = b"VP8X" + struct.pack("<I", 10) + b"\x00\x00\x00\x00" + (w - 1).to_bytes(3, "little") + (h - 1).to_bytes(3, "little")
    return b"RIFF" + struct.pack("<I", 4 + len(body)) + b"WEBP" + body


class ImageUnitTests(unittest.TestCase):
    def test_valid_types_and_sizes(self):
        self.assertEqual(images.validate(png(), "a.png"), "image/png")
        self.assertEqual(images.validate(gif(), "a.GIF"), "image/gif")
        self.assertEqual(images.validate(jpeg(), "a.jpeg"), "image/jpeg")
        self.assertEqual(images.validate(webp_vp8x(), "a.webp"), "image/webp")
        self.assertEqual(images.size(png(9, 10), "png"), (9, 10))
        self.assertEqual(images.size(jpeg(5, 6), "jpeg"), (5, 6))
        self.assertEqual(images.size(webp_vp8x(7, 8), "webp"), (7, 8))

    def test_refusals(self):
        for data, name in [(png(), "a.jpg"), (b"<svg xmlns='x'/>", "a.svg"), (b"<svg/>", "a.png"),
                           (b"nothing", "a.png"), (png(20000, 2), "a.png"), (png(10000, 10000), "a.png"),
                           (png()[:20], "a.png"), (b"\xff\xd8\xff\xe0", "a.jpg")]:
            with self.assertRaises(images.BadImage, msg=name):
                images.validate(data, name)


class ImageHttpTests(Base):
    def setUp(self):
        super().setUp()
        for n, data in [("docs/ok.png", png()), ("docs/work/hidden.png", png()), ("docs/bad.jpg", png()),
                        ("docs/fake.png", b"<svg onload=alert(1)/>"), ("docs/huge.png", png(20000, 2)),
                        ("docs/big.png", png() + b"\0" * (images.MAX_IMG_BYTES + 1))]:
            with open(os.path.join(self.d, n), "wb") as fh:
                fh.write(data)
        with open(os.path.join(self.d, "docs/doc.md"), "w") as fh:
            fh.write("![a](ok.png) ![b](http://example.com/x.png) ![c](work/hidden.png) ![d](/etc/x.png) ![e](../outside.png)")
        self.srv = ThreadingHTTPServer(("127.0.0.1", 0), server.make_handler(self.cfg, None))
        self.port = self.srv.server_address[1]
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()

    def get(self, p):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{self.port}{p}") as r:
                return r.status, r.headers, r.read()
        except urllib.error.HTTPError as e:
            try:
                return e.code, e.headers, e.read()
            finally:
                e.close()

    def img(self, n):
        return self.get("/img?p=" + os.path.join(self.d, "docs", n))

    def test_serves_png_with_safe_headers(self):
        s, h, b = self.img("ok.png")
        self.assertEqual(s, 200)
        self.assertEqual(h["Content-Type"], "image/png")
        self.assertEqual(h["X-Content-Type-Options"], "nosniff")
        self.assertIn("sandbox", h["Content-Security-Policy"])
        self.assertEqual(b, png())

    def test_refused_cases(self):
        for n in ["work/hidden.png", "bad.jpg", "fake.png", "huge.png", "big.png", "doc.md"]:
            self.assertEqual(self.img(n)[0], 403, n)
        self.assertEqual(self.img("missing.png")[0], 403)  # not a file: refused by policy, same as denied

    def test_outside_allow_list_refused(self):
        self.assertEqual(self.get("/img?p=" + os.path.join(self.d, "outside.md"))[0], 403)

    def test_view_image_path_shows_img_tag(self):
        s, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/ok.png"))
        self.assertEqual(s, 200)
        self.assertIn(b"/img?p=", b)

    def test_markdown_images_only_policy_passing_relative(self):
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/doc.md"))
        t = b.decode()
        self.assertEqual(t.count("<img "), 1)
        self.assertIn("ok.png", t)
        self.assertIn("not shown", t)
        self.assertNotIn("example.com/x.png\"", t)


class ImageEdgeTests(ImageHttpTests):
    def write(self, name, data, mode="wb"):
        with open(os.path.join(self.d, "docs", name), mode) as fh:
            fh.write(data)

    def test_twenty_ref_limit_and_unbounded_external_refs(self):
        import time
        self.write("many.md", "![](http://x/a.png)" * 60000, "w")  # about 1 MB of external refs
        t = time.time()
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/many.md"))
        self.assertLess(time.time() - t, 3)
        self.assertLessEqual(b.decode().count("<li>"), 22)
        self.assertIn("limit 20", b.decode())
        self.write("some.md", "![a](ok.png)\n" * 30, "w")
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/some.md"))
        self.assertEqual(b.decode().count("<img "), 20)

    def test_long_line_is_bounded(self):
        import time
        self.write("long.md", "![" + "a" * 900000 + "](x.png", "w")
        t = time.time()
        self.assertEqual(self.get("/view?p=" + os.path.join(self.d, "docs/long.md"))[0], 200)
        self.assertLess(time.time() - t, 3)

    def test_titled_ref_is_resolved(self):
        self.write("t.md", '![a](ok.png "a title")', "w")
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/t.md"))
        self.assertEqual(b.decode().count("<img "), 1)

    def test_not_shown_ref_is_redacted(self):
        fake = "gh" + "p_" + "abcdefghijklmnopqrstuvwxyz01"
        self.write("r.md", f"![a](http://x/{fake})", "w")
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/r.md"))
        self.assertNotIn(fake, b.decode())

    def test_symlinked_image_refused(self):
        os.symlink(os.path.join(self.d, "docs", "ok.png"), os.path.join(self.d, "docs", "ln.png"))
        os.symlink(os.path.join(self.d, "docs", "work", "hidden.png"), os.path.join(self.d, "docs", "ln2.png"))
        self.assertEqual(self.img("ln.png")[0], 200)  # target is inside the allow list and not denied
        self.assertEqual(self.img("ln2.png")[0], 403)  # target is in a denied folder

    def test_size_boundaries(self):
        ok = png() + b"\0" * (images.MAX_IMG_BYTES - len(png()))
        self.write("edge.png", ok)
        self.assertEqual(self.img("edge.png")[0], 200)
        self.write("edge2.png", ok + b"\0")
        self.assertEqual(self.img("edge2.png")[0], 403)
        self.assertEqual(images.validate(png(16384, 1), "a.png"), "image/png")
        self.assertEqual(images.validate(png(4000, 4000), "a.png"), "image/png")
        with self.assertRaises(images.BadImage):
            images.validate(png(4001, 4000), "a.png")
        with self.assertRaises(images.BadImage):
            images.validate(png(16385, 1), "a.png")

    def test_headers_on_all_responses(self):
        for path in ["/", "/view?p=" + os.path.join(self.d, "docs/doc.md"), "/img?p=" + os.path.join(self.d, "docs/ok.png")]:
            with urllib.request.urlopen(f"http://127.0.0.1:{self.port}{path}") as r:
                self.assertEqual(r.headers["Cross-Origin-Resource-Policy"], "same-origin", path)
                self.assertEqual(r.headers["Cache-Control"], "no-store", path)
                self.assertEqual(r.headers["X-Content-Type-Options"], "nosniff", path)

    def test_cross_site_fetch_refused(self):
        for site, code in [("cross-site", 403), ("same-site", 403), ("same-origin", 200), ("none", 200)]:
            r = urllib.request.Request(f"http://127.0.0.1:{self.port}/img?p=" + os.path.join(self.d, "docs/ok.png"))
            r.add_header("Sec-Fetch-Site", site)
            try:
                with urllib.request.urlopen(r) as x:
                    got = x.status
            except urllib.error.HTTPError as e:
                got = e.code
                e.close()
            self.assertEqual(got, code, site)

    def test_view_decides_image_by_resolved_name(self):
        os.symlink(os.path.join(self.d, "docs", "ok.png"), os.path.join(self.d, "docs", "alias.md"))
        _, _, b = self.get("/view?p=" + os.path.join(self.d, "docs/alias.md"))
        self.assertIn(b"<img", b)

    def test_jpeg_marker_loop_is_capped(self):
        data = b"\xff\xd8" + b"\xff\xd0" * 100000
        with self.assertRaises(images.BadImage):
            images.validate(data + b"\0" * 20, "a.jpg")


class GateHttpTests(Base):
    def test_gate_page_and_truncation(self):
        g = os.path.join(self.d, "docs", "up.md")
        with open(g, "w") as fh:
            fh.write("| d | x | **t: PASS on abc1234** |\n")
        srv = ThreadingHTTPServer(("127.0.0.1", 0), server.make_handler(self.cfg, g))
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{srv.server_address[1]}/gate") as r:
                body = r.read().decode()
        finally:
            srv.shutdown()
            srv.server_close()
        self.assertIn("abc1234", body)
        self.assertIn("does NOT mean", body)


if __name__ == "__main__":
    unittest.main()

