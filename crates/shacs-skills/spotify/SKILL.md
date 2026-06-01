---
name: spotify
description: "Spotify: play, search, queue, manage playlists and devices."
version: 1.0.0
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
prerequisites:
  tools: [configured spotify integration playback, configured spotify integration devices, configured spotify integration queue, configured spotify integration search, configured spotify integration playlists, configured spotify integration albums, configured spotify integration library]
metadata:
  hermes:
    tags: [spotify, music, playback, playlists, media]
    related_skills: [gif-search]
metadata.shacs.imported_from: media/spotify
disabled: true
metadata.shacs.deferred_reason: Hermes-specific runtime surface; keep as reference until shacs-bot support exists.
---

> shacs-bot deferred: This imported Hermes skill is kept in the source tree as reference material but is not bundled as an active built-in skill because it depends on Hermes-specific runtime, CLI, tool, or channel surfaces that shacs-bot does not currently expose.

> shacs-bot adaptation: This skill was imported from Hermes Agent. Use shacs-bot workspace and built-in skill paths, and prefer shacs-bot tools such as `exec`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `web_fetch`, `web_search`, `spawn`, and `ask_user`. Hermes-only commands, services, or slash commands mentioned below are reference material unless the same capability is configured in shacs-bot.

# Spotify

Control the user's Spotify account via the Hermes Spotify toolset (7 tools). Setup guide: https://hermes-agent.nousresearch.com/docs/user-guide/features/spotify

## When to use this skill

The user says something like "play X", "pause", "skip", "queue up X", "what's playing", "search for X", "add to my X playlist", "make a playlist", "save this to my library", etc.

## The 7 tools

- `configured spotify integration playback` — play, pause, next, previous, seek, set_repeat, set_shuffle, set_volume, get_state, get_currently_playing, recently_played
- `configured spotify integration devices` — list, transfer
- `configured spotify integration queue` — get, add
- `configured spotify integration search` — search the catalog
- `configured spotify integration playlists` — list, get, create, add_items, remove_items, update_details
- `configured spotify integration albums` — get, tracks
- `configured spotify integration library` — list/save/remove with `kind: "tracks"|"albums"`

Playback-mutating actions require Spotify Premium; search/library/playlist ops work on Free.

## Canonical patterns (minimize tool calls)

### "Play <artist/track/album>"
One search, then play by URI. Do NOT loop through search results describing them unless the user asked for options.

```
configured spotify integration search({"query": "miles davis kind of blue", "types": ["album"], "limit": 1})
→ got album URI spotify:album:1weenld61qoidwYuZ1GESA
configured spotify integration playback({"action": "play", "context_uri": "spotify:album:1weenld61qoidwYuZ1GESA"})
```

For "play some <artist>" (no specific song), prefer `types: ["artist"]` and play the artist context URI — Spotify handles smart shuffle. If the user says "the song" or "that track", search `types: ["track"]` and pass `uris: [track_uri]` to play.

### "What's playing?" / "What am I listening to?"
Single call — don't chain get_state after get_currently_playing.

```
configured spotify integration playback({"action": "get_currently_playing"})
```

If it returns 204/empty (`is_playing: false`), tell the user nothing is playing. Don't retry.

### "Pause" / "Skip" / "Volume 50"
Direct action, no preflight inspection needed.

```
configured spotify integration playback({"action": "pause"})
configured spotify integration playback({"action": "next"})
configured spotify integration playback({"action": "set_volume", "volume_percent": 50})
```

### "Add to my <playlist name> playlist"
1. `configured spotify integration playlists list` to find the playlist ID by name
2. Get the track URI (from currently playing, or search)
3. `configured spotify integration playlists add_items` with the playlist_id and URIs

```
configured spotify integration playlists({"action": "list"})
→ found "Late Night Jazz" = 37i9dQZF1DX4wta20PHgwo
configured spotify integration playback({"action": "get_currently_playing"})
→ current track uri = spotify:track:0DiWol3AO6WpXZgp0goxAV
configured spotify integration playlists({"action": "add_items",
                   "playlist_id": "37i9dQZF1DX4wta20PHgwo",
                   "uris": ["spotify:track:0DiWol3AO6WpXZgp0goxAV"]})
```

### "Create a playlist called X and add the last 3 songs I played"
```
configured spotify integration playback({"action": "recently_played", "limit": 3})
configured spotify integration playlists({"action": "create", "name": "Focus 2026"})
→ got playlist_id back in response
configured spotify integration playlists({"action": "add_items", "playlist_id": <id>, "uris": [<3 uris>]})
```

### "Save / unsave / is this saved?"
Use `configured spotify integration library` with the right `kind`.

```
configured spotify integration library({"kind": "tracks", "action": "save", "uris": ["spotify:track:..."]})
configured spotify integration library({"kind": "albums", "action": "list", "limit": 50})
```

### "Transfer playback to my <device>"
```
configured spotify integration devices({"action": "list"})
→ pick the device_id by matching name/type
configured spotify integration devices({"action": "transfer", "device_id": "<id>", "play": true})
```

## Critical failure modes

**`403 Forbidden — No active device found`** on any playback action means Spotify isn't running anywhere. Tell the user: "Open Spotify on your phone/desktop/web player first, start any track for a second, then retry." Don't retry the tool call blindly — it will fail the same way. You can call `configured spotify integration devices list` to confirm; an empty list means no active device.

**`403 Forbidden — Premium required`** means the user is on Free and tried to mutate playback. Don't retry; tell them this action needs Premium. Reads still work (search, playlists, library, get_state).

**`204 No Content` on `get_currently_playing`** is NOT an error — it means nothing is playing. The tool returns `is_playing: false`. Just report that to the user.

**`429 Too Many Requests`** = rate limit. Wait and retry once. If it keeps happening, you're looping — stop.

**`401 Unauthorized` after a retry** — refresh token revoked. Tell the user to run `hermes auth spotify` again.

## URI and ID formats

Spotify uses three interchangeable ID formats. The tools accept all three and normalize:

- URI: `spotify:track:0DiWol3AO6WpXZgp0goxAV` (preferred)
- URL: `https://open.spotify.com/track/0DiWol3AO6WpXZgp0goxAV`
- Bare ID: `0DiWol3AO6WpXZgp0goxAV`

When in doubt, use full URIs. Search results return URIs in the `uri` field — pass those directly.

Entity types: `track`, `album`, `artist`, `playlist`, `show`, `episode`. Use the right type for the action — `configured spotify integration playback.play` with a `context_uri` expects album/playlist/artist; `uris` expects an array of track URIs.

## What NOT to do

- **Don't call `get_state` before every action.** Spotify accepts play/pause/skip without preflight. Only inspect state when the user asked "what's playing" or you need to reason about device/track.
- **Don't describe search results unless asked.** If the user said "play X", search, grab the top URI, play it. They'll hear it's wrong if it's wrong.
- **Don't retry on `403 Premium required` or `403 No active device`.** Those are permanent until user action.
- **Don't use `configured spotify integration search` to find a playlist by name** — that searches the public Spotify catalog. User playlists come from `configured spotify integration playlists list`.
- **Don't mix `kind: "tracks"` with album URIs** in `configured spotify integration library` (or vice versa). The tool normalizes IDs but the API endpoint differs.
