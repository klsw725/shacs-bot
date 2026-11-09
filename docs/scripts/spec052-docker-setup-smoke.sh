#!/usr/bin/env bash
# Opt-in real Docker check of scripts/docker-setup.sh (Spec 052 PRD 020
# acceptance 6). It copies the current commit into a temporary git worktree,
# runs the setup script there without a terminal for a non-1000 user with an
# empty temporary data folder, checks the result, and removes what it made.
# It rebuilds the image shacs-bot:local, so it runs only with --yes.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)

fail() {
  printf 'Spec052 Docker setup smoke failed: %s\n' "$1" >&2
  exit 1
}

if [ "${1:-}" != --yes ]; then
  printf '%s\n' \
    'This rebuilds the image shacs-bot:local from a copy of the current commit and' \
    'starts a temporary shacs-gateway container with an empty temporary data folder.' \
    'Your ~/.shacs-bot and the repository .env are not used. Run again with --yes.'
  exit 1
fi
[ "$(id -u)" != 0 ] || fail 'run it as a non-root user'
[ "$(id -u)" != 1000 ] || fail 'run it as a user whose UID is not 1000'

# The checks below read the .env the setup script writes.
unset SHACS_UID SHACS_GID
export DOCKER_CONFIG="${DOCKER_CONFIG:-$HOME/.docker}"
tmp_root=''
worktree_added=''

compose() {
  (cd "$worktree" && HOME="$temp_home" docker compose "$@")
}

cleanup() {
  if [ -n "$worktree_added" ]; then
    compose down --remove-orphans >/dev/null 2>&1 || true
    git -C "$repo_root" worktree remove --force "$worktree" >/dev/null 2>&1 || true
  fi
  if [ -n "$tmp_root" ]; then
    rm -rf "$tmp_root"
  fi
}
trap cleanup EXIT

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/shacs-spec052-smoke.XXXXXX")
worktree="$tmp_root/repo"
temp_home="$tmp_root/home"
mkdir "$temp_home"

names=$(HOME="$temp_home" docker ps -a --format '{{.Names}}') || fail 'docker ps failed'
if grep -qx shacs-gateway <<<"$names"; then
  fail 'a shacs-gateway container already exists; stop and remove it first'
fi
worktree_added=1
git -C "$repo_root" worktree add --detach "$worktree" HEAD >/dev/null

(cd "$worktree" && HOME="$temp_home" ./scripts/docker-setup.sh </dev/null) ||
  fail 'scripts/docker-setup.sh failed'

running=$(compose ps --status running --services) || fail 'docker compose ps failed'
grep -qx shacs-gateway <<<"$running" || fail 'shacs-gateway is not running'
foreign=$(find "$temp_home/.shacs-bot" ! -user "$(id -u)" -print) || fail 'could not list the data folder'
[ -z "$foreign" ] || fail "data files not owned by you: $(head -n 5 <<<"$foreign")"
home_check=$(compose run --rm -T --entrypoint /bin/sh shacs-cli \
  -c 'mkdir -p "$HOME/.cache/spec052" && echo ok') ||
  fail 'the container could not write to its HOME'
[ "$home_check" = ok ] || fail "the container HOME check printed: $home_check"
inspect=$(compose run --rm -T shacs-cli runtime inspect) || fail 'runtime inspect failed'
grep -q 'official-container' <<<"$inspect" ||
  fail 'runtime inspect shows no official-container evidence'

deadline=$((SECONDS + 60))
until logs=$(compose logs shacs-gateway 2>&1) && grep -q 'provider: missing' <<<"$logs"; do
  [ "$SECONDS" -lt "$deadline" ] || fail 'the gateway start output has no provider line'
  sleep 2
done

printf 'Spec052 Docker setup smoke passed\n'
