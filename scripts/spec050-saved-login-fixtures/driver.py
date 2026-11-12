"""Production gateway/serve with a loopback provider, for opt-in Docker only.

Reuse PRD 010's HTTP/WebSocket driver and process inspection. The one extra
provider operation is exec, to test the real permission path for lock writes.
"""
import json
import pathlib
import sys

sys.path.insert(0, "/takeover-fixtures")
import takeover_check as driver


class Provider(driver.Provider):
    def do_POST(self) -> None:
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if driver.commands:
            action = driver.commands.pop(0)
            name = action.pop("__tool", "browser")
            driver.request_counter += 1
            response = {"role": "assistant", "content": None, "tool_calls": [{
                "id": f"saved-login-{driver.request_counter}", "type": "function",
                "function": {"name": name, "arguments": json.dumps(action)}}]}
            reason = "tool_calls"
        else:
            tools = [message for message in body["messages"] if message.get("role") == "tool"]
            if tools:
                driver.results.append(tools[-1]["content"])
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


original_handle = driver.handle


def handle(request):
    match request["op"]:
        case "exec":
            return {"result": driver.call({"__tool": "exec", "command": request["command"]})}
        case "disk":
            facts = driver.facts()
            profile = pathlib.Path("/check-data/browser/profile")
            facts["profile_exists"] = profile.is_dir()
            facts["profile_mode"] = profile.stat().st_mode & 0o777 if profile.exists() else None
            facts["profile_cache"] = (profile / "Default/Cache").exists()
            facts["singleton"] = (profile / "SingletonLock").is_symlink()
            facts["cache_files"] = {
                root: sum(path.is_file() for path in (pathlib.Path(root) / "cache").rglob("*"))
                for root in facts["roots"]
            }
            return facts
        case _:
            return original_handle(request)


if __name__ == "__main__":
    driver.Provider = Provider
    driver.handle = handle
    match sys.argv[1]:
        case "serve":
            driver.role = sys.argv[2]
            driver.serve()
        case "client":
            driver.client(sys.argv[2])
        case _:
            raise SystemExit("driver.py serve gateway|api | client <json>")
