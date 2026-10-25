# PRD 000. 비디오 분석 실제 사용과 호환성 판정

Status: Complete (Decision)

## Goal

비디오 analyzer의 **기본/명시 주입 여부, 저장 첨부의 실제 분석 결과, 이후 소비자와 비용**을 한 조사 흐름에서 확인해 유지·범위 축소·제거 중 근거 있는 결론을 기록한다. 이 PRD 하나가 [040 SPEC](../SPEC.md)의 독립 종료 단위이며 코드나 저장된 영상을 제거하지 않는다.

## Scope

1. 기본 runtime 구성과 `ContextBuilder::with_video_analyzer` 호출자를 나눠 확인한다. 실제 analyzer가 없는 경우 `analyzer: None`과 injected test success를 다른 증거로 남긴다.
2. 사용자 소유 격리 자료에서 저장된 비디오 첨부 → 제한된 context evidence → provider input/inspect/API/TUI/channel 중 실제 지원 표면을 추적한다. 정상, 미설정, 비지원, 크기/시간 초과, 취소·실패를 함께 기록한다.
3. 관측된 호출·실패·비용·민감정보 범위와 projection/lineage의 저장·읽기 소비자를 확인한다. 제거/축소하더라도 027 attachment, 이미지와 오디오 경로, 과거 media record의 읽기 의미가 어떻게 남는지 비교한다.
4. 유지/축소/제거의 사용자-visible 지원 범위·상태 vocabulary·대체 동작·호환 테스트를 비교하고 선택한 결론을 문서화한다. 생성자/config/호출자/문서/보존 record 조사에도 production 주입 경로가 없으면 그 범위의 부재를 증거로 삼되, 저장소 밖 소비는 `미확인` 위험으로 남긴다. 공개 주입 API와 과거 record의 보존/deprecation 계약 없이 축소·제거를 확정하지 않는다.

## Non Scope

- 새 비디오 analyzer/provider 구현, CDN/editor, 이미지 생성·audio 분석의 삭제 또는 이전.
- 완전한 영상 이해, 비밀정보 제거, 모든 입력 URL 수용, universal sandbox를 증명했다고 주장하는 일.

## Parent Requirement Mapping

- [040 SPEC](../SPEC.md)의 조사 계약 1–4, 세 판정 규칙, 변경 전 불변식 및 수용 조건 1–3을 한 PRD에서 모두 소유한다. 조사와 결론을 따로 나눠 뒤 PRD에 완료를 맡기지 않는다.

## Acceptance Criteria

1. 소스와 실행 구성을 대조해 기본/주입 analyzer availability와 실제 production 호출자·미확인 부분을 표시한다. 구성이 있다면 제어 비디오로 attachment→context→표면을 관찰한다. 없으면 unsupported의 실제 의미와 시험 주입 결과의 한계를 각각 남긴다.
2. 크기/시간 초과·비지원 codec·취소/timeout·result publication 실패를 안전한 status와 구분한다. API/TUI/channel 등의 소비가 없는 표면을 성공으로 만들지 않고 저장된 기록 및 audio/image 경계 영향을 확인한다.
3. 현재 사용자 가치·실패/저장·측정된 비용 또는 미측정 한계와 유지/축소/제거별 대체·호환 범위를 비교한 뒤 하나의 근거 있는 결론과 후속 작업·회귀 기준을 남긴다. 조사 범위 내 production 경로 부재도 유효한 결과이며 외부 소비자는 미확인 위험으로 표시한다. 조사 범위 또는 과거 record 처리 자체의 근거가 부족하면 미결정으로 두고 완료하지 않는다.

## Closure Evidence

- 격리된 stored video, 기본/주입 구성, 실제 표면과 안전한 projection/failure 기록.
- 데이터·호출자·비용/민감정보 범위와 선택지 비교표, 미확인 사항, 검토 가능한 결정문.

이 PRD는 **비디오 analyzer를 유지할지에 관한 판정**으로만 닫힌다. 다른 판정 스펙이나 실제 제거 구현을 필요로 하지 않는다.

## 조사 기록 (2026-10-24)

실행 증거는 로컬 `.omo/evidence/spec040/prd000/`(추적하지 않음)에 있다. `sNN`은 실제 binary 단계, `hNN`은 저장소 밖 주입 harness 단계이며 `transcript.md`에 정리했다. 이 evidence가 없는 checkout에서는 실행 증명으로 보지 않는다.

