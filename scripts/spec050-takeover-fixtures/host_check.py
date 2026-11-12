"""Opt-in host-side takeover checks (Spec 050 PRD 010 scope 17). Standard library only.

Usage: python3 host_check.py <compose project> <compose file> <env file with the password>

The DES, RFB framebuffer and ServerCutText helpers are the Spec 049 viewer
check's (`scripts/spec049-viewer-fixtures/host_check.py`), loaded by path.
The password is read from the temporary `.env.json` and used only for the
RFB handshake and the paste digest; it is never printed.
"""
import hashlib
import importlib.util
import json
import pathlib
import socket
import struct
import subprocess
import sys
import time
import urllib.request

PAGE = "http://172.30.250.2:8000/takeover.html"
GATEWAY, API = 6080, 6081
SENTENCE = "값을 넣었습니다, 맞죠? 끝!"
ROUTE_VIEW = "view: 127.0.0.1:5900"

_spec = importlib.util.spec_from_file_location(
    "spec049_host_check",
    pathlib.Path(__file__).resolve().parent.parent / "spec049-viewer-fixtures" / "host_check.py")
assert _spec is not None and _spec.loader is not None
spec049 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(spec049)


class Refused(Exception):
    """The server answered the WebSocket handshake with another status."""

    def __init__(self, status):
        super().__init__(f"handshake status {status}")
        self.status = status


class Socket:
    """One WebSocket connection to websockify, read as a byte stream."""

    def __init__(self, port, token, origin="same"):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=20)
        host = f"127.0.0.1:{port}"
        if origin == "same":
            origin = f"http://{host}"
        lines = [f"GET /websockify?token={token} HTTP/1.1", f"Host: {host}"]
        if origin is not None:
            lines.append(f"Origin: {origin}")
        lines += ["Upgrade: websocket", "Connection: Upgrade",
                  "Sec-WebSocket-Key: c2hhY3MtdGFrZW92ZXItY2hlY2s=",
                  "Sec-WebSocket-Version: 13", "Sec-WebSocket-Protocol: binary", "", ""]
        self.sock.sendall("\r\n".join(lines).encode())
        header = b""
        while b"\r\n\r\n" not in header:
            chunk = self.sock.recv(4096)
            if not chunk:
                break
            header += chunk
        head, _, rest = header.partition(b"\r\n\r\n")
        status_line = head.split(b"\r\n")[0].split()
        status = int(status_line[1]) if len(status_line) > 1 and status_line[1].isdigit() else 0
        if status != 101:
            self.sock.close()
            raise Refused(status)
        self.frames = bytearray(rest)
        self.stream = bytearray()

    def raw(self, count):
        while len(self.frames) < count:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("websocket closed")
            self.frames += chunk
        data = bytes(self.frames[:count])
        del self.frames[:count]
        return data

    def recv_frame(self):
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
            raise ConnectionError("websocket closed by server")
        if opcode == 0x9:
            self.send_frame(payload, 0xA)
            return b""
        return payload

    def send_frame(self, payload, opcode=0x2):
        mask = b"\x5a\xa5\x3c\xc3"
        header = bytes([0x80 | opcode])
        if len(payload) < 126:
            header += bytes([0x80 | len(payload)])
        elif len(payload) < 65536:
            header += bytes([0x80 | 126]) + struct.pack("!H", len(payload))
        else:
            header += bytes([0x80 | 127]) + struct.pack("!Q", len(payload))
        self.sock.sendall(header + mask + bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload)))

    def read(self, count):
        while len(self.stream) < count:
            self.stream += self.recv_frame()
        data = bytes(self.stream[:count])
        del self.stream[:count]
        return data

    def line(self):
        while b"\n" not in self.stream:
            self.stream += self.recv_frame()
        data, _, rest = bytes(self.stream).partition(b"\n")
        self.stream = bytearray(rest)
        return data.decode()

    def write(self, data):
        self.send_frame(data)

    def close(self):
        self.sock.close()


