# Spec 034 closure record

Status: Complete (Scoped)

Spec034의 현재 self-hosted/personal-use 범위는 구현, 정확한 22/22 요구사항 통합, review remediation과 실제 CLI/API/WebSocket/channel/TUI/artifact 표면 QA baseline까지 완료되었다. 이 기록은 구현 범위와 final seal 판정 계약을 설명한다. 최신 release 상태는 [canonical final manifest](../../../.omo/evidence/spec034/task-15-closure/final-committed/manifest.json)만 판정하며, 이 문서의 고정 문구에서 추론하지 않는다.

## Scope boundary

- Domain owner: Codex generated-media event, edit/mask/variation 계약, partial/final lifecycle, guarded remote output, local artifact persistence, bounded video analyzer, recorded-only replay와 media/analyzer owner facts.
- Adapter consumer: Spec035는 이미 생산된 canonical media/analyzer fact를 CLI/TUI/API/WebSocket/channel에 read-only로 투영한다. Spec035의 transport negotiation, reconnect와 Tasks parity는 계속 Open이다.
- Release boundary: [Remediation PASS](../../../.omo/evidence/spec034/remediation/PASS.json)는 failed candidate1의 blocker를 닫고 fresh QA와 새 source freeze를 요구했다. Todo 13은 `runner-mechanics-only`, `closure_eligible: false`다. [Todo 14 QA baseline](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/PASS.json)은 QA PASS지만 `closureClaimed: false`이고 surface receipt도 `final_release_evidence: false`다. 이후 source 변경은 새 frozen manifest와 변경 범위 재검증이 필요하다. Canonical final manifest가 현재 committed source digest, 최종 5개 PASS receipt, clean-tree runner evidence를 모두 결합하고 자체 verdict를 PASS로 기록한 경우에만 closure-eligible final seal이다.

## Must Have mapping

| ID | Implemented evidence |
|---|---|
| `034-MH001` | Codex image event를 text와 분리하고 commit 뒤 safe artifact ref로 반환: [Todo 6](../../../.omo/evidence/spec034/task-6-codex-events.json), [Todo 8](../../../.omo/evidence/spec034/task-8-persistence.json) |
| `034-MH002` | Provider-neutral generate/edit/mask/variation request/result와 adapter capability: [Todo 2](../../../.omo/evidence/spec034/task-2-provider-operations.json) |
| `034-MH003` | Descriptor-relative source/mask admission, MIME/size/provenance 재검사: [Todo 7](../../../.omo/evidence/spec034/task-7-image-edit.json) |
| `034-MH004` | `awaiting_start/started/partial/final/failed/cancelled` lifecycle와 final-only 확정: [Todo 2](../../../.omo/evidence/spec034/task-2-provider-operations.json), [Todo 6](../../../.omo/evidence/spec034/task-6-codex-events.json) |
| `034-MH005` | Guarded persisted/reference/rejected remote-output 결과: [Todo 3](../../../.omo/evidence/spec034/task-3-remote-policy.json) |
| `034-MH006` | Relative ref, MIME, bytes, digest, provider/model/source/options/retention/disclosure metadata: [Todo 1](../../../.omo/evidence/spec034/task-1-generated-media.json), [Todo 8](../../../.omo/evidence/spec034/task-8-persistence.json) |
| `034-MH007` | Raw payload omission과 `raw_content_possible` disclosure를 보존한 diagnostics: [Todo 10](../../../.omo/evidence/spec034/task-10-diagnostics-replay.json) |
| `034-MH008` | 여섯 media 상태와 035 adapter 입력: [Todo 11](../../../.omo/evidence/spec034/task-11-parity.json) |
| `034-MH009` | 주입형 analyzer, bounded evidence, source/trust/sandbox/credential/disclosure/snapshot fact: [Todo 4](../../../.omo/evidence/spec034/task-4-analyzer.json), [Todo 9](../../../.omo/evidence/spec034/task-9-video-runtime.json) |
| `034-MH010` | Live network/credential/analyzer/resource 호출 없는 recorded-only replay: [Todo 10](../../../.omo/evidence/spec034/task-10-diagnostics-replay.json) |

