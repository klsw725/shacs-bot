# PRD 004. sequential integration and Spec 034 closure

Status: Planned

## Goal

PRD 000-003을 generated-media와 video-analysis product flow로 통합하고 Spec 034 parent requirement와 closure evidence를 완전히 검증한다.

## Scope

1. Dependency DAG and one-to-one requirement mapping.
2. Codex event, edit/stream, remote output, artifact persistence, analyzer end-to-end smoke.
3. Projection/disclosure/snapshot and external owner-fact audits.
4. Coverage, documentation, cleanup, final closure verdict.

## Non Scope

1. 새 provider, network, credential, sandbox, projection, snapshot truth를 정의하지 않는다.
2. Specs 030, 031, 035의 `Complete` 상태를 요구하지 않는다.
3. Missing analyzer/provider fact를 success fixture로 대체하지 않는다.

## Dependency DAG

```text
PRD000_event_normalization
  -> PRD001_edit_stream
PRD000..PRD001
  -> PRD002_remote_persistence
PRD003_video_analyzer
PRD000..PRD003
required_owner_fact_audits
  -> PRD004_final_closure
```

## Requirement Mapping

1. Codex event normalization: PRD 000.
2. Edit/mask/variation and streaming: PRD 001.
3. Remote output, persistence, replay: PRD 002.
4. Video analyzer and bounded evidence: PRD 003.
5. Integration, projection, documentation, final closure: PRD 004.

Primary parent requirements owned by this PRD:

- Primary Acceptance Criteria: 10

## Acceptance Criteria

1. Every parent Must Have and Acceptance Criterion has one primary PRD.
2. End-to-end smoke covers persisted, reference-only, rejected, partial/final, analyzer missing/failure paths.
3. Exact owner facts may pass local audits while source specs remain Open.
4. Closure summary records artifact provenance, disclosure, commands, failures, cleanup and non-guarantees.

## Closure Evidence

1. Requirement/DAG audit.
2. Generated-media and analyzer real-surface artifacts.
3. External owner-fact/projection/snapshot audits.
4. Documentation and final Spec034 closure summary.
