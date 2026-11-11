"""Opt-in host-side viewer checks (Spec 049 PRD 040 scope 8). Standard library only.

Usage: python3 host_check.py <compose project> <compose file> <env file with the password>

The password is read from the temporary `.env.json` and used only for the
RFB handshake; it is never printed.
"""
import json
import pathlib
import socket
import struct
import subprocess
import sys
import time
import urllib.request

PAGE = "http://172.30.250.2:8000/viewer.html"
SERVICES = {"gateway": 6080, "api": 6081}
ALLOWLIST = {"PATH", "LANG", "LC_ALL", "LC_CTYPE", "DISPLAY", "XAUTHORITY", "HOME",
             "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR", "TMPDIR"}

# --- DES (FIPS 46-3), enough for the 16-byte VNC authentication challenge ---
IP = [58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4,
      62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48, 40, 32, 24, 16, 8,
      57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3,
      61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7]
FP = [40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31,
      38, 6, 46, 14, 54, 22, 62, 30, 37, 5, 45, 13, 53, 21, 61, 29,
      36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
      34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25]
E = [32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13,
     12, 13, 14, 15, 16, 17, 16, 17, 18, 19, 20, 21, 20, 21, 22, 23, 24, 25,
     24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1]
P = [16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10,
     2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6, 22, 11, 4, 25]
PC1 = [57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18,
       10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44, 36,
       63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22,
       14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20, 12, 4]
PC2 = [14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10,
       23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2,
       41, 52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48,
       44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32]
SHIFTS = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1]
SBOX = [
    [[14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7],
     [0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8],
     [4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0],
     [15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13]],
    [[15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10],
     [3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5],
     [0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15],
     [13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9]],
    [[10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8],
     [13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1],
     [13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7],
     [1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12]],
    [[7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15],
     [13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9],
     [10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4],
     [3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14]],
    [[2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9],
     [14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6],
     [4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14],
     [11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3]],
    [[12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11],
     [10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8],
     [9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6],
     [4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13]],
    [[4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1],
     [13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6],
     [1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2],
     [6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12]],
    [[13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7],
     [1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2],
     [7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8],
     [2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11]],
]


def bits(value, width):
    return [(value >> (width - 1 - index)) & 1 for index in range(width)]


def number(bit_list):
    result = 0
    for bit in bit_list:
        result = (result << 1) | bit
    return result


def permute(bit_list, table):
    return [bit_list[index - 1] for index in table]


def des_subkeys(key):
    permuted = permute(bits(int.from_bytes(key, "big"), 64), PC1)
    left, right = permuted[:28], permuted[28:]
    subkeys = []
    for shift in SHIFTS:
        left = left[shift:] + left[:shift]
        right = right[shift:] + right[:shift]
        subkeys.append(permute(left + right, PC2))
    return subkeys


def des_block(block, subkeys):
    state = permute(bits(int.from_bytes(block, "big"), 64), IP)
    left, right = state[:32], state[32:]
    for subkey in subkeys:
        expanded = [bit ^ key_bit for bit, key_bit in zip(permute(right, E), subkey)]
        substituted = []
        for box in range(8):
            chunk = expanded[box * 6:box * 6 + 6]
            row = (chunk[0] << 1) | chunk[5]
            column = number(chunk[1:5])
            substituted.extend(bits(SBOX[box][row][column], 4))
        mixed = permute(substituted, P)
        left, right = right, [bit ^ mix for bit, mix in zip(left, mixed)]
    return number(permute(right + left, FP)).to_bytes(8, "big")


def vnc_response(password, challenge):
    """VNC authentication: DES with the first 8 password bytes, each bit-reversed."""
    key = bytes(int(f"{byte:08b}"[::-1], 2) for byte in password.encode()[:8].ljust(8, b"\0"))
    subkeys = des_subkeys(key)
    return des_block(challenge[:8], subkeys) + des_block(challenge[8:], subkeys)


