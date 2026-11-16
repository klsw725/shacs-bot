"""Approved host-side checks against real gateway, Chromium, flock and SMTP."""
import concurrent.futures
import hashlib
import json
import pathlib
import subprocess
import sys
import time
from typing import TypedDict

PAGE = "http://172.30.250.2:8000"
T0 = 1800000000000
CADENCE = 600000


class ChangeNotice(TypedDict):
    kind: str
    event_id: str
    correlation_id: str
    target: str


class WatchState(TypedDict):
    notifications: list[ChangeNotice]


class DeliveryRecord(TypedDict):
    id: str
    channel: str
    chat_id: str
    correlation_id: str | None
    content_sha256: str
    status: str


class MailReceipt(TypedDict):
    envelope_from: str
    envelope_to: list[str]
    message_id: str | None
    subject: str | None
    body: str


class EventReceipt(TypedDict):
    correlation_id: str
    delivery_id: str
    recipient: str
    content_sha256: str
    body: str
    message_id: str | None
    received_count: int


def verify_change_receipts(
    watches: list[WatchState],
    deliveries: list[DeliveryRecord],
    receipts: list[MailReceipt],
    expected: dict[str, EventReceipt] | None = None,
) -> dict[str, EventReceipt]:
    """Bind each event to production delivery metadata and the entire received body."""
    bindings: dict[str, EventReceipt] = {}
    for watch in watches:
        notices = [notice for notice in watch["notifications"] if notice["kind"] == "change"]
        assert len(notices) == 1, watch
        notice = notices[0]
        event_id = notice["event_id"]
        correlation = notice["correlation_id"]
        assert event_id not in bindings, event_id
        assert notice["target"].startswith("email:"), notice
        target = notice["target"].removeprefix("email:")
        records = [record for record in deliveries if record["correlation_id"] == correlation]
        assert [record["status"] for record in records] == ["pending", "sent"], records
        sent = records[-1]
        assert all(record["channel"] == "email" and record["chat_id"] == target
                   and record["id"] == sent["id"]
                   and record["content_sha256"] == sent["content_sha256"]
                   for record in records), records
        # SMTP uses CRLF and may add one terminating newline. The runtime's
        # final "Tracking: <correlation>" line has no terminating newline.
        received = [(mail, mail["body"].replace("\r\n", "\n").removesuffix("\n"))
                    for mail in receipts]
        matched = [(mail, body) for mail, body in received
                   if body.rsplit("\n", 1)[-1] == f"Tracking: {correlation}"]
        assert len(matched) == 1, (event_id, matched)
        mail, body = matched[0]
        assert mail["envelope_to"] == [target], (event_id, mail["envelope_to"])
        assert mail["envelope_from"] == "watch@example.test", mail["envelope_from"]
        assert hashlib.sha256(body.encode("utf-8")).hexdigest() == sent["content_sha256"], event_id
        bindings[event_id] = {
            "correlation_id": correlation, "delivery_id": sent["id"], "recipient": target,
            "content_sha256": sent["content_sha256"], "body": body,
            "message_id": mail["message_id"], "received_count": len(matched),
        }
    assert len(bindings) == len(receipts), (bindings, receipts)
    assert len({binding["correlation_id"] for binding in bindings.values()}) == len(bindings)
    if expected is not None:
        assert bindings == expected, (expected, bindings)
    return bindings


