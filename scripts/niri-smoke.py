#!/usr/bin/env python3
"""Private nested Niri/Wayland smoke for AppImages or synthetic test binaries.
Never attaches to the real desktop.

The shell entrypoint always creates an Xvfb. No --session flag or host config,
runtime directory, save data, IPC socket, clipboard or audio server is reused.
This tests native rendering/tiling/scaling, not physical input or performance.
"""
import argparse
from contextlib import suppress
import json
import os
from pathlib import Path
import signal
import shutil
import stat
import subprocess
import tempfile
import time
import re


def check_render(path, theme="dark"):
    accent={"dark":"#5ea1ff","light":"#0057d1"}[theme]
    background={"dark":"#16181a","light":"#ffffff"}[theme]
    result = subprocess.run(["magick", str(path), "-background", background, "-alpha", "remove", "-alpha", "off",
                             "-fuzz", "14%", "-fill", "black", "+opaque", accent,
                             "-fill", "white", "-opaque", accent, "-format", "%[fx:mean]", "info:"],
                            capture_output=True, text=True, check=True, timeout=10)
    fraction = float(result.stdout)
    if not .0001 < fraction < .25:
        raise RuntimeError("Workspace accent pixels absent; refusing a blank-window pass")
    return fraction


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--native-fixture", action="store_true", help="Test-only workspace-e2e binary with synthetic live terminals")
    parser.add_argument("--theme", choices=["dark","light"], default="dark", help="Synthetic native fixture theme")
    args = parser.parse_args()
    if args.theme!="dark" and not args.native_fixture:parser.error("Light fixture theme requires --native-fixture")
    image = args.image.resolve(strict=True)
    run = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="agentview-niri-"))
    if args.output:
        run.mkdir(parents=True, exist_ok=False)
    os.chmod(run, 0o700)
    (run / "harness.pid").write_text(str(os.getpid()))
    for name in ["runtime", "data", "config", "cache", "tmp"]:
        (run / name).mkdir(mode=0o700)
    (run / "config/agent-app").mkdir(mode=0o700)
    (run / "config/agent-app/allowlist.toml").write_text("allow=[]\n")
    os.chmod(run / "config/agent-app/allowlist.toml", 0o600)
    config = run / "niri.kdl"
    config.write_text('animations { off; }\nhotkey-overlay { skip-at-startup; }\nlayout { default-column-width { proportion 0.5; }; focus-ring { active-color "#7b8496"; }; }\n')
    bus_config = run / "session-bus.conf"
    bus_config.write_text(f'<busconfig><type>session</type><listen>unix:tmpdir={run / "runtime"}</listen>'
                          '<auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>'
                          '<allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    if args.native_fixture:
        fixture = run / "tmp/claude-fixture"
        fixture.write_text("#!" + shutil.which("python3") + "\n" + (Path(__file__).resolve().parents[1] / "tests/live-cli-fixture.py").read_text().split("\n", 1)[1])
        fixture.chmod(0o700)
        codex_fixture=run / "tmp/codex-fixture"
        codex_fixture.write_bytes(fixture.read_bytes());codex_fixture.chmod(0o700)
        for project in ["alpha", "beta"]:
            (run / "tmp" / project).mkdir(mode=0o700)
    niri = str(Path(shutil.which(os.environ.get("NIRI_BINARY", "niri")) or "missing-niri").resolve(strict=True))
    version = subprocess.check_output([niri, "--version"], text=True).strip()
    env = os.environ.copy()
    for key in ["WAYLAND_DISPLAY", "NIRI_SOCKET", "DBUS_SESSION_BUS_ADDRESS", "PULSE_SERVER", "PIPEWIRE_REMOTE"]:
        env.pop(key, None)
    env.update(XDG_RUNTIME_DIR=str(run / "runtime"), XDG_CONFIG_HOME=str(run / "config"),
               XDG_DATA_HOME=str(run / "data"), XDG_CACHE_HOME=str(run / "cache"),
               TMPDIR=str(run / "tmp"), LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe")
    subprocess.run([niri, "validate", "--config", str(config)], env=env, check=True, capture_output=True)
    compositor = app = None
    cid = run / "container.id"
    def interrupted(signum, frame):
        raise InterruptedError("Private Niri test interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)

    def ipc(*command, json_output=False, timeout=10):
        sock = env.get("NIRI_SOCKET")
        if not sock or Path(sock).parent != run / "runtime":
            raise RuntimeError("Refusing IPC outside the task-owned runtime directory")
        cmd = [niri, "msg"] + (["--json"] if json_output else []) + list(command)
        result = subprocess.run(cmd, env=env, check=True, text=True, capture_output=True, timeout=timeout)
        return json.loads(result.stdout) if json_output else result.stdout

    def until(fn, label, seconds=25):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if fn():
                return
            if compositor.poll() is not None:
                raise RuntimeError("Nested compositor exited; inspect niri.log")
            if app is not None and app.poll() is not None:
                raise RuntimeError("AppImage exited; inspect app.log")
            time.sleep(.1)
        raise RuntimeError("Timed out waiting for " + label)

    try:
        with (run / "niri.log").open("w") as log:
            compositor = subprocess.Popen(["dbus-run-session", "--config-file", str(bus_config), "--", niri, "--config", str(config)],
                                          env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        (run / "compositor.pid").write_text(str(compositor.pid))
        sockets = lambda: [p for p in (run / "runtime").glob("niri.*.sock") if stat.S_ISSOCK(p.stat().st_mode)]
        until(lambda: bool(sockets()), "private compositor IPC")
        env["NIRI_SOCKET"] = str(sockets()[0])
        displays = [p.name for p in (run / "runtime").glob("wayland-*") if stat.S_ISSOCK(p.stat().st_mode)]
        if len(displays) != 1:
            raise RuntimeError("Expected one private Wayland display")
        output_info = ipc("outputs", json_output=True)
        (run / "outputs.json").write_text(json.dumps(output_info, indent=2))
        output_name = next(iter(output_info))
        command = ["podman", "run", "--rm", "--network=none", "--userns=host", "--cidfile", str(cid),
                   "-v", f"{image}:/app/agentVIEW.AppImage:ro", "-v", f"{run / 'runtime'}:/run/agentview-runtime",
                   "-v", f"{run / 'data'}:/data", "-v", f"{run / 'config'}:/config",
                   "-e", "XDG_RUNTIME_DIR=/run/agentview-runtime", "-e", f"WAYLAND_DISPLAY={displays[0]}",
                   "-e", "XDG_DATA_HOME=/data", "-e", "XDG_CONFIG_HOME=/config", "-e", "GDK_BACKEND=wayland",
                   "-e", "LIBGL_ALWAYS_SOFTWARE=1", "-e", "WEBKIT_DISABLE_COMPOSITING_MODE=1",
                   "-e", "WEBKIT_DISABLE_DMABUF_RENDERER=1", "agent-app-test:jammy",
                   "dbus-run-session", "--", "/app/agentVIEW.AppImage", "--appimage-extract-and-run"]
        app_env = None
        if args.native_fixture:
            app_env = env.copy()
            app_env.pop("DISPLAY", None)
            app_env.update(GDK_BACKEND="wayland", WAYLAND_DISPLAY=displays[0],
                           WEBKIT_DISABLE_COMPOSITING_MODE="1", WEBKIT_DISABLE_DMABUF_RENDERER="1",
                           AGENT_APP_DATA_DIR=str(run / "data/agent-app"),
                           AGENT_APP_CONFIG=str(run / "config/agent-app/allowlist.toml"),
                           AGENT_APP_TEST_LIVE="1", AGENT_APP_TEST_PHASE="first",
                           AGENT_APP_TEST_CLAUDE=str(fixture), AGENT_APP_TEST_CODEX=str(codex_fixture), AGENT_APP_TEST_PROJECT_ROOT=str(run / "tmp"),
                           AGENT_APP_TEST_LAYOUT="1", AGENT_APP_TEST_THEME=args.theme)
            command = ["dbus-run-session", "--config-file", str(bus_config), "--", str(image)]
        with (run / "app.log").open("w") as log:
            app = subprocess.Popen(command, env=app_env, stdout=log, stderr=subprocess.STDOUT, start_new_session=args.native_fixture)
        windows = lambda: [w for w in ipc("windows", json_output=True) if "agentVIEW" in (w.get("title") or "")]
        until(lambda: bool(windows()), "agentVIEW native Wayland window")
        before = windows()[0]
        if args.native_fixture:
            until(lambda: "WORKSPACE_E2E phase=live-first-preview" in (run / "app.log").read_text(), "live fixture GUI acceptance")
        (run / "window.json").write_text(json.dumps(before, indent=2))
        ipc("action", "focus-window", "--id", str(before["id"]))
        fractions = {}
        cases = [("narrow", "55%", "1"), ("wide", "100%", "1"), ("scaled", "100%", "1.5")]
        if args.native_fixture:
            cases += [("scale125", "100%", "1.25"), ("scale200", "100%", "2")]
        for label, width, scale in cases:
            layout_before = (run / "app.log").read_text().count("WORKSPACE_E2E layout=") if args.native_fixture else 0
            ipc("output", output_name, "scale", scale)
            ipc("action", "set-column-width", width)
            time.sleep(1)
            shot = run / (label + ".png")
            ipc("action", "screenshot-screen", "--show-pointer", "false", "--path", str(shot))
            until(lambda: shot.exists() and shot.stat().st_size > 1000, "screenshot " + label)
            fractions[label] = check_render(shot,theme=args.theme)
            if args.native_fixture:
                until(lambda: (run / "app.log").read_text().count("WORKSPACE_E2E layout=") > layout_before, "terminal fit assertion " + label)
            if app.poll() is not None or not windows():
                raise RuntimeError("App did not survive resize/scale")
        if not (run / "data/agent-app/workspace.sqlite3").is_file():
            raise RuntimeError("Application database was not created")
        if args.native_fixture:
            if app.wait(timeout=35) != 0 or "WORKSPACE_E2E result=pass" not in (run / "app.log").read_text():
                raise RuntimeError("Native live fixture GUI acceptance failed")
        result = {"niri": version, "binary": niri, "image": image.name,
                  "backend": "native Wayland inside nested Niri on private Xvfb",
                  "client": "native workspace-e2e synthetic terminals" if args.native_fixture else "release AppImage in Ubuntu container",
                  "app_id": before.get("app_id"), "cases": [f"{width} column / scale {scale}" for _, width, scale in cases],
                  "accent_fractions": fractions, "palette": "cyberdream-"+args.theme,
                  "limits": "software rendering; physical input/IME/clipboard/multi-monitor/performance unverified"}
        (run / "result.json").write_text(json.dumps(result, indent=2))
        print("Niri Wayland smoke: PASS", run)
        print(json.dumps(result, sort_keys=True))
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        if cid.exists():
            container_id = cid.read_text().strip()
            if re.fullmatch(r"[0-9a-f]{12,64}", container_id):
                try:
                    subprocess.run(["podman", "stop", "--time", "1", container_id], capture_output=True, timeout=3, check=True)
                except (OSError, subprocess.SubprocessError):
                    with suppress(OSError, subprocess.SubprocessError):
                        subprocess.run(["podman", "kill", container_id], capture_output=True, timeout=2)
        if app is not None:
            try:
                if args.native_fixture and app.poll() is None:
                    with suppress(ProcessLookupError):
                        os.killpg(app.pid, signal.SIGTERM)
                app.wait(timeout=2)
            except subprocess.TimeoutExpired:
                with suppress(OSError, subprocess.SubprocessError):
                    if args.native_fixture:
                        os.killpg(app.pid, signal.SIGKILL)
                    else:
                        app.kill()
                    app.wait(timeout=2)
        if compositor is not None and compositor.poll() is None:
            try:
                ipc("action", "quit", "--skip-confirmation", timeout=2)
                compositor.wait(timeout=2)
            except (OSError, RuntimeError, subprocess.SubprocessError):
                with suppress(OSError, subprocess.SubprocessError):
                    os.killpg(compositor.pid, signal.SIGTERM)
                    try:
                        compositor.wait(timeout=1)
                    except subprocess.TimeoutExpired:
                        os.killpg(compositor.pid, signal.SIGKILL)
                        compositor.wait(timeout=1)
        print("Private Niri evidence:", run)


if __name__ == "__main__":
    main()