def self_test():
    subkeys = des_subkeys(bytes.fromhex("133457799BBCDFF1"))
    assert des_block(bytes.fromhex("0123456789ABCDEF"), subkeys) == bytes.fromhex("85E813540F0AB405")


# --- WebSocket client framing (RFC 6455) over which websockify carries RFB ---
class WebSocketRfb:
    def __init__(self, port):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=20)
        request = (f"GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
                   "Connection: Upgrade\r\nSec-WebSocket-Key: c2hhY3Mtdmlld2VyLWNoZWNrAA==\r\n"
                   "Sec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: binary\r\n\r\n")
        self.sock.sendall(request.encode())
        header = b""
        while b"\r\n\r\n" not in header:
            chunk = self.sock.recv(4096)
            assert chunk, "websocket handshake closed"
            header += chunk
        head, _, rest = header.partition(b"\r\n\r\n")
        assert head.split(b"\r\n")[0].endswith(b"101 Switching Protocols"), head
        self.frames = bytearray(rest)
        self.stream = bytearray()

    def raw(self, count):
        while len(self.frames) < count:
            chunk = self.sock.recv(65536)
            assert chunk, "websocket closed"
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
        masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        self.sock.sendall(header + mask + masked)

    def read(self, count):
        while len(self.stream) < count:
            self.stream += self.recv_frame()
        data = bytes(self.stream[:count])
        del self.stream[:count]
        return data

    def write(self, data):
        self.send_frame(data)

    def close(self):
        self.sock.close()


def rfb_authenticate(port, password):
    """Returns (connection, True) after a successful VNC auth, or (None, False)."""
    rfb = WebSocketRfb(port)
    version = rfb.read(12)
    assert version.startswith(b"RFB 003.00"), version
    rfb.write(b"RFB 003.008\n")
    count = rfb.read(1)[0]
    assert count > 0, "server refused the connection before authentication"
    types = rfb.read(count)
    assert 2 in types and 1 not in types, f"VNC authentication must be required, got {list(types)}"
    rfb.write(b"\x02")
    challenge = rfb.read(16)
    rfb.write(vnc_response(password, challenge))
    result = struct.unpack("!I", rfb.read(4))[0]
    if result != 0:
        rfb.close()
        return None, False
    return rfb, True


def read_update(rfb, message_type=None):
    """One FramebufferUpdate with raw rectangles; returns the rectangle list."""
    if message_type is None:
        message_type = rfb.read(1)[0]
    assert message_type == 0, f"expected FramebufferUpdate, got {message_type}"
    rfb.read(1)
    count = struct.unpack("!H", rfb.read(2))[0]
    rectangles = []
    for _ in range(count):
        x, y, rect_width, rect_height, encoding = struct.unpack("!HHHHi", rfb.read(12))
        assert encoding == 0, (x, y, rect_width, rect_height, encoding)
        pixels = rfb.read(rect_width * rect_height * 4)
        assert len(pixels) == rect_width * rect_height * 4
        rectangles.append((x, y, rect_width, rect_height))
    assert rectangles, "empty framebuffer update"
    return rectangles


def framebuffer(rfb):
    rfb.write(b"\x01")  # ClientInit: shared
    width, height = struct.unpack("!HH", rfb.read(4))
    rfb.read(16)
    name_length = struct.unpack("!I", rfb.read(4))[0]
    name = rfb.read(name_length)
    rfb.write(struct.pack("!BxHi", 2, 1, 0))  # SetEncodings: raw only
    rfb.write(struct.pack("!BBHHHH", 3, 0, 0, 0, 64, 64))  # FramebufferUpdateRequest
    rectangles = read_update(rfb)
    return width, height, name.decode(errors="replace"), len(rectangles)


