"""Opt-in fixture driver. Never imported by Cargo or CI."""
import http.server
import json
import os
import pathlib
import socket
import struct
import subprocess
import select
import tempfile
import threading
import time
import urllib.request

BASE = "http://172.30.249.2:8000"
commands = []
results = []
request_counter = 0


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
            assert tools, body
            results.append(tools[-1]["content"])
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


def call(action):
    commands.append(action)
    count = len(results)
    request = urllib.request.Request("http://127.0.0.1:8900/v1/chat/completions",
        json.dumps({"model": "gpt-4o", "messages": [{"role": "user", "content": "run fixture"}],
                    "session_id": "spec049:disposable", "stream": False}).encode(),
        {"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=150) as response:
        response.read()
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


def target(snapshot, name):
    assert isinstance(snapshot, dict), snapshot
    return next(element["element"] for element in snapshot["elements"] if element["name"] == name)


def processes():
    found = []
    for directory in pathlib.Path("/proc").iterdir():
        if directory.name.isdigit():
            try:
                arguments = (directory / "cmdline").read_bytes().split(b"\0")
                if any(b"--remote-debugging-pipe" == arg for arg in arguments):
                    found.append((directory, [arg.decode() for arg in arguments if arg]))
            except (FileNotFoundError, PermissionError):
                continue
    return found


def socks(proxy, address, port):
    with socket.create_connection(("127.0.0.1", proxy), timeout=5) as connection:
        connection.sendall(b"\x05\x01\x00")
        assert connection.recv(2) == b"\x05\x00"
        connection.sendall(b"\x05\x01\x00\x01" + socket.inet_aton(address) + struct.pack("!H", port))
        response = connection.recv(10)
        assert len(response) >= 2
        return response[1]


def applied_policy(proxy):
    """Inspect Chromium's policy WebUI out of band, never through the model tool."""
    # The probe browser may still be removing its own profile files while it
    # exits; a cleanup race there is not a finding about the browser under test.
    with tempfile.TemporaryDirectory(prefix="shacs-policy-probe-", ignore_cleanup_errors=True) as profile:
        read_input, write_input = os.pipe()
        read_output, write_output = os.pipe()
        arguments = ["chromium", "--headless", "--no-sandbox", "--disable-gpu",
                     "--no-first-run", "--disable-background-networking",
                     "--remote-debugging-pipe", "--lang=en-US", f"--user-data-dir={profile}",
                     f"--proxy-server=socks5://127.0.0.1:{proxy}", "--proxy-bypass-list=<-loopback>",
                     "--disable-quic", "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
                     "about:blank"]
        process = subprocess.Popen(
            ["bash", "-c", f'exec 3<&{read_input} 4>&{write_output}; exec "$@"',
             "policy-probe", *arguments],
            pass_fds=(read_input, write_output), stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, env={"PATH": os.environ["PATH"], "HOME": profile})
        os.close(read_input)
        os.close(write_output)
        pending = bytearray()
        sequence = 0

        def request(method, params, session=None):
            nonlocal sequence
            sequence += 1
            message = {"id": sequence, "method": method, "params": params}
            if session:
                message["sessionId"] = session
            os.write(write_input, json.dumps(message).encode() + b"\0")
            end = time.monotonic() + 15
            while True:
                while b"\0" in pending:
                    raw, _, remainder = pending.partition(b"\0")
                    pending[:] = remainder
                    response = json.loads(raw)
                    if response.get("id") == sequence:
                        assert "error" not in response, response
                        return response["result"]
                remaining = end - time.monotonic()
                assert remaining > 0 and select.select([read_output], [], [], remaining)[0], method
                data = os.read(read_output, 65536)
                assert data, "policy probe pipe closed"
                pending.extend(data)
        try:
            target = request("Target.createTarget", {"url": "chrome://policy"})["targetId"]
            session = request("Target.attachToTarget", {"targetId": target, "flatten": True})["sessionId"]
            end = time.monotonic() + 15
            expression = """(()=>{const text=[];function walk(root){for(const e of root.querySelectorAll('*')){
                if(e.shadowRoot)walk(e.shadowRoot);if(e.children.length===0&&e.textContent)text.push(e.textContent);
            }}walk(document);return text.join('\\n');})()"""
            while True:
                result = request("Runtime.evaluate", {"expression": expression, "returnByValue": True}, session)
                text = result.get("result", {}).get("value", "")
                if all(name in text for name in ["URLBlocklist", "URLAllowlist", "DownloadRestrictions"]):
                    assert text.count("OK") >= 3, text
                    print("Applied Chromium policy:", text, flush=True)
                    break
                assert time.monotonic() < end, "Chromium did not report the managed policies: " + text
                time.sleep(0.1)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            os.close(write_input)
            os.close(read_output)


provider = http.server.ThreadingHTTPServer(("127.0.0.1", 9001), Provider)
threading.Thread(target=provider.serve_forever, daemon=True).start()
# A real listener proves denial does not merely mean the VNC service was absent.
vnc = socket.socket()
vnc.bind(("127.0.0.1", 5900))
vnc.listen()
log = open("/check-data/runtime.log", "w+")
bot = subprocess.Popen(["shacs-bot", "--config", "/check-data/config.json",
                        "serve", "--host", "127.0.0.1", "--port", "8900",
                        "--workspace", "/check-data/workspace", "--allow-api-side-effects"],
                       stdout=log, stderr=subprocess.STDOUT)
try:
    until = time.monotonic() + 60
    while True:
        if bot.poll() is not None:
            raise RuntimeError("bot exited: " + pathlib.Path(log.name).read_text())
        try:
            with urllib.request.urlopen("http://127.0.0.1:8900/health", timeout=1):
                break
        except OSError:
            if time.monotonic() >= until:
                raise
            time.sleep(0.2)
    snapshot = call({"action": "open", "url": BASE})
    assert isinstance(snapshot, dict), snapshot
    browser = processes()
    assert len(browser) == 1, browser
    process, argv = browser[0]
    assert "--headless" not in argv and any(arg.startswith("--display=") for arg in argv)
    assert any(b"Xvfb" in path.read_bytes() for path in pathlib.Path("/proc").glob("[0-9]*/cmdline"))
    profile = pathlib.Path(next(arg.split("=", 1)[1] for arg in argv if arg.startswith("--user-data-dir=")))
    proxy = int(next(arg.rsplit(":", 1)[1] for arg in argv if arg.startswith("--proxy-server=")))
    snapshot = call({"action": "type", "element": target(snapshot, "Query"),
                     "revision": snapshot["revision"], "text": "hello"})
    snapshot = call({"action": "press", "key": "Tab"})
    assert isinstance(snapshot, dict), snapshot
    snapshot = call({"action": "click", "element": target(snapshot, "Apply"), "revision": snapshot["revision"]})
    assert "RESULT: hello" in snapshot["text"], snapshot
    assert processes()[0][0] == process, "browser changed between API turns"
    assert "tool-results/" in results[-1] and "base64," not in results[-1]
    for name in ["Password", "New password", "Upload"]:
        snapshot = call({"action": "look"})
        refused = call({"action": "type", "element": target(snapshot, name),
                        "revision": snapshot["revision"], "text": "refuse"})
        assert "browser_input_denied" in str(refused), refused
    snapshot = call({"action": "look"})
    refused = call({"action": "click", "element": target(snapshot, "Upload"),
                    "revision": snapshot["revision"]})
    assert "browser_input_denied" in str(refused), refused
    snapshot = call({"action": "look"})
    call({"action": "click", "element": target(snapshot, "Password"), "revision": snapshot["revision"]})
    assert "browser_input_denied" in str(call({"action": "press", "key": "Enter"}))
    snapshot = call({"action": "look"})
    popup = call({"action": "click", "element": target(snapshot, "Popup"), "revision": snapshot["revision"]})
    assert isinstance(popup, dict) and "POPUP RESULT" in popup["text"], popup
    images = call({"action": "open", "url": BASE + "/images.html"})
    assert "MANY IMAGES" in images["text"]
    assert isinstance(call({"action": "look"}), dict), "image-heavy tab died"
    assert socks(proxy, "172.30.249.2", 8000) == 0
    for address in ["127.0.0.1", "172.30.249.3"]:
        for port in [8900, 8765, 5900, 9001]:
            assert socks(proxy, address, port) != 0, (address, port)
    assert not any(arg.startswith("--remote-debugging-port") for arg in argv)
    sockets = {entry.readlink().as_posix()[8:-1] for entry in (process / "fd").iterdir()
               if entry.readlink().as_posix().startswith("socket:[")}
    for table in ["/proc/net/tcp", "/proc/net/tcp6"]:
        for line in pathlib.Path(table).read_text().splitlines()[1:]:
            fields = line.split()
            assert fields[3] != "0A" or fields[9] not in sockets, "Chromium opened a TCP listener"
    for port in [8900, 8765, 5900]:
        denied = call({"action": "open", "url": f"http://127.0.0.1:{port}"})
        assert not isinstance(denied, dict), denied
        call({"action": "open", "url": BASE})
    for address in ["file:///etc/passwd", "chrome://version", "data:text/plain,PRIVATE"]:
        assert "browser_invalid_url" in str(call({"action": "open", "url": address}))
    snapshot = call({"action": "open", "url": BASE})
    download = call({"action": "click", "element": target(snapshot, "Download"), "revision": snapshot["revision"]})
    assert not list(pathlib.Path("/check-data").rglob("download.txt"))
    assert "browser_file_transfer_denied" in str(download) or isinstance(download, dict), download
    policy_path = pathlib.Path("/etc/chromium/policies/managed/shacs-browser.json")
    policy = json.loads(policy_path.read_text())
    assert policy["URLBlocklist"] == ["*"] and policy["URLAllowlist"] == ["http://*", "https://*"]
    assert policy["DownloadRestrictions"] == 3
    print("Policy file:", policy_path, json.dumps(policy), flush=True)
    print("Chromium:", subprocess.check_output(["chromium", "--version"], text=True).strip(), flush=True)
    call({"action": "open", "url": BASE})
    applied_policy(int(next(arg.rsplit(":", 1)[1] for arg in processes()[0][1] if arg.startswith("--proxy-server="))))
    # Internal navigation must never return page content, including script-triggered navigation.
    snapshot = call({"action": "open", "url": BASE})
    internal = call({"action": "click", "element": target(snapshot, "Internal script"), "revision": snapshot["revision"]})
    assert "Executable Path" not in str(internal) and "/etc/passwd" not in str(internal)
    call({"action": "open", "url": BASE})
    last_profile = pathlib.Path(next(arg.split("=", 1)[1] for arg in processes()[0][1] if arg.startswith("--user-data-dir=")))
    assert "browser_closed" in str(call({"action": "close"}))
    assert not profile.exists() and not last_profile.exists(), "temporary profile remains"
    print("PASS: numbered input, capture, turns, popup, images, sensitive inputs, proxy and cleanup", flush=True)
finally:
    bot.terminate()
    try:
        bot.wait(timeout=15)
    except subprocess.TimeoutExpired:
        bot.kill()
        bot.wait()
    provider.shutdown()
    vnc.close()
    log.close()
