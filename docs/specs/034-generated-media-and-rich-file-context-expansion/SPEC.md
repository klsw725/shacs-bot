# 034. generated media and rich file context expansion 아키텍처 명세

Status: Open

Origin specs: 004, 019, 027

## 문서 목적

이 문서는 기존 004, 019, 027이 구현 완료 범위를 닫은 뒤에도 남아 있는 generated media와 rich file context 작업의 새 owner boundary를 고정한다. 019는 OpenAI Images API baseline, `image_generate` tool, local generated image artifact 저장까지 닫는다. 027은 channel attachment intake, safe storage, image, document, audio, video analyzer handoff까지 닫는다. 004는 tool runtime의 현재 call/result 경계를 닫는다.

034의 목적은 닫힌 구현 범위를 다시 여는 것이 아니다. 남은 작업을 한곳에 모아 Codex Responses image generation, image edit/mask/variation, partial streaming images, remote URL provider output policy, generated artifact persistence, video-specific projection, analyzer capability를 제품 계약으로 만든다.

이 문서는 개인용 self-hosted runtime을 기준으로 한다. 사용자는 raw credential이 아니라 provider/model, credential source kind/status, media root policy, opaque artifact ref, relative path, digest, retention/provenance, sandbox/data-disclosure summary를 확인할 수 있어야 한다. 생성·분석 작업은 현재 사용자 OS 권한으로 실행될 수 있고 sandbox는 실제 적용 adapter 범위에서만 유효하다. CDN, public gallery, hosted editor 같은 원격 제품 표면은 기본 전제가 아니다.

## 현재 구현 기준선

현재 기준선은 다음과 같다.

1. 019는 provider-neutral image generation capability, OpenAI Images API baseline, `image_generate` tool, local media artifact 저장을 구현된 범위로 닫았다.
2. 019는 OpenRouter image generation provider slice를 구현된 범위로 인정하지만, Codex Responses image generation과 edit 계열은 닫지 않았다.
3. 027은 inbound channel attachment storage, routing, image/document/audio/video analyzer handoff를 구현된 범위로 닫았다.
4. 027은 video-specific local API, inspect, channel status projection이 제한적이며 열려 있다고 남겼다.
5. 004는 tool request가 `RuntimeToolCall`로 실행되고 `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`로 돌아오는 현재 tool runtime 경계를 닫았다.
6. 현재 generated media artifact는 raw bytes, raw base64, 만료성 remote URL을 user-facing artifact reference로 직접 남기지 않는 방향을 따른다. 이는 session/log/trace/tool output 전체의 complete redaction을 보장하지 않는다.

이 기준선은 이미 구현된 storage, routing, handoff를 존중한다. 034는 같은 기능을 다시 설계하거나 이름만 바꿔 재구현하지 않는다.

## 034가 소유하는 열린 범위

034는 다음 작업을 소유한다.

1. Codex Responses `image_generation` tool event를 provider output으로 받아 generated artifact로 저장하는 경계.
2. Image edit, mask, variation request/result model과 artifact 저장 계약.
3. Partial streaming images의 event 처리, 중간 산출물 표시, final artifact 확정 규칙.
4. Provider가 remote URL output을 반환할 때의 다운로드, 거절, persisted reference 정책.
5. Generated artifact persistence의 metadata, retention, provenance, replay, diagnostics 계약.
6. Video analyzer/media domain state vocabulary, evidence와 projection input의 최소 필드. Local API/inspect/channel adapter parity는 035가 소유한다.
7. Video analyzer capability의 provider/tool/runtime abstraction, bounded evidence, failure reason.
8. Media/analyzer evidence가 030 credential·trusted-resource·sandbox·raw-content disclosure와 031 execution snapshot을 참조하는 계약.

이 범위는 019의 generated image 계약과 027의 inbound attachment 계약을 함께 소비한다. Generated artifact는 agent가 만든 산출물이고, stored attachment는 사용자가 보낸 입력이다. 두 종류가 같은 media root 아래 있을 수 있어도 provenance와 lifecycle은 섞이면 안 된다.

## 구현 불변식