def inject_input(rfb, width, height):
    x, y = width // 2, height // 2
    for mask in (1, 0):
        rfb.write(struct.pack("!BBHH", 5, mask, x, y))  # PointerEvent
    for key in (0x61, 0xFF0D):  # 'a', Return
        rfb.write(struct.pack("!BBxxI", 4, 1, key))
        rfb.write(struct.pack("!BBxxI", 4, 0, key))
    text = b"VIEWER PASTE"
    rfb.write(struct.pack("!BxxxI", 6, len(text)) + text)  # ClientCutText
    # The server must still be serving us after the ignored input.
    rfb.write(struct.pack("!BBHHHH", 3, 0, 0, 0, 8, 8))
    read_update(rfb)


def expect_no_server_cut_text(rfb, seconds):
    """Reads server messages for `seconds`; a ServerCutText (type 3) fails."""
    rfb.sock.settimeout(seconds)
    seen = []
    try:
        while True:
            message_type = rfb.read(1)[0]
            seen.append(message_type)
            assert message_type != 3, "server forwarded a selection (ServerCutText)"
            if message_type == 0:
                read_update(rfb, message_type)
            elif message_type == 2:
                continue  # Bell
            else:
                raise AssertionError(f"unexpected server message {message_type}")
    except (socket.timeout, TimeoutError):
        pass
    finally:
        rfb.sock.settimeout(20)
    return seen


# --- Compose orchestration ---
class Project:
    def __init__(self, name, compose_file):
        self.compose = ["docker", "compose", "--project-name", name, "--file", compose_file]

    def control(self, service, request):
        output = subprocess.run(self.compose + ["exec", "-T", service, "python3", "-I",
                                                "/fixtures/viewer_check.py", "client", json.dumps(request)],
                                capture_output=True, text=True, timeout=240)
        assert output.returncode == 0, output.stderr
        answer = json.loads(output.stdout)
        assert answer["ok"], f"{service} {request.get('op')}: {answer.get('error')}"
        return answer

    def call(self, service, action):
        return self.control(service, {"op": "call", "action": action})["result"]

    def wait_ready(self, service):
        until = time.monotonic() + 180
        while True:
            output = subprocess.run(self.compose + ["exec", "-T", service, "python3", "-I",
                                                    "/fixtures/viewer_check.py", "client",
                                                    json.dumps({"op": "ready"})],
                                    capture_output=True, text=True, timeout=60)
            if output.returncode == 0 and '"ready": true' in output.stdout:
                return
            assert time.monotonic() < until, f"{service} control server not ready: {output.stderr}"
            time.sleep(1)

    def published_ports(self, service):
        container = subprocess.run(self.compose + ["ps", "-q", service], capture_output=True,
                                   text=True, check=True).stdout.strip()
        assert container, f"{service} container missing"
        raw = subprocess.run(["docker", "inspect", "--format", "{{json .NetworkSettings.Ports}}", container],
                             capture_output=True, text=True, check=True).stdout
        return json.loads(raw)


def events_count(snapshot):
    assert isinstance(snapshot, dict), snapshot
    text = snapshot["text"]
    assert "EVENTS: " in text, text
    return int(text.split("EVENTS: ", 1)[1].split()[0])


