# image generation and generated media 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: OpenAI image generation baseline, `image_generate` local media artifact storage, and the implemented OpenRouter provider slice are closed in this owner scope.
Open work moved to: [034 generated media and rich file context expansion](../034-generated-media-and-rich-file-context-expansion/SPEC.md).
Not carried forward: Generic media asset management, audio generation, video generation, and music generation are non-goals for 019.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`, `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 바탕으로 이미지 생성 기능을 어떻게 구현할지 정의한다.

목표는 다음과 같다.

- 이미지 생성은 agent가 호출하는 tool surface이고, provider별 API 차이는 provider capability 뒤에 숨긴다는 경계를 고정한다.
- chat `ProviderClient`에 이미지 생성을 억지로 합치지 않고, transcription처럼 capability-specific client를 추가하는 기준을 세운다.
- 생성된 이미지는 session text가 아니라 local generated media artifact로 저장하고, tool 결과에는 redacted artifact reference만 반환한다.
- OpenAI Images API baseline, Codex Responses `image_generation` tool, 다른 provider 확장을 단계적으로 나눈다.
- self-hosted / personal-use 환경에서 비용, 네트워크, 파일 저장, diagnostics, redaction의 최소 안전 기준을 정한다.

이 문서는 generic media runtime 전체를 소유하지 않는다. 범위는 이미지 생성과 그 결과물인 generated image artifact에 한정한다. 오디오 생성, 비디오 생성, 음악 생성, 일반 media asset manager는 별도 owner가 필요해질 때 분리한다.

---

## 상위 기준과의 관계

- 003은 provider 호출과 provider별 raw API adapter 경계를 소유한다. 019는 이미지 생성이라는 provider capability의 제품 계약을 소유하고, 구현은 003의 provider crate 패턴을 소비한다.
- 004는 tool registry, tool dispatch, `ToolResult` 변환 경계를 소유한다. 019는 `image_generate`라는 개별 tool의 의미와 artifact 반환 계약만 소유한다.
- 008은 config data dir, runtime media dir, provider config layout을 소유한다. 019는 generated image가 008의 data dir 아래 media subtree를 사용해야 한다고 요구한다.
- 010은 host safety, permission, secret, redaction을 소유한다. 019는 이미지 생성이 네트워크와 파일 생성이 있는 side-effect action임을 표시하고 010의 gate를 소비한다.
- 014는 diagnostics bundle과 inspect surface를 소유한다. 019는 generated image record와 provider call summary가 diagnostics에서 어떻게 보일 수 있는지 요구한다.
- 017은 app bundle과 app task 의미를 소유한다. 019의 image generation은 app capability로 노출될 수 있지만 app lifecycle을 직접 소유하지 않는다.

따라서 이 문서는 SaaS image studio, 조직 관리자 승인, remote asset library, public gallery, marketplace billing workflow를 다루지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- provider-neutral image generation request/result model
- `ImageGenerationClient` capability와 provider resolution
- OpenAI Images API 기반 text-to-image baseline
- `image_generate` built-in tool surface
- generated image artifact 저장 위치, metadata, tool result shape
- config, permission, diagnostics, tests의 최소 요구사항
- Codex Responses `image_generation` tool과 다른 provider 확장을 위한 후속 계약

이 문서는 다음을 정의하지 않는다.

- chat response parser를 이미지 생성 전용으로 즉시 대체하는 작업
- 모든 provider의 이미지 생성 구현
- 이미지 편집, variation, mask, streaming partial image의 1차 구현 완료
- prompt safety classifier 또는 content moderation product 설계
- 이미지 뷰어 UI, gallery, asset library
- remote upload, sharing, CDN, public URL 발급

---

## 핵심 계약

### Provider capability

이미지 생성은 `ProviderClient`의 chat 메서드가 아니라 별도 capability다. `shacs-providers`는 transcription의 `TranscriptionClient`와 같은 방식으로 `ImageGenerationClient`를 제공해야 한다.

provider-neutral 입력은 사용자의 의도를 보존해야 하지만 provider별 option을 그대로 노출하면 안 된다. 안정적인 최소 필드는 prompt, size, quality, output format, background, count, optional input images다. provider별 raw field는 diagnostics용 metadata 또는 explicit advanced options로만 제한한다.

provider-neutral 출력은 raw base64를 session text로 반환하지 않는다. provider adapter는 generated image bytes, mime type, provider/model metadata, optional revised prompt, usage 또는 request id를 runtime으로 넘긴다.

### Tool surface

agent가 보는 공식 표면은 `image_generate` tool이다. 이 tool은 provider id, API endpoint, auth token을 직접 받지 않는다. 현재 config의 image generation provider capability를 해결하고, 실패하면 user-visible error를 반환한다.

`image_generate`는 read-only tool이 아니다. 외부 provider 호출과 local file write를 수행하므로 side-effect tools가 허용된 production registry에서만 등록되어야 한다.

### Generated media artifact

생성 이미지는 config data dir의 runtime media subtree 아래에 저장한다. 기본 위치는 `~/.shacs-bot/media/image-generation/` 계열이어야 한다. 파일명에는 raw prompt를 넣지 않고, stable id 또는 digest 기반 이름을 사용한다.