1. Generated media는 session text가 아니라 artifact record와 opaque safe reference로 남아야 한다.
2. Provider raw response, raw base64 image, signed URL 전체, raw token은 artifact projection, normalized tool result, diagnostics bundle에 그대로 저장하면 안 된다. Runtime session/log/trace의 raw-content 가능성은 030 disclosure를 따른다.
3. Codex Responses image event는 chat text처럼 취급하지 않고 generated media event로 해석해야 한다.
4. Edit, mask, variation 입력으로 쓰인 source artifact와 output artifact는 provenance chain으로 연결해야 한다.
5. Partial image stream은 final artifact를 대체하지 않는다. 중간 상태는 status evidence이고, 완료 산출물은 별도 확정 규칙을 통과해야 한다.
6. Remote URL provider output은 만료 URL을 장기 artifact처럼 표시하면 안 된다. Download를 선택하면 034 media policy와 기존 network/SSRF guard, scheme allowlist, redirect별 재검증, byte/MIME cap을 통과하고 credential/header forwarding 차단 evidence가 있어야 하며 guard를 사용할 수 없으면 거절한다.
7. Video analyzer output은 bounded metadata, subtitle, transcript, keyframe 또는 scene summary 같은 evidence만 provider context나 projection에 넣는다.
8. Analyzer가 없거나 codec이 지원되지 않으면 성공처럼 말하지 않고 unsupported, skipped, extraction_failed reason을 남긴다.
9. Generated artifact와 inbound attachment는 id namespace, metadata kind, diagnostics label에서 구분되어야 한다.
10. 004의 tool runtime 경계를 우회해 provider나 analyzer가 session truth를 직접 수정하면 안 된다.
11. Media-root admission과 path traversal 방지는 artifact path contract이지 전체 filesystem/process containment가 아니다.
12. Analyzer는 trusted executable resource일 수 있다. Activation/source disclosure는 030, config/snapshot은 031을 소비하며 analyzer가 안전하거나 sandboxed라고 자동 추론하지 않는다.
13. Bounded transcript/subtitle/scene summary는 extraction bound이며 semantic privacy redaction, 민감정보 제거, video 완전 이해를 보장하지 않는다.

## Must Have

1. Codex Responses image generation support는 Responses stream event에서 generated image payload 또는 reference를 찾아 artifact persistence path로 연결해야 한다.
2. Image edit, mask, variation은 provider-neutral request model을 가져야 하며, provider별 raw option은 안정 계약 밖에 가둬야 한다.
3. Source image, mask image, variation input은 media-root admission/path traversal 방지와 provenance 검사를 통과해야 한다. OS-level containment 여부는 030 sandbox status로 별도 표시한다.
4. Partial streaming images는 started, partial, final, failed, cancelled 같은 상태를 구분해야 한다.
5. Remote URL output policy는 최소 세 선택지를 명시해야 한다. Guard를 통과한 download 후 persisted artifact 저장, provider/domain·expiry-known·download-not-selected만 보존하는 safe remote reference, provider output 거절. 각 결과는 persistence/retention/retry 가능성을 과장하지 않는다.
6. Generated artifact metadata는 artifact id, kind, media root relative path, MIME, byte length, sha256, provider id, model id, source artifact ids, generation options summary, created at, projection disclosure status를 포함해야 한다. Disclosure status는 redaction/exfiltration proof가 아니다.
7. Diagnostics와 inspect는 artifact를 설명하되 raw payload와 credential을 projection에 포함하지 않고 `raw_content_possible_elsewhere` 같은 030 disclosure를 보존해야 한다.
8. 034는 stored/generated video evidence의 처리 상태와 최소 필드를 생산하고, 035 shared adapter가 local API, inspect, channel status에서 같은 의미로 투영할 수 있어야 한다.
9. Video analyzer capability는 runtime에 주입 가능한 abstraction이어야 하며, analyzer missing/unsupported/failed, analyzer source/trusted-code, sandbox scope/status를 safe summary로 설명해야 한다.
10. Replay는 live provider URL, credential store, current resource discovery를 다시 따라가지 않고 031 snapshot과 recorded artifact metadata/digest evidence를 사용해야 한다.

## Must Not Have

