"""Opt-in container driver for the takeover check (Spec 050 PRD 010 scope 17).
Never imported by Cargo or CI.

`serve gateway`: starts a fake provider and the production gateway command
(`shacs-bot run`), then answers control requests on 127.0.0.1:9100.
`serve api`: the same with `shacs-bot serve` (local API).
`client <json>`: posts one control request and prints the JSON answer.
The password is never read, printed or compared by this file.
"""
import base64
import ctypes
import http.server
import json
import os
import pathlib
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.request

CONTROL = ("127.0.0.1", 9100)
WEBSOCKET = ("127.0.0.1", 8765)
CHAT = "takeover-check"
API_SESSION = "spec050:takeover"
BOT = ["shacs-bot", "--config", "/check-data/config.json"]
commands = []
results = []
request_counter = 0
role = "api"
selection_holder = {}


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        global request_counter
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if commands:
            action = commands.pop(0)
            request_counter += 1
            response = {"role": "assistant", "content": None, "tool_calls": [{
                "id": f"fixture-{request_counter}", "type": "function",
                "function": {"name": "browser", "arguments": json.dumps(action)}}]}
            reason = "tool_calls"
        else:
            tools = [message for message in body["messages"] if message.get("role") == "tool"]
            if tools:
                results.append(tools[-1]["content"])
            # A `/browser` command never reaches the provider; a turn without
            # a scripted action just ends.
            response = {"role": "assistant", "content": "fixture complete"}
            reason = "stop"
        if body.get("stream"):
            for index, tool in enumerate(response.get("tool_calls", [])):
                tool["index"] = index
            chunks = [
                {"choices": [{"index": 0, "delta": response, "finish_reason": None}]},
                {"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]},
            ]
            data = ("".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks)
                    + "data: [DONE]\n\n").encode()
            content_type = "text/event-stream"
        else:
            data = json.dumps({"choices": [{"index": 0, "message": response, "finish_reason": reason}],
                               "usage": {"prompt_tokens": 1, "completion_tokens": 1}}).encode()
            content_type = "application/json"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