def handshake_status(port, token, origin):
    try:
        Socket(port, token, origin).close()
        return 101
    except Refused as refused:
        return refused.status


# --- RFB over the `view` token ---
def rfb_open(port, password):
    """An authenticated RFB connection past ServerInit, asking for raw rectangles."""
    rfb = Socket(port, "view")
    version = rfb.read(12)
    assert version.startswith(b"RFB 003.00"), version
    rfb.write(b"RFB 003.008\n")
    count = rfb.read(1)[0]
    assert count > 0, "server refused the connection before authentication"
    types = rfb.read(count)
    assert 2 in types and 1 not in types, f"VNC authentication must be required, got {list(types)}"
    rfb.write(b"\x02")
    rfb.write(spec049.vnc_response(password, rfb.read(16)))
    assert struct.unpack("!I", rfb.read(4))[0] == 0, "the view password was rejected"
    rfb.write(b"\x01")
    width, height = struct.unpack("!HH", rfb.read(4))
    rfb.read(16)
    rfb.read(struct.unpack("!I", rfb.read(4))[0])
    rfb.write(struct.pack("!BxHi", 2, 1, 0))
    return rfb, width, height


def settle(rfb):
    """The server answered a framebuffer request, so it read what came before it."""
    rfb.write(struct.pack("!BBHHHH", 3, 0, 0, 0, 8, 8))
    spec049.read_update(rfb)


def key(rfb, symbol, down):
    rfb.write(struct.pack("!BBxxI", 4, 1 if down else 0, symbol))


def tap(rfb, symbol):
    key(rfb, symbol, True)
    key(rfb, symbol, False)


