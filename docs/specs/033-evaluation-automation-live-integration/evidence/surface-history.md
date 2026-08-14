# Spec 033 surface evidence history

Status: current surface QA v4 PASS; all five final reviews PASS; final source-bound release execution PASS; Complete (Scoped).

## Recorded surface

- Goal CLI and `GET /v1/sessions/{session}/goal-snapshot` were exercised in surface QA v4.
- Local improvement CLI/API and recorded trajectory/replay were exercised in the final surface QA v4 pass.
- Generated release output lived under `.omo/evidence/spec033-current/` and was intentionally not committed because it can contain bulky transcripts.
- The current recorded trajectory is `spec033-production-no-provider-20260814-v6` under `.omo/evidence/spec033-current/production-trajectories-v6`.
- Surface QA v4 passed with `shacs-bot` SHA-256 `150bfcc0afc48c5670fd99fee76919231731594b9de84f9ae815aa8481e097b0` and `shacs-tui` SHA-256 `cc0e2ca476e795fd1aef8e67661e500cfe3f4627164d102c326a8c5cf90b0f63`.

## Current audit rule

This file is an index, not a replacement transcript. QA, goal, code, security, docs와 final source-bound release execution은 current candidate에 대해 모두 final PASS다. exact source manifest digest, commands, redacted transcript locators와 cleanup receipt는 `.omo/evidence/spec033-current/final-production-20260814-v6/manifest.json`에 생성되었다. 향후 실행 실패 또는 locator 미생성은 shipping을 차단한다. Cargo gate output under generated `gates/` directories must not be labeled as a QA, goal, code, security, or docs review verdict.

## Cleanup

The release runner deletes raw temporary command output through `TempDir` cleanup and publishes staging atomically. After auditing a generated run, remove only its selected `.omo/evidence/spec033-current/<run-id>` directory and `/tmp/shacs-spec033-data-v6`; do not remove unrelated user or agent evidence.

## Non-guarantees

- Surface QA PASS alone does not make the current closure Complete.
- It does not prove complete runtime redaction, remote delivery, exactly-once execution, authorization, or Spec 035 parity closure.