- **실행 조건:** source `c33632e`. `cargo clean` 뒤 `cargo build --locked -p shacs-cli -p shacs-tui`로 만든 binary를 썼다. 모든 명령은 가짜 `HOME`, 비운 환경 변수, 격리 `--config`(data dir 분리)로 바로가기 없는 실제 경로에서 실행했다. 모델은 요청 본문을 기록하는 OpenAI 호환 가짜 서버(`custom` provider)로 연결했다. 첫 반복 실행(`invalid-loop1`)은 스크립트 결함으로 영상 대신 빈 파일을 올렸으므로 판정에서 뺐다.
- **시험 영상:** 직접 만든 5초 H.264+AAC MP4(화면 글자 "SPEC040 CONTROL Ns", 음성 "The code word is pineapple"), 같은 구성의 QuickTime MOV, 960초 MP4, EBML 헤더만 있는 WebM, 깨진 MP4, 11 MiB 파일이다. 컨테이너 사실은 제품 코드와 별개로 확인했다(`control-video-facts.jsonl`).
- **실제 사용자 자료:** 이 스펙의 불변식에 따라 실제 사용자 config/auth/data dir은 판정 근거로 쓰지 않는다. 처음에 실제 data dir의 파일 목록을 나열하고 대화 기록 안 영상 note 문자열의 개수를 세는 읽기 전용 확인을 했지만, 이 불변식에 어긋나므로 그 결과를 판정 근거와 이 문서에서 뺐다(1차 독립 검토 지적). 실제 사용은 범위 전체를 미확인으로 둔다.

### 기본 runtime의 analyzer 구성

| 확인 대상 | 결과 |
|---|---|
| 생성자 | `ContextBuilder::new`의 `MediaContext::new`가 video·audio analyzer를 모두 `None`으로 둔다 (`crates/shacs-core/src/runtime/context/video.rs:32`) |
| production 호출자 | `with_video_analyzer`, `with_video_analyzer_staging_root`, `with_video_analyzer_owner_refs`, `with_video_media_provenance`의 호출자는 테스트와 예제뿐이다. media root를 연결하는 production ContextBuilder는 `AgentLoopChatCompletionAdapter::context_builder` 하나이며(`crates/shacs-cli/src/spec035_cli_media/context_builder.rs:15`) `ask`, `agent`, `run`·`runtime start`(채널), `serve`(API), `web`, `trajectory`의 turn이 모두 이를 쓴다. 이 builder는 analyzer 없이 `with_video_projection_publication(store, None)`만 붙인다. subagent의 ContextBuilder는 system prompt만 만들고 첨부를 다루지 않는다 |
| 설정·명령·plugin | video analyzer config 키가 없다(onboard 결과 s01). CLI 옵션, plugin manifest, MCP 설정으로도 주입할 수 없다. 주입하려면 Rust 코드를 고쳐 다시 빌드해야 한다 |
| 실제 사용 | 미확인이다. 위 불변식에 따라 실제 사용자 자료를 검사하지 않았다. 다만 수정하지 않은 binary에서는 analyzer가 실행되거나 분석 기록이 생길 경로가 없다 |
| 미확인 범위 | 모든 실제 data dir(기본 Docker Compose 포함), `shacs-core`를 라이브러리로 써서 `with_video_analyzer`를 부르는 fork나 외부 코드. 이 범위의 사용을 없다고 보지 않는다 |

### 기본 경로의 입력별 결과 (실제 binary, 로컬 API multipart와 data URL)

| 입력 | 모델이 받은 내용 | 저장·기록 |
|---|---|---|
| 5초 MP4 (s03) | 실행 문맥, `[attachment:unsupported] name=… channel=api mime=video/mp4 bytes=45164 sha256_prefix=7b1dfd5bb5d3 reason=video analyzer is not configured` 한 줄, 사용자 질문. 화면 글자와 음성 단어는 들어가지 않았다 | `media/attachments/api/`에 원본과 같은 sha256으로 저장. `media/projections/`와 staging 폴더는 생기지 않음 |
| QuickTime MOV (s08) | 같은 note, `mime=video/mp4` | 같음 |
| 960초 MP4 (s09) | `reason=video duration exceeds configured limit: duration_seconds=960 limit_seconds=900`. analyzer가 없어도 길이 제한 문구가 먼저 나와 짧으면 분석될 것처럼 읽힌다 | 같음 |
| EBML 헤더만 있는 WebM (s10), 깨진 MP4 (s11) | 확장자로 영상 판정, `video analyzer is not configured` | 같음 |
| 11 MiB (s12) | HTTP 413 `multipart field exceeds 10485760 bytes`, 모델 호출 없음 | 저장 안 됨. API와 외부 채널 다운로드의 intake 한도(10 MiB, `shacs_api::MAX_MEDIA_BYTES`)가 video route의 20 MiB 한도보다 먼저 걸리므로 route 한도는 현재 입구에서 닿지 않는다(코드) |
| data URL MP4 (s13) | 같은 note, 이름은 `upload.bin` | 저장 |

