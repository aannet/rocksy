"""Minimal Chrome DevTools Protocol driver (standard library only).

Runs on Windows against Microsoft Edge headless. Plays a JSON scenario:
  python drive.py scenario.json [--out DIR] [--dist URL] [--port N]
  Real app (scenarios with an "app" key): [--exe rocksy.exe] [--it TEST_DIR] [--vaults REFERENCE_VAULTS_DIR]
  The app always runs as an isolated instance (ROCKSY_DATA_DIR under the test folder); the default exe is the
  work-in-progress build (the "wip" folder of the test area), never the one the user runs.

Step kinds (one key per step object):
  {"nav": "{DIST}?tree=1000"}            navigate, wait for load
  {"wait": "js expression", "timeout": 10000}
  {"eval": "js (may return a promise)", "save": "name"}
  {"type": "text", "delay": 5}           real key events, one per character
  {"key": "Enter", "mods": "ctrl+shift"}
  {"click": "css selector"} / {"click": [x, y]}  ("button": "middle", "mods": "ctrl")
  {"shot": "name.png"}
  {"sleep": 300}
  {"size": [1400, 900]}
"""
import base64
import json
import os
import socket
import struct
import subprocess
import sys
import time
import urllib.request

EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
BASE = r"C:\Users\aleca\rocksy-test\ui"

KEYS = {
    "Enter": (13, "Enter", "\r"), "Escape": (27, "Escape", None), "Tab": (9, "Tab", None),
    "Backspace": (8, "Backspace", None), "Delete": (46, "Delete", None),
    "ArrowLeft": (37, "ArrowLeft", None), "ArrowUp": (38, "ArrowUp", None),
    "ArrowRight": (39, "ArrowRight", None), "ArrowDown": (40, "ArrowDown", None),
    "Home": (36, "Home", None), "End": (35, "End", None),
    "PageDown": (34, "PageDown", None), "PageUp": (33, "PageUp", None),
}
MODS = {"alt": 1, "ctrl": 2, "meta": 4, "shift": 8}


