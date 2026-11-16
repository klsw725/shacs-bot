#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "--approved" || $# != 1 ]]; then
  printf '%s\n' 'Ask the user before running Docker. Usage: bash scripts/spec051-web-watch-check.sh --approved' >&2
  exit 2
fi
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/shacs-web-watch-check.XXXXXXXX")"
project="shacs-web-watch-check-$(date +%s)-$$"
compose=(docker compose --project-name "$project" --file "$temporary/compose.json")
cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [[ $status != 0 ]]; then
    "${compose[@]}" logs --no-color >&2 || true
    "${compose[@]}" exec -T gateway python3 -I /fixtures/driver.py client '{"op":"log_tail"}' >&2 || true
  fi
  "${compose[@]}" down --volumes --remove-orphans --rmi local || status=1
  rm -rf -- "$temporary"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$temporary/data/workspace" "$temporary/mail"
python3 "$repo/scripts/spec051-web-watch-fixtures/compose.py" "$repo" "$temporary" "$(id -u)" "$(id -g)"
"${compose[@]}" build gateway
"${compose[@]}" up -d pages smtp gateway
python3 "$repo/scripts/spec051-web-watch-fixtures/check.py" "$project" "$temporary/compose.json" "$temporary/mail"
printf '%s\n' 'Spec 051 rendered and saved-login watches passed; removing the disposable project.'