1. CDN 업로드, public URL 발급, hosted gallery를 034 완료 조건으로 삼으면 안 된다.
2. 이미지 editor UI, canvas editor, prompt gallery를 요구하면 안 된다.
3. 모든 provider에서 image edit, variation, streaming이 같은 수준으로 동작해야 한다고 요구하면 안 된다.
4. Built-in ffmpeg나 full codec understanding을 완료 조건으로 삼으면 안 된다.
5. 사용자가 입력한 arbitrary URL을 media intake처럼 다운로드하면 안 된다.
6. Remote URL output을 장기 artifact라고 표시하면서 로컬 persistence나 만료 정책을 생략하면 안 된다.
7. Partial image stream의 중간 frame을 final artifact로 조용히 승격하면 안 된다.
8. Generated artifact와 inbound attachment를 같은 provenance kind로 저장하면 안 된다.
9. Analyzer output을 근거보다 크게 말해 video 내용을 완전히 이해한 것처럼 표시하면 안 된다.
10. Tool result에 raw base64나 provider raw response 전체를 넣으면 안 된다.
11. Media-root containment를 universal filesystem/process containment로 표현하면 안 된다.
12. Sandbox가 active가 아닌 adapter, analyzer, provider, kernel, extension까지 격리됐다고 표시하면 안 된다.
13. Safe/redacted reference, disclosure status, diagnostics omission을 complete redaction, secret exfiltration prevention, privacy guarantee로 표현하면 안 된다.
14. Credential source status를 credential 자체, typed secret reference, vault guarantee로 표현하면 안 된다.
15. Remote reference를 영구 접근 가능하거나 재다운로드 가능한 artifact로 표현하면 안 된다.
16. Trusted local profile이 untrusted file/repository를 안전하게 격리한다고 주장하면 안 된다. 필요한 경우 별도 sandbox 또는 OS account 경계를 안내한다.

## Acceptance Criteria

1. Codex Responses image generation path가 text response와 분리되어 artifact record를 만든다.
2. Image edit, mask, variation request가 source artifact containment, MIME, size, provenance 검사를 통과한다.
3. Streaming image event test가 partial 상태와 final 확정 상태를 구분한다.
4. Remote URL output test가 034 media policy와 existing network/SSRF guard의 owner evidence를 구분하며 private/link-local/loopback target 차단, redirect별 재검증, scheme allowlist, byte/MIME cap, credential/header 미전달, guard 부재 시 거절, persisted/reference/rejected 결과를 검증한다.
5. Artifact persistence test가 metadata, digest, source chain, projection disclosure status를 검증한다.
6. Diagnostics projection test가 raw base64, signed URL, token, provider raw response 전체가 빠지고 session/log/trace의 raw-content 가능성 disclosure가 보존되는지 확인한다.
7. 034 domain state가 included, unsupported, extraction_failed, analyzer_missing, truncated를 구분하고 035 adapter parity test가 local API, inspect, channel status에서 그 의미를 보존한다.
8. Analyzer capability test가 analyzer 주입, analyzer 부재, codec 미지원, duration cap 초과를 구분한다.
9. Replay test가 remote URL 재요청 없이 artifact digest와 metadata evidence를 사용한다.
10. 사용자 문서가 CDN, gallery, UI editor, all-provider parity, built-in ffmpeg, full codec understanding, arbitrary URL intake를 지원 기능으로 말하지 않는다.
11. Projection parity가 credential source status, analyzer source/trust, sandbox mode/scope, data-disclosure status를 safe summary로 보존한다.
12. Replay가 credential 재해석, live URL 재접속, current resource 재검색 없이 031 snapshot과 artifact evidence만 사용한다.

## Source Handoff Table

<table>
  <thead>
    <tr>
      <th>Source spec</th>
      <th>닫힌 범위</th>
      <th>034로 넘어온 범위</th>
      <th>034의 처리 원칙</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>004 tool runtime</td>
      <td>`RuntimeToolCall`, `ToolResult`, `RuntimeToolMessage`, checkpoint, tool event의 현재 실행 경계</td>
      <td>Generated media tool 결과의 richer outcome, streaming status, artifact persistence evidence</td>
      <td>Tool runtime을 우회하지 않고 generated media 결과를 normalized artifact reference로 반환한다.</td>
    </tr>
    <tr>
      <td>019 image generation and generated media</td>
      <td>OpenAI baseline, `image_generate`, local generated image artifact 저장, OpenRouter slice</td>
      <td>Codex Responses image generation, image edit/mask/variation, partial streaming images, remote URL output persistence</td>
      <td>기존 artifact 저장 계약을 확장하되 raw payload와 remote URL을 session truth로 만들지 않는다.</td>
    </tr>
    <tr>
      <td>027 channel attachment intake and file context</td>
      <td>Inbound attachment storage, routing, analyzer handoff</td>
      <td>Video-specific projection과 analyzer capability의 제품 표면</td>
      <td>Inbound attachment provenance와 generated artifact provenance를 분리하고 video status를 사용자에게 보이게 한다.</td>
    </tr>
    <tr>
      <td>030 trusted agent runtime</td>
      <td>Current OS-user authority, credential source/status, optional sandbox scope/fallback, trusted resource/data disclosure</td>
      <td>Media/analyzer 실행과 artifact evidence가 참조할 operational facts</td>
      <td>034는 owner fact를 표시하되 universal containment, complete redaction, credential safety proof를 만들지 않는다.</td>
    </tr>
    <tr>
      <td>035 UI projection parity</td>
      <td>Shared projection vocabulary and cross-surface adapters</td>
      <td>Artifact/analyzer canonical state와 safe disclosure summary의 surface parity</td>
      <td>034가 domain fact를 생산하고 035가 CLI/TUI/API/channel에 같은 의미로 표시한다.</td>
    </tr>
    <tr>
      <td>031 configuration and execution snapshots</td>
      <td>Provider/profile declaration, credential source reference, immutable execution snapshot</td>
      <td>Media/analyzer replay provenance와 snapshot reference</td>
      <td>034는 config/schema/auth persistence를 재소유하지 않고 snapshot/artifact evidence를 소비한다.</td>
    </tr>
  </tbody>