표면은 모두 기본 상태다. `runtime inspect`는 `Spec035 media projections: unavailable (canonical records unavailable)`(s06), `GET /v1/media/diagnostics`는 404(s05), TUI는 영상을 보낸 session에도 `media: state=unavailable`(s07)이다. 외부 채널 전송은 기록 파일이 없으면 media metadata를 붙이지 않고 그대로 전달된다(코드, 전달은 s16). `runtime inspect`의 `Spec031 media … severity=error` 줄은 생성 이미지 개수로만 정해지며(`crates/shacs-cli/src/spec035_cli_media/runtime_format/header.rs:115`) 비디오와 무관하다.

### 시험 주입 경로 (저장소 밖 harness)

production builder와 같은 구성(`with_video_projection_publication(store, None)`)에 가짜 analyzer만 더한 harness로 s03의 저장 영상을 다시 처리하고, 같은 data dir을 실제 binary의 `runtime inspect`·API·TUI로 읽었다. 가짜 값에는 모두 `FAKE`를 붙였고 영상에서 나온 값이 아니다.

| 경우 | 모델에 보낼 문맥 (`build_messages` 결과, provider 호출 없음) | 기록과 표면 |
|---|---|---|
| 분석 성공 (h02) | `[attachment:included_text]`와 제한된 metadata·scene 문맥 | `media/projections/current.json`이 `state=unavailable`, `freshness=unavailable`, `missing_analyzer…`로 기록되고 inspect·API·TUI가 모두 unavailable을 보인다. production builder가 analyzer source·snapshot owner fact를 넘기지 않기 때문이며, 이 fact를 만드는 production 코드가 없다(테스트 지원 `OwnerFixture`뿐) |
| 문맥 잘림 (h03) | `[attachment:truncated]`, `subtitles truncated to 8000 chars` | unavailable |
| 형식 미지원 (h04), 추출 실패 (h05) | `unsupported`, `extraction_failed` | 같은 상태로 기록·표시 |
| 시간 초과 (h06), 취소 (h07) | 0.3초 마감·취소를 걸었는데 `timeout`·`cancelled` note가 3.05초 뒤 나왔다. supervisor는 상태만 고정하고 analyzer가 끝날 때까지 기다린다(`crates/shacs-core/src/runtime/video_analyzer_runtime/supervisor.rs:131`). 협조하지 않는 analyzer는 그동안 turn을 붙잡는다 | 둘 다 `extraction_failed`로 기록 |
| 960초 영상 (h08) | analyzer를 부르지 않고 길이 제한 note | 기록을 고치지 않아 표면은 이전 영상(h07)의 `extraction_failed`를 계속 보인다. 기록은 첨부별이 아니라 data dir 하나에 하나뿐이다 |
| 기록 저장 실패 (h09) | analyzer는 성공했지만 `[attachment:extraction_failed] … video projection publication failed`로 바뀐다 | `runtime inspect`가 종료 코드 1로 실패, API 404, TUI unavailable |

이 harness는 주입이 가능한 배관을 보여줄 뿐 실제 영상 이해의 가치를 보여주지 않는다. 저장소 테스트도 같은 한계다. `shacs-core`의 `--test it spec034_video_analyzer` 26개, `spec034_media_projection` 8개, `runtime_agent`의 주입 테스트 1개, `spec034_sequential_integration` 2개(171.65초), `--lib file_context` 19개가 `c33632e`에서 통과했다. 모두 test double analyzer와 `ftyp`·`moov/mvhd`만 있는 합성 MP4를 쓴다. analyzer 하나는 동시에 한 영상만 처리하고 두 번째 요청은 `busy`로 실패한다(위 26개 중 `context_clones_reject_a_second_worker_while_the_shared_analyzer_is_busy`. harness로는 재현하지 않았다).

### 남은 기록이 손상됐을 때