def click_field(rfb, width, height):
    for mask in (1, 0):
        rfb.write(struct.pack("!BBHH", 5, mask, width // 2, height // 2))


def select_all(rfb):
    key(rfb, 0xFFE3, True)  # Control_L
    tap(rfb, ord("a"))
    key(rfb, 0xFFE3, False)


def write_text(rfb, width, height, text):
    """Click the field, select what it holds and type `text` over it."""
    click_field(rfb, width, height)
    select_all(rfb)
    for letter in text:
        tap(rfb, ord(letter))


def expect_closed(rfb, what):
    """The connection must be over: input sent on it goes nowhere."""
    rfb.sock.settimeout(5)
    try:
        for _ in range(50):
            click_field(rfb, 1280, 720)
            tap(rfb, ord("z"))
            rfb.read(1)
    except (ConnectionError, OSError) as error:
        if isinstance(error, (socket.timeout, TimeoutError)):
            raise AssertionError(f"{what}: the connection is still open") from error
        return
    raise AssertionError(f"{what}: the connection still answers")


# --- the paste line protocol over the `paste` token ---
def paste_digest(nonce, password):
    return hashlib.sha256(f"shacs-paste-v1\n{nonce}\n{password}".encode()).hexdigest()


def paste_greeting(port):
    connection = Socket(port, "paste")
    parts = connection.line().split(" ")
    assert len(parts) == 4 and parts[:2] == ["shacs-paste", "1"] and len(parts[3]) == 32, parts[:3]
    return connection, parts[2], parts[3]


def paste(port, digest_of, text):
    """One paste connection. `digest_of(nonce)` makes the auth line's digest.
    Returns (last server line, episode label, digest sent)."""
    connection, label, nonce = paste_greeting(port)
    try:
        digest = digest_of(nonce)
        connection.write(f"auth {digest}\n".encode())
        answer = connection.line()
        if answer != "ok":
            return answer, label, digest
        body = text.encode()
        connection.write(f"paste {len(body)}\n".encode() + body)
        return connection.line(), label, digest
    finally:
        connection.close()


def paste_unreachable(port):
    """No paste listener answers behind the `paste` token."""
    try:
        connection = Socket(port, "paste")
    except Refused:
        return True
    connection.sock.settimeout(5)
    try:
        return not connection.line().startswith("shacs-paste ")
    except (ConnectionError, OSError):
        return True
    finally:
        connection.close()


# --- Compose orchestration ---
class Project:
    def __init__(self, name, compose_file):
        self.compose = ["docker", "compose", "--project-name", name, "--file", compose_file]

    def client(self, service, request, timeout=240):
        return subprocess.run(self.compose + ["exec", "-T", service, "python3", "-I",
                                              "/fixtures/takeover_check.py", "client", json.dumps(request)],
                              capture_output=True, text=True, timeout=timeout)

    def control(self, service, request):
        output = self.client(service, request)
        assert output.returncode == 0, output.stderr
        answer = json.loads(output.stdout)
        assert answer["ok"], f"{service} {request.get('op')}: {answer.get('error')}"
        return answer

    def call(self, service, action):
        return self.control(service, {"op": "call", "action": action})["result"]

    def say(self, service, text):
        return self.control(service, {"op": "say", "text": text})["text"]

    def wait_ready(self, service):
        until = time.monotonic() + 180
        while True:
            output = self.client(service, {"op": "ready"}, timeout=60)
            if output.returncode == 0 and '"ready": true' in output.stdout:
                return
            assert time.monotonic() < until, f"{service} control server not ready: {output.stderr}"
            time.sleep(1)


def one(facts, name):
    found = [entry for entry in facts["processes"] if entry["name"] == name]
    assert len(found) == 1, (name, [entry["name"] for entry in facts["processes"]])
    return found[0]


def field_value(snapshot):
    assert isinstance(snapshot, dict), snapshot
    assert "TAKEOVER FIXTURE" in snapshot["text"], snapshot["text"]
    return next(element for element in snapshot["elements"] if element["name"] == "Field")["value"]


def check_control_state(project, user, label=""):
    """x11vnc's input flag, the token routes and the paste socket of one state."""
    facts = project.control("gateway", {"op": "facts"})
    argv = one(facts, "x11vnc")["argv"]
    assert ("-viewonly" in argv) != user, argv
    for flag in ["-nosel", "-noclipboard", "-noremote"]:
        assert flag in argv, (flag, argv)
    assert len(facts["roots"]) == 1, facts["roots"]
    socket_path = f"{facts['roots'][0]}/paste.sock"
    if user:
        assert facts["routes"] == [ROUTE_VIEW, f"paste: unix_socket:{socket_path}"], facts["routes"]
        assert facts["paste_sockets"] == [socket_path], facts["paste_sockets"]
    else:
        assert facts["routes"] == [ROUTE_VIEW], facts["routes"]
        assert facts["paste_sockets"] == [], facts["paste_sockets"]
        assert paste_unreachable(GATEWAY), "the paste token connects outside a takeover"
    state = "user" if user else "assistant"
    print(f"[gateway] {state}{label}: x11vnc {'without' if user else 'with'} -viewonly, "
          f"-nosel -noclipboard -noremote present, view-routes {len(facts['routes'])} line(s), "
          f"paste.sock {'present' if user else 'absent'}", flush=True)
    return facts


def check_swap(project, before, after, when):
    """websockify was replaced and nothing of the old one's process group lives."""
    old, new = one(before, "websockify"), one(after, "websockify")
    assert old["pid"] != new["pid"], (when, old["pid"])
    members = project.control("gateway", {"op": "group", "group": old["group"]})["members"]
    assert members == [], (when, members)
    print(f"[gateway] {when}: websockify pid {old['pid']} -> {new['pid']}, "
          f"no live process left in the old process group {old['group']}", flush=True)


def check_connection_group(project):
    """With a connection open, its websockify process shares the listener's group."""
    facts = project.control("gateway", {"op": "facts"})
    listener = one(facts, "websockify")
    connections = [entry for entry in facts["processes"] if entry["name"] == "websockify-connection"]
    assert connections, "no websockify connection process while a view connection is open"
    for entry in connections:
        assert entry["group"] == listener["group"], (entry, listener)
    print(f"[gateway] websockify connection process(es) {[entry['pid'] for entry in connections]} "
          f"are in the listener's process group {listener['group']}", flush=True)


def check_origin(port):
    for token in ("view", "paste"):
        for origin in (None, "http://evil.example", f"http://127.0.0.1:{port + 1}"):
            status = handshake_status(port, token, origin)
            assert status == 403, (token, origin, status)
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/shacs-view.html", timeout=20) as response:
        assert response.status == 200
    print("[gateway] WebSocket without an Origin or with another Origin: 403 for view and paste; "
          "the static page is still 200", flush=True)


def check_clipboard(project, rfb):
    before = project.control("gateway", {"op": "selection"})
    text = b"TAKEOVER CUT"
    rfb.write(struct.pack("!BxxxI", 6, len(text)) + text)  # ClientCutText
    settle(rfb)
    if not before.get("available"):
        print(f"[gateway] SKIPPED X selection check: {before.get('detail')}", flush=True)
    else:
        after = project.control("gateway", {"op": "selection"})
        for name in ("primary_owner", "clipboard_owner", "cut_buffer0"):
            assert after[name] == before[name], (name, before, after)
        print("[gateway] user state: ClientCutText left PRIMARY, CLIPBOARD and CUT_BUFFER0 unchanged", flush=True)
    owned = project.control("gateway", {"op": "set_selection", "text": "SERVER SIDE SELECTION MARKER"})
    if not owned.get("available"):
        print(f"[gateway] SKIPPED server-side selection probe: {owned.get('detail')}", flush=True)
        return
    seen = spec049.expect_no_server_cut_text(rfb, 3)
    print(f"[gateway] user state: a selection owned on the server produced no ServerCutText in 3 s "
          f"(server messages seen: {seen})", flush=True)


def take(project):
    answer = project.say("gateway", "/browser takeover")
    url = f"http://127.0.0.1:{GATEWAY}/shacs-view.html"
    assert answer.startswith("브라우저를 넘겨받았습니다(") and f"화면 보기: {url}" in answer, answer
    with urllib.request.urlopen(url, timeout=20) as response:
        assert response.status == 200
    label = answer.split("(", 1)[1].split(")", 1)[0]
    print(f"[gateway] /browser takeover answered with {url} (episode {label}); the page is 200", flush=True)
    return label


def main():
    spec049.self_test()
    project_name, compose_file, env_file = sys.argv[1:4]
    password = json.loads(pathlib.Path(env_file).read_text())["SHACS_BROWSER_VIEW_PASSWORD"]
    assert len(password) >= 8
    project = Project(project_name, compose_file)
    for service in ("gateway", "api"):
        project.wait_ready(service)
    try:
        checks(project, password)
    except Exception:
        # Diagnostics for a failed run: runtime log tail and process list of
        # each container. The runtime log has previews only, never the password.
        for service in ("gateway", "api"):
            for op in ("log_tail", "ps"):
                try:
                    answer = project.control(service, {"op": op})
                    print(f"[{service}] {op}: {json.dumps(answer)[-6000:]}", flush=True)
                except Exception as error:  # the primary failure is re-raised below
                    print(f"[{service}] {op} unavailable: {error}", flush=True)
        raise


def checks(project, password):
    assert field_value(project.call("gateway", {"action": "open", "url": PAGE})) == "untouched"
    assistant = check_control_state(project, user=False)

    # First episode: the screen's clicks and keys, the swap of websockify,
    # the Origin check and the clipboard.
    stale, width, height = rfb_open(GATEWAY, password)
    check_connection_group(project)
    first = take(project)
    user = check_control_state(project, user=True, label=f" ({first})")
    check_swap(project, assistant, user, "takeover")
    expect_closed(stale, "the view connection opened before the takeover")
    print("[gateway] the view connection opened before the takeover is closed after it", flush=True)
    check_origin(GATEWAY)
    status = project.say("gateway", "/browser status")
    assert "조작: 사용자" in status and first in status and "답 기다리기: 멈춤" in status, status
    live, width, height = rfb_open(GATEWAY, password)
    write_text(live, width, height, "ab")
    settle(live)
    check_clipboard(project, live)
    answer = project.say("gateway", "/browser return")
    assert answer == "브라우저를 돌려줬습니다.", answer
    returned = check_control_state(project, user=False, label=" (after /browser return)")
    check_swap(project, user, returned, "return")
    expect_closed(live, "the view connection opened during the takeover")
    late, width, height = rfb_open(GATEWAY, password)
    write_text(late, width, height, "q")
    settle(late)
    late.close()
    time.sleep(1)
    # No `open` before this `look`: the page the user worked on is still there.
    value = field_value(project.call("gateway", {"action": "look"}))
    assert value == "ab", value
    print("[gateway] clicks and keys of the takeover changed the field to 'ab'; after the return the old "
          "connection is closed, a new connection's clicks and keys change nothing, the paste token does "
          "not connect, and look reads the same page without opening it again", flush=True)

    # Second episode: the paste line.
    second = take(project)
    assert second != first, (first, second)
    check_control_state(project, user=True, label=f" ({second})")
    live, width, height = rfb_open(GATEWAY, password)
    click_field(live, width, height)
    select_all(live)
    settle(live)
    answer, label, digest = paste(GATEWAY, lambda nonce: paste_digest(nonce, password), SENTENCE)
    assert answer == "done" and label == second, (answer, label)
    wrong = ("x" if password[0] != "x" else "y") + password[1:]
    started = time.monotonic()
    answer, _, _ = paste(GATEWAY, lambda nonce: paste_digest(nonce, wrong), "WRONG PASSWORD")
    assert answer == "error auth" and time.monotonic() - started >= 0.9, answer
    answer, _, _ = paste(GATEWAY, lambda nonce: digest, "REPLAYED DIGEST")
    assert answer == "error auth", answer
    live.close()
    answer = project.say("gateway", f"/browser return {second}")
    assert answer == "브라우저를 돌려줬습니다.", answer
    check_control_state(project, user=False, label=" (after /browser return <episode>)")
    value = field_value(project.call("gateway", {"action": "look"}))
    assert value == SENTENCE, "the pasted sentence did not arrive unchanged"
    print(f"[gateway] the pasted Korean sentence ({len(SENTENCE)} characters) reads back the same; a wrong "
          "password and a digest replayed on a new connection both get 'error auth' and change nothing", flush=True)

    status = project.say("gateway", "/browser status")
    assert "브라우저: 열림" in status and "조작: 어시스턴트" in status and "넘겨받은 곳: 없음" in status, status
    assert f"화면 보기: http://127.0.0.1:{GATEWAY}/shacs-view.html" in status, status
    assert project.say("gateway", "/browser close") == "브라우저를 닫았습니다."
    time.sleep(1)
    names = [entry["name"] for entry in project.control("gateway", {"op": "facts"})["processes"]]
    assert names == [], names
    print("[gateway] /browser status and /browser close answered; no browser or viewer process is left", flush=True)

    # The api role advertises its own host port.
    assert field_value(project.call("api", {"action": "open", "url": PAGE})) == "untouched"
    status = project.say("api", "/browser status")
    assert f"화면 보기: http://127.0.0.1:{API}/shacs-view.html" in status, status
    project.call("api", {"action": "close"})
    print(f"[api] /browser status shows http://127.0.0.1:{API}/shacs-view.html", flush=True)
    print("PASS: takeover URL, screen input, paste (Korean, wrong password, replay), Origin check, x11vnc "
          "flags, view-routes, websockify swap and process group, stale connections, clipboard, return, "
          "status and close, api URL", flush=True)


if __name__ == "__main__":
    main()