class Check:
    def __init__(self, project: str, compose: str, mail: str):
        self.compose = ["docker", "compose", "--project-name", project, "--file", compose]
        self.mail = pathlib.Path(mail)
        self.now = T0

    def command(self, *args: str) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(self.compose + list(args), capture_output=True, text=True, timeout=240)
        assert result.returncode == 0, result.stdout + result.stderr
        return result

    def control(self, op: str, **fields):
        result = self.command("exec", "-T", "gateway", "python3", "-I", "/fixtures/driver.py",
                              "client", json.dumps({"op": op, **fields}))
        value = json.loads(result.stdout)
        assert value["ok"], value
        return value

    def until(self, description: str, predicate, seconds: int = 180):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(0.1)
        raise TimeoutError(description)

    def ready(self) -> None:
        def available():
            result = subprocess.run(self.compose + ["exec", "-T", "gateway", "python3", "-I",
                "/fixtures/driver.py", "client", '{"op":"ready"}'], capture_output=True, text=True)
            return result.returncode == 0 and json.loads(result.stdout).get("ready")
        self.until("gateway readiness", available, 180)

    def state(self):
        return self.control("status")

    def messages(self) -> list[MailReceipt]:
        return [json.loads(path.read_text()) for path in self.mail.glob("*.json")]

    def sent_deliveries(self, correlations: set[str]) -> list[DeliveryRecord]:
        records = self.control("deliveries")["deliveries"]
        sent = {record["correlation_id"] for record in records if record["status"] == "sent"}
        return records if correlations <= sent else []

    def message(self, chat: str, text: str, proposal=None) -> str:
        fields = {"chat": chat, "text": text}
        if proposal is not None:
            fields["proposal"] = proposal
        answer = self.control("message", **fields)["text"]
        print(json.dumps({"chat": chat, "reply": answer}, ensure_ascii=False), flush=True)
        return answer

    def step(self):
        before = {watch["work_id"]: len(watch["history"]) for watch in self.state()["watches"]}
        self.now += CADENCE
        self.control("clock", now=self.now)
        self.until("both due observations completed", lambda: all(
            len(watch["history"]) > before[watch["work_id"]]
            for watch in self.state()["watches"]))
        return self.state()

    def clean_browser(self) -> None:
        self.until("Chromium and temporary cache cleanup", lambda:
            not self.control("disk")["roots"])

    def run(self) -> None:
        self.ready()
        initial = self.state()
        assert initial["watches"] == [] and self.messages() == []
        # The actual 050 browser obtains the cookie through HTTP, never through profile edits.
        self.control("call", action={"action": "open", "url": PAGE + "/login"})
        self.control("call", action={"action": "close"})
        self.clean_browser()
        assert self.control("disk")["profile_exists"]
        for chat, page, saved in [("rendered-owner", "/rendered", False), ("account-owner", "/account", True)]:
            url = PAGE + page
            proposal = {"url": url, "value_label": "Price:", "condition": "at_most",
                "condition_value": "11000", "cadence_seconds": 600,
                "failure_threshold_seconds": 1200, "notify_email": chat + "@example.test"}
            text = f"Watch {url} for Price: <= 11000; email {chat}@example.test"
            if saved:
                # First attempt remains anonymous and cannot offer an account value.
                first = self.message(chat, text, proposal)
                assert "12900" not in first and "로그인한 상태로 보기" in first
                proposal.update(reading_mode="browser", use_saved_login=True)
                text = "로그인한 상태로 보기"
            before = len(self.state()["watches"])
            summary = self.message(chat, text, proposal)
            assert "브라우저로 읽기 (rendered_text_v1)" in summary and "12900" in summary
            assert ("로그인 사용: 예" in summary) == saved
            if saved:
                assert "browser/profile/" in summary and "HTTP cache" in summary
            assert len(self.state()["watches"]) == before
            self.message(chat, "응")
            assert len(self.state()["watches"]) == before + 1
            self.clean_browser()
        registered = self.state()
        calls = registered["provider_calls"]
        assert sorted(w["plan"]["use_saved_login"] for w in registered["watches"]) == [False, True]
        self.step()  # A, unchanged.
        self.control("page", control={"decoration": "unrelated change"})
        self.step()
        assert self.messages() == []
        self.control("page", control={"price": 10900})
        self.step()
        self.until("two actual SMTP receipts", lambda: len(self.messages()) == 2)
        receipts = self.messages()
        changed = self.state()
        correlations = {notice["correlation_id"] for watch in changed["watches"]
                        for notice in watch["notifications"] if notice["kind"] == "change"}
        assert len(correlations) == 2, changed
        deliveries = self.until("two completed Email delivery records",
                                lambda: self.sent_deliveries(correlations))
        bindings = verify_change_receipts(changed["watches"], deliveries, receipts)
        events = list(bindings)
        for watch in changed["watches"]:
            notices = [notice for notice in watch["notifications"] if notice["kind"] == "change"]
            notice = notices[0]
            body = bindings[notice["event_id"]]["body"]
            assert "10900" in body and "rendered_text_v1" in body
            assert ("no saved login profile" not in body) == watch["plan"]["use_saved_login"]
            for marker in ["unique-whole-page-marker", "spec051_login", "disposable-fixture"]:
                assert marker not in body and marker not in json.dumps(watch), marker
        assert len(set(events)) == 2
        assert self.state()["provider_calls"] == calls
        # Another worker pass and restart at the same clock cannot deliver those events again.
        self.control("clock", now=self.now)
        self.command("restart", "gateway")
        self.ready()
        self.step()
        verify_change_receipts(changed["watches"], self.control("deliveries")["deliveries"],
                               self.messages(), expected=bindings)
        assert all(n["event_id"] in events for w in self.state()["watches"]
                   for n in w["notifications"] if n["kind"] == "change")
        print(json.dumps({"events": events, "event_receipts": bindings, "smtp_receipts": receipts,
                          "provider_calls_before_restart": calls, "watches": self.state()["watches"]}))
        # Conversation ownership causes busy and leaves the next regular wake.
        self.control("call", action={"action": "open", "url": PAGE + "/login"})
        busy = self.step()
        for watch in busy["watches"]:
            assert watch["history"][-1]["result"]["failure"] == "browser_busy"
            assert watch["next_wake"]["due_at_ms"] > self.now
        self.control("call", action={"action": "close"})
        self.clean_browser()
        self.step()
        # Hold the cookie-gated network request while another ingress requests takeover.
        self.control("page", control={"pause": True})
        self.now += CADENCE
        self.control("clock", now=self.now)
        self.until("login watch waiting on fixture", lambda: self.control("page")["page"]["waiting"] == 1)
        disk = self.control("disk")
        chromium = [p for p in disk["processes"] if p["name"] == "chromium"]
        assert len(chromium) == 1
        argv = chromium[0]["argv"]
        assert "--user-data-dir=/check-data/browser/profile" in argv
        cache = next(arg.split("=", 1)[1] for arg in argv if arg.startswith("--disk-cache-dir="))
        assert not cache.startswith("/check-data/browser/profile/")
        started = time.monotonic()
        takeover = self.message("takeover-owner", "/browser takeover")
        assert "busy" in takeover or "쓰는 중" in takeover, takeover
        assert time.monotonic() - started < 5, "takeover waited for the watch"
        # Exec-only second container shares the data volume; locks are never edited on host.
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
            removal = executor.submit(self.command, "run", "--rm", "--no-deps", "--entrypoint",
                "shacs-bot", "gateway", "--config", "/check-data/config.json", "browser", "forget", "--yes")
            result = removal.result(timeout=20)
        assert "지웠습니다" in result.stdout, result.stdout
        self.control("page", control={"release": True})
        self.clean_browser()
        assert not self.control("disk")["profile_exists"]
        self.until("cancelled login watch has next wake", lambda: all(
            "due_at_ms" in watch["next_wake"] for watch in self.state()["watches"]))
        after = self.step()
        saved = next(w for w in after["watches"] if w["plan"]["use_saved_login"])
        assert saved["history"][-1]["result"]["failure"] == "rendered_content_unavailable"
        # Neither raw page text nor fixture credentials can enter public diagnostics.
        diagnostic = self.command("exec", "-T", "gateway", "shacs-bot", "--config",
                                  "/check-data/config.json", "status").stdout
        for marker in ["unique-whole-page-marker", "spec051_login", "disposable-fixture"]:
            assert marker not in diagnostic
        print(json.dumps({"cache_removed": cache, "forget": result.stdout,
                          "profile_removed": True, "final_status": after}, ensure_ascii=False))


if __name__ == "__main__":
    Check(*sys.argv[1:]).run()
