# shacs-bot Skills

This directory contains built-in skills that extend shacs-bot's capabilities.

## Skill Format

Each skill is a directory containing a `SKILL.md` file with:
- YAML frontmatter (name, description, metadata)
- Markdown instructions for the agent

When skills reference large local documentation or logs, prefer shacs-bot's built-in
`grep` / `glob` tools to narrow the search space before loading full files.
Use `grep(output_mode="count")` / `files_with_matches` for broad searches first,
use `head_limit` / `offset` to page through large result sets,
and `glob(entry_type="dirs")` when discovering directory structure matters.

## Built-in Catalog

Built-in registrations are generated from the skill directories by
`scripts/generate_builtins.py` into `src/builtins_generated.rs`. Run the generator
after adding, removing, renaming, or changing executable bits for files in an
active bundled skill directory.

`onboard` materializes active built-ins under a workspace's `builtin_skills/`
directory. `deferred_builtins.txt` lists imported Hermes skills kept in this
source tree as reference material but intentionally omitted from the active
built-in catalog and not copied by `onboard`. Those names are also ignored when
stale copies already exist under a workspace's `builtin_skills/` directory.
User-owned workspace skills with the same names can still be loaded from
`skills/`.

## Attribution

The original shacs-bot skills are adapted from [OpenClaw](https://github.com/openclaw/openclaw)'s skill system.
The imported extended catalog is adapted from the checked-in Hermes Agent reference under
`docs/refs/hermes-agent/skills`; each imported `SKILL.md` includes `metadata.shacs.imported_from`
and a shacs-bot adaptation note. The Hermes `powerpoint` skill is intentionally not bundled
because its included license forbids redistribution outside Anthropic services.

## Core Skills

| Skill | Description |
|-------|-------------|
| `cron` | Schedule reminders and recurring tasks. |
| `weather` | Get current weather and forecasts (no API key required). |
| `tmux` | Remote-control tmux sessions for interactive CLIs by sending keystrokes and scraping pane output. |
| `my` | Check and set the agent's own runtime state (model, iterations, context window, token usage, web config). Use when diagnosing why something doesn't work ("why can't you search the web?", "why did you stop?"), checking resource limits before complex tasks, adapting configuration for long or simple tasks, or remembering user preferences across turns. Also use when the user asks what model you are running, how many tokens you've used, or what your settings are. |
| `github` | Interact with GitHub using the `gh` CLI. Use `gh issue`, `gh pr`, `gh run`, and `gh api` for issues, PRs, CI runs, and advanced queries. |
| `skill-creator` | Create or update AgentSkills. Use when designing, structuring, or packaging skills with scripts, references, and assets. |
| `clawhub` | Search and install agent skills from ClawHub, the public skill registry. |
| `summarize` | Summarize or extract text/transcripts from URLs, podcasts, and local files (great fallback for “transcribe this YouTube/video”). |
| `memory` | Two-layer memory system with Dream-managed knowledge files. |

## Active Imported Skills

68 imported Hermes Agent skills are active shacs-bot built-ins after shacs-bot adaptation:

- `airtable`, `apple-notes`, `apple-reminders`, `architecture-diagram`, `arxiv`, `ascii-art`
- `ascii-video`, `audiocraft-audio-generation`, `baoyu-article-illustrator`, `baoyu-comic`
- `baoyu-infographic`, `blogwatcher`, `claude-design`, `codebase-inspection`, `comfyui`, `design-md`
- `dogfood`, `dspy`, `evaluating-llms-harness`, `excalidraw`, `gif-search`, `github-auth`
- `github-code-review`, `github-issues`, `github-pr-workflow`, `github-repo-management`
- `google-workspace`, `heartmula`, `himalaya`, `huggingface-hub`, `humanizer`, `ideation`, `imessage`
- `jupyter-live-kernel`, `linear`, `llama-cpp`, `llm-wiki`, `manim-video`, `maps`
- `minecraft-modpack-server`, `nano-pdf`, `node-inspect-debugger`, `notion`, `obliteratus`, `obsidian`
- `ocr-and-documents`, `openhue`, `p5js`, `pixel-art`, `plan`, `polymarket`, `popular-web-designs`
- `pretext`, `python-debugpy`, `requesting-code-review`, `research-paper-writing`
- `segment-anything-model`, `serving-llms-vllm`, `sketch`, `songwriting-and-ai-music`, `spike`
- `subagent-driven-development`, `systematic-debugging`, `test-driven-development`
- `weights-and-biases`, `writing-plans`, `xurl`, `youtube-content`

## Deferred Reference Skills

21 imported Hermes Agent skills are kept as source references but are not bundled as active built-ins because they depend on Hermes-specific runtime, CLI, tool, channel, or safety surfaces that shacs-bot does not currently expose:

- `claude-code`, `codex`, `debugging-hermes-tui-commands`, `findmy`, `godmode`, `hermes-agent`
- `hermes-agent-skill-authoring`, `hermes-s6-container-supervision`, `kanban-codex-lane`
- `kanban-orchestrator`, `kanban-worker`, `macos-computer-use`, `native-mcp`, `opencode`
- `pokemon-player`, `songsee`, `spotify`, `teams-meeting-pipeline`, `touchdesigner-mcp`
- `webhook-subscriptions`, `yuanbao`
