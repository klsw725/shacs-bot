---
name: gif-search
description: "Search/download GIFs from Tenor via curl + jq."
version: 1.1.0
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
prerequisites:
  env_vars: [TENOR_API_KEY]
  commands: [curl, jq]
metadata:
  hermes:
    tags: [GIF, Media, Search, Tenor, API]
metadata.shacs.imported_from: media/gif-search
---

> shacs-bot adaptation: This skill was adapted for shacs-bot from the checked-in upstream reference. Use shacs-bot workspace and built-in skill paths, and prefer shacs-bot tools such as `exec`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `web_fetch`, `web_search`, `spawn`, and `ask_user`. Source-specific commands, services, or slash commands mentioned below are reference material unless the same capability is configured in shacs-bot.

# GIF Search (Tenor API)

Search and download GIFs directly via the Tenor API using curl. No extra tools needed.

## When to use

Useful for finding reaction GIFs, creating visual content, and sending GIFs in chat.

## Setup

Edit `.env.json` beside the actual `config.json` in your data folder (default `~/.shacs-bot/.env.json`). Preserve other keys in the JSON object and replace this example locally in your editor:

```json
{
  "TENOR_API_KEY": "tenor_example_replace_locally"
}
```

In Docker, the default host file `~/.shacs-bot/.env.json` appears as `/home/shacs/.shacs-bot/.env.json` inside the container. If the mount or config path differs, edit the host file beside that config. This is not Compose's repository `.env`; do not add shell assignments or `export` to the JSON. For a file you create, use `chmod 600` on its actual path.

Restart the running shacs-bot runtime after saving. When this skill is active, its declared key reaches every `exec`, even before the skill is read; no config env entry is required. A non-empty process value wins over the file, and explicit top-level `env` / `tools.exec.env` can override it. Never ask for or paste secrets in chat: the user must edit the file locally. Check `shacs-bot skills show gif-search` for missing names, then use a task that does not print the key. The CLI reads current inputs, not the running gateway's saved values, and readiness does not prove API access. Do not inspect secrets with `env`, `printenv`, or `echo`; general output, transcripts, and logs can retain them.

Get a free API key at https://developers.google.com/tenor/guides/quickstart — the Google Cloud Console Tenor API key is free and has generous rate limits.

## Prerequisites

- `curl` and `jq` (both standard on macOS/Linux)
- `TENOR_API_KEY` environment variable

## Search for GIFs

```bash
# Search and get GIF URLs
curl -s "https://tenor.googleapis.com/v2/search?q=thumbs+up&limit=5&key=${TENOR_API_KEY}" | jq -r '.results[].media_formats.gif.url'

# Get smaller/preview versions
curl -s "https://tenor.googleapis.com/v2/search?q=nice+work&limit=3&key=${TENOR_API_KEY}" | jq -r '.results[].media_formats.tinygif.url'
```

## Download a GIF

```bash
# Search and download the top result
URL=$(curl -s "https://tenor.googleapis.com/v2/search?q=celebration&limit=1&key=${TENOR_API_KEY}" | jq -r '.results[0].media_formats.gif.url')
curl -sL "$URL" -o celebration.gif
```

## Get Full Metadata

```bash
curl -s "https://tenor.googleapis.com/v2/search?q=cat&limit=3&key=${TENOR_API_KEY}" | jq '.results[] | {title: .title, url: .media_formats.gif.url, preview: .media_formats.tinygif.url, dimensions: .media_formats.gif.dims}'
```

## API Parameters

| Parameter | Description |
|-----------|-------------|
| `q` | Search query (URL-encode spaces as `+`) |
| `limit` | Max results (1-50, default 20) |
| `key` | API key (from `$TENOR_API_KEY` env var) |
| `media_filter` | Filter formats: `gif`, `tinygif`, `mp4`, `tinymp4`, `webm` |
| `contentfilter` | Safety: `off`, `low`, `medium`, `high` |
| `locale` | Language: `en_US`, `es`, `fr`, etc. |

## Available Media Formats

Each result has multiple formats under `.media_formats`:

| Format | Use case |
|--------|----------|
| `gif` | Full quality GIF |
| `tinygif` | Small preview GIF |
| `mp4` | Video version (smaller file size) |
| `tinymp4` | Small preview video |
| `webm` | WebM video |
| `nanogif` | Tiny thumbnail |

## Notes

- URL-encode the query: spaces as `+`, special chars as `%XX`
- For sending in chat, `tinygif` URLs are lighter weight
- GIF URLs can be used directly in markdown: `![alt](url)`
