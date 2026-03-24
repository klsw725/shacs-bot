# Tool Usage Notes

Tool signatures are provided automatically via function calling.
This file documents non-obvious constraints and usage patterns.

## File Output — Sandbox Rule

When creating output files (transcripts, reports, exports, etc.), always save them under `sandbox/{source}/` instead of the workspace root.

- `sandbox/youtube/transcript_abc.txt` ✅
- `sandbox/summarize/report.md` ✅
- `transcript_abc.txt` (workspace root) ❌

This keeps the workspace clean as skills and agents accumulate output files.

## exec — Safety Limits

- Commands have a configurable timeout (default 60s)
- Dangerous commands are blocked (rm -rf, format, dd, shutdown, etc.)
- Output is truncated at 10,000 characters
- `restrictToWorkspace` config can limit file access to the workspace

## cron — Scheduled Reminders

- Please refer to cron skill for usage.
