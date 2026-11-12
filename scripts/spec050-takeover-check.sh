#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "--approved" || $# != 1 ]]; then
  printf '%s\n' 'Ask the user before running Docker. Usage: bash scripts/spec050-takeover-check.sh --approved' >&2
  exit 2
fi
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/shacs-takeover-check.XXXXXXXX")"
project="shacs-takeover-check-$(date +%s)-$$"
compose=(docker compose --project-name "$project" --file "$temporary/compose.json")
cleanup() {
  status=$?
  trap - EXIT INT TERM
  # --rmi local removes only the images this project tagged; shared layers stay cached.
  "${compose[@]}" down --volumes --remove-orphans --rmi local || status=1
  rm -rf -- "$temporary"
  exit "$status"
}
trap cleanup EXIT INT TERM
mkdir -p "$temporary/gateway/workspace" "$temporary/api/workspace"
# The viewer password lives only in the temporary .env.json files (0600) and
# is never printed by this script or its Python helpers.
python3 - "$repo" "$temporary" "$(id -u)" "$(id -g)" <<'PY'
import json, os, pathlib, secrets, sys
repo, temporary, uid, gid = sys.argv[1:]
root = pathlib.Path(temporary)
password = secrets.token_urlsafe(12)
for name in ["gateway", "api"]:
    data = root / name
    config = {
        "agents": {"defaults": {"provider": "openai", "model": "gpt-4o",
                                "workspace": "/check-data/workspace", "unifiedSession": False}},
        "providers": {"openai": {"apiBase": "http://127.0.0.1:9001/v1", "apiKey": "disposable-fixture"}},
        "permissions": {"mode": "bypass_permissions"},
        "tools": {"exec": {"enable": False},
                  "browser": {"enable": True, "screenshots": False,
                              "view": {"enable": True, "password": "${SHACS_BROWSER_VIEW_PASSWORD}"}},
                  "ssrfWhitelist": ["172.30.250.2/32"], "toolSearch": {"enabled": "off"}},
    }
    (data / "config.json").write_text(json.dumps(config))
    env_file = data / ".env.json"
    env_file.write_text(json.dumps({"SHACS_BROWSER_VIEW_PASSWORD": password}))
    os.chmod(env_file, 0o600)
def bot(name, host_port):
    return {
        "build": {"context": repo, "dockerfile": "Dockerfile"},
        "user": f"{uid}:{gid}", "shm_size": "256mb", "mem_limit": "2g",
        # gateway runs the production `shacs-bot run` command, api runs `shacs-bot serve`.
        "entrypoint": ["python3", "-I", "/fixtures/takeover_check.py", "serve", name],
        "volumes": [f"{root}/{name}:/check-data", f"{repo}/scripts/spec050-takeover-fixtures:/fixtures:ro"],
        "environment": {"HOME": "/check-data"},
        "ports": [f"127.0.0.1:{host_port}:6080"],
        "networks": {"check": {"ipv4_address": "172.30.250.3" if name == "gateway" else "172.30.250.4"}},
    }
compose = {
    "services": {
        "pages": {
            "image": "python:3.14-slim-bookworm",
            "command": ["python3", "-m", "http.server", "8000", "--directory", "/pages"],
            "volumes": [f"{repo}/scripts/spec050-takeover-fixtures:/pages:ro"],
            "networks": {"check": {"ipv4_address": "172.30.250.2"}},
            "read_only": True,
        },
        "gateway": bot("gateway", 6080),
        "api": bot("api", 6081),
    },
    # Not `internal`: host port publishing needs a routable bridge. Only the
    # two viewer ports are published, both on 127.0.0.1.
    "networks": {"check": {"ipam": {"config": [{"subnet": "172.30.250.0/24"}]}}},
}
(root / "compose.json").write_text(json.dumps(compose))
PY
"${compose[@]}" build gateway
"${compose[@]}" up -d pages gateway api
python3 "$repo/scripts/spec050-takeover-fixtures/host_check.py" "$project" "$temporary/compose.json" "$temporary/gateway/.env.json"
printf '%s\n' 'Spec 050 takeover checks passed; removing only this disposable Compose project.'
