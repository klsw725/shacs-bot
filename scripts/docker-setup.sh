#!/bin/sh
# First run of shacs-bot with Docker Compose (Spec 052 PRD 020): runs the
# container as your user, builds the image, runs the first setup, offers a
# ChatGPT sign-in and a Discord connection, and starts the gateway.
# Run it again at any time; steps that are already done are only checked.

set -u

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
root=$(CDPATH='' cd -- "$script_dir/.." && pwd) || exit 1
cd "$root" || exit 1

env_file="$root/.env"
env_tmp=''
echo_off=''

cleanup() {
  if [ -n "$env_tmp" ]; then
    rm -f "$env_tmp"
    env_tmp=''
  fi
  if [ -n "$echo_off" ]; then
    stty echo 2>/dev/null
    echo_off=''
    printf '\n' >&2
  fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'docker-setup: %s\n' "$*" >&2
  exit 1
}

# Prints the question and reads one line into `answer`; false at end of input.
ask() {
  printf '%s' "$1"
  if IFS= read -r answer; then
    return 0
  fi
  answer=''
  printf '\n'
  return 1
}

# Like `ask`, with the terminal's echo off before the question shows; reads
# into `secret`. False with `secret` empty when echo cannot be turned off or
# back on, or at end of input. While echo may still be off, `echo_off` stays
# set so the cleanup tries again.
ask_hidden() {
  secret=''
  echo_off=1
  if ! stty -echo; then
    echo_off=''
    say 'Could not hide the input on this terminal.'
    return 1
  fi
  printf '%s' "$1"
  hidden_status=0
  IFS= read -r secret || hidden_status=1
  if stty echo; then
    echo_off=''
  else
    hidden_status=1
  fi
  printf '\n'
  [ "$hidden_status" -eq 0 ] || secret=''
  return "$hidden_status"
}

is_yes() {
  case "$1" in
    y | Y | yes | Yes | YES) return 0 ;;
  esac
  return 1
}

cli() {
  docker compose run --rm -T shacs-cli "$@"
}

# 1. Your user. A container started with user 0 would run as root.
uid=$(id -u) || fail 'could not read your user id'
gid=$(id -g) || fail 'could not read your group id'
if [ "$uid" = 0 ]; then
  fail 'do not run this as root: the container would then run as root. Run it as your own user.'
fi
if { [ "${SHACS_UID+set}" = set ] && [ "$SHACS_UID" != "$uid" ]; } ||
  { [ "${SHACS_GID+set}" = set ] && [ "$SHACS_GID" != "$gid" ]; }; then
  say "Your shell exports SHACS_UID or SHACS_GID with other values, and Docker Compose uses them before .env."
  say "This run uses $uid:$gid. Remove them from your shell: unset SHACS_UID SHACS_GID"
fi
export SHACS_UID="$uid" SHACS_GID="$gid"

# 2. Docker and Compose v2.
command -v docker >/dev/null 2>&1 || fail 'docker is not installed (see https://docs.docker.com/get-docker/)'
docker compose version >/dev/null 2>&1 || fail 'Docker Compose v2 (the `docker compose` command) is required'

# 3. The data folder Compose mounts as ~/.shacs-bot.
data="${HOME:?HOME is not set}/.shacs-bot"
if [ -e "$data" ] || [ -L "$data" ]; then
  if [ ! -d "$data" ]; then
    fail "$data exists but is not a folder; move it away and run this again"
  fi
  if [ ! -w "$data" ]; then
    owner=$(ls -ldn "$data" | awk '{ print $3 ":" $4 }')
    fail "$data belongs to $owner and you ($uid:$gid) cannot write to it. Fix it with: sudo chown -R \"\$(id -u):\$(id -g)\" ~/.shacs-bot"
  fi
else
  mkdir -p "$data" || fail "could not create $data"
  say "Created $data"
fi

# 4. The user and group the containers run as, in the .env Compose reads.
env_needs_update() {
  uid_lines=0
  gid_lines=0
  uid_value=''
  gid_value=''
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      SHACS_UID=*)
        uid_lines=$((uid_lines + 1))
        [ "$uid_lines" -eq 1 ] && uid_value=${line#SHACS_UID=}
        ;;
      SHACS_GID=*)
        gid_lines=$((gid_lines + 1))
        [ "$gid_lines" -eq 1 ] && gid_value=${line#SHACS_GID=}
        ;;
    esac
  done <"$env_file"
  if [ "$uid_lines" -eq 1 ] && [ "$gid_lines" -eq 1 ] &&
    [ "$uid_value" = "$uid" ] && [ "$gid_value" = "$gid" ]; then
    return 1
  fi
  return 0
}

write_env_lines() {
  seen_uid=''
  seen_gid=''
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      SHACS_UID=*)
        if [ -z "$seen_uid" ]; then
          seen_uid=1
          printf 'SHACS_UID=%s\n' "$uid"
        fi
        ;;
      SHACS_GID=*)
        if [ -z "$seen_gid" ]; then
          seen_gid=1
          printf 'SHACS_GID=%s\n' "$gid"
        fi
        ;;
      *) printf '%s\n' "$line" ;;
    esac
  done <"$env_file"
  [ -n "$seen_uid" ] || printf 'SHACS_UID=%s\n' "$uid"
  [ -n "$seen_gid" ] || printf 'SHACS_GID=%s\n' "$gid"
}