## Acceptance Criteria mapping

| ID | Implemented evidence |
|---|---|
| `034-AC001` | Codex image candidate가 text delta 없이 artifact path로 이동: [Todo 6](../../../.omo/evidence/spec034/task-6-codex-events.json) |
| `034-AC002` | Source/mask containment, MIME, size, provenance와 traversal rejection: [Todo 7](../../../.omo/evidence/spec034/task-7-image-edit.json) |
| `034-AC003` | Partial은 final로 자동 승격되지 않고 terminal transition을 별도 검증: [Todo 2](../../../.omo/evidence/spec034/task-2-provider-operations.json), [Todo 6](../../../.omo/evidence/spec034/task-6-codex-events.json) |
| `034-AC004` | Initial/redirect target 재검증, scheme, peer, byte/MIME, header omission과 3-way outcome: [Todo 3](../../../.omo/evidence/spec034/task-3-remote-policy.json) |
| `034-AC005` | Persisted record/file/ref/hash와 source chain/retention/disclosure: [Todo 8](../../../.omo/evidence/spec034/task-8-persistence.json), [Candidate2 Todo 14 surface QA](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/surfaces/PASS.json) |
| `034-AC006` | Base64, URL, credential, provider body, absolute path omission과 disclosure 보존: [Todo 10](../../../.omo/evidence/spec034/task-10-diagnostics-replay.json), [Candidate2 Todo 14 surface QA](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/surfaces/PASS.json) |
| `034-AC007` | `included/unsupported/extraction_failed/analyzer_missing/truncated/unavailable`의 CLI/API/WebSocket/channel parity와 TUI fail-closed: [Todo 11](../../../.omo/evidence/spec034/task-11-parity.json) |
| `034-AC008` | Injected/missing/unsupported codec/duration cap 및 cancellation/deadline: [Todo 4](../../../.omo/evidence/spec034/task-4-analyzer.json), [Todo 9](../../../.omo/evidence/spec034/task-9-video-runtime.json) |
| `034-AC009` | Recorded metadata/digest replay와 live dependency spy 호출 0회: [Todo 10](../../../.omo/evidence/spec034/task-10-diagnostics-replay.json) |
| `034-AC010` | 구조화 문서 정책의 exact `complete_scoped` status, 7개 unsupported claim=false, scoped non-guarantee와 실제 index의 `Complete (Scoped)` 상태: [documentation-policy.json](documentation-policy.json), [Todo 12](../../../.omo/evidence/spec034/task-12-integration.json) |
| `034-AC011` | Credential source, analyzer source/trust, sandbox scope와 disclosure를 canonical safe summary로 보존: [Todo 4](../../../.omo/evidence/spec034/task-4-analyzer.json), [Todo 11](../../../.omo/evidence/spec034/task-11-parity.json) |
| `034-AC012` | Credential 재해석, live URL 재접속, analyzer/current resource 검색 없는 replay: [Todo 10](../../../.omo/evidence/spec034/task-10-diagnostics-replay.json) |

Canonical catalog와 sequential integration은 정확히 10개 Must Have와 12개 Acceptance Criteria만 허용한다. [Todo 5 schema receipt](../../../.omo/evidence/spec034/task-5-schemas.json), [Todo 12 integration receipt](../../../.omo/evidence/spec034/task-12-integration.json), [Todo 13 coverage matrix](../../../.omo/evidence/spec034/task-13-release/coverage-matrix.json), [Candidate2 Todo 14 PASS](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/PASS.json)에서 22/22 매핑을 확인할 수 있다.

## User-visible behavior

