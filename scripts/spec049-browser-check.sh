#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "--approved" || $# != 1 ]]; then
  printf '%s\n' 'Ask the user before running Docker. Usage: bash scripts/spec049-browser-check.sh --approved' >&2
  exit 2
fi
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/shacs-browser-check.XXXXXXXX")"
project="shacs-browser-check-$(date +%s)-$$"
compose=(docker compose --project-name "$project" --file "$temporary/compose.json")
cleanup() {
  status=$?
  trap - EXIT INT TERM
  "${compose[@]}" down --volumes --remove-orphans || status=1
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
    "tools": {"exec": {"enable": False}, "browser": {"enable": True, "screenshots": True},
              "ssrfWhitelist": ["172.30.249.2/32"], "toolSearch": {"enabled": "off"}},
}
(root / "data/config.json").write_text(json.dumps(config))
compose = {
    "services": {
        "pages": {
            "image": "python:3.14-slim-bookworm",
            "command": ["python3", "-m", "http.server", "8000", "--directory", "/pages"],
            "volumes": [f"{repo}/scripts/spec049-browser-fixtures:/pages:ro"],
            "networks": {"check": {"ipv4_address": "172.30.249.2"}},
            "read_only": True,
        },
        "bot": {
            "build": {"context": repo, "dockerfile": "Dockerfile"},
            "user": f"{uid}:{gid}", "shm_size": "256mb", "mem_limit": "2g",
            "entrypoint": ["python3", "/fixtures/check.py"],
            "volumes": [f"{root}/data:/check-data",
                        f"{repo}/scripts/spec049-browser-fixtures:/fixtures:ro"],
            "environment": {"HOME": "/check-data"},
            "networks": {"check": {"ipv4_address": "172.30.249.3"}},
        },
    },
    "networks": {"check": {"internal": True, "ipam": {"config": [{"subnet": "172.30.249.0/24"}]}}},
}
(root / "compose.json").write_text(json.dumps(compose))
PY
"${compose[@]}" build bot
"${compose[@]}" up -d pages
"${compose[@]}" run --rm bot
printf '%s\n' 'Spec 049 browser checks passed; removing only this disposable Compose project.'
