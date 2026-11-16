"""Generate an isolated image from the official Dockerfile without changing it."""
import json
import pathlib
import sys

repo_arg, temporary, uid, gid = sys.argv[1:]
repo = pathlib.Path(repo_arg)
root = pathlib.Path(temporary)
build = "cargo build --manifest-path crates/Cargo.toml -p shacs-cli --release --locked"
dockerfile = (repo / "Dockerfile").read_text()
assert dockerfile.count(build) == 1, "official image build command changed"
(root / "Dockerfile").write_text(dockerfile.replace(
    build, "cargo clean --manifest-path crates/Cargo.toml && " + build + " --features spec051-test-profile"))
config = {
    "agents": {"defaults": {"provider": "openai", "model": "gpt-4o",
                           "workspace": "/check-data/workspace", "unifiedSession": False}},
    "providers": {"openai": {"apiBase": "http://127.0.0.1:9001/v1", "apiKey": "disposable-fixture"}},
    "permissions": {"mode": "bypass_permissions"},
    "tools": {"exec": {"enable": True}, "browser": {"enable": True, "persistLogin": True,
               "screenshots": False}, "ssrfWhitelist": ["172.30.250.2/32"],
               "toolSearch": {"enabled": "off"}},
    "channels": {"email": {"enabled": True, "consentGranted": True,
        "smtp": {"host": "172.30.250.5", "port": 2525, "security": "plain",
                 "from": "watch@example.test"}}},
}
(root / "data/config.json").write_text(json.dumps(config))
(root / "data/clock").write_text("1800000000000")
fixtures = f"{repo}/scripts/spec051-web-watch-fixtures:/fixtures:ro"
gateway = {
    "build": {"context": str(repo), "dockerfile": str(root / "Dockerfile")},
    "user": f"{uid}:{gid}", "shm_size": "256mb", "mem_limit": "2g",
    "entrypoint": ["python3", "-I", "/fixtures/driver.py", "serve", "gateway"],
    "volumes": [f"{root}/data:/check-data", fixtures,
        f"{repo}/scripts/spec050-takeover-fixtures:/takeover-fixtures:ro",
        f"{repo}/scripts/spec050-saved-login-fixtures:/saved-fixtures:ro"],
    "environment": {"HOME": "/check-data", "SHACS_SPEC051_ALLOW_TEST_PAGE": "1",
                    "SHACS_SPEC042_CLOCK_FILE": "/check-data/clock"},
    "networks": {"check": {"ipv4_address": "172.30.250.3"}},
}
compose = {
    "services": {
        "gateway": gateway,
        "pages": {"image": "python:3.14-slim-bookworm",
                  "command": ["python3", "-I", "/fixtures/pages.py"],
                  "volumes": [fixtures], "read_only": True,
                  "networks": {"check": {"ipv4_address": "172.30.250.2"}}},
        "smtp": {"image": "python:3.14-slim-bookworm",
                 "command": ["python3", "-I", "/fixtures/smtp.py"],
                 "volumes": [fixtures, f"{root}/mail:/mail"],
                 "networks": {"check": {"ipv4_address": "172.30.250.5"}}},
    },
    "networks": {"check": {"ipam": {"config": [{"subnet": "172.30.250.0/24"}]}}},
}
(root / "compose.json").write_text(json.dumps(compose))
print(json.dumps({"disposable_data": str(root / "data"), "feature": "spec051-test-profile"}))