class WS:
    def __init__(self, url):
        assert url.startswith("ws://")
        hostport, path = url[5:].split("/", 1)
        host, port = hostport.split(":")
        self.s = socket.create_connection((host, int(port)))
        key = base64.b64encode(os.urandom(16)).decode()
        req = (f"GET /{path} HTTP/1.1\r\nHost: {hostport}\r\nUpgrade: websocket\r\n"
               f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
        self.s.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += self.s.recv(4096)
        if b" 101 " not in buf.split(b"\r\n", 1)[0]:
            raise RuntimeError("websocket handshake failed: %r" % buf[:200])
        self.buf = buf.split(b"\r\n\r\n", 1)[1]

    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.s.recv(65536)
            if not chunk:
                raise RuntimeError("socket closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def send(self, text):
        data = text.encode()
        head = bytearray([0x81])
        n = len(data)
        if n < 126:
            head.append(0x80 | n)
        elif n < 65536:
            head.append(0x80 | 126)
            head += struct.pack(">H", n)
        else:
            head.append(0x80 | 127)
            head += struct.pack(">Q", n)
        mask = os.urandom(4)
        head += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        self.s.sendall(bytes(head) + masked)

    def recv(self):
        msg = b""
        while True:
            b0, b1 = self._read(2)
            op = b0 & 0x0F
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._read(8))[0]
            payload = self._read(n)
            if op == 8:
                raise RuntimeError("websocket closed by peer")
            if op in (9, 10):
                continue
            msg += payload
            if b0 & 0x80:
                return msg.decode("utf-8", "replace")


class CDP:
    def __init__(self, ws):
        self.ws = ws
        self.id = 0
        self.events = []

    def call(self, method, **params):
        self.id += 1
        mid = self.id
        self.ws.send(json.dumps({"id": mid, "method": method, "params": params}))
        while True:
            m = json.loads(self.ws.recv())
            if m.get("id") == mid:
                if "error" in m:
                    raise RuntimeError(f"{method}: {m['error']}")
                return m.get("result", {})
            self.events.append(m)

    def evaluate(self, expr):
        r = self.call("Runtime.evaluate", expression=expr, awaitPromise=True, returnByValue=True)
        if "exceptionDetails" in r:
            d = r["exceptionDetails"]
            raise RuntimeError("JS error: " + json.dumps(d.get("exception", d))[:600])
        return r["result"].get("value")


def mods_of(step):
    m = 0
    for name in (step.get("mods") or "").split("+"):
        if name:
            m |= MODS[name]
    return m


def press(cdp, key, mods=0):
    if key in KEYS:
        vk, code, text = KEYS[key]
        base = dict(key=key, code=code, windowsVirtualKeyCode=vk, nativeVirtualKeyCode=vk, modifiers=mods)
        if text and not mods:
            cdp.call("Input.dispatchKeyEvent", type="keyDown", text=text, unmodifiedText=text, **base)
        else:
            cdp.call("Input.dispatchKeyEvent", type="rawKeyDown", **base)
        cdp.call("Input.dispatchKeyEvent", type="keyUp", **base)
        return
    ch = key
    vk = ord(ch.upper()) if len(ch) == 1 and (ch.isalnum()) else 0
    base = dict(key=ch, windowsVirtualKeyCode=vk, nativeVirtualKeyCode=vk, modifiers=mods)
    if len(ch) == 1 and ch.isalpha():
        base["code"] = "Key" + ch.upper()
    if mods & ~MODS["shift"]:
        cdp.call("Input.dispatchKeyEvent", type="rawKeyDown", **base)
    else:
        cdp.call("Input.dispatchKeyEvent", type="keyDown", text=ch, unmodifiedText=ch, **base)
    cdp.call("Input.dispatchKeyEvent", type="keyUp", **base)


def click(cdp, step):
    target = step["click"]
    if isinstance(target, str):
        pt = cdp.evaluate(
            "(()=>{const e=document.querySelector(%s); if(!e) return null; e.scrollIntoView({block:'nearest'});"
            "const r=e.getBoundingClientRect(); return [r.left+Math.min(r.width/2, %s), r.top+r.height/2]})()"
            % (json.dumps(target), step.get("dx", 100000)))
        if not pt:
            raise RuntimeError("click target not found: " + target)
        x, y = pt
    else:
        x, y = target
    button = step.get("button", "left")
    mods = mods_of(step)
    cdp.call("Input.dispatchMouseEvent", type="mouseMoved", x=x, y=y, modifiers=mods)
    cdp.call("Input.dispatchMouseEvent", type="mousePressed", x=x, y=y, button=button, clickCount=1, modifiers=mods)
    cdp.call("Input.dispatchMouseEvent", type="mouseReleased", x=x, y=y, button=button, clickCount=1, modifiers=mods)


IT = r"C:\Users\aleca\rocksy-test\it"
VAULTS = r"C:\Users\aleca\rocksy-test\vaults"
ROCKSY = r"C:\Users\aleca\rocksy-test\wip\rocksy.exe"


def subst(v):
    if isinstance(v, str):
        return v.replace("{IT}", IT).replace("{VAULTS}", VAULTS)
    if isinstance(v, list):
        return [subst(x) for x in v]
    if isinstance(v, dict):
        return {k: subst(x) for k, x in v.items()}
    return v


def js(v):
    return json.dumps(v)


def paths_prelude():
    """Tells the helpers injected in the page where the test folder and the vaults are."""
    return "window.__ROCKSY_PATHS=%s;\n" % json.dumps({"it": IT, "vaults": VAULTS})


def attach(port, want=None, timeout=20):
    """Waits for a debuggable page on `port` and returns a CDP session."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=1) as r:
                for t in json.load(r):
                    if t.get("type") == "page" and (want is None or want in t.get("url", "")):
                        cdp = CDP(WS(t["webSocketDebuggerUrl"]))
                        cdp.call("Page.enable")
                        cdp.call("Runtime.enable")
                        cdp.call("Log.enable")
                        cdp.call("Network.enable")
                        return cdp
        except Exception:
            pass
        time.sleep(0.1)
    raise RuntimeError("no debuggable page on port %d" % port)


def rocksy_running():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq rocksy.exe", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return "rocksy.exe" in out


def kill(proc):
    if proc is None:
        return
    try:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        pass
    for _ in range(50):
        if proc.poll() is not None:
            break
        time.sleep(0.1)


def launch_app(app, port, extra_args=None, debug=True):
    env = dict(os.environ)
    if debug:
        env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={port}"
    else:
        env.pop("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", None)
    data = app["dataDir"]
    assert data.lower().startswith(IT.lower()), "ROCKSY_DATA_DIR must be under the test folder"
    env["ROCKSY_DATA_DIR"] = data
    args = [ROCKSY] + list(extra_args if extra_args is not None else app.get("args", []))
    return subprocess.Popen(args, env=env, cwd=IT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def hash_tree(root):
    import hashlib
    out = {}
    for d, _dirs, files in os.walk(root):
        for f in files:
            full = os.path.join(d, f)
            h = hashlib.sha1()
            with open(full, "rb") as fh:
                while True:
                    b = fh.read(1 << 20)
                    if not b:
                        break
                    h.update(b)
            out[os.path.relpath(full, root)] = [os.path.getsize(full), h.hexdigest()]
    return out


def describe_bytes(b):
    import hashlib
    crlf = b.count(b"\r\n")
    return {"size": len(b), "sha1": hashlib.sha1(b).hexdigest(), "bom": b[:3] == b"\xef\xbb\xbf", "crlf": crlf, "lf": b.count(b"\n") - crlf,
            "endsWithNewline": b.endswith(b"\n"), "tailHex": b[-12:].hex()}


def py_step(step, mem, results):
    kind = step["py"]
    if kind == "hash":
        mem[step["save"]] = hash_tree(step["dir"])
        return {"files": len(mem[step["save"]])}
    if kind == "diff":
        a, b = mem[step["a"]], mem[step["b"]]
        return {"added": sorted(set(b) - set(a)), "removed": sorted(set(a) - set(b)), "changed": sorted(k for k in a if k in b and a[k] != b[k]), "files": len(b)}
    if kind == "snap":
        with open(step["path"], "rb") as f:
            mem[step["save"]] = f.read()
        return describe_bytes(mem[step["save"]])
    if kind == "linediff":
        before = mem[step["snap"]]
        with open(step["path"], "rb") as f:
            after = f.read()
        la, lb = before.split(b"\n"), after.split(b"\n")
        import difflib
        sm = difflib.SequenceMatcher(None, la, lb, autojunk=False)
        ops = [op for op in sm.get_opcodes() if op[0] != "equal"]
        changed = [{"op": op[0], "before": [x.decode("utf-8", "replace")[:80] for x in la[op[1]:op[2]]][:4], "after": [x.decode("utf-8", "replace")[:80] for x in lb[op[3]:op[4]]][:4]} for op in ops[:6]]
        return {"before": describe_bytes(before), "after": describe_bytes(after), "changedBlocks": len(ops), "changes": changed}
    if kind == "write":
        text = step.get("text", "")
        data = text.encode("utf-8")
        if step.get("crlf"):
            data = data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        if step.get("bom"):
            data = b"\xef\xbb\xbf" + data
        if "hex" in step:
            data = bytes.fromhex(step["hex"])
        os.makedirs(os.path.dirname(step["path"]), exist_ok=True)
        mode = "ab" if step.get("append") else "wb"
        with open(step["path"], mode) as f:
            f.write(data)
        return {"size": os.path.getsize(step["path"])}
    if kind == "delete":
        os.remove(step["path"])
        return True
    if kind == "exists":
        return os.path.exists(step["path"])
    if kind == "listdir":
        return sorted(os.listdir(step["path"]))
    if kind == "copytree":
        r = subprocess.run(["robocopy", step["src"], step["dst"], "/MIR", "/NFL", "/NDL", "/NJH", "/NJS", "/NP", "/R:1", "/W:1"], capture_output=True, text=True)
        if r.returncode >= 8:
            raise RuntimeError("robocopy failed: " + r.stdout[-300:])
        return {"robocopy": r.returncode}
    if kind == "rmtree":
        import shutil
        assert step["path"].lower().startswith(IT.lower())
        shutil.rmtree(step["path"], ignore_errors=True)
        return True
    if kind == "mem":
        # Working set of the app process we launched and of its WebView2 children (matched by our data dir).
        pid = mem["__pid"]
        data = mem["__data"].replace("\\", "\\\\")
        ps = ("$p=Get-Process -Id %d; $wv=Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | Where-Object { $_.CommandLine -like '*%s*' };"
              "@{appMb=[int]($p.WorkingSet64/1MB); appPrivateMb=[int]($p.PrivateMemorySize64/1MB); webviewProcs=($wv|Measure-Object).Count; webviewMb=[int](($wv|Measure-Object WorkingSetSize -Sum).Sum/1MB)} | ConvertTo-Json -Compress") % (pid, mem["__data"])
        r = subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True)
        return json.loads(r.stdout.strip() or "{}")
    if kind == "copyfile":
        import shutil
        os.makedirs(os.path.dirname(step["dst"]), exist_ok=True)
        shutil.copyfile(step["src"], step["dst"])
        return {"size": os.path.getsize(step["dst"])}
    if kind == "waitexit":
        # Waits until the app process we launched is gone; returns the time it took, or -1.
        t0 = time.time()
        deadline = t0 + step.get("waitMs", 8000) / 1000
        while time.time() < deadline:
            r = subprocess.run(["tasklist", "/FI", "PID eq %d" % mem["__pid"], "/NH", "/FO", "CSV"], capture_output=True, text=True)
            if ('"%d"' % mem["__pid"]) not in r.stdout:
                return round((time.time() - t0) * 1000)
            time.sleep(0.05)
        return -1
    if kind == "closetimed":
        # Same as "close", and reports how long the process took to exit after the close request.
        pid = mem["__pid"]
        ps = ("$p=Get-Process -Id %d; $sw=[Diagnostics.Stopwatch]::StartNew(); $ok=$p.CloseMainWindow(); $w=$p.WaitForExit(%d); "
              "\"$ok $w $($sw.ElapsedMilliseconds)\"") % (pid, step.get("waitMs", 8000))
        r = subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True)
        parts = r.stdout.strip().split()
        return {"closeRequested": parts[0] == "True", "exited": parts[1] == "True", "exitMs": int(parts[2])} if len(parts) == 3 else {"raw": r.stdout.strip()}
    if kind == "close":
        # Asks the main window to close (graceful exit), then waits for the process to end.
        pid = mem["__pid"]
        r = subprocess.run(["powershell", "-NoProfile", "-Command", "$p=Get-Process -Id %d; $ok=$p.CloseMainWindow(); $w=$p.WaitForExit(%d); \"$ok $w\"" % (pid, step.get("waitMs", 8000))], capture_output=True, text=True)
        return r.stdout.strip()
    if kind == "alive":
        r = subprocess.run(["tasklist", "/FI", "PID eq %d" % mem["__pid"], "/NH", "/FO", "CSV"], capture_output=True, text=True)
        return str(mem["__pid"]) in r.stdout
    if kind == "jsonl":
        out = {}
        try:
            with open(step["path"], encoding="utf-8") as f:
                for line in f:
                    j = json.loads(line)
                    out.setdefault(j["name"], round(j.get("sinceProcessStartMs", j.get("ms", 0))))
        except FileNotFoundError:
            pass
        if step.get("delete"):
            try:
                os.remove(step["path"])
            except OSError:
                pass
        return out
    if kind == "jsonlall":
        # Every line of a .jsonl file whose `name` is the one asked (all lines without `name`).
        out = []
        try:
            with open(step["path"], encoding="utf-8") as f:
                for line in f:
                    j = json.loads(line)
                    if "name" not in step or j.get("name") == step["name"]:
                        out.append(j)
        except FileNotFoundError:
            pass
        return out
    if kind == "bytes":
        # Raw bytes of a file as hex (small files: byte-exact comparisons).
        with open(step["path"], "rb") as f:
            return f.read(step.get("max", 4096)).hex()
    if kind == "corrupt":
        # Overwrites `count` bytes at `offset` of every file of a folder whose name ends with `suffix` (app stopped).
        done = []
        for d, _dirs, files in os.walk(step["path"]):
            for name in files:
                if name.endswith(step.get("suffix", "")):
                    full = os.path.join(d, name)
                    size = os.path.getsize(full)
                    if size < 64:
                        continue
                    with open(full, "r+b") as f:
                        f.seek(min(step.get("offset", 40), size - 16))
                        f.write(bytes([0xA5]) * step.get("count", 16))
                    done.append(os.path.relpath(full, step["path"]))
        return done
    if kind == "targets":
        with urllib.request.urlopen("http://127.0.0.1:9223/json", timeout=2) as r:
            return [{"type": t.get("type"), "url": t.get("url", "")[:80]} for t in json.load(r)]
    if kind == "readtext":
        with open(step["path"], "r", encoding="utf-8", errors="replace") as f:
            return f.read()[: step.get("max", 4000)]
    raise RuntimeError("unknown py step " + kind)


def main():
    global IT, VAULTS, ROCKSY
    args = sys.argv[1:]
    scenario_path = args[0]
    out = os.path.join(BASE, "shots")
    dist = "file:///C:/Users/aleca/rocksy-test/ui/dist/index.html"
    port = 9333
    i = 1
    while i < len(args):
        if args[i] == "--out":
            out = args[i + 1]
        elif args[i] == "--dist":
            dist = args[i + 1]
        elif args[i] == "--port":
            port = int(args[i + 1])
        elif args[i] == "--exe":
            ROCKSY = args[i + 1]
        elif args[i] == "--it":
            IT = args[i + 1]
        elif args[i] == "--vaults":
            VAULTS = args[i + 1]
        i += 2
    with open(scenario_path, encoding="utf-8") as f:
        scenario = subst(json.load(f))
    name = scenario.get("name") or os.path.splitext(os.path.basename(scenario_path))[0]
    size = scenario.get("size", [1400, 900])
    app = scenario.get("app")
    if app:
        out = os.path.join(BASE, "shots-app")
        port = 9223
    os.makedirs(out, exist_ok=True)
    results = {"scenario": name, "saved": {}, "errors": []}
    if app:
        results["exe"] = ROCKSY
    mem = {}
    proc = None
    extra = []
    cdp = None
    prior_events = []
    try:
        if app:
            if app.get("fresh", True):
                import shutil
                assert app["dataDir"].lower().startswith(IT.lower())
                shutil.rmtree(app["dataDir"], ignore_errors=True)
            for st in scenario.get("before", []):
                r = py_step(st, mem, results)
                if st.get("save") and st["py"] not in ("hash", "snap"):
                    results["saved"][st["save"]] = r
            t_launch = time.time()
            proc = launch_app(app, port)
            mem["__pid"] = proc.pid
            mem["__data"] = app["dataDir"]
            results["pids"] = [proc.pid]
            cdp = attach(port, timeout=30)
            results["saved"]["attachMs"] = round((time.time() - t_launch) * 1000)
        else:
            profile = os.path.join(BASE, "profile-%d" % port)
            proc = subprocess.Popen([
                EDGE, "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
                "--disable-extensions", "--disable-sync", "--disable-background-networking",
                "--allow-file-access-from-files", "--hide-scrollbars=false",
                f"--remote-debugging-port={port}", f"--user-data-dir={profile}",
                f"--window-size={size[0]},{size[1]}", "about:blank",
            ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            cdp = attach(port, timeout=15)
            cdp.call("Emulation.setDeviceMetricsOverride", width=size[0], height=size[1], deviceScaleFactor=1, mobile=False)
        for n, step in enumerate(scenario["steps"]):
            try:
                if "nav" in step:
                    cdp.call("Page.navigate", url=step["nav"].replace("{DIST}", dist))
                    time.sleep(0.05)
                elif "wait" in step:
                    deadline = time.time() + step.get("timeout", 15000) / 1000
                    ok = False
                    t0 = time.time()
                    while time.time() < deadline:
                        try:
                            if cdp.evaluate("!!(" + step["wait"] + ")"):
                                ok = True
                                break
                        except RuntimeError:
                            pass
                        time.sleep(0.03)
                    if step.get("save"):
                        results["saved"][step["save"]] = round((time.time() - t0) * 1000)
                    if not ok:
                        raise RuntimeError("wait timed out: " + step["wait"])
                elif "evalfile" in step:
                    with open(os.path.join(os.path.dirname(os.path.abspath(scenario_path)), step["evalfile"]), encoding="utf-8") as f:
                        v = cdp.evaluate((paths_prelude() if app else "") + f.read())
                    if step.get("save"):
                        results["saved"][step["save"]] = v
                elif "eval" in step:
                    v = cdp.evaluate(step["eval"])
                    if step.get("collect_eval"):
                        results["saved"].setdefault(step["collect_eval"], []).append(v)
                    if step.get("save"):
                        results["saved"][step["save"]] = v
                elif "type" in step:
                    for ch in step["type"]:
                        if ch == "\n":
                            press(cdp, "Enter")
                        else:
                            press(cdp, ch)
                        if step.get("delay"):
                            time.sleep(step["delay"] / 1000)
                elif "key" in step:
                    for _ in range(step.get("times", 1)):
                        press(cdp, step["key"], mods_of(step))
                elif "click" in step:
                    click(cdp, step)
                elif "shot" in step:
                    time.sleep(step.get("settle", 150) / 1000)
                    if app:
                        dims = cdp.evaluate("[innerWidth, innerHeight, devicePixelRatio]")
                        r = cdp.call("Page.captureScreenshot", format="png", clip={"x": 0, "y": 0, "width": dims[0], "height": dims[1], "scale": 1 / dims[2]})
                    else:
                        r = cdp.call("Page.captureScreenshot", format="png")
                    with open(os.path.join(out, step["shot"]), "wb") as f:
                        f.write(base64.b64decode(r["data"]))
                elif "sleep" in step:
                    time.sleep(step["sleep"] / 1000)
                elif "size" in step:
                    w, h = step["size"]
                    cdp.call("Emulation.setDeviceMetricsOverride", width=w, height=h, deviceScaleFactor=1, mobile=False)
                elif "py" in step:
                    r = py_step(step, mem, results)
                    if step.get("collect"):
                        results["saved"].setdefault(step["collect"], []).append(r)
                    elif step.get("save") and step["py"] not in ("hash", "snap"):
                        results["saved"][step["save"]] = r
                    elif step.get("report"):
                        results["saved"][step["report"]] = r
                elif "kill" in step:
                    kill(proc)
                    time.sleep(0.8)
                elif "restart" in step:
                    # Kill and relaunch the app with the same data dir (session restore), then re-attach.
                    # (After a "close" step the process is already gone: this is then a clean restart.)
                    try:
                        results["cspBeforeRestart"] = results.get("cspBeforeRestart", []) + json.loads(cdp.evaluate("JSON.stringify(window.rocksyCsp||[])"))
                    except Exception:
                        pass
                    prior_events.extend(cdp.events)
                    kill(proc)
                    time.sleep(0.5)
                    t_launch = time.time()
                    proc = launch_app(app, port, step["restart"].get("args", app.get("args", [])))
                    mem["__pid"] = proc.pid
                    results["pids"].append(proc.pid)
                    cdp = attach(port, timeout=30)
                    if step.get("save"):
                        results["saved"][step["save"]] = round((time.time() - t_launch) * 1000)
                elif "launch" in step:
                    # Second launch: must be absorbed by the running instance.
                    p2 = launch_app(app, port, step["launch"], debug=False)
                    extra.append(p2)
                    mem["launchAt"] = time.time()
                    if step.get("until"):
                        deadline = time.time() + 15
                        while time.time() < deadline:
                            try:
                                if cdp.evaluate("!!(" + step["until"] + ")"):
                                    break
                            except RuntimeError:
                                pass
                            time.sleep(0.01)
                        results["saved"][step.get("save", "launchMs")] = round((time.time() - mem["launchAt"]) * 1000)
                        results["saved"][step.get("save", "launchMs") + "_exited"] = p2.poll() is not None or (time.sleep(1.5) or p2.poll() is not None)
            except Exception as e:  # keep going: later screenshots are still useful
                results["errors"].append({"step": n, "what": json.dumps(step)[:160], "error": str(e)[:600]})
        # Content-Security-Policy violations recorded by the page itself (none is expected).
        try:
            csp = cdp.evaluate("JSON.stringify(window.rocksyCsp||[])")
            results["csp"] = json.loads(csp)
            if results["csp"]:
                results["errors"].append({"csp": csp[:600]})
        except Exception:
            results["csp"] = None
        # Requests the page made to anything but the app itself (none is expected), and the requests that were blocked.
        own = ("http://tauri.localhost", "http://ipc.localhost", "http://asset.localhost", "ipc:", "asset:", "data:", "blob:", "file:", "about:", "devtools:", "chrome")
        all_events = prior_events + cdp.events
        if results.get("cspBeforeRestart"):
            results["errors"].append({"cspBeforeRestart": json.dumps(results["cspBeforeRestart"])[:600]})
        for ev in all_events:
            if ev.get("method") == "Network.requestWillBeSent":
                url = ev["params"]["request"]["url"]
                if not url.startswith(own):
                    results.setdefault("externalRequests", []).append(url[:200])
            elif ev.get("method") == "Network.loadingFailed" and ev["params"].get("blockedReason"):
                results.setdefault("blockedRequests", []).append({"reason": ev["params"]["blockedReason"], "type": ev["params"].get("type")})
        for ev in all_events:
            if ev.get("method") == "Log.entryAdded":
                entry = ev["params"]["entry"]
                if entry.get("level") == "error":
                    line = {"source": entry.get("source"), "text": entry.get("text", "")[:300], "url": entry.get("url", "")[:120]}
                    results.setdefault("log", []).append(line)
                    # A scenario that injects hostile content on purpose says so ("allowCspLog").
                    if "Content Security Policy" in line["text"] and not scenario.get("allowCspLog"):
                        results["errors"].append({"cspLog": line["text"]})
            elif ev.get("method") == "Runtime.exceptionThrown":
                d = ev["params"]["exceptionDetails"]
                results["errors"].append({"pageException": json.dumps(d.get("exception", d))[:600]})
            elif ev.get("method") == "Runtime.consoleAPICalled" and ev["params"]["type"] in ("error", "warning"):
                results["errors"].append({"console": json.dumps(ev["params"]["args"])[:400]})
    except Exception as e:
        results["errors"].append({"fatal": str(e)})
    finally:
        if proc is not None and not app:
            proc.terminate()
        kill(proc)
        for p2 in extra:
            kill(p2)
    if app:
        alive = []
        for pid in results.get("pids", []):
            r = subprocess.run(["tasklist", "/FI", "PID eq %d" % pid, "/NH", "/FO", "CSV"], capture_output=True, text=True)
            if ('"%d"' % pid) in r.stdout:
                alive.append(pid)
        results["ourPidsStillAlive"] = alive
    with open(os.path.join(out, name + ".json"), "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2, ensure_ascii=False)
    sys.stdout.reconfigure(encoding="utf-8")
    print(json.dumps(results, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main()