- CLI `runtime inspect`는 data directory의 검증된 canonical media projection을 표시한다. Record가 없으면 성공을 합성하지 않고 unavailable로 표시한다.
- Local API `GET /v1/media/diagnostics`와 WebSocket `{"type":"media_projection"}`은 configured projection을 그대로 반환한다. HTTP surface는 projection이 없으면 404다.
- Channel adapter는 canonical projection을 보존하되 remote delivery 성공을 만들지 않는다. Included/truncated는 pending이고 외부 transport fact가 없는 실패는 unknown 또는 unavailable이다.
- TUI는 session metadata의 canonical media envelope를 읽어 상태, safe reason, freshness, lineage와 disclosure를 표시한다. Missing/malformed/stale-success 입력은 unavailable이다.
- Codex generated image와 validated edit/mask output은 local media root의 `artifacts/<id>/` 아래 record와 payload로 commit된 뒤 relative opaque ref로 노출된다. Variation과 provider별 미지원 operation은 explicit unsupported가 될 수 있다.
- Video analyzer는 runtime에 주입된 경우에만 bounded evidence를 만든다. Missing, unsupported, extraction failure, truncation과 timeout/cancellation은 성공으로 합성되지 않는다.

## Explicit non-guarantees

- CDN upload, public URL, hosted gallery, image editor UI, canvas, prompt gallery를 제공하지 않는다.
- 모든 provider의 edit, variation, streaming parity를 보장하지 않는다.
- Built-in ffmpeg, full codec coverage, complete video understanding을 보장하지 않는다.
- Arbitrary user URL intake를 제공하지 않는다. Provider remote output만 guarded policy를 통과한다.
- Remote reference는 persisted artifact가 아니며 영구 접근이나 재다운로드를 보장하지 않는다.
- Media-root admission은 universal filesystem/process containment나 OS sandbox가 아니다.
- Sandbox status는 표시된 adapter 범위의 fact일 뿐 provider, analyzer, kernel, extension 전체 격리를 뜻하지 않는다.
- Bounded evidence와 projection omission은 privacy, semantic redaction, complete redaction 또는 exfiltration prevention proof가 아니다.
- Credential source/status는 credential 자체, typed secret reference 또는 vault guarantee가 아니다.
- Trusted local profile은 untrusted file/repository를 자동 격리하지 않는다.

## Verification and mechanical seal

[Candidate2 Todo 14 Cargo receipt](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/cargo/receipt.json)은 전체 gate baseline에서 다음 명령이 모두 exit 0이었음을 기록한다. 이후 source 변경은 새 frozen manifest와 해당 변경 범위의 재검증으로 별도 결합해야 한다.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path crates/Cargo.toml --workspace --locked
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --workspace --locked
```

[Candidate2 Todo 14 surface receipt](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/surfaces/PASS.json)은 실제 CLI, loopback HTTP/WebSocket, channel, persisted artifact와 invalid-input 표면 및 sequential 22/22 PASS를 기록하고, [candidate2 TUI receipt](../../../.omo/evidence/spec034/task-14-final-qa-candidate2/tui/receipt.json)은 26개 TUI 테스트와 5개 상태 x 3개 geometry의 15개 capture PASS를 기록한다. [Remediation PASS](../../../.omo/evidence/spec034/remediation/PASS.json)의 code, security, docs, production mapper verifier도 모두 PASS다.

Release 절차는 최신 frozen source bytes에 대해 QA, goal, code, security, docs의 최종 5개 리뷰를 실행하고, 모두 PASS인 경우에만 동일한 reviewed bytes를 커밋한 뒤 committed-tree source-bound release evidence를 생성한다. 이전 candidate의 QA나 review receipt는 변경된 source의 최종 PASS를 대신하지 않는다. [Canonical final manifest](../../../.omo/evidence/spec034/task-15-closure/final-committed/manifest.json)가 이 결합을 검증해 최신 상태를 판정하며, manifest가 없거나 현재 HEAD/source digest와 불일치하거나 자체 verdict가 PASS가 아니면 final seal이 아니다.
