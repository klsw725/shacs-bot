"""Local SMTP receipt sink, with no relay or external credentials."""
import email
import email.policy
import json
import os
import pathlib
import socketserver
import uuid


def envelope_address(value: str) -> str:
    """Keep the SMTP envelope mailbox, separate from command and ESMTP options."""
    _, separator, path = value.partition(":")
    address, closing, _ = path[1:].partition(">")
    assert separator and path.startswith("<") and closing, value
    return address


class Smtp(socketserver.StreamRequestHandler):
    def reply(self, line: bytes) -> None:
        self.wfile.write(line + b"\r\n")
        self.wfile.flush()

    def handle(self) -> None:
        self.reply(b"220 spec051 local sink")
        sender = ""
        recipients: list[str] = []
        while line := self.rfile.readline(65536):
            verb, _, value = line.decode().strip().partition(" ")
            match verb.upper():
                case "EHLO" | "HELO":
                    self.reply(b"250-spec051")
                    self.reply(b"250 8BITMIME")
                case "MAIL":
                    sender = envelope_address(value)
                    recipients = []
                    self.reply(b"250 sender accepted")
                case "RCPT":
                    recipients.append(envelope_address(value))
                    self.reply(b"250 recipient accepted")
                case "DATA":
                    self.reply(b"354 end with dot")
                    body = bytearray()
                    while data := self.rfile.readline(65536):
                        if data == b".\r\n":
                            break
                        body.extend(data[1:] if data.startswith(b"..") else data)
                    message = email.message_from_bytes(bytes(body), policy=email.policy.default)
                    payload = {"envelope_from": sender, "envelope_to": recipients,
                               "message_id": message.get("Message-ID"),
                               "subject": message.get("Subject"), "body": message.get_content()}
                    destination = pathlib.Path("/mail") / f"{uuid.uuid4()}.json"
                    temporary = destination.with_name(f".tmp-{destination.stem}.part")
                    with temporary.open("x", encoding="utf-8") as record:
                        json.dump(payload, record)
                        record.flush()
                        os.fsync(record.fileno())
                    os.replace(temporary, destination)
                    self.reply(b"250 stored")
                case "RSET":
                    recipients = []
                    self.reply(b"250 reset")
                case "NOOP":
                    self.reply(b"250 ok")
                case "QUIT":
                    self.reply(b"221 bye")
                    return
                case _:
                    self.reply(b"502 unsupported")


if __name__ == "__main__":
    with socketserver.ThreadingTCPServer(("0.0.0.0", 2525), Smtp) as server:
        server.serve_forever()
