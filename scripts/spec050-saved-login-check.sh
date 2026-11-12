#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "--approved" || $# != 1 ]]; then
  printf '%s\n' 'Ask the user before running Docker. Usage: bash scripts/spec050-saved-login-check.sh --approved' >&2
  exit 2
fi
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/shacs-saved-login-check.XXXXXXXX")"
project="shacs-saved-login-check-$(date +%s)-$$"
compose=(docker compose --project-name "$project" --file "$temporary/compose.json")
cleanup() {
  status=$?
  trap - EXIT INT TERM
  "${compose[@]}" down --volumes --remove-orphans --rmi local || status=1
  rm -rf -- "$temporary"
  exit "$status"
}
trap cleanup EXIT INT TERM
mkdir -p "$temporary/data/workspace"
python3 - "$repo" "$temporary" "$(id -u)" "$(id -g)" <<'PY'
import json, pathlib, sys
repo, temporary, uid, gid = sys.argv[1:]
root = pathlib.Path(temporary)
config = {
    "agents": {"defaults": {"provider": "openai", "model": "gpt-4o",
                           "workspace": "/check-data/workspace", "unifiedSession": False}},
    "providers": {"openai": {"apiBase": "http://127.0.0.1:9001/v1", "apiKey": "disposable-fixture"}},
    "permissions": {"mode": "bypass_permissions"},
    "tools": {"exec": {"enable": True}, "browser": {"enable": True, "persistLogin": True,
               "screenshots": False}, "ssrfWhitelist": ["172.30.250.2/32"],
               "toolSearch": {"enabled": "off"}},
}
(root / "data/config.json").write_text(json.dumps(config))
def bot(role, address):
    return {
        "build": {"context": repo, "dockerfile": "Dockerfile"},
        "user": f"{uid}:{gid}", "shm_size": "256mb", "mem_limit": "2g",
        "entrypoint": ["python3", "-I", "/fixtures/driver.py", "serve", role],
        "volumes": [f"{root}/data:/check-data",
                    f"{repo}/scripts/spec050-saved-login-fixtures:/fixtures:ro",
                    f"{repo}/scripts/spec050-takeover-fixtures:/takeover-fixtures:ro"],
        "environment": {"HOME": "/check-data"},
        "networks": {"check": {"ipv4_address": address}},
    }
compose = {
    "services": {
        "pages": {
            "image": "python:3.14-slim-bookworm",
            "command": ["python3", "-I", "/fixtures/pages.py"],
            "volumes": [f"{repo}/scripts/spec050-saved-login-fixtures:/fixtures:ro"],
            "networks": {"check": {"ipv4_address": "172.30.250.2"}},
            "read_only": True,
        },
        "gateway": bot("gateway", "172.30.250.3"),
        "api": bot("api", "172.30.250.4"),
    },
    "networks": {"check": {"ipam": {"config": [{"subnet": "172.30.250.0/24"}]}}},
}
(root / "compose.json").write_text(json.dumps(compose))
PY
"${compose[@]}" build gateway api
"${compose[@]}" up -d pages gateway
# The api service shares the data volume. It starts later after the gateway
# stops, and also supplies the other hostname for exec-only forget checks.
python3 "$repo/scripts/spec050-saved-login-fixtures/check.py" "$project" "$temporary/compose.json" "$temporary/data"
printf '%s\n' 'Spec 050 saved-login checks passed; removing only this disposable Compose project.'