`media/projections/current.json`을 잘린 JSON으로 두면(s14) `runtime inspect`는 `invalid canonical record`로 종료 코드 1이 되어 lifecycle·recovery·readiness를 보여주지 못한다. `runtime diagnostics --bundle`도 같은 오류로 실패해 진단 묶음을 만들지 못한다(s20, 정상 기록에서는 만들어짐 s21). API는 404, TUI는 unavailable로 오류를 삼킨다. `status`와 `/v1/readiness`는 영향이 없다. WhatsApp bridge 가짜 서버로 `run`을 실행하자(s15) 에이전트는 답을 만들었지만 `channel delivery failed: media projection is invalid`로 답장이 나가지 않았다. 외부 채널 adapter는 보낼 때마다 이 파일을 읽고 실패를 전송 실패로 돌린다(`crates/shacs-cli/src/spec035_cli_media/channel_adapter.rs:59`). 이 답장은 session 기록에 남았지만 파일을 치운 뒤 다시 실행해도(s16) 재전송되지 않았다. 정상 기록이 있을 때도(s17) bridge로 나간 frame은 `{"type":"send","to":…,"text":…}`뿐이며 media metadata는 사용자에게 보이지 않는 내부 정보다. 현재 제품 명령은 이 파일을 만들지 않는다.

### 저장 자료와 읽는 곳

| 자료 | 위치 (`<data>` = config가 있는 폴더) | 쓰는 곳 | 읽는 곳 |
|---|---|---|---|
| 저장된 영상 첨부 | `<data>/media/attachments/<channel>/` | 027 intake (API multipart·data URL, 외부 채널) | context routing. 027 소유이며 이 판정의 대상이 아니다 |
| 최신 영상 분석 기록 | `<data>/media/projections/current.json` (하나뿐, 64 KiB 상한) | `VideoAnalyzerSpec035Publisher`뿐(`crates/shacs-core/src/runtime/video_analyzer_spec035/publication.rs:84`). analyzer가 없으면 도달하지 않는다 | `runtime inspect`와 `runtime diagnostics`의 진단 JSON `spec035_media_projections`(오류 시 두 명령 모두 실패), `GET /v1/media/diagnostics`·WebSocket `{"type":"media_projection"}`(오류는 기록이 없을 때와 같은 not found), TUI 모든 session 행(오류는 unavailable), 외부 채널 전송마다(오류는 전송 실패) |
| analyzer staging | `<workspace>/.shacs-video-analyzer-staging/<pid>-<n>/` | analyzer 실행 중에만 생기고 끝나면 지운다 | analyzer |

### 이미지·오디오 경계

- 이미지는 provider가 native image input을 지원하면 image block, 아니면 `native image input is not supported by provider/model` note다(s19). 비디오 판정과 무관하다.
- 오디오도 production에서 analyzer가 연결되지 않아 `audio analyzer is not configured` note가 된다(s18). `TranscriptionAudioAnalyzer`는 있지만 production 생성자가 없고 `channels.transcriptionProvider` 설정을 쓰는 곳도 없다. 오디오의 유지 여부는 이 판정의 범위가 아니며 바꾸지 않는다.
- 영상에서 뽑은 오디오를 오디오 analyzer에 넘기는 경로는 두 analyzer가 모두 있어야 동작하므로 production에서 쓰이지 않는다. MP4 길이 읽기(`sniff_mp4_duration_seconds`)는 `audio/mp4`도 쓰므로 비디오와 함께 사라지면 안 된다.
- `message` 도구의 영상 파일 전송, `image_generate`와 생성 artifact, 027의 영상 첨부 저장은 비디오 분석과 별개다.

### 가치와 비용 비교

