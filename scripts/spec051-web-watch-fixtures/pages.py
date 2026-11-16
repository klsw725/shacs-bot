"""JS-rendered and cookie-gated pages; control stays inside the disposable network."""
import http.server
import json
import threading
import urllib.parse

lock = threading.Lock()
released = threading.Event()
released.set()
price = 12900
decoration = "first"
waiting = 0
COOKIE = "spec051_login=disposable-fixture"


class Pages(http.server.BaseHTTPRequestHandler):
    def log_message(self, format: str, *args) -> None:
        """Do not log cookie headers or request contents."""

    def send(self, content: str, kind: str = "text/html", cookie: bool = False) -> None:
        data = content.encode()
        self.send_response(200)
        self.send_header("Content-Type", kind + "; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        if cookie:
            self.send_header("Set-Cookie", COOKIE + "; Max-Age=86400; Path=/")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:
        global waiting
        url = urllib.parse.urlsplit(self.path)
        match url.path:
            case "/login":
                self.send("<p>Fixture login ready</p>", cookie=True)
            case "/rendered" | "/account":
                account = url.path == "/account"
                self.send("<p>unique-whole-page-marker</p><div id='decoration'></div><div id='value'>Loading</div><script>"
                    f"fetch('/value?account={int(account)}').then(r=>r.json()).then(v=>{{"
                    "document.getElementById('value').innerText=v.allowed?'Price: '+v.price+' USD':"
                    "'Please log in';document.getElementById('decoration').innerText=v.decoration;});</script>")
            case "/value":
                account = urllib.parse.parse_qs(url.query).get("account") == ["1"]
                if account:
                    with lock:
                        waiting += 1
                    try:
                        if not released.wait(30):
                            raise TimeoutError("page release deadline")
                    finally:
                        with lock:
                            waiting -= 1
                with lock:
                    payload = {"price": price, "decoration": decoration,
                        "allowed": not account or COOKIE in self.headers.get("Cookie", "")}
                self.send(json.dumps(payload), "application/json")
            case "/state":
                with lock:
                    state = {"price": price, "decoration": decoration, "waiting": waiting}
                self.send(json.dumps(state), "application/json")
            case _:
                self.send_error(404)

    def do_POST(self) -> None:
        global price, decoration
        assert self.path == "/control"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with lock:
            price = int(body.get("price", price))
            decoration = str(body.get("decoration", decoration))
        if body.get("pause"):
            released.clear()
        if body.get("release"):
            released.set()
        self.send("{}", "application/json")


if __name__ == "__main__":
    http.server.ThreadingHTTPServer(("0.0.0.0", 8000), Pages).serve_forever()