def check_viewer_facts(service, facts, password):
    names = [entry["name"] for entry in facts["processes"]]
    assert names.count("x11vnc") == 1 and names.count("websockify") == 1, names
    assert names.count("chromium") == 1 and names.count("xvfb") == 1, names
    x11vnc = next(entry for entry in facts["processes"] if entry["name"] == "x11vnc")
    argv = x11vnc["argv"]
    for flag in ["-norc", "-noremote", "-viewonly", "-nosel", "-noclipboard", "-localhost", "-forever", "-shared"]:
        assert flag in argv, (flag, argv)
    assert argv[argv.index("-listen") + 1] == "127.0.0.1" and argv[argv.index("-rfbport") + 1] == "5900"
    password_file = argv[argv.index("-passwdfile") + 1]
    assert password_file.startswith("rm:/tmp/shacs-browser-"), password_file
    assert password not in " ".join(argv), "password in x11vnc argv"
    assert "-passwd" not in argv and "-env" not in argv
    websockify = next(entry for entry in facts["processes"] if entry["name"] == "websockify")
    assert "--web=/usr/share/novnc" in websockify["argv"] and "0.0.0.0:6080" in websockify["argv"]
    assert "127.0.0.1:5900" in websockify["argv"], websockify["argv"]
    for entry in facts["processes"]:
        if entry["name"] in ("x11vnc", "websockify"):
            assert entry["env_keys"] is not None and set(entry["env_keys"]) <= ALLOWLIST, entry["env_keys"]
    assert facts["password_files"] == [], facts["password_files"]
    assert facts["roots"], "browser temporary folder missing"
    # /proc/net/tcp{,6} hex forms of 127.0.0.1 and ::1; x11vnc -localhost binds both.
    loopback = ("0100007F:", "00000000000000000000000001000000:")
    assert facts["vnc_listeners"] and all(local.startswith(loopback) for local in facts["vnc_listeners"]), \
        facts["vnc_listeners"]
    assert any(local.startswith("00000000:") for local in facts["web_listeners"]), facts["web_listeners"]
    print(f"[{service}] x11vnc argv: {' '.join(argv)}", flush=True)
    print(f"[{service}] viewer child env keys: {sorted(set(k for e in facts['processes'] if e['name'] in ('x11vnc', 'websockify') for k in e['env_keys']))}", flush=True)
    print(f"[{service}] password file after start: absent ({password_file[3:]} does not exist, no /tmp/shacs-browser-*/view-*)", flush=True)
    print(f"[{service}] VNC listeners: {facts['vnc_listeners']} (loopback only)", flush=True)


def check_published_ports(project, service, host_port):
    ports = project.published_ports(service)
    bound = {key: value for key, value in ports.items() if value}
    assert set(bound) == {"6080/tcp"}, ports
    assert all(binding["HostIp"] == "127.0.0.1" and binding["HostPort"] == str(host_port)
               for binding in bound["6080/tcp"]), ports
    assert "5900/tcp" not in ports, ports
    print(f"[{service}] Compose publishes only 127.0.0.1:{host_port} -> 6080/tcp; 5900 is not published", flush=True)


def check_clipboard(project, service, rfb, selection_before):
    """Clipboard is blocked both ways on the server: the client's ClientCutText
    changed no X selection or cut buffer, and a selection set on the server
    side (PRIMARY, CLIPBOARD and CUT_BUFFER0 from a helper window) produces
    no ServerCutText for the authenticated client."""
    if not selection_before.get("available"):
        print(f"[{service}] SKIPPED X selection check: {selection_before.get('detail')} "
              "(libX11 is expected from the chromium dependency; no package was added)", flush=True)
    else:
        selection_after = project.control(service, {"op": "selection"})
        for key in ("primary_owner", "clipboard_owner", "cut_buffer0"):
            assert selection_after[key] == selection_before[key], (key, selection_before, selection_after)
        assert "VIEWER PASTE" not in selection_after["cut_buffer0"]
        print(f"[{service}] X PRIMARY/CLIPBOARD owners and CUT_BUFFER0 unchanged after ClientCutText: "
              f"{json.dumps({key: selection_after[key] for key in ('primary_owner', 'clipboard_owner', 'cut_buffer0')})}", flush=True)
    marker = "SERVER SIDE SELECTION MARKER"
    owned = project.control(service, {"op": "set_selection", "text": marker})
    if not owned.get("available"):
        print(f"[{service}] SKIPPED server-side selection probe: {owned.get('detail')}", flush=True)
        return
    assert owned["primary_owner"] == owned["window"] and owned["clipboard_owner"] == owned["window"], owned
    state = project.control(service, {"op": "selection"})
    assert state["cut_buffer0"] == marker and state["primary_owner"] == owned["window"], state
    seen = expect_no_server_cut_text(rfb, 3)
    print(f"[{service}] a selection owned on the server (PRIMARY/CLIPBOARD window {owned['window']}, "
          f"CUT_BUFFER0 set) produced no ServerCutText in 3 s (server messages seen: {seen})", flush=True)