| | 내용 |
|---|---|
| 지금 사용자에게 주는 것 | intake를 통과한 영상을 보내면 모델이 "영상이 첨부됐지만 분석하지 못했다"는 사실(이름·형식·크기·hash 앞부분)을 안다. 이 동작은 analyzer가 아니라 027 routing의 note이며 제거해도 남길 수 있다 |
| 주입했을 때 주는 것 | 제한된 metadata·자막·scene/keyframe 요약을 모델에 넣는 배관. 실제 analyzer 구현은 저장소에 없고, 만들려면 ffmpeg 같은 외부 도구, 음성 전사 provider, keyframe을 읽을 vision 모델이 필요하다. 034는 built-in ffmpeg를 범위 밖으로 두었고 새 analyzer 구현은 이 PRD의 비범위다 |
| 주입했을 때의 결함 | 성공한 분석도 표면에는 unavailable, 표면 기록이 첨부별이 아니라 마지막 하나라 다른 영상의 상태를 보임, 마감·취소가 대기 시간을 줄이지 못함, 기록 저장 실패가 성공한 분석을 버림, 한 번에 한 영상 |
| 사용하지 않는 사용자의 비용 | 분석 기록 파일이 손상되면 `runtime inspect`, `runtime diagnostics`와 모든 외부 채널 답장이 실패한다(s14, s15, s20). turn마다 analyzer invocation을 만들고 채널 전송마다 파일을 연다. 시간은 측정하지 않았다 |
| 유지 비용 | analyzer 전용 core source 약 2,230줄과 `file_context.rs` 안 helper 약 250줄. media projection 전용 형식·표시 약 1,070줄(`shacs-projection` spec035 media 761줄 포함)과 CLI·API·WebSocket·TUI·채널의 읽기 코드. 전용 테스트·예제 약 3,460줄. 034 release runner의 `spec034_sequential_integration`(22개 요구사항 중 영상의 `034-MH008`, `034-MH009`, `034-AC007`, `034-AC008`, `034-AC011`, media evidence 진단·replay의 `034-MH007`, `034-MH010`, `034-AC009`, `034-AC012`, 금지 자료 검사 `034-AC006`의 일부 입력과 다섯 표면 parity)과 `spec034_evidence_schema`, Spec031 runner의 `spec031-owner-spec034` 정확한 테스트 이름(`runtime_context_routes_stored_video_with_injected_analyzer`)과 결합 |
| 같은 결과의 다른 길 | 영상 내용을 다뤄야 하면 화면 캡처를 이미지로 보내 이미지 입력을 지원하는 모델로 처리하거나, 설치된 경우 `summarize` 같은 외부 도구 skill을 쓴다. 이번에 실행하지 않았다 |
| 사용 근거 | 실제 사용은 미확인이다. 수정하지 않은 binary에서는 분석이 일어날 경로가 없다. 제품 의도의 중심 경험(맡긴 일의 지속과 근거 있는 선제 알림)과 연결되는 사용 장면도 없다 |

### 세 선택지의 호환 계약

- **유지:** 사용 근거를 확인하지 못했고 지원 가능한 주입 경로가 없다. 유지하려면 적어도 실제 analyzer 구현과 사용자가 켜는 설정, production owner fact 연결, 마감·취소 시 실제로 멈추는 실행, 첨부별 기록, 손상된 기록에도 실패하지 않는 읽기가 먼저 필요하다. 현재 note·기록 형식과 표면은 그대로 읽어야 한다.
- **범위 축소:** (a) analyzer 배관만 남기면 production 주입 경로가 없어 가치가 그대로 0이다. (b) 표면만 남기면 기록을 쓰는 곳이 없다. (c) 컨테이너에서 읽은 길이 같은 기본 정보만 note에 넣는 것은 새 기능이며 수요 근거가 없다. 가치가 확인된 입력이나 bounded evidence가 없어 축소의 근거가 없다.
- **제거:** 아래 판정과 계약이다.

## 판정 (2026-10-24): 제거

video analyzer capability와 그 분석 기록 표면을 제품에서 제거한다. 영상 첨부의 저장과 `unsupported` note는 남긴다. 근거는 다음과 같다.

1. production에 analyzer를 넣는 경로가 없다. 생성자는 `None`이고 설정·명령·plugin으로 켤 수 없으며, 주입은 Rust 코드를 고쳐 다시 빌드해야만 된다. 그래서 수정하지 않은 binary에서는 intake를 통과해 context routing에 도달한 영상이 모두 `unsupported` note가 되고 분석 기록이 생기지 않는다(s03, s08–s11, s13. 10 MiB를 넘는 영상은 intake에서 거절된다, s12).
2. 실제 analyzer 구현이 없다. 가치를 내려면 외부 도구·전사·vision을 묶은 새 구현이 필요하고, 이는 034가 범위 밖으로 둔 일이며 이 PRD의 비범위다. 시험 주입의 성공은 배관 확인일 뿐이다.
3. 주입해도 결과가 일관되지 않다. 성공한 분석이 표면에 unavailable로 보이고, 표면은 마지막 영상 하나의 상태만 보이며, 마감·취소는 대기 시간을 줄이지 못하고, 기록 저장 실패가 성공한 분석을 버린다.
4. 쓰지 않는 사용자에게도 위험이 있다. 제품이 쓰지 않는 기록 파일 하나가 손상되면 `runtime inspect`와 진단 묶음이 실패하고 외부 채널 답장이 사라진다(s14, s15, s20).
5. 유지 비용이 크다. 위 코드·테스트와 두 release runner 결합을 계속 지켜야 한다.

