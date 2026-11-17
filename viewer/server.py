"""Read-only viewer. Localhost only. Never writes, never grants approvals."""
import argparse
import difflib
import html
import os
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, urlparse

sys.path.insert(0, os.path.dirname(__file__))
import gate  # noqa: E402
import images  # noqa: E402
import paths  # noqa: E402

MAX_BYTES = 1_000_000
GATE_BYTES = 16_000_000
DIFF_BYTES = 200_000
DIFF_LINES = 2000
MAX_REF_LINES = 5000
REF_RE = re.compile(r"!\[[^\]]{0,200}\]\(([^)]{1,300})\)")
PAGE = """<!doctype html><meta charset=utf-8><title>agent-app</title>
<style>body{{font:15px system-ui;max-width:60rem;margin:1rem auto;padding:0 1rem}}
nav a{{margin-right:1rem}}pre{{white-space:pre-wrap;background:#8881;padding:.5rem}}
.add{{color:#0a0}}.del{{color:#c00}}</style>
<nav><a href="/">Docs</a><a href="/gate">Gate status</a><a href="/diff">Diff</a></nav>
<p><i>Read-only. This page never grants an approval; approval is given by a person, elsewhere.</i></p>
{body}"""


def make_handler(cfg, gate_file):
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def send(self, body, code=200):
            data = PAGE.format(body=body).encode()
            self.send_response(code)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("Cross-Origin-Resource-Policy", "same-origin")
            self.send_header("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; img-src 'self'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(data)

        def send_image(self, p):
            if self.headers.get("Sec-Fetch-Site", "same-origin") not in ("same-origin", "none"):
                raise paths.Refused("cross-site request")
            real, data = paths.read_bytes_checked(p, cfg, images.MAX_IMG_BYTES)
            try:
                mime = images.validate(data, real)
            except images.BadImage as ex:
                raise paths.Refused(str(ex))
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'none'; sandbox; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("Cross-Origin-Resource-Policy", "same-origin")
            self.end_headers()
            self.wfile.write(data)

        def doc_images(self, real, text):
            """Relative markdown image refs, resolved against the doc and policy-checked.
            Bounded: at most MAX_REF_LINES lines scanned, lines cut at 2000 characters, 20 refs in all."""
            out = []
            for line in text.splitlines()[:MAX_REF_LINES]:
                for m in REF_RE.finditer(line[:2000]):
                    if len(out) >= 20:
                        break
                    ref = m.group(1).strip().split(None, 1)[0] if m.group(1).strip() else ""
                    shown = html.escape(paths.redact(ref[:80]))
                    if not ref or "://" in ref or ref.startswith(("/", "data:", "~")):
                        out.append(f"<li>not shown (only relative local images): {shown}</li>")
                        continue
                    target = os.path.join(os.path.dirname(real), ref)
                    try:
                        paths.check(target, cfg)
                        ok = os.path.splitext(target)[1].lower() in images.ALL_EXT
                    except paths.Refused:
                        ok = False
                    if ok:
                        out.append(f'<li><img src="/img?p={quote(os.path.realpath(target))}" alt="" style="max-width:100%"></li>')
                    else:
                        out.append(f"<li>not shown: {shown}</li>")
                if len(out) >= 20:
                    out.append("<li>more references not processed (limit 20)</li>")
                    break
            return f"<h3>Images referenced</h3><ul>{''.join(out)}</ul>" if out else ""

        def read(self, p, cap=MAX_BYTES):
            return paths.read_checked(p, cfg, cap)

        def do_GET(self):
            host = (self.headers.get("Host") or "").split(":")[0]
            if host not in ("127.0.0.1", "localhost"):
                return self.send("bad host", 403)
            u = urlparse(self.path)
            q = parse_qs(u.query)
            e = html.escape
            try:
                if u.path == "/":
                    items = "".join(
                        f'<li><a href="/view?p={quote(f)}">{e(paths.short(f))}</a></li>'
                        for f in paths.listing(cfg))
                    return self.send(f"<h2>Allow-listed docs</h2><ul>{items}</ul>")
                if u.path == "/img":
                    return self.send_image(q.get("p", [""])[0])
                if u.path == "/view":
                    real = paths.check(q.get("p", [""])[0], cfg)
                    if os.path.splitext(real)[1].lower() in images.ALL_EXT:
                        return self.send(f'<h2>{e(paths.short(real))}</h2><img src="/img?p={quote(real)}" alt="" style="max-width:100%">')
                    real, text, trunc = self.read(q.get("p", [""])[0])
                    note = f"<p><b>Truncated: showing the first {MAX_BYTES} bytes.</b></p>" if trunc else ""
                    return self.send(f"<h2>{e(paths.short(real))}</h2>{note}<pre>{e(paths.redact(text))}</pre>{self.doc_images(real, text)}")
                if u.path == "/gate":
                    if not gate_file:
                        return self.send("<p>No gate file configured.</p>")
                    _, text, trunc = self.read(gate_file, GATE_BYTES)
                    note = ("<p><b>Truncated: the file is larger than the read limit, older rows are missing.</b></p>"
                            if trunc else "")
                    rows = "".join(f"<li>{e(paths.redact(r))}</li>" for r in gate.pending(text))
                    return self.send("<h2>Possibly pending (best effort)</h2>"
                                     "<p>Best-effort text parse. A PASS row is listed when no later row names the same head "
                                     "with a closing word. A PASS row you have just approved stays listed until a separate later row (the read-back) names the same head. An empty list does NOT mean nothing is pending; check the source.</p>"
                                     f"{note}<ul>{rows}</ul>")
                if u.path == "/diff":
                    a, b = q.get("a", [""])[0], q.get("b", [""])[0]
                    if not a or not b:
                        return self.send("<p>Use /diff?a=PATH&amp;b=PATH, both allow-listed.</p>")
                    ra, ta, t1 = self.read(a, DIFF_BYTES)
                    rb, tb, t2 = self.read(b, DIFF_BYTES)
                    lines = []
                    if t1 or t2:
                        lines.append(e(f"[inputs truncated to {DIFF_BYTES} bytes each]"))
                    ta, tb = paths.redact(ta), paths.redact(tb)
                    for l in difflib.unified_diff(ta.splitlines(), tb.splitlines(), paths.short(ra), paths.short(rb), lineterm=""):
                        if len(lines) >= DIFF_LINES:
                            lines.append(e("[output truncated]"))
                            break
                        c = "add" if l.startswith("+") else "del" if l.startswith("-") else ""
                        lines.append(f'<span class="{c}">{e(l)}</span>')
                    return self.send("<pre>" + "\n".join(lines) + "</pre>")
                return self.send("not found", 404)
            except paths.Refused as ex:
                return self.send(f"<p>Refused: {e(paths.short(str(ex)))}</p>", 403)
            except OSError:
                return self.send("<p>Cannot read file.</p>", 404)

        do_POST = do_PUT = do_DELETE = lambda self: self.send("read-only", 405)
    return H


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--config", default=os.path.join(os.path.dirname(__file__), "..", "allowlist.toml"))
    ap.add_argument("--gate-file", default=None, help="path to the gate log file; must also be allow-listed")
    ap.add_argument("--port", type=int, default=8765)
    a = ap.parse_args()
    try:
        cfg = paths.load_config(a.config)
    except (ValueError, OSError) as ex:
        sys.exit(f"config error: {ex}")
    srv = ThreadingHTTPServer(("127.0.0.1", a.port), make_handler(cfg, a.gate_file))
    print(f"http://127.0.0.1:{a.port}/")
    srv.serve_forever()


if __name__ == "__main__":
    main()