def check_host_5900(project):
    try:
        with socket.create_connection(("127.0.0.1", 5900), timeout=2):
            reachable = True
    except OSError:
        reachable = False
    if reachable:
        # Some hosts run their own VNC service on 5900 (macOS Screen Sharing).
        # The contract is that nothing from this project publishes it.
        for service in SERVICES:
            assert "5900/tcp" not in project.published_ports(service)
        print("[host] 127.0.0.1:5900 answers, but neither container publishes 5900; that listener is not ours", flush=True)
    else:
        print("[host] 127.0.0.1:5900 is not reachable", flush=True)


def main():
    self_test()
    project_name, compose_file, env_file = sys.argv[1:4]
    password = json.loads(pathlib.Path(env_file).read_text())["SHACS_BROWSER_VIEW_PASSWORD"]
    assert len(password) >= 8
    project = Project(project_name, compose_file)
    for service in SERVICES:
        project.wait_ready(service)
    try:
        checks(project, password)
    except Exception:
        # Diagnostics for a failed run: runtime log tail and process list of
        # each container. The runtime log has previews only, never the password.
        for service in SERVICES:
            for op in ("log_tail", "ps"):
                try:
                    answer = project.control(service, {"op": op})
                    print(f"[{service}] {op}: {json.dumps(answer)[-6000:]}", flush=True)
                except Exception as error:  # the primary failure is re-raised below
                    print(f"[{service}] {op} unavailable: {error}", flush=True)
        raise