실제 사용은 미확인이다. 이 스펙은 실제 사용자 자료를 검사하지 않으며, 그 위험은 아래 비파괴 계약으로 다룬다. 단순히 기본 analyzer가 `None`이라는 이유로 내린 결론이 아니다. 이 판정은 코드 변경이나 자료 삭제가 아니며 데이터 삭제 허가도 아니다. 아래 계약은 별도 구현 작업이 지킬 경계다.

### 보존할 사용자 자료와 동작

- 영상 첨부는 지금과 같은 위치·한도로 계속 받고 저장한다. 이미 저장된 첨부를 지우거나 고쳐 쓰지 않는다.
- 영상 첨부마다 모델 입력에 `[attachment:unsupported]` note 하나를 지금과 같은 필드(name, channel, mime, bytes, sha256_prefix)로 넣는다. reason은 `video content analysis is not supported`로 고정하고 설정으로 켤 수 있다는 뜻이나 길이 제한 문구를 쓰지 않는다. 영상을 조용히 빼거나 본 것처럼 표시하지 않는다.
- 남은 `media/projections/current.json`과 staging 폴더를 읽지도, 고치지도, 지우지도 않는다. 이전 작업은 필요 없다. 지우기는 사용자가 직접 한다.
- 이미지·문서·오디오 routing과 오디오 analyzer 추상화, `audio/mp4` 길이 읽기, `message` 도구의 영상 전송, 생성 이미지 artifact와 그 inspect 표시는 바꾸지 않는다.

### 바뀌는 표면

- **모델 입력:** 위 note 외의 영상 문맥, 길이 제한 판정, analyzer staging과 분석 기록 쓰기를 없앤다.
- **분석 기록을 읽던 표면:** 모두 분석 기록을 읽지 않고, 남은 기록이 있든 없든 지금 기록이 없는 data dir에서 내는 결과로 고정한다. 남은 기록의 위치와 지워도 된다는 안내는 사용 가이드가 맡는다.
  - `runtime inspect`·`runtime diagnostics`: 사람이 읽는 줄은 `Spec035 media projections: unavailable (canonical records unavailable)`, 진단 JSON의 `spec035_media_projections` 키는 빈 배열이다. 키는 진단 형식을 바꾸지 않는 동안 남긴다.
  - API: `GET /v1/media/diagnostics`는 404 `media projection is unavailable`, WebSocket `{"type":"media_projection"}`은 같은 not found 오류다.
  - TUI: session 행의 세 줄은 `media: state=unavailable reason=unavailable freshness=unavailable`, `media lineage: unavailable`, `media disclosure: unavailable`이다.
  - 외부 채널: `media_capability`·`media_delivery_status` metadata를 붙이지 않는다. 남은 기록 때문에 전송이 실패하지 않는다.