if [ ! -e "$env_file" ]; then
  (umask 077 && printf 'SHACS_UID=%s\nSHACS_GID=%s\n' "$uid" "$gid" >"$env_file") ||
    fail "could not write $env_file"
  say "Wrote SHACS_UID=$uid and SHACS_GID=$gid to .env"
elif env_needs_update; then
  if [ "$uid_lines" -gt 0 ] && [ "$uid_value" != "$uid" ]; then
    say "Changing SHACS_UID in .env from $uid_value to $uid"
  fi
  if [ "$gid_lines" -gt 0 ] && [ "$gid_value" != "$gid" ]; then
    say "Changing SHACS_GID in .env from $gid_value to $gid"
  fi
  env_tmp=$(mktemp "$root/.env.docker-setup.XXXXXX") || fail 'could not create a temporary file next to .env'
  cp -p "$env_file" "$env_tmp" || fail "could not copy $env_file"
  write_env_lines >"$env_tmp" || fail "could not write $env_tmp"
  mv "$env_tmp" "$env_file" || fail "could not replace $env_file"
  env_tmp=''
  say "Set SHACS_UID=$uid and SHACS_GID=$gid in .env"
fi

# 5. The image and the first setup.
say 'Building the image (the first build takes a few minutes)...'
docker compose build || fail 'docker compose build failed'
say 'Running the first setup...'
cli onboard || fail 'the first setup (onboard) failed; see the message above'

interactive=''
[ -t 0 ] && interactive=1
login_ok=''
connect_ok=''
login_todo=''
discord_todo=''

# 6. The model provider.
check_provider() {
  provider_out=$(cli provider status) || {
    printf '%s\n' "$provider_out"
    fail 'provider status failed'
  }
}

check_provider
case "$provider_out" in
  'provider: ready'*) ;;
  *)
    printf '%s\n' "$provider_out"
    login_todo=1
    if [ -n "$interactive" ]; then
      say ''
      say 'Sign in to a model provider now?'
      say '  1) Sign in with a ChatGPT account (Codex): open a link and enter a code in your browser'
      say '  2) Skip for now'
      ask 'Choose 1 or 2 [2]: ' || true
      if [ "$answer" = 1 ]; then
        if cli provider codex login --headless; then
          login_ok=1
          check_provider
          case "$provider_out" in
            'provider: ready'*) login_todo='' ;;
            *) printf '%s\n' "$provider_out" ;;
          esac
        else
          say 'The sign-in did not finish.'
        fi
      fi
    fi
    ;;
esac

# 6. Discord.
connect_discord() {
  ask_hidden 'Discord bot token (input hidden): ' || return 1
  token=$secret
  secret=''
  while :; do
    if ! ask 'Your Discord user ID (the bot answers only this user; empty allows anyone): '; then
      token=''
      return 1
    fi
    case "$answer" in
      *[![:space:]]*)
        user_id=$answer
        break
        ;;
    esac
    user_id=''
    say 'Without a user ID, anyone who can see the bot can use it, and the gateway runs with command and file tools on.'
    if ! ask 'Continue without a user ID? [y/N]: '; then
      token=''
      return 1
    fi
    if is_yes "$answer"; then
      break
    fi
  done
  if [ -n "$user_id" ]; then
    printf '%s\n' "$token" | cli channels connect discord --token-stdin --allow-from "$user_id"
  else
    printf '%s\n' "$token" | cli channels connect discord --token-stdin
  fi
  connect_status=$?
  token=''
  return "$connect_status"
}

discord_out=$(cli channels connect discord --status) || {
  printf '%s\n' "$discord_out"
  fail 'channels connect discord --status failed'
}
case "$discord_out" in
  'discord: connected'*) ;;
  *)
    printf '%s\n' "$discord_out"
    discord_todo=1
    if [ -n "$interactive" ]; then
      say ''
      ask 'Connect a Discord bot now? [y/N]: ' || answer=''
      if is_yes "$answer"; then
        if connect_discord; then
          connect_ok=1
          discord_todo=''
        else
          say 'The Discord connection did not finish.'
        fi
      fi
    fi
    ;;
esac

# 7. Start the gateway; restart it if it was running and something changed.
running=$(docker compose ps --status running --services) || fail 'could not check whether the gateway is running'
was_running=''
if printf '%s\n' "$running" | grep -qx shacs-gateway; then
  was_running=1
fi
say 'Starting the gateway...'
docker compose up -d shacs-gateway || fail 'docker compose up -d shacs-gateway failed'
if [ -n "$was_running" ] && { [ -n "$login_ok" ] || [ -n "$connect_ok" ]; }; then
  docker compose restart shacs-gateway || fail 'docker compose restart shacs-gateway failed'
fi

# 8. What next.
say ''
say 'The gateway is running.'
say '  Logs:            docker compose logs -f shacs-gateway'
say '  Provider status: docker compose run --rm shacs-cli provider status'
say '  Runtime status:  docker compose run --rm shacs-cli runtime inspect'
if [ -n "$login_todo" ] || [ -n "$discord_todo" ]; then
  say 'Still to do (then restart the gateway: docker compose restart shacs-gateway):'
  if [ -n "$login_todo" ]; then
    say '  Sign in:         docker compose run --rm shacs-cli provider codex login --headless'
  fi
  if [ -n "$discord_todo" ]; then
    say '  Connect Discord: docker compose run --rm shacs-cli channels connect discord --token-stdin --allow-from YOUR_DISCORD_USER_ID'
  fi
  say '  API key providers and other channels: see docs/USAGE.md (Docker Compose)'
fi
