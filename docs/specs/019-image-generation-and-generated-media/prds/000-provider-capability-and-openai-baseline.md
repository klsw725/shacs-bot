# PRD 000. provider capability and OpenAI baseline

## 목표

이 문서는 `docs/specs/019-image-generation-and-generated-media/SPEC.md`의 첫 실행 문서다. 목표는 이미지 생성을 provider별 ad-hoc API 호출이 아니라 `ImageGenerationClient` capability로 고정하고, OpenAI Images API 기반 text-to-image baseline을 구현 가능한 수준으로 내리는 것이다.

1차 구현은 provider-neutral request/result, OpenAI request builder, response parser, base64 decode, error normalization에 집중한다. Agent tool 등록과 media artifact write는 PRD 001이 소유한다.

## SPEC 입력

- 주관 spec: `docs/specs/019-image-generation-and-generated-media/SPEC.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 003은 provider registry, provider config validation, provider-specific client construction 패턴을 제공한다.
- 기존 transcription capability는 chat `ProviderClient`와 별도 trait을 두는 선례다.
- 008은 provider config와 data dir을 소유한다. 이 PRD는 새 secret store를 만들지 않는다.
- 010은 auth secret이 config, output, diagnostics에 새지 않아야 한다는 기준을 제공한다.
- PRD 001은 tool registry, permission gate, media artifact write를 소비한다.

## 범위

- `ImageGenerationRequest`, `ImageGenerationResult`, `GeneratedImage` 타입 의미
- `ImageGenerationClient` trait과 HTTP transport abstraction
- provider registry에서 image generation capability를 판정하는 최소 flag 또는 capability descriptor
- OpenAI Images API `POST /v1/images/generations` request builder
- OpenAI base64 output parser와 error normalization
- unsupported provider와 missing auth error family

## 범위 제외

- `image_generate` tool 등록과 실행
- media file write와 artifact metadata persistence
- Codex `/codex/responses` image generation stream parsing
- image edit, variation, mask upload
- provider별 가격 계산, quota dashboard, billing UX
- content moderation product policy

## 구현 요구사항

- `ImageGenerationClient`는 `generate_image(request) -> Result<ImageGenerationResult, ProviderError>` 형태의 단일 책임 trait이어야 한다.
- `ImageGenerationRequest`는 prompt, model override, size, quality, output format, background, count를 표현해야 한다.
- 1차 baseline은 text-to-image만 required로 둔다. input image, mask, edit action은 타입에 optional expansion point를 둘 수 있지만 구현 완료로 주장하지 않는다.
- `ImageGenerationResult`는 하나 이상의 `GeneratedImage`와 provider metadata를 포함해야 한다.
- `GeneratedImage`는 bytes 또는 decoded binary, mime type, optional revised prompt, provider item id를 포함해야 한다.
- OpenAI adapter는 GPT image model의 base64 output을 decode해야 하며, raw base64를 long-lived result text로 넘기면 안 된다.
- `ProviderSpec` 또는 별도 capability registry는 어떤 provider가 image generation을 지원하는지 표현해야 한다.
- `resolve_image_generation_client`는 provider id와 config를 받아 capability client를 만들고, unsupported provider를 typed error로 반환해야 한다.
- OpenAI API key는 기존 provider config/auth 흐름을 사용하며 새 secret path를 만들지 않는다.
- OpenAI request builder는 model, prompt, size, quality, output format, background, count를 안정적으로 JSON body에 반영해야 한다.

## 데이터/상태 모델

- `ImageGenerationRequest`: prompt, model, size, quality, output_format, background, count, provider_options.
- `ImageGenerationResult`: provider_id, model, images, usage, request_id, provider_metadata.
- `GeneratedImage`: index, mime_type, bytes, byte_len, revised_prompt, provider_item_id.
- `ImageGenerationCapability`: provider id, supported actions, supported formats, supported size policy, default model.
- `ImageGenerationError`: unsupported provider, auth required, provider rejected, malformed response, decode failed, transport failed.

## 정상 시퀀스

1. runtime 또는 tool layer가 configured image generation provider를 선택한다.
2. resolver가 provider spec과 config를 확인하고 `ImageGenerationClient`를 만든다.
3. caller가 provider-neutral `ImageGenerationRequest`를 전달한다.
4. OpenAI adapter가 Images API JSON request를 만든다.
5. provider response의 base64 image를 decode한다.
6. adapter가 `ImageGenerationResult`를 반환한다.
7. PRD 001의 tool layer가 result를 media artifact로 저장한다.

## 실패 시퀀스

1. provider가 image generation capability를 지원하지 않으면 unsupported provider error를 반환한다.
2. API key나 OAuth token이 없으면 auth required error를 반환하고 request body를 만들지 않는다.
3. provider가 policy 또는 validation error를 반환하면 provider rejected error로 정규화한다.
4. response에 image data가 없거나 알 수 없는 형식이면 malformed response로 실패한다.
5. base64 decode가 실패하면 decode failed로 실패하고 partial bytes를 노출하지 않는다.

## 검증 관점

- OpenAI request builder가 prompt와 option을 정확히 직렬화하는지 확인한다.
- OpenAI base64 response가 `GeneratedImage` bytes와 mime type으로 변환되는지 확인한다.
- unsupported provider와 missing auth가 구분되는지 확인한다.
- provider error body가 raw secret 없이 redacted message로 정규화되는지 확인한다.
- multiple image response가 안정적인 순서로 반환되는지 확인한다.

## 현재 구현 상태

2026-05-28 기준 PRD 000은 `shacs-providers` 안에서 구현됐다. 구현 범위는 provider-neutral `ImageGenerationClient`, OpenAI Images API request builder, base64 response parser, provider capability metadata, resolver, typed unsupported capability error까지다.

구현 evidence는 다음 경로에 있다.

- `crates/shacs-providers/src/clients/image_generation.rs`
- `crates/shacs-providers/src/clients/mod.rs`
- `crates/shacs-providers/src/registry.rs`
- `crates/shacs-providers/src/error.rs`
- `crates/shacs-providers/tests/image_generation.rs`

이 closure는 `image_generate` tool 또는 generated media artifact 저장까지 구현했다는 뜻은 아니다. 해당 범위는 PRD 001이 소유했고, 이후 구현됐다. Codex Responses image generation, edit/mask/variation support, streaming output은 PRD 002와 후속 문서가 계속 소유한다.

## 완료 기준

- provider-neutral image generation trait과 타입이 public crate surface로 고정된다.
- OpenAI Images API text-to-image baseline이 unit test로 검증된다.
- unsupported provider, auth required, malformed response, decode failure가 테스트로 고정된다.
- PRD 001이 tool/media 구현에 사용할 수 있는 result shape가 확정된다.
