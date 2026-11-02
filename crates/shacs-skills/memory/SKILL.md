---
name: memory
description: Two-layer memory system with Dream-managed knowledge files.
always: true
---

# Memory

## Structure

- `SOUL.md` — Bot personality and communication style. **Managed by Dream.** Do NOT edit.
- `USER.md` — User profile and preferences. **Managed by Dream.** Do NOT edit.
- `memory/MEMORY.md` — Long-term facts (project context, important events). **Managed by Dream.** Do NOT edit.
- `memory/history.jsonl` — append-only JSONL, not loaded into context. Search it with `memory_search` first.

## Search Past Events

Start with `memory_search`. It searches `memory/MEMORY.md` and `memory/history.jsonl` by the words and by the meaning of the query with a model that runs on this machine, so a question in other words, with other particles or with an abbreviation can still find the item.

- `memory_search(query="치과 예약 언제였지")`; add `limit` (1 to 20, default 5) for more items
- Each result has the exact text, `matched_by` (`text`, `semantic` or `both`) and its source: `MEMORY.md` lines, or the history `cursor`, `timestamp` and item
- When `semantic.status` is `unavailable`, only the words of the query were matched; `semantic.reason` says why. For `model_missing` or `model_unverified`, give the user the install command that `semantic.detail` ends with; it names the configured model
- `index.status` `incomplete` means some items have no vector yet; they are found by their words only until later searches compute them

Use `grep` when you need an exact string or time, or the entry around a result. `memory/history.jsonl` is JSONL — each line is a JSON object with `cursor`, `timestamp`, `content`.

- `grep(pattern="2026-04-02 10:00", path="memory/history.jsonl", fixed_strings=true)` for a literal timestamp
- `grep(pattern="\"cursor\":42,", path="memory/history.jsonl", fixed_strings=true, output_mode="content")` for the entry of a `memory_search` result with cursor 42
- Use `output_mode="content"` plus `context_before` / `context_after` for the exact lines, and `head_limit` / `offset` to page through long histories
- Use `exec` only as a last-resort fallback when the built-in tools cannot express what you need

## Important

- **Do NOT edit SOUL.md, USER.md, or MEMORY.md.** They are automatically managed by Dream.
- If you notice outdated information, it will be corrected when Dream runs next.
- Users can view Dream's activity with the `/dream-log` command.
