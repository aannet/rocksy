#!/usr/bin/env python3
"""
Pilote minimal Chrome DevTools Protocol pour tester l'app Rocksy (WebView2) sans capture d'ecran du bureau.
Bibliotheque standard uniquement : a lancer avec le Python de Windows (le port de debogage n'ecoute que sur 127.0.0.1).

Lancer l'app avec : $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9223"

Usage
  python cdp.py eval "<expression JS>"         # resultat en JSON sur stdout (les promesses sont attendues)
  python cdp.py evalfile script.js             # idem, expression lue dans un fichier
  python cdp.py shot C:\\chemin\\capture.png     # capture de la page (pas de l'ecran)
  python cdp.py key "Control+o"                # touche (modificateurs Control, Shift, Alt) ou texte : key "text:bonjour"
Options : --port 9223  --timeout 30
"""
import base64
import json
import os
import socket
import struct
import sys
import urllib.request


class Ws:
    def __init__(self, url, timeout):
        assert url.startswith("ws://")
        hostport, _, path = url[5:].partition("/")
        host, _, port = hostport.partition(":")
        self.s = socket.create_connection((host, int(port or 80)), timeout=timeout)
        key = base64.b64encode(os.urandom(16)).decode()
        req = ("GET /%s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
               "Sec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n") % (path, hostport, key)
        self.s.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += self.s.recv(4096)
        if b" 101 " not in buf.split(b"\r\n", 1)[0]:
            raise RuntimeError("handshake refuse : %r" % buf[:200])
        self.buf = buf.split(b"\r\n\r\n", 1)[1]

    def send(self, text):
        data = text.encode()
        head = bytearray([0x81])
        n = len(data)
        if n < 126:
            head.append(0x80 | n)
        elif n < 65536:
            head.append(0x80 | 126); head += struct.pack(">H", n)
        else:
            head.append(0x80 | 127); head += struct.pack(">Q", n)
        mask = os.urandom(4)
        head += mask
        self.s.sendall(bytes(head) + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.s.recv(65536)
            if not chunk:
                raise RuntimeError("connexion fermee")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv(self):
        msg = b""
        while True:
            b0, b1 = self._read(2)
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._read(8))[0]
            payload = self._read(n)
            op = b0 & 0x0F
            if op in (0, 1, 2):
                msg += payload
                if b0 & 0x80:
                    return msg.decode("utf-8", "replace")
            elif op == 8:
                raise RuntimeError("connexion fermee par le pair")


class Cdp:
    def __init__(self, port, timeout):
        pages = json.load(urllib.request.urlopen("http://127.0.0.1:%d/json" % port, timeout=5))
        page = next(p for p in pages if p.get("type") == "page")
        self.ws = Ws(page["webSocketDebuggerUrl"], timeout)
        self.n = 0

    def call(self, method, **params):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        while True:
            m = json.loads(self.ws.recv())
            if m.get("id") == self.n:
                if "error" in m:
                    raise RuntimeError(m["error"])
                return m["result"]


KEYS = {"Enter": 13, "Escape": 27, "Tab": 9, "Backspace": 8, "ArrowDown": 40, "ArrowUp": 38,
        "ArrowLeft": 37, "ArrowRight": 39, "Home": 36, "End": 35, "Delete": 46}


def main():
    a = sys.argv[1:]
    port, timeout = 9223, 30
    while a and a[0].startswith("--"):
        if a[0] == "--port": port = int(a[1])
        elif a[0] == "--timeout": timeout = int(a[1])
        a = a[2:]
    if len(a) < 2:
        print(__doc__); sys.exit(2)
    cmd, arg = a[0], a[1]
    c = Cdp(port, timeout)
    if cmd in ("eval", "evalfile"):
        expr = open(arg, encoding="utf-8").read() if cmd == "evalfile" else arg
        r = c.call("Runtime.evaluate", expression=expr, awaitPromise=True, returnByValue=True)
        if "exceptionDetails" in r:
            print(json.dumps({"exception": r["exceptionDetails"].get("exception", {}).get("description", str(r["exceptionDetails"]))}))
            sys.exit(1)
        sys.stdout.buffer.write(json.dumps(r["result"].get("value"), ensure_ascii=False).encode("utf-8") + b"\n")
    elif cmd == "shot":
        r = c.call("Page.captureScreenshot", format="png")
        open(arg, "wb").write(base64.b64decode(r["data"]))
        print("ok", arg)
    elif cmd == "key":
        if arg.startswith("text:"):
            c.call("Input.insertText", text=arg[5:])
        else:
            parts = arg.split("+"); key = parts[-1]
            mods = sum({"Alt": 1, "Control": 2, "Meta": 4, "Shift": 8}[m] for m in parts[:-1])
            code = KEYS.get(key, ord(key.upper()) if len(key) == 1 else 0)
            base = dict(modifiers=mods, key=key, windowsVirtualKeyCode=code, nativeVirtualKeyCode=code)
            if len(key) == 1:
                base["code"] = "Key" + key.upper()
                if not (mods & 7): base["text"] = key
            c.call("Input.dispatchKeyEvent", type="keyDown" if "text" not in base else "keyDown", **base)
            c.call("Input.dispatchKeyEvent", type="keyUp", **{k: v for k, v in base.items() if k != "text"})
        print("ok")
    else:
        print(__doc__); sys.exit(2)


if __name__ == "__main__":
    main()
