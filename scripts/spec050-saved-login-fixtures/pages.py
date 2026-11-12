"""Disposable fixed pages for the opt-in saved-login check."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Pages(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args) -> None:
        pass

    def do_GET(self) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        match self.path:
            case "/login":
                self.send_header("Set-Cookie", "spec050_login=saved-fixture; Max-Age=86400; Path=/")
                text = "signed in"
            case "/whoami":
                text = self.headers.get("Cookie", "no cookie")
            case "/cached":
                self.send_header("Cache-Control", "max-age=3600")
                text = "fixed cache response " * 1024
            case _:
                text = "saved login fixture"
        body = ("<!doctype html><html><body><p>" + text + "</p></body></html>").encode()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8000), Pages).serve_forever()