- **공개 Rust API:** `ContextBuilder`의 `with_video_analyzer`·`with_video_analyzer_staging_root`·`with_video_analyzer_owner_refs`·`with_video_media_provenance`·`with_video_projection_publication`, `VideoContextAnalyzer`와 그 요청·결과·오류·정책·metadata 타입, `AnalyzerInvocation`·`AnalyzerMediaProvenance`, video analyzer projection·owner fact·disclosure·Spec035 mapping, `Spec035MediaProjectionStore`, `ContextBuildRequest.analyzer_invocation`을 deprecation 기간 없이 제거한다. production 호출자·설정·명령이 없고, `shacs-core`는 문서화된 라이브러리 약속 없이 path dependency로만 쓰이는 0.1.0 workspace crate다. 외부 코드는 조용히 다르게 동작하지 않고 컴파일 단계에서 실패한다. turn의 취소·마감은 provider 호출과 도구 실행에 그대로 전달한다.
- **media evidence replay:** `media_evidence_replay` 모듈과 그 공개 함수·타입(`project_media_evidence_diagnostics`, `replay_recorded_media_evidence`, `MediaEvidence*`, `AnalyzerEvidenceSummary`, `ArtifactEvidenceSummary`, `RecordedAnalyzerStatus`, `RecordedArtifactStatus`, `MediaDisclosureSummary`)을 전부 제거한다. production 호출자가 없고 입력이 analyzer projection을 요구한다. 생성 artifact의 저장 형식(`artifacts/<artifact-id>/record.json`과 payload), `image_generate`, `runtime inspect`의 생성 이미지 목록은 그대로다.
- **release runner:** 같은 변경에서 034 runner의 `spec034_sequential_integration`과 22개 요구사항 catalog·`spec034_evidence_schema`, Spec031 runner의 `spec031-owner-spec034` 테스트 이름·audit fact·exploit 목록을 고친다. 034 쪽에서 근거가 사라지는 요구사항은 영상의 `034-MH008`, `034-MH009`, `034-AC007`, `034-AC008`, `034-AC011`과 media evidence 진단·replay의 `034-MH007`, `034-MH010`, `034-AC009`, `034-AC012`다. `034-AC006`의 금지 자료 검사는 남는 생성 artifact·edit·remote output 직렬화 결과로 다시 묶고 금지 항목 목록은 줄이지 않는다. 진단·analyzer·canonical 기록·표면 출력 입력은 빠진다. 다른 요구사항과 `034-AC010`의 문서 정책은 바뀌지 않는다. 없는 테스트 때문에 blocked가 되거나 근거가 사라진 요구사항을 현재 PASS로 보고하지 않아야 한다. 034의 과거 22/22 증거는 과거 기록으로 남기고 다시 해석하지 않는다. 041의 runner 판정과 043의 앱 퇴역은 이 변경의 선행 조건이 아니다.
- **문서:** `docs/USAGE.md`의 로컬 API 첨부 설명과 Spec 034 media 절, README의 Spec 034 media 문단, 027·034 문서에는 과거 완료 판정을 그대로 두고 퇴역 안내만 더한다. `docs/PRODUCT-PATH-MAP.md`와 스펙 색인도 맞춘다.

### 구현 때 필요한 회귀 테스트

1. 실제 MP4·MOV, EBML 헤더 WebM, 깨진 MP4를 API로 보내면 모델 입력에 고정 reason의 `unsupported` note가 하나씩 들어가고 `media/projections/`와 staging 폴더가 생기지 않는다.
2. 정상·손상 `media/projections/current.json`과 staging 폴더가 남은 fixture에서 `runtime inspect`와 `runtime diagnostics --bundle`이 종료 코드 0이고 위에 정한 줄과 빈 배열을 내며, `/v1/media/diagnostics`가 404 `media projection is unavailable`, WebSocket `media_projection`이 not found, TUI `--once`가 위에 정한 세 줄을 내고, 외부 채널 전송이 metadata 없이 성공한다. 남은 파일의 sha256은 그대로다.
3. 이미지·문서·오디오 routing 테스트(`file_context`의 audio·image·text 테스트)는 바뀌지 않고 통과한다.
4. 034 runner와 Spec031 runner 테스트가 제거된 영상·진단·replay 테스트 때문에 blocked가 되지 않고, 위 9개 요구사항(`034-MH007`–`034-MH010`, `034-AC007`–`034-AC009`, `034-AC011`, `034-AC012`)을 현재 PASS로 보고하지 않으며, `034-AC006`은 남는 생성 artifact·edit·remote 출력에서 금지 자료가 없음을 확인한다.
5. 지금 analyzer·분석 기록·replay 동작을 고정하는 테스트는 같은 변경에서 지우거나 바꾼다: `shacs-core/tests/it/`의 `spec034_video_analyzer*`, `spec034_media_projection_*`, `spec034_diagnostics_replay*`, `examples/spec034_diagnostics_replay_fixture.rs`, `tests/spec034_video_analyzer_owner_facts/`, `tests/runtime_agent.rs`의 영상 주입 테스트, `tests/spec034_sequential_integration.rs`와 그 example fixture의 analyzer·replay·표면 부분, `examples/spec034_video_*`, `file_context`·`video_route`·`video_analyzer_spec035`의 inline 영상 테스트. `shacs-cli/src/spec035_cli_media/tests.rs`, `shacs-api/tests/it/spec034_media_api*`, `shacs-tui/tests/it/spec034_media.rs`, `shacs-channels/examples/spec034_media_projection_fixture.rs`, `shacs-projection/tests/it/spec035_media_model.rs`의 분석 기록 부분.

