"""Reuse the 050 gateway driver; only provider scripts and local controls differ."""
import json
import importlib
import pathlib
import subprocess
import sys
import urllib.request

sys.path.insert(0, "/saved-fixtures")
saved = importlib.import_module("driver")

base = saved.driver
original = saved.handle
provider_calls = 0
type Json = str | int | float | bool | None | list[Json] | dict[str, Json]


class Provider(saved.Provider):
    def do_POST(self) -> None:
        global provider_calls
        provider_calls += 1
        super().do_POST()


def say(chat_id: str, text: str) -> str:
    chat = base.WebSocketChat()
    try:
        chat.send(json.dumps({"type": "message", "chat_id": chat_id, "content": text}))
        for event in chat.events():
            if event.get("event") == "error":
                raise RuntimeError(json.dumps(event))
            if event.get("event") == "message" and event.get("chat_id") == chat_id:
                return event.get("text", "")
        raise RuntimeError("gateway closed without a response")
    finally:
        chat.close()


def statuses() -> list[dict[str, Json]]:
    snapshots = []
    for path in pathlib.Path("/check-data/workspace/sessions").glob("*.jsonl"):
        with path.open() as stream:
            header = json.loads(stream.readline())
        for watch in header.get("metadata", {}).get("web_watch", {}).get("watches", []):
            result = subprocess.run(base.BOT + ["work", "web-status", watch["work_id"],
                "--workspace", "/check-data/workspace", "--session", header["key"],
                "--data-dir", "/check-data"], capture_output=True, text=True, check=True)
            snapshots.append(json.loads(result.stdout))
    return snapshots


def handle(request):
    match request["op"]:
        case "message":
            if "proposal" in request:
                base.commands.append({"__tool": "propose_web_watch", **request["proposal"]})
            return {"text": say(request["chat"], request["text"])}
        case "clock":
            path = pathlib.Path("/check-data/clock")
            temporary = path.with_suffix(".next")
            temporary.write_text(str(request["now"]))
            temporary.replace(path)
            return {"now": request["now"]}
        case "status":
            return {"watches": statuses(), "provider_calls": provider_calls}
        case "deliveries":
            path = pathlib.Path("/check-data/runtime/channels/worker-metadata/email-imap.json")
            return {"deliveries": json.loads(path.read_text()).get("deliveries", [])
                    if path.exists() else []}
        case "page":
            payload = request.get("control")
            url = "http://172.30.250.2:8000/" + ("control" if payload is not None else "state")
            query = urllib.request.Request(url, None if payload is None else json.dumps(payload).encode(),
                                           {"Content-Type": "application/json"})
            with urllib.request.urlopen(query, timeout=10) as response:
                return {"page": json.loads(response.read())}
        case _:
            return original(request)


if __name__ == "__main__":
    base.Provider = Provider
    base.handle = handle
    match sys.argv[1]:
        case "serve":
            base.role = "gateway"
            base.serve()
        case "client":
            base.client(sys.argv[2])
        case _:
            raise SystemExit("driver.py serve gateway | client <json>")