tool result는 JSON object여야 하며 최소한 artifact id, local path 또는 media ref, mime type, byte length, sha256, provider id, model id, request options summary를 포함해야 한다. prompt 원문은 사용자가 볼 수 있는 tool call 입력에는 남을 수 있지만 artifact metadata와 diagnostics에는 redacted 또는 digest form을 우선한다.

### Safety and diagnostics

이미지 생성은 비용이 발생하고 외부 네트워크를 사용하며 파일을 만든다. 따라서 explicit tool enable flag, provider auth presence, side-effect permission, media write guard를 모두 통과해야 한다.

diagnostics는 provider id, model id, artifact ref, request status, error family, redaction status를 남길 수 있다. raw API key, raw token, full base64 image payload, provider raw response 전체는 diagnostics에 넣지 않는다.

---

## External API 기준

1차 baseline은 OpenAI Images API다.

- 공식 가이드: <https://developers.openai.com/api/docs/guides/image-generation>
- Images API reference: <https://developers.openai.com/api/reference/resources/images/methods/generate/>
- Responses create reference: <https://developers.openai.com/api/reference/resources/responses/methods/create/>

OpenAI Images API는 `POST /v1/images/generations`에 prompt, model, size, quality, output format, background 같은 option을 전달한다. GPT image models는 base64 output을 기본으로 보므로 adapter는 base64 decode와 media write를 공식 경계로 포함해야 한다.

Responses API의 `tools: [{ "type": "image_generation" }]`는 agentic multi-turn image generation에 더 자연스럽지만, 이 프로젝트의 현재 Codex provider는 `/codex/responses` text/tool streaming 중심이다. 따라서 Codex Responses image generation은 1차 baseline 이후 별도 PRD에서 스트림 event parsing, generated image extraction, artifact 저장을 붙인다.

---

## 구현 불변식

- provider raw API response는 session visible truth가 아니다. generated image artifact record와 tool result만 공식 출력이다.
- tool은 provider별 endpoint를 직접 알면 안 된다. provider capability client만 호출해야 한다.
- raw base64 image payload는 tool result, session message, diagnostics bundle에 그대로 넣지 않는다.
- generated artifact path는 config data dir media subtree 밖을 가리키면 안 된다.
- provider가 URL output을 주더라도 runtime은 필요한 경우 다운로드 또는 persisted reference 정책을 명시해야 하며, 만료 URL만을 long-lived artifact처럼 표시하면 안 된다.
- image generation failure는 text answer로 성공처럼 포장하지 않고 typed error family와 retry 가능성을 반환해야 한다.

---

## PRD 분할

- `prds/000-provider-capability-and-openai-baseline.md`: provider-neutral image generation capability, OpenAI Images API baseline, result decoding.
- `prds/001-image-generate-tool-and-media-artifact.md`: `image_generate` tool, media 저장, permission/config gate, tool result shape.
- `prds/002-codex-responses-and-provider-expansion.md`: Codex Responses `image_generation` tool, provider expansion, edit/streaming/future capability.

---

## 현재 구현 상태

2026-07-06 기준 PRD 000과 PRD 001은 구현됐다. PRD 002는 OpenRouter image generation provider slice만 구현됐고 나머지는 열려 있다.

- PRD 000 provider capability baseline은 구현됐다. 근거는 `crates/shacs-providers/src/clients/image_generation.rs`, `crates/shacs-providers/src/clients/mod.rs`, `crates/shacs-providers/src/registry.rs`, `crates/shacs-providers/src/error.rs`, `crates/shacs-providers/tests/image_generation.rs`다.
- PRD 001 `image_generate` tool과 local media artifact 저장은 구현됐다. `image_generate`는 side-effect gate와 config gate를 통과할 때만 노출되고, `ImageGenerationClient`를 호출하며, 생성 결과를 local media 아래 artifact로 저장한다. tool result는 raw bytes, base64, remote URL이 아니라 `artifacts[]` entries with `mediaRef`, `path`, `metadataRef`, and metadata summaries를 반환한다. 근거는 `crates/shacs-core/src/tools/image_generation.rs`, production registry wiring in `crates/shacs-cli/src/lib.rs`, and `crates/shacs-core/tests/tools.rs` image generation coverage다.
- PRD 002 중 OpenRouter image generation provider slice는 구현됐다. 근거는 `OpenRouterImageGenerationClient`, `build_openrouter_image_generation_request`, `parse_openrouter_image_generation_response`, resolver/model mapping, remote URL rejection, and OpenRouter regression tests in `crates/shacs-providers/src/clients/image_generation.rs` and `crates/shacs-providers/tests/image_generation.rs`다.
- 이 상태는 Codex Responses image generation, image edit/mask/variation, streaming partial image, remote URL output provider persistence, remote gallery, UI viewer가 구현됐다는 뜻이 아니다. 해당 범위는 PRD 002와 후속 문서가 계속 소유한다.

---

## 완료 기준

- `shacs-providers`가 `ImageGenerationClient`와 OpenAI baseline implementation을 제공한다.
- production tool registry가 config와 side-effect gate를 통과할 때만 `image_generate`를 등록한다.
- 생성 이미지는 data dir media subtree에 저장되고, tool result는 artifact reference JSON만 반환한다.
- OpenAI success, auth failure, unsupported provider, malformed provider output, media write failure가 테스트로 고정된다.
- docs와 diagnostics가 이미지 생성 지원 범위와 미지원 범위를 과장하지 않는다.