### 후속 구현 범위

- 별도 구현 스펙 [044](../../044-video-analyzer-retirement/SPEC.md)가 위 계약 전체를 소유한다. 자료 이전은 없다. note·표면·공개 API·release runner·문서를 한 변경 단위로 바꾸고 analyzer와 분석 기록 전용 코드를 지운다. 이 구현은 다른 신규 기능이나 041의 판정, 043의 구현을 기다리지 않는다.
- 구현 전까지 현재 동작은 그대로다. 이번 작업에서 [사용 가이드의 Spec 034 media 절](../../../USAGE.md#spec-034-generated-media와-media-diagnostics)에 판정과 확인된 한계를 적었다.

### 남은 위험과 한계

- 실제 사용(모든 실제 data dir)과 `shacs-core`를 라이브러리로 쓰는 외부 코드는 미확인이다. 저장된 첨부와 남은 기록은 그대로 두고, 외부 코드는 컴파일 단계에서 실패하므로 자료를 조용히 잃거나 잘못 읽지 않는다.
- 실제 analyzer의 가치는 측정하지 않았다. 측정하려면 새 analyzer를 만들어야 한다. 실제 모델의 답 문구도 측정하지 않았지만 기본 경로의 모델 입력에는 영상 내용이 없었다.
- WebSocket `media_projection` 응답은 HTTP와 같은 함수를 쓰는 것을 코드로만 확인했다. turn마다의 overhead 시간은 측정하지 않았다.
- 오디오 analyzer도 production에 연결되어 있지 않다는 사실을 기록만 했다. 오디오 판정은 별도이며 이 결론으로 자동 결정하지 않는다.

## 종료 확인 (2026-10-24)

- 수용 조건 1: 생성자·production 호출자·설정·명령·plugin을 대조해 기본 analyzer 부재와 production 주입 경로 부재를 확인했다. 실제 binary로 저장 첨부부터 모델 입력·inspect·diagnostics·API·TUI·채널까지 기본 경로를 실행했고(s03–s21), 시험 주입은 저장소 밖 harness와 기존 테스트로 실행해 그 한계를 따로 적었다(h01–h09).
- 수용 조건 2: 정상·누락·비지원·크기 초과·길이 초과·추출 실패·잘림·시간 초과·취소·기록 저장 실패와 손상된 기록을 모델 입력과 표면별로 비교했다. 사용자에게 보이지 않는 채널 metadata를 전달 성공으로 보지 않았다. 이미지·오디오 경계를 확인했다(s18, s19).
- 수용 조건 3: 저장 자료·소비자·테스트·runner·문서 영향표, 조사 범위 안의 production 경로 부재와 미확인 범위(실제 사용 전체), 측정한 비용과 측정하지 않은 한계, 세 선택지의 계약과 하나의 결론(제거)을 기록했다.
- 문서만 바꿨으므로 Cargo 검증은 생략했다(AGENTS.md). 실행에 쓴 binary는 `cargo clean` 뒤 `cargo build --locked`로 만들었고, 기존 주입 테스트는 증거 수집용으로 대상만 골라 실행했다.
- 독립 검토(읽기 전용 Oracle): **PASS** (3차). 기록은 `.omo/evidence/spec040/prd000/review/gates.md`에 있다.
  - 1차는 BLOCKED였다. 실제 사용자 data dir 조회가 이 스펙의 불변식에 어긋나고 그 서술도 부정확했으며, TUI 계약이 하나로 정해지지 않았고, media evidence replay 처리를 구현에 넘겼으며, 사용 가이드가 남은 유효 기록의 표시를 빠뜨렸다는 지적이었다. 실제 사용자 자료를 근거에서 빼고 실제 사용을 미확인으로 고쳤고, 기록을 읽던 모든 표면을 기록이 없을 때의 결과로 고정했으며, replay 모듈 전체 제거를 확정하고 사용 가이드를 바로잡았다.
  - 2차는 BLOCKED였다. replay 제거로 근거가 사라지는 `034-MH007`·`034-AC009`와 `034-AC006`의 입력 변화가 빠졌고, "모든 영상" 표현이 11 MiB 거절(s12)과 모순된다는 지적이었다. 영향 목록과 회귀 테스트를 고치고 표현을 intake를 통과한 영상으로 좁혔다.
  - 3차는 수정 사항과 22개 receipt 전체를 다시 확인했고 차단·비차단 지적이 없었다.
