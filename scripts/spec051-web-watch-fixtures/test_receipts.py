"""Local fixture regression checks; no Docker, Rust, sockets or external services."""
import copy
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import tempfile
from types import ModuleType, SimpleNamespace
from unittest.mock import patch


def load_fixture(name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        f"spec051_{name}", pathlib.Path(__file__).with_name(f"{name}.py"))
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_receipt_publication_is_atomic() -> None:
    smtp = load_fixture("smtp")
    check = load_fixture("check")
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        checker = check.Check("receipt-test", str(root / "compose.json"), directory)
        handler = smtp.Smtp.__new__(smtp.Smtp)
        handler.rfile = io.BytesIO(
            b"MAIL FROM:<watch@example.test> BODY=8BITMIME\r\n"
            b"RCPT TO:<user@example.test>\r\nDATA\r\n"
            b"Message-ID: <actual-smtp-id@example.test>\r\n"
            b"Content-Type: text/plain; charset=utf-8\r\n\r\n"
            b"Full body\r\nTracking: event-correlation\r\n.\r\nQUIT\r\n")
        handler.wfile = io.BytesIO()
        original_replace = os.replace
        original_fsync = os.fsync

        def publish(source: pathlib.Path, destination: pathlib.Path) -> None:
            assert source.parent == destination.parent == root
            assert source.name.startswith(".tmp-") and source.suffix == ".part"
            assert destination.suffix == ".json"
            synced.assert_called_once()
            assert checker.messages() == [], "an unfinished receipt was visible"
            original_replace(source, destination)
            assert len(checker.messages()) == 1

        with patch.object(smtp, "pathlib", SimpleNamespace(Path=lambda _: root)), \
                patch.object(smtp.os, "fsync", wraps=original_fsync) as synced, \
                patch.object(smtp.os, "replace", side_effect=publish) as replaced:
            handler.handle()
        replaced.assert_called_once()
        receipts = checker.messages()
        assert receipts[0]["envelope_to"] == ["user@example.test"]
        assert receipts[0]["envelope_from"] == "watch@example.test"
        assert receipts[0]["message_id"] == "<actual-smtp-id@example.test>"
        assert not list(root.glob("*.part"))
        assert b"250 stored\r\n" in handler.wfile.getvalue()


def test_receipts_are_bound_to_event_and_full_body_across_restart() -> None:
    check = load_fixture("check")
    target = "user@example.test"
    correlation = "confirmed-event-correlation"
    event_id = "change-revision-2-time-1800000600000"
    body = ("Web page change notification\nCondition revision 2\n"
            "New: 10900 read at 1800000600000 ms\n"
            f"Tracking: {correlation}")
    content_hash = hashlib.sha256(body.encode()).hexdigest()
    delivery_id = hashlib.sha256(f"email\0{target}\0{body}".encode()).hexdigest()
    watches = [{"notifications": [{"kind": "change", "event_id": event_id,
                                  "correlation_id": correlation, "target": f"email:{target}"}]}]
    deliveries = [{"id": delivery_id, "channel": "email", "chat_id": target,
                   "correlation_id": correlation, "content_sha256": content_hash, "status": status}
                  for status in ["pending", "sent"]]
    receipts = [{"envelope_from": "watch@example.test", "envelope_to": [target],
                 "message_id": "<actual-smtp-id@example.test>", "subject": "shacs-bot",
                 "body": body.replace("\n", "\r\n") + "\r\n"}]
    expected = check.verify_change_receipts(watches, deliveries, receipts)
    assert expected[event_id]["body"] == body
    assert expected[event_id]["received_count"] == 1
    check.verify_change_receipts(watches, deliveries, copy.deepcopy(receipts), expected=expected)
    for fault in ["recipient", "extra_recipient", "revision", "observation_time",
                  "tracking", "duplicate", "message_id", "second_delivery"]:
        changed = copy.deepcopy(receipts)
        records = copy.deepcopy(deliveries)
        match fault:
            case "recipient":
                changed[0]["envelope_to"] = [target + ".other"]
            case "extra_recipient":
                changed[0]["envelope_to"].append("other@example.test")
            case "revision":
                changed[0]["body"] = changed[0]["body"].replace("revision 2", "revision 3")
            case "observation_time":
                changed[0]["body"] = changed[0]["body"].replace("1800000600000", "1800001200000")
            case "tracking":
                changed[0]["body"] = changed[0]["body"].replace(correlation, "another-event")
            case "duplicate":
                changed.append(copy.deepcopy(changed[0]))
            case "message_id":
                changed[0]["message_id"] = "<different-smtp-id@example.test>"
            case "second_delivery":
                records.append(copy.deepcopy(records[0]))
        try:
            check.verify_change_receipts(
                watches, records, changed,
                expected=expected if fault == "message_id" else None)
        except AssertionError:
            continue
        raise AssertionError(f"accepted receipt fault: {fault}")


if __name__ == "__main__":
    test_receipt_publication_is_atomic()
    test_receipts_are_bound_to_event_and_full_body_across_restart()
    print(json.dumps({"fixture_regressions": 2, "status": "passed"}))