# --- the gateway role is driven over its WebSocket channel (RFC 6455 client) ---
class WebSocketChat:
    def __init__(self):
        self.sock = socket.create_connection(WEBSOCKET, timeout=200)
        key = base64.b64encode(os.urandom(16)).decode()
        # No query: a query is the Spec 035 reconnect form (client_id AND
        # session_id). The chat id travels in the message frame instead.
        request = ("GET / HTTP/1.1\r\n"
                   f"Host: {WEBSOCKET[0]}:{WEBSOCKET[1]}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                   f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
        self.sock.sendall(request.encode())
        header = b""
        while b"\r\n\r\n" not in header:
            chunk = self.sock.recv(4096)
            assert chunk, "websocket handshake closed"
            header += chunk
        head, _, rest = header.partition(b"\r\n\r\n")
        assert head.split(b"\r\n")[0].endswith(b"101 Switching Protocols"), head
        self.buffer = bytearray(rest)

    def raw(self, count):
        while len(self.buffer) < count:
            chunk = self.sock.recv(65536)
            assert chunk, "websocket closed"
            self.buffer += chunk
        data = bytes(self.buffer[:count])
        del self.buffer[:count]
        return data

    def send(self, payload):
        data = payload.encode()
        mask = os.urandom(4)
        header = bytes([0x81])
        if len(data) < 126:
            header += bytes([0x80 | len(data)])
        elif len(data) < 65536:
            header += bytes([0x80 | 126]) + struct.pack("!H", len(data))
        else:
            header += bytes([0x80 | 127]) + struct.pack("!Q", len(data))
        self.sock.sendall(header + mask + bytes(byte ^ mask[index % 4] for index, byte in enumerate(data)))

    def events(self):
        """Yields parsed JSON events; answers pings; stops on close."""
        while True:
            first, second = self.raw(2)
            opcode = first & 0x0F
            length = second & 0x7F
            if length == 126:
                length = struct.unpack("!H", self.raw(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self.raw(8))[0]
            mask = self.raw(4) if second & 0x80 else None
            payload = self.raw(length)
            if mask:
                payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
            if opcode == 0x8:
                return
            if opcode == 0x9:
                self.sock.sendall(bytes([0x8A, len(payload)]) + payload)
                continue
            if opcode in (0x1, 0x2):
                yield json.loads(payload)

    def close(self):
        self.sock.close()


def say(text):
    """Sends one user message in the check's chat and returns the answer text."""
    if role == "gateway":
        chat = WebSocketChat()
        try:
            chat.send(json.dumps({"type": "message", "chat_id": CHAT, "content": text}))
            for event in chat.events():
                if event.get("event") == "error":
                    raise RuntimeError("gateway websocket error: " + json.dumps(event))
                if event.get("event") == "message" and event.get("chat_id") == CHAT:
                    return event.get("text", "")
        finally:
            chat.close()
        raise RuntimeError("gateway websocket closed without an answer")
    request = urllib.request.Request("http://127.0.0.1:8900/v1/chat/completions",
        json.dumps({"model": "gpt-4o", "messages": [{"role": "user", "content": text}],
                    "session_id": API_SESSION, "stream": False}).encode(),
        {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=150) as response:
        answer = json.loads(response.read())
    return answer["choices"][0]["message"].get("content") or ""


def call(action):
    """Runs one scripted `browser` action as a turn and returns its tool result."""
    commands.append(action)
    count = len(results)
    say("run fixture")
    assert len(results) == count + 1, "tool did not execute"
    raw = results[-1]
    try:
        value = json.loads(raw)
        if isinstance(value, list):
            raw = value[0]["text"]
        elif isinstance(value, dict):
            return value
    except json.JSONDecodeError:
        pass
    try:
        return json.loads(raw.split("\n[image", 1)[0])
    except json.JSONDecodeError:
        return raw


def process_stat(directory):
    """(state, parent pid, process group) of a /proc entry."""
    text = (directory / "stat").read_text()
    state, parent, group = text.rsplit(")", 1)[1].split()[:3]
    return state, int(parent), int(group)


def every_process():
    found = []
    for directory in pathlib.Path("/proc").iterdir():
        if not directory.name.isdigit():
            continue
        try:
            arguments = [arg.decode(errors="replace")
                         for arg in (directory / "cmdline").read_bytes().split(b"\0") if arg]
            state, parent, group = process_stat(directory)
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        found.append({"pid": int(directory.name), "parent": parent, "group": group,
                      "state": state, "argv": arguments})
    return found


def process_table():
    """The browser's own processes by name. A websockify connection process
    (a fork of the listener) carries the listener's argv and is named
    `websockify` too; the listener is the one whose parent is not websockify."""
    found = []
    for entry in every_process():
        arguments = entry["argv"]
        if not arguments:
            continue
        if any(arg == "--remote-debugging-pipe" for arg in arguments):
            name = "chromium"
        elif any(arg.endswith("x11vnc") for arg in arguments[:1]):
            name = "x11vnc"
        elif any(arg.endswith("websockify") for arg in arguments[:2]):
            name = "websockify"
        elif arguments[0].endswith("Xvfb"):
            name = "xvfb"
        else:
            continue
        found.append({"name": name, **entry})
    websockify = {entry["pid"] for entry in found if entry["name"] == "websockify"}
    for entry in found:
        if entry["name"] == "websockify" and entry["parent"] in websockify:
            entry["name"] = "websockify-connection"
    return found


def facts():
    roots = sorted(path for path in pathlib.Path("/tmp").glob("shacs-browser-*") if path.is_dir())
    routes = []
    for root in roots:
        path = root / "view-routes"
        if path.exists():
            routes.extend(path.read_text().splitlines())
    return {
        "processes": process_table(),
        "roots": [str(root) for root in roots],
        "routes": routes,
        "paste_sockets": [str(root / "paste.sock") for root in roots if (root / "paste.sock").exists()],
    }


def group_members(group):
    """Live (not zombie) processes of a process group."""
    return [{"pid": entry["pid"], "parent": entry["parent"], "argv": entry["argv"][:3]}
            for entry in every_process() if entry["group"] == group and entry["state"] != "Z"]


def inspect_browser():
    output = subprocess.run(BOT + ["runtime", "inspect", "--workspace", "/check-data/workspace"],
                            capture_output=True, text=True, timeout=60)
    for line in output.stdout.splitlines():
        if line.startswith("Browser: "):
            return json.loads(line[len("Browser: "):])
    raise RuntimeError("runtime inspect gave no Browser line: " + output.stdout + output.stderr)


# --- X selection facts through libX11 (a Chromium dependency already in the image) ---
def x11():
    """The browser's display and a configured libX11 handle, or an explanation."""
    xvfb = next((entry for entry in process_table() if entry["name"] == "xvfb"), None)
    if xvfb is None:
        return None, "no Xvfb process"
    argv = xvfb["argv"]
    display = argv[1]
    auth = argv[argv.index("-auth") + 1]
    try:
        lib = ctypes.CDLL("libX11.so.6")
    except OSError as error:
        return None, f"libX11 unavailable: {error}"
    void_p, ulong, c_int, char_p = ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_char_p
    lib.XOpenDisplay.argtypes, lib.XOpenDisplay.restype = [char_p], void_p
    lib.XCloseDisplay.argtypes = [void_p]
    lib.XDefaultRootWindow.argtypes, lib.XDefaultRootWindow.restype = [void_p], ulong
    lib.XInternAtom.argtypes, lib.XInternAtom.restype = [void_p, char_p, c_int], ulong
    lib.XGetSelectionOwner.argtypes, lib.XGetSelectionOwner.restype = [void_p, ulong], ulong
    lib.XSetSelectionOwner.argtypes = [void_p, ulong, ulong, ulong]
    lib.XFetchBytes.argtypes, lib.XFetchBytes.restype = [void_p, ctypes.POINTER(c_int)], void_p
    lib.XStoreBytes.argtypes = [void_p, char_p, c_int]
    lib.XFree.argtypes = [void_p]
    lib.XCreateSimpleWindow.argtypes = [void_p, ulong, c_int, c_int, ctypes.c_uint, ctypes.c_uint,
                                        ctypes.c_uint, ulong, ulong]
    lib.XCreateSimpleWindow.restype = ulong
    lib.XSync.argtypes = [void_p, c_int]
    os.environ["DISPLAY"] = display
    os.environ["XAUTHORITY"] = auth
    return lib, display


def selection_state():
    lib, detail = x11()
    if lib is None:
        return {"available": False, "detail": detail}
    display = lib.XOpenDisplay(None)
    if not display:
        return {"available": False, "detail": "XOpenDisplay failed"}
    try:
        clipboard = lib.XInternAtom(display, b"CLIPBOARD", 0)
        length = ctypes.c_int(0)
        pointer = lib.XFetchBytes(display, ctypes.byref(length))
        cut = ctypes.string_at(pointer, length.value) if pointer and length.value else b""
        if pointer:
            lib.XFree(pointer)
        return {"available": True, "display": detail,
                "primary_owner": lib.XGetSelectionOwner(display, 1),
                "clipboard_owner": lib.XGetSelectionOwner(display, clipboard),
                "cut_buffer0": cut.decode(errors="replace")}
    finally:
        lib.XCloseDisplay(display)


def set_selection(text):
    """Owns PRIMARY and CLIPBOARD from a hidden window (kept alive by a thread)
    and stores CUT_BUFFER0, so a selection-forwarding VNC server would have
    something to send."""
    lib, detail = x11()
    if lib is None:
        return {"available": False, "detail": detail}
    display = lib.XOpenDisplay(None)
    if not display:
        return {"available": False, "detail": "XOpenDisplay failed"}
    data = text.encode()
    clipboard = lib.XInternAtom(display, b"CLIPBOARD", 0)
    window = lib.XCreateSimpleWindow(display, lib.XDefaultRootWindow(display), 0, 0, 1, 1, 0, 0, 0)
    lib.XStoreBytes(display, data, len(data))
    lib.XSetSelectionOwner(display, 1, window, 0)
    lib.XSetSelectionOwner(display, clipboard, window, 0)
    lib.XSync(display, 0)
    selection_holder["display"] = display  # the connection must stay open to keep ownership
    threading.Thread(target=lambda: time.sleep(3600), daemon=True).start()
    return {"available": True, "window": window,
            "primary_owner": lib.XGetSelectionOwner(display, 1),
            "clipboard_owner": lib.XGetSelectionOwner(display, clipboard)}


def handle(request):
    op = request["op"]
    if op == "ready":
        return {"ready": True, "role": role}
    if op == "call":
        return {"result": call(request["action"])}
    if op == "say":
        return {"text": say(request["text"])}
    if op == "facts":
        return facts()
    if op == "group":
        return {"members": group_members(request["group"])}
    if op == "inspect":
        return {"browser": inspect_browser()}
    if op == "selection":
        return selection_state()
    if op == "set_selection":
        return set_selection(request["text"])
    if op == "log_tail":
        return {"log": pathlib.Path("/check-data/runtime.log").read_text()[-6000:]}
    if op == "ps":
        return {"processes": sorted([entry["pid"], " ".join(entry["argv"][:3])[:160]]
                                    for entry in every_process())}
    raise ValueError(op)


def start_bot(log):
    if role == "gateway":
        # The production gateway command from docker-compose.yml.
        arguments = ["run", "--websocket-host", "0.0.0.0", "--websocket-port", str(WEBSOCKET[1]),
                     "--allow-remote", "--verbose", "--workspace", "/check-data/workspace",
                     "--allow-side-effects"]
    else:
        arguments = ["serve", "--host", "127.0.0.1", "--port", "8900",
                     "--workspace", "/check-data/workspace", "--allow-api-side-effects"]
    return subprocess.Popen(BOT + arguments, stdout=log, stderr=subprocess.STDOUT)


def wait_for_bot(bot, log):
    until = time.monotonic() + 60
    while True:
        if bot.poll() is not None:
            raise RuntimeError("bot exited: " + pathlib.Path(log.name).read_text())
        try:
            if role == "gateway":
                with socket.create_connection(WEBSOCKET, timeout=1):
                    return
            with urllib.request.urlopen("http://127.0.0.1:8900/health", timeout=1):
                return
        except OSError:
            if time.monotonic() >= until:
                raise
            time.sleep(0.2)


def serve():
    provider = http.server.ThreadingHTTPServer(("127.0.0.1", 9001), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    log = open("/check-data/runtime.log", "w+")
    bot = start_bot(log)
    wait_for_bot(bot, log)

    class Control(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            try:
                answer = {"ok": True, **handle(request)}
            except Exception as error:  # reported to the host side as a failed check
                answer = {"ok": False, "error": f"{type(error).__name__}: {error}"}
            data = json.dumps(answer).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    control = http.server.ThreadingHTTPServer(CONTROL, Control)
    try:
        control.serve_forever()
    finally:
        bot.terminate()
        try:
            bot.wait(timeout=15)
        except subprocess.TimeoutExpired:
            bot.kill()
            bot.wait()
        provider.shutdown()
        log.close()


def client(payload):
    request = urllib.request.Request(f"http://{CONTROL[0]}:{CONTROL[1]}/", payload.encode(),
                                     {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=200) as response:
        sys.stdout.write(response.read().decode())


if __name__ == "__main__":
    if sys.argv[1] == "serve":
        role = sys.argv[2]
        assert role in ("gateway", "api"), role
        serve()
    elif sys.argv[1] == "client":
        client(sys.argv[2])
    else:
        raise SystemExit("usage: takeover_check.py serve gateway|api | client <json>")
