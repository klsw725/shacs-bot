#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/shacs-spec023-compose-smoke.XXXXXX")
cleanup() {
  rm -rf "$tmp_root"
}
trap cleanup EXIT

host_data_dir="$tmp_root/shacs-data"
compose_file="$tmp_root/docker-compose.spec023-smoke.yml"
mkdir -p "$host_data_dir/workspace"

fail() {
  printf 'Spec023 Compose smoke failed: %s\n' "$1" >&2
  exit 1
}

compose_config=$(docker compose -f "$repo_root/docker-compose.yml" config)
if printf '%s\n' "$compose_config" | grep -Eq '(^|[/:])docker\.sock([^[:alnum:]_.-]|$)'; then
  fail 'default docker-compose.yml mounts a Docker socket'
fi
if printf '%s\n' "$compose_config" | grep -Eq '^[[:space:]]*privileged:[[:space:]]*true[[:space:]]*$'; then
  fail 'default docker-compose.yml enables privileged mode'
fi
if printf '%s\n' "$compose_config" | grep -Eq '^[[:space:]]*network_mode:[[:space:]]*["'"'"']?host["'"'"']?[[:space:]]*$'; then
  fail 'default docker-compose.yml uses host network mode'
fi

cat > "$compose_file" <<YAML
services:
  shacs-cli:
    build:
      context: "$repo_root"
      dockerfile: Dockerfile
    image: shacs-bot:spec023-smoke
    user: "${SHACS_UID:-$(id -u)}:${SHACS_GID:-$(id -g)}"
    volumes:
      - "$host_data_dir:/home/shacs/.shacs-bot"
    environment:
      HOME: /home/shacs
      RUST_LOG: info
    command: ["status"]
YAML

docker compose -f "$compose_file" build shacs-cli

inspect_output=$(docker compose -f "$compose_file" run --rm shacs-cli runtime inspect --workspace /home/shacs/.shacs-bot/workspace)
printf '%s\n' "$inspect_output"

if ! printf '%s\n' "$inspect_output" | grep -Fq 'Runtime containment: contained=true'; then
  fail 'runtime inspect did not report contained=true inside the Compose service'
fi
if ! printf '%s\n' "$inspect_output" | grep -Fq 'backend=official-container'; then
  fail 'runtime inspect did not report official-container backend evidence'
fi

container_evidence=$(docker compose -f "$compose_file" run --rm --entrypoint /bin/sh shacs-cli -lc 'test "$SHACS_RUNTIME_PACKAGE" = "shacs-bot-official-container" && { test -e /.dockerenv || grep -Eiq "docker|containerd|kubepods|podman|lxc" /proc/1/cgroup /proc/self/cgroup; } && test ! -e /var/run/docker.sock && shacs-bot --help >/dev/null')
printf '%s\n' "$container_evidence" >/dev/null

if [ -e "$HOME/.shacs-bot/spec023-compose-smoke-should-not-exist" ]; then
  fail 'smoke touched the user home data directory sentinel path'
fi

printf 'Spec023 Compose smoke passed with temp host data dir: %s\n' "$host_data_dir"