</table>

## Implementation PRDs

Spec 034는 provider event normalization에서 artifact persistence, video analyzer와 final closure까지 아래 단계로 구현한다. 각 PRD는 media-domain 산출물로 독립 종료되고 외부 spec의 `Complete` 상태 대신 필요한 exact runtime/projection/snapshot facts만 소비한다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-codex-media-event-and-artifact-normalization.md) | Codex Responses image event normalization과 generated artifact handoff | 004/019 baselines |
| [PRD 001](prds/001-image-edit-variation-and-streaming-lifecycle.md) | Edit/mask/variation model과 partial/final lifecycle | PRD 000 |
| [PRD 002](prds/002-remote-output-and-artifact-persistence.md) | Remote URL policy, safe download/reference/reject, persistence/provenance | PRDs 000-001, Specs 030/031 fact contracts |
| [PRD 003](prds/003-video-analyzer-capability-and-evidence.md) | Video analyzer capability, bounded evidence, projection input | 027 baseline, Specs 030/031/035 fact contracts |
| [PRD 004](prds/004-sequential-integration-and-spec034-closure.md) | End-to-end media integration, requirement mapping, final Spec034 closure | PRDs 000-003, required owner-fact audits |

Current PRD status:

Codex OAuth를 사용하는 `/codex/images/generations` provider slice는 구현되어 기존 `image_generate` artifact 저장 경계에 연결됐다. 이는 PRD 000이 요구하는 Codex Responses stream event normalization을 완료한 것이 아니므로 아래 PRD 상태는 유지한다.

| PRD | Status |
|---|---|
| PRD 000 | Planned |
| PRD 001 | Planned |
| PRD 002 | Planned |
| PRD 003 | Planned |
| PRD 004 | Planned |

Dependency rules:

1. PRD 001은 final artifact contract를 PRD 000보다 먼저 정의하지 않는다.
2. PRD 002는 network/credential/sandbox enforcement를 재소유하지 않고 exact owner evidence를 소비한다.
3. PRD 003은 analyzer execution safety나 universal containment를 주장하지 않는다.
4. PRD 004는 외부 spec closure가 아니라 owner-fact artifacts와 local media evidence만 검사한다.

## Closure Evidence

034를 닫으려면 아래 증거가 같은 변경 안에 있어야 한다.

1. Provider 또는 runtime contract test가 Codex Responses image generation event를 artifact persistence로 연결한다.
2. Image edit, mask, variation에 대한 provider-neutral model test와 최소 한 provider adapter test가 있다.
3. Streaming image partial/final/failure 상태를 검증하는 test가 있다.
4. Remote URL provider output의 safe download, persisted reference, rejection policy test가 있다.
5. Generated artifact metadata와 provenance chain을 검증하는 persistence test가 있다.
6. Video analyzer capability와 video projection status를 검증하는 core, projection, diagnostics test가 있다.
7. Diagnostics bundle 또는 inspect evidence가 secret, signed URL, raw base64를 포함하지 않는다는 regression test가 있다.
8. README, usage, specs index 중 사용자에게 노출되는 문서가 실제 지원 범위와 비범위를 정확히 반영한다.
9. 닫는 문서에는 구현 파일, 테스트 이름, 사용자 표면, 비범위 유지 여부가 함께 기록되어야 한다.
10. 030 read audit가 current OS authority, credential status, sandbox adapter scope/fallback, trusted analyzer source, raw-content disclosure를 검증해야 한다.
11. 035 parity audit와 031 snapshot audit가 artifact/analyzer safe summary와 replay provenance를 검증해야 한다.
12. Redaction audit는 projection surface omission만 검증하고 complete redaction이나 exfiltration prevention을 주장하지 않아야 한다.

현재 이 문서는 Open 상태다. 위 evidence가 없으면 034의 범위를 구현 완료로 닫을 수 없다.
