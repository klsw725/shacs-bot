# PRD 001. image generate tool and media artifact

## 목표

이 문서는 agent-facing `image_generate` tool과 generated image artifact 저장 계약을 구현하기 위한 실행 기준이다. 목표는 agent가 이미지 생성을 하나의 tool로 요청하고, runtime이 provider capability를 호출한 뒤 결과 이미지를 local media artifact로 저장해 reference만 반환하게 하는 것이다.

이미지 생성은 비용과 네트워크, 파일 생성이 있는 side-effect action이다. 따라서 tool registration, permission, provider auth, media write guard가 명시적으로 통과해야 한다.

## SPEC 입력

- 주관 spec: `docs/specs/019-image-generation-and-generated-media/SPEC.md`
- 선행 PRD: `docs/specs/019-image-generation-and-generated-media/prds/000-provider-capability-and-openai-baseline.md`
- 교차 의존:
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 004는 `Tool`, `ToolRegistry`, parameter validation, `ToolResult::Json` 변환을 소유한다.
- 008은 config data dir과 `media/` runtime layout을 소유한다.
- 010은 side-effect tool gate, file write boundary, secret redaction을 소유한다.
- 014는 generated artifact를 diagnostics와 inspect에 노출할 때의 redaction 기준을 소유한다.
- 이 PRD는 provider별 HTTP request 구현을 소유하지 않는다. PRD 000의 `ImageGenerationClient`를 소비한다.

## 범위

- `image_generate` built-in tool schema
- production tool registry 등록 조건
- `tools.imageGeneration` config 의미
- provider capability 호출과 typed error rendering
- generated image file 저장 위치와 metadata
- tool result JSON shape
- CLI/API/channel에서 같은 artifact reference를 읽을 수 있는 최소 projection

## 범위 제외

- image viewer UI
- public URL 발급 또는 remote upload
- generated media gallery와 검색
- prompt template marketplace
- image edit, mask, variation tool
- provider별 raw option passthrough 전체 노출

## 구현 요구사항

- tool 이름은 `image_generate`로 고정한다.
- 필수 parameter는 `prompt` 하나다. 선택 parameter는 `size`, `quality`, `format`, `background`, `count`로 제한한다.
- tool schema는 provider id, API key, endpoint URL을 받으면 안 된다.
- production registry는 `allow_side_effect_tools`가 false이면 `image_generate`를 등록하지 않아야 한다.
- config에는 `tools.imageGeneration.enable`, default provider, default model, default output format, max count 같은 최소 설정을 둘 수 있다.
- enable 기본값은 안전하게 정해야 한다. 비용과 외부 호출이 있으므로 명시적으로 켜는 방식을 우선한다.
- tool은 `ImageGenerationClient`를 호출하고, 반환된 각 image를 config data dir의 `media/image-generation/` 아래에 저장해야 한다.
- 저장 파일명은 prompt 원문을 포함하지 않아야 하며 artifact id, timestamp, digest, extension만 사용해야 한다.
- 각 artifact에는 mime type, byte length, sha256, provider id, model id, created at, request option summary를 포함하는 metadata가 있어야 한다.
- tool result는 raw bytes나 base64를 포함하지 않고 artifact refs만 포함하는 JSON이어야 한다.
- provider failure, media write failure, unsupported config는 서로 다른 user-visible error로 반환해야 한다.

## 데이터/상태 모델

- `ImageGenerateToolParams`: prompt, size, quality, format, background, count.
- `GeneratedMediaArtifact`: artifact id, path, media ref, mime type, byte len, sha256, provider id, model id, created at.
- `GeneratedMediaMetadata`: request option summary, revised prompt digest, redaction status, provider request id.
- `ImageGenerateToolResult`: artifacts, warnings, provider summary, retryable flag.
- `ImageGenerationToolConfig`: enable, provider, model, default format, max count, max bytes.

## 정상 시퀀스

1. production tool registry가 config와 side-effect gate를 확인한다.
2. `image_generate`가 registry에 등록된다.
3. agent가 prompt와 optional image options로 tool을 호출한다.
4. tool이 parameter validation과 count 제한을 수행한다.
5. tool이 image generation provider capability를 호출한다.
6. returned image bytes를 `media/image-generation/` 아래에 저장하고 metadata를 만든다.
7. tool result JSON이 artifact references를 provider tool message로 반환한다.
8. diagnostics와 inspect surface가 artifact summary를 redacted 형태로 읽을 수 있다.

## 실패 시퀀스

1. image generation tool이 disabled이면 registry에 노출되지 않는다.
2. provider capability가 없으면 tool call은 unsupported provider error를 반환한다.
3. provider auth가 없으면 auth required error를 반환하고 prompt를 외부로 보내지 않는다.
4. provider가 이미지를 반환했지만 media write가 실패하면 tool result를 성공으로 표시하지 않는다.
5. metadata write가 실패하면 artifact를 orphan success처럼 반환하지 않는다.
6. diagnostics redaction이 실패하면 raw prompt와 raw image payload를 숨기고 redaction failure marker를 남긴다.

## 검증 관점

- side-effect tools가 비활성일 때 `image_generate`가 registry schema에 없는지 확인한다.
- enabled config와 provider capability가 있을 때 tool schema가 노출되는지 확인한다.
- prompt와 option이 `ImageGenerationRequest`로 정확히 변환되는지 확인한다.
- 생성 이미지가 media subtree 밖에 저장되지 않는지 확인한다.
- tool result에 raw base64가 포함되지 않는지 확인한다.
- provider failure와 media write failure가 서로 구분되는지 확인한다.

## 완료 기준

- `image_generate` tool이 config와 side-effect gate에 따라 등록된다.
- OpenAI baseline provider를 통해 generated image artifact가 local media dir에 저장된다.
- tool result는 artifact reference JSON만 반환한다.
- registry, tool execution, media write, diagnostics redaction 테스트가 추가된다.

## 현재 구현 상태

2026-06-13 기준 PRD 001은 구현됐다. 근거는 `crates/shacs-core/src/tools/image_generation.rs`, production registry wiring in `crates/shacs-cli/src/lib.rs`, and `crates/shacs-core/tests/tools.rs` image generation coverage다.

`image_generate`는 side-effect gate와 config gate를 통과할 때만 등록된다. tool 실행은 `ImageGenerationClient`를 호출하고, 생성 결과를 local media 아래 artifact로 저장하며, raw bytes, base64, or remote URL 대신 `artifacts[]` entries with `mediaRef`, `path`, `metadataRef`, and metadata summaries를 반환한다.

이 상태는 Codex Responses image generation, provider expansion, edit/mask/variation, streaming partial image, remote gallery, UI viewer가 구현됐다는 뜻이 아니다. 해당 범위는 PRD 002와 후속 문서가 계속 소유한다.
