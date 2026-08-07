# PRD 003. sequential integration and Spec 032 closure

Status: Planned

## Goal

PRD 000-002를 하나의 app authoring/install/start/recover 흐름으로 통합하고 Spec 032의 parent requirement와 closure evidence를 완전히 검증한다.

## Scope

1. Parent requirement-to-PRD mapping과 acyclic dependency audit.
2. App Maker proposal/apply/install에서 AppSupervisor start/stop/recover까지 end-to-end smoke.
3. Extension activation blocker, credential/trusted-runtime/snapshot handoff, diagnostics receipt.
4. Coverage entry, user documentation, cleanup, final closure verdict.

## Non Scope

1. 새 domain state나 owner contract를 정의하지 않는다.
2. Specs 030, 031, 035의 `Complete` 상태를 요구하지 않는다.
3. Missing owner fact를 fixture-only success로 대체하지 않는다.

## Dependency DAG

```text
PRD000_app_supervisor
  -> PRD002_extension_activation_boundary
PRD001_authoring_apply_install
  -> PRD002_extension_activation_boundary
PRD000..PRD002
required_owner_fact_audits
  -> PRD003_final_closure
```

## Requirement Mapping

1. Process lifecycle, blocker, receipt: PRD 000.
2. Proposal, authoring decision, checkpoint, apply, verify, install/update: PRD 001.
3. Extension provenance, activation boundary, dependency blocker: PRD 002.
4. Release coverage, documentation, cross-surface QA, final closure: PRD 003.

## Acceptance Criteria

1. Every parent Must Have and Acceptance Criterion maps to exactly one primary PRD.
2. End-to-end fixture covers create, apply, install, blocked start, successful start, stop, recover, update, disable/uninstall.
3. Exact 030/031/035 owner facts may pass local audits while their specs remain Open; closure status is not required.
4. Release artifact records commands, tests, owner-fact locators, failures, cleanup receipts, and non-guarantees.

## Closure Evidence

1. Requirement mapping and dependency audit.
2. End-to-end CLI/local API lifecycle transcript.
3. External owner-fact read audits and coverage entry.
4. User-documentation audit and final Spec032 closure summary.