def checks(project, password):

    # Both services open the fixture page; the viewer starts with the browser.
    counts = {}
    for service, host_port in SERVICES.items():
        snapshot = project.call(service, {"action": "open", "url": PAGE})
        counts[service] = events_count(snapshot)
        # Evidence first, assertions second: the browser status as the separate
        # inspect sees it right after the first turn.
        inspected_after_open = project.control(service, {"op": "inspect"})["browser"]
        assert password not in json.dumps(inspected_after_open)
        print(f"[{service}] inspect after open: {json.dumps(inspected_after_open)}", flush=True)
        facts = project.control(service, {"op": "viewer"})
        if not any(entry["name"] == "chromium" for entry in facts["processes"]):
            print(f"[{service}] ps after open: {json.dumps(project.control(service, {'op': 'ps'})['processes'])}", flush=True)
        check_viewer_facts(service, facts, password)
        check_published_ports(project, service, host_port)
        inspected = project.control(service, {"op": "inspect"})["browser"]
        assert password not in json.dumps(inspected)
        # The gateway role runs `shacs-bot run` and advertises 6080; the api
        # role runs `shacs-bot serve` and advertises 6081.
        expected_url = f"http://127.0.0.1:{host_port}/vnc.html"
        assert inspected["view"]["state"] == "open" and inspected["view"]["url"] == expected_url, inspected
        print(f"[{service}] separate runtime inspect view: {json.dumps(inspected['view'])}", flush=True)

    # Host side: static noVNC page without a password, framebuffer with the
    # right password, refusal with a wrong one.
    for service, host_port in SERVICES.items():
        with urllib.request.urlopen(f"http://127.0.0.1:{host_port}/vnc.html", timeout=20) as response:
            body = response.read().decode(errors="replace")
            assert response.status == 200 and "noVNC" in body
        rfb, ok = rfb_authenticate(host_port, password)
        assert ok, f"{service}: correct password rejected"
        width, height, name, rectangles = framebuffer(rfb)
        assert (width, height) == (1280, 720), (width, height)
        print(f"[{service}] host 127.0.0.1:{host_port}: framebuffer {width}x{height} '{name}', {rectangles} rectangle(s)", flush=True)
        if service == "gateway":
            # Prove the fixture counter is live, then show the viewer's input never reaches it.
            before = events_count(project.call(service, {"action": "press", "key": "Tab"}))
            assert before > counts[service], before
            selection_before = project.control(service, {"op": "selection"})
            inject_input(rfb, width, height)
            time.sleep(1)
            after = project.call(service, {"action": "look"})
            assert events_count(after) == before, after["text"]
            assert "VIEWER PASTE" not in after["text"]
            field = next(element for element in after["elements"] if element["name"] == "Field")
            assert field["value"] == "untouched", field
            print(f"[{service}] PointerEvent/KeyEvent/ClientCutText from an authenticated client changed nothing (EVENTS stayed {before})", flush=True)
            check_clipboard(project, service, rfb, selection_before)
        rfb.close()
        wrong = ("x" if password[0] != "x" else "y") + password[1:]
        refused, ok = rfb_authenticate(host_port, wrong)
        assert not ok and refused is None, f"{service}: wrong password accepted"
        print(f"[{service}] wrong password (first character changed) rejected", flush=True)
        # Documented limit: VNC authentication uses the first 8 characters only.
        tail_changed, ok = rfb_authenticate(host_port, password[:8] + "ZZZZZZZZ")
        assert ok, f"{service}: a password with the same first 8 characters must authenticate (VNC limit)"
        tail_changed.close()
        print(f"[{service}] a password matching only the first 8 characters authenticates: VNC checks 8 characters", flush=True)

    check_host_5900(project)

    # The browser cannot reach the viewer, WebSocket, API or control ports of
    # its own container.
    for address in ["127.0.0.1", "172.30.250.3"]:
        for port in [6080, 5900, 8765, 8900, 9100]:
            reply = project.control("gateway", {"op": "socks", "address": address, "port": port})["reply"]
            assert reply != 0, (address, port)
    denied = project.call("gateway", {"action": "open", "url": "http://127.0.0.1:6080/vnc.html"})
    assert not isinstance(denied, dict), denied
    project.call("gateway", {"action": "open", "url": PAGE})
    print("[gateway] browser proxy refuses 127.0.0.1/172.30.250.3 on 6080, 5900, 8765, 8900, 9100", flush=True)

    # Killing one viewer child keeps the browser; the pair is closed.
    killed = project.control("gateway", {"op": "kill", "name": "x11vnc"})["killed"]
    assert killed
    time.sleep(2)
    facts = project.control("gateway", {"op": "viewer"})
    names = [entry["name"] for entry in facts["processes"]]
    assert "x11vnc" not in names and "websockify" not in names, names
    assert "chromium" in names and "xvfb" in names, names
    look = project.call("gateway", {"action": "look"})
    assert "VIEWER FIXTURE" in look["text"], look
    inspected = project.control("gateway", {"op": "inspect"})["browser"]
    assert inspected["view"]["state"] == "failed" and inspected["view"]["url"] is None, inspected
    assert inspected["state"] in ("busy", "running"), inspected
    print(f"[gateway] after killing x11vnc: websockify gone, browser look works, inspect view={json.dumps(inspected['view'])}", flush=True)

    # Browser close ends the viewer.
    closed = project.call("api", {"action": "close"})
    assert "browser_closed" in str(closed), closed
    time.sleep(1)
    facts = project.control("api", {"op": "viewer"})
    names = [entry["name"] for entry in facts["processes"]]
    assert not any(name in names for name in ("x11vnc", "websockify", "chromium", "xvfb")), names
    assert facts["password_files"] == [] and facts["roots"] == [], facts
    try:
        rfb_authenticate(6081, password)
        raise AssertionError("api viewer still answers after browser close")
    except (AssertionError, ConnectionError, OSError) as error:
        if isinstance(error, AssertionError) and "still answers" in str(error):
            raise
    print("[api] browser close ended x11vnc and websockify; host 6081 no longer serves a framebuffer", flush=True)
    print("PASS: viewer auth, framebuffer, wrong password, server-side input rejection, clipboard blocked both ways, gateway(run)=6080/api(serve)=6081 URLs, loopback-only publish, proxy boundary, password file, viewer death, browser close", flush=True)


if __name__ == "__main__":
    main()
