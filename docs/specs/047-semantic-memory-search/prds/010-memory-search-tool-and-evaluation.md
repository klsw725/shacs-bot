# PRD 010. 검색 tool과 평가

Status: Complete (Scoped)

Depends on: [PRD 000](000-local-embedding-model-and-index.md) 완료 사실. 뒤 번호 PRD에 의존하지 않는다.

## Goal

agent와 사용자가 글자와 뜻을 함께 써서 기억을 찾게 하고, 그 검색이 지금의 grep보다 나은지 한국어 질문으로 잰다.

## Scope

1. **동점 순서(T):** 점수가 같은 항목은 `MEMORY.md` 항목(줄 시작 오름차순), history 항목(cursor 내림차순, 같은 cursor면 항목 순번 오름차순) 순으로 놓고, 그래도 같으면 key 오름차순으로 놓는다. 아래의 모든 순위와 상위 N개 자르기에 이 순서를 쓴다.
2. **질문 낱말:** 질문을 Unicode NFC로 맞추고 Unicode 소문자로 바꾼 뒤, Unicode 일반 범주 Z*(공백), P*(문장 부호), S*(기호), Cc(제어 문자)에 속한 글자를 경계로 나눈다. 빈 낱말을 버리고, 처음 나온 순서를 지키며 중복을 버리고, 앞에서 32개까지 쓴다. 남은 낱말이 없으면 검색하지 않고 `empty_query` 오류를 돌려준다. 이때도 뜻 검색 상태(`not_requested`)와, 검색 전에 실행한 맞추기의 색인 상태를 붙인다.
3. **글자 검색:** 항목 쪽은 PRD 000의 검색용 text(NFC·Unicode 소문자)를 쓰고, 결과에는 원래 text를 돌려준다. 3글자 이상 낱말은 각 낱말을 큰따옴표로 감싼 구(안의 `"`는 두 번 써서 escape)를 `OR`로 이은 FTS5 trigram `MATCH`로 찾고, 3글자보다 짧은 낱말은 검색용 text에 그 낱말이 들어 있는지 직접 비교해 찾는다. 후보는 두 방법으로 찾은 항목의 합집합이다. 순위는 검색용 text에 들어 있는 서로 다른 질문 낱말 수가 많은 순, 같으면 FTS5 `bm25`가 작은 순(FTS로 걸리지 않은 항목은 가장 뒤), 같으면 T 순서이며, 앞의 50개를 쓴다.
4. **뜻 검색:** 질문에 manifest의 질문 접두어를 붙여 임베딩하고, 유효한 벡터가 있는 모든 항목과 cosine(정규화한 벡터의 내적)을 비교한다. 순위는 cosine이 큰 순, 같으면 T 순서이며, 앞의 50개를 쓴다. 질문 벡터의 크기가 0이거나 NaN·무한대가 있거나 차원이 다르면 뜻 검색을 쓰지 않는다.
5. **합치기:** 항목마다 RRF 점수 Σ 1/(60 + 순위)를 글자·뜻 두 목록에 같은 무게로 더한다(순위는 1부터, 없는 목록은 0). 점수가 큰 순, 같으면 T 순서로 상위 k개(기본 5, 최대 20)를 낸다. 뜻 검색을 못 쓰면 글자 목록만으로 같은 규칙을 적용한다.
6. **결과:** 항목마다 key, 출처(파일과 cursor·timestamp 또는 줄 범위), text, 걸린 검색(글자·뜻·둘 다)을 붙인다. 결과 전체에 뜻 검색 상태(`available` 또는 이유: `disabled`·`model_missing`·`model_unverified`·`model_load_failed`·`embedding_failed`·`invalid_query_embedding`·`not_requested`)와 색인 상태(`complete`, 남은 벡터 수, 벡터 실패 수, 건너뛴 원본 줄 수, 또는 `index_unavailable`)를 붙인다. 결과가 tool 결과 한도를 넘으면 상태와 각 항목의 key·출처는 남기고, 뒤 순위 항목의 text부터 줄인다. 줄인 항목에는 `text_truncated`와 원래 글자 수를 붙인다.
7. **`memory_search` tool:** 위 검색을 agent tool로 등록한다. 입력은 질문과 개수, 출력은 위 결과다. 검색 전에 PRD 000의 원본 맞추기를 실행한다. 맞추기가 snapshot과 `index_unavailable`을 담은 실패 값을 돌려주면, 색인 없이 snapshot 항목에 같은 글자 규칙을 적용한다(검색용 text는 그 자리에서 만들고, FTS `bm25` 단계는 건너뛴다). 뜻 검색은 쓰지 않는다. 원본 읽기 실패면 그 이유를 결과로 돌려준다.
8. **skill과 config:** `memory` skill 안내를 "먼저 `memory_search`, 정확한 글자·시각이 필요하면 `grep`"으로 바꾼다. config `agents.defaults.memorySearch`는 `enable`(기본 true)과 `model`(기본 `dragonkue/multilingual-e5-small-ko-v2`, manifest에 있는 id만)을 가진다.
9. **CLI:** `shacs-bot memory search "<질문>" [--limit N] [--json]`이 tool과 같은 검색을 실행한다.
10. **평가 자료:** 허구의 일상·업무 기록으로 만든 한국어 fixture 기억(history 항목 수십 개와 `MEMORY.md`)과 질문 30개 이상을 `crates/shacs-core/tests/it/` 아래 테스트 자료로 둔다. 질문마다 id, 유형(조사·줄임말·다른 표현·두 글자 단어·날짜), 정답 항목의 출처(파일과 cursor 또는 줄)를 적는다.
11. **평가 방법:** 질문이 hit라는 것은 정답 항목 중 하나가 상위 3개 안에 있다는 뜻이다. 적중률은 hit 질문 수 / 질문 수이며 전체와 유형별로 낸다. grep 기준선은 같은 낱말 규칙과 검색용 text로, 질문 낱말을 그대로 포함한 서로 다른 낱말 수가 많은 순, 같으면 T 순서로 상위 3개를 고른다. 기본 모델로 `memory index`를 끝까지 실행한 뒤 두 방법을 잰다. 측정은 사용자의 실제 기억이 아닌 fixture로만 한다.
12. **사용자 문서:** `docs/USAGE.md`에 모델 설치와 색인, 사용 여부와 모델 바꾸기, 뜻 검색 불가·색인 미완료일 때의 결과, 기억 내용이 밖으로 나가지 않는다는 점과 한계(날짜 질문, 개인 규모 전제, 메모리 사용, arm64 Docker image는 FP16 확장(`FEAT_FP16`)을 실제로 갖춘 CPU에서만 돈다는 점, Linux arm64에서 직접 빌드할 때 필요한 `RUSTFLAGS`, Linux arm64에서 Spec 034 release runner를 지원하지 않는다는 점)를 쓴다. README의 기능 소개에 한 줄을 더한다.

## 구현 결정 (2026-11-02)

위 범위가 정하지 않은 부분을 구현 전에 정한다.

1. **결과 형식:** tool과 `memory search --json`은 같은 JSON을 낸다.
   - `results`: 순위대로 `rank`, `key`, `source`, `text`, `matched_by`(`text`·`semantic`·`both`)를 담는다. 줄인 항목에는 `text_truncated: {"original_chars": n}`을 붙인다.
     - `MEMORY.md` 항목의 `source`: `{"file": "memory/MEMORY.md", "first_line", "last_line"}`
     - history 항목의 `source`: `{"file": "memory/history.jsonl", "cursor", "timestamp", "item"}`
   - `semantic`: `{"status": "available"}` 또는 `{"status": "unavailable", "reason", "detail"}`.
   - `index`: 상태마다 담는 값이 다르다.
     - `{"status": "complete"|"incomplete", "vectors_remaining", "vectors_failed", "skipped_history_lines"}`. `complete`는 남은 벡터가 0이라는 뜻이다.
     - `{"status": "index_unavailable", "reason"}`. 원본을 읽었으면 `skipped_history_lines`를 더한다. 벡터 수는 알 수 없으므로 넣지 않는다.
   - `error`(있을 때만): `{"code": "empty_query"|"source_unreadable"|"result_limit_exceeded"|"search_unavailable", "detail"}`.
   - 모든 `detail`과 `reason`은 500자까지다.
2. **실패 처리:** 검색은 아래 순서로 진행하고, 단계마다 결과를 표대로 정한다. 뜻 검색 상태는 가장 먼저 걸린 이유 하나다. 범위 6의 이유에 `source_unreadable`, `index_unavailable`, `index_superseded`를 더한다.

   | 단계와 실패 | 글자 검색 | `semantic` | `index` | `error` |
   |---|---|---|---|---|
   | 원본 읽기 실패(`SourceUnreadable`) | 하지 않음 | `source_unreadable` | `index_unavailable` | `source_unreadable` |
   | 맞추기 실패(`IndexUnavailable`) | snapshot | `index_unavailable` | `index_unavailable` | 없음 |
   | 맞춘 뒤 빈 질문 | 하지 않음 | `not_requested` | 맞추기 결과 | `empty_query` |
   | config로 끔 | 색인 | `disabled` | 색인 상태 | 없음 |
   | 모델 불러오기 실패 | 색인 | `model_missing`·`model_unverified`·`model_load_failed` | 색인 상태 | 없음 |
   | 벡터 계산의 임베딩 실패 | 색인 | `embedding_failed` | 색인 상태 | 없음 |
   | 벡터 계산 중 다른 모델로 바뀜(`Superseded`) | 색인 | 아래 읽기 단계에서 정함 | 색인 상태 | 없음 |
   | 벡터 계산이나 색인 읽기의 SQLite·I/O·meta 실패 | 원본을 다시 읽은 snapshot | `index_unavailable` | `index_unavailable` | 없음 |
   | 읽은 snapshot의 meta 모델이 다름 | 색인 | `index_superseded` | 색인 상태 | 없음 |
   | 질문 임베딩 실패 | 색인 | `embedding_failed` | 색인 상태 | 없음 |
   | 질문 벡터의 크기 0·NaN·무한대·차원 다름 | 색인 | `invalid_query_embedding` | 색인 상태 | 없음 |

   - 다시 읽은 원본도 읽을 수 없으면 첫 줄과 같다.
   - 글자 검색과 뜻 검색은 한 read transaction의 색인 snapshot에서 읽는다. 색인 상태도 그 snapshot에서 읽는다.
3. **벡터 계산:** 검색마다 뜻 검색 전에 남은 벡터를 최대 256개(`VECTOR_BATCH_LIMIT`) 계산한다(SPEC 소유 범위 4). 실제 모델 측정에서 이 검색이 지나치게 느리면, 측정값을 근거로 검색당 개수를 줄인다.
4. **불러오기 실패의 이유:**
   - `model_missing`: 모델 파일이 없다.
   - `model_unverified`: 크기·sha256이 틀렸거나, symlink이거나, 읽을 수 없다.
   - `model_load_failed`: 확인한 파일로 모델을 만들지 못했다.
   - 불러온 모델은 process 안에서 공유한다(`LazyEmbedder`). runtime은 tool 목록을 만들 때 한 번 만든다.
5. **config 검증:**
   - `agents.defaults.memorySearch.model`이 manifest에 없으면, tool 목록을 만들 때와 `memory index`·`memory search`에서 알려진 id를 담은 config 오류로 거절한다.
   - `enable`이 false여도 tool은 등록되고, 뜻 검색만 `disabled`다.
   - 색인 meta의 모델은 `enable`과 상관없이 설정한 모델이다.
6. **결과 한도:** tool과 CLI는 직렬화한 결과 JSON의 Unicode 글자 수를 `agents.defaults.maxToolResultChars` 아래로 맞춘다. 0은 지금 runtime처럼 무제한이다.
   - 0이 아닌 값은 1,000 이상이어야 한다. 더 작으면 tool 목록을 만들 때와 `memory search`에서 config 오류로 거절한다.
   - 넘으면 범위 6대로 뒤 순위 항목의 text부터 줄인다.
   - 모든 text를 비워도 넘으면 줄인 결과를 낸다. 범위 6의 "각 항목의 key·출처는 남긴다"의 예외다.
     - `results`를 비운다.
     - `semantic`은 `status`와 `reason`만 남긴다.
     - `index`는 상태와 벡터·건너뛴 줄 수만 남기고, `index_unavailable`의 `reason`은 `"omitted"`로 바꾼다.
     - `error`는 `{"code": "result_limit_exceeded", "detail": "result exceeds maxToolResultChars"}`다.
     - 자유 글이 없으므로 이 JSON은 1,000자 안에 들어간다.
   - 시험은 다음을 확인한다: 한도 0, 1,000 미만의 config 오류, 정확히 1,000, 결과 길이가 한도와 같을 때와 한 글자 넘을 때, JSON escape가 많은 text.
7. **동시 실행:**
   - tool은 사용자 파일을 고치지 않으므로 `read_only`다. 다만 `concurrency_safe`는 false라서 같은 turn의 다른 tool과 병렬로 돌지 않는다.
   - tool이 가진 잠금 하나로 같은 process의 검색(맞추기부터 벡터 계산, 읽기, 질문 임베딩까지)을 차례로 돌린다. 앞선 검색이 panic해 잠금이 독이 되었으면 검색하지 않는다. 결과는 빈 `results`, `semantic: index_unavailable`, `index: index_unavailable`, `error: search_unavailable`이다.
   - 다른 process와의 경쟁은 SQLite와 `index_superseded`가 다룬다.
8. **동점 순서 T:** `MEMORY.md` 항목이 history 항목보다 앞이다. `MEMORY.md`끼리는 첫 줄 오름차순, history끼리는 cursor 내림차순, 같은 cursor면 항목 순번 오름차순, 그래도 같으면 key 오름차순이다.
9. **CLI:**
   - `memory index`는 설정한 모델을 쓴다. `enable`이 false면 글자 색인만 맞추고 성공한다.
   - `memory search`의 `--limit`은 1–20(기본 5)이다. `--json`이면 tool과 같은 JSON을 낸다. 아니면 같은 결과의 순위·출처·text·`matched_by`와 `semantic`·`index`·`error`를 사람이 읽는 형식으로 모두 낸다.
10. **등록 위치:** 주 agent의 production tool 목록(`production_tool_registry`)에 등록한다. subagent 목록에는 넣지 않는다.
11. **평가 자료와 시험:**
    - fixture와 질문은 `crates/shacs-core/tests/it/spec047_memory_search/` 아래에 둔다.
    - 가짜 임베딩 시험이 평가 계산(적중률, grep 기준선)과 AC 1·2를 덮는다. grep 기준선은 같은 낱말 규칙, 검색용 text, T 순서를 쓰는 시험 코드다.
12. **실제 모델 측정(AC 3·4):** `#[ignore]` 시험 하나다.
    - 실행 명령: `SHACS_SPEC047_MODEL_DATA_DIR=<기본 모델을 설치한 data directory> SHACS_SPEC047_REPORT=<보고서 경로> cargo test --manifest-path crates/Cargo.toml --release --locked -p shacs-core --test it -q spec047_memory_search::real_model -- --ignored --exact --nocapture --test-threads=1`.
    - 절차:
      1. 임시 workspace에 fixture를 둔다. 그 모델 폴더만 임시 data directory로 복사하고, 원본은 그대로 둔다.
      2. 모델을 올리기 전 RSS를 잰다.
      3. 모델을 올린 뒤 RSS를 잰다. 둘 다 `ps -o rss= -p <pid>`의 KiB 값이다.
      4. 남은 벡터가 모두 있는 상태로 첫 검색 하나의 시간을 잰다(벡터 계산 포함, 남은 개수와 함께 기록한다).
      5. 벡터를 끝까지 채운다.
      6. 고정 질문(질문 파일의 `timing_query_id`)으로 warm-up 검색 1회를 버린다.
      7. 같은 질문을 10회 잰다. 중앙값은 정렬한 10개 중 5·6번째 값의 평균이다.
      8. 모든 질문을 두 방법으로 잰다.
    - 보고서 내용:
      - 모델 id·revision, fixture·질문 파일의 sha256, OS·arch·build profile
      - RSS 두 값과 차이, 첫 검색 시간과 그때 남은 벡터 수, 10개 원시 시간과 중앙값
      - 질문마다 두 방법의 상위 3개 key와 hit 여부, 전체·유형별 적중률
    - 보고서를 쓴 뒤 AC 3의 기준(전체는 기준선 이상, 다른 표현·줄임말은 기준선 초과)을 확인한다.
13. **낱말 경계:** Unicode 일반 범주는 이미 쓰는 `regex`의 `\p{Z}`·`\p{P}`·`\p{S}`·`\p{Cc}`로 판정한다. 새 의존성은 더하지 않는다(034 runner의 의존성 트리 크기).

## Non Scope

- 모델 실행·설치·색인·원본 맞추기([PRD 000](000-local-embedding-model-and-index.md)), 날짜 거르기, LLM 재순위, `MEMORY.md`를 context에 넣는 방식의 변경.

## Parent Requirement Mapping

- [047 SPEC](../SPEC.md)의 소유 범위 5–7(설치·색인 CLI 제외)과 수용 조건 1·2·5, 이 PRD 변경에 대한 수용 조건 6을 소유한다.

## Required Contract

1. 같은 색인과 질문에는 언제나 같은 결과가 같은 순서로 나온다.
2. 결과의 text는 기억 파일에 있는 그대로이며, 출처로 원본 위치를 찾을 수 있다.
3. 뜻 검색을 못 써도, 질문 낱말을 그대로 포함한 항목은 글자 검색으로 찾는다.
4. 결과에는 언제나 뜻 검색 상태와 색인 상태가 들어 있다.
5. 검색은 기억 파일을 고치지 않는다.

## Acceptance Criteria

1. 가짜 임베딩으로 다음을 테스트한다: NFC·대소문자·문장 부호·기호가 섞인 질문의 낱말 나누기, NFD로 적힌 원본과 NFC 질문, 대소문자가 섞인 원본과 소문자 질문, 빈 질문의 `empty_query`와 함께 붙는 상태, 3글자 미만 낱말, 여러 낱말의 FTS 질의(따옴표 escape 포함), 글자 순위, 같은 벡터를 가진 항목들의 동점 처리, 50위 경계, RRF 합치기, 결과 개수 상한, 한도를 넘는 결과의 줄이기와 `text_truncated`.
2. 뜻 검색 불가의 각 이유(config로 끔, 모델 없음, 확인 실패, 불러오기 실패, 계산 실패, 잘못된 질문 벡터)에서 tool이 같은 글자 검색 결과와 그 이유를 돌려주고 turn이 정상으로 끝난다. 벡터가 덜 계산된 색인에서는 남은 벡터 수가 결과에 나온다. 맞추기에 실패한 검색은 snapshot 글자 검색 결과와 `index_unavailable`을 돌려준다.
3. 기본 모델로 잰 fixture 결과에서 이 검색의 상위 3개 적중률이 전체에서 grep 기준선보다 낮지 않고, 다른 표현·줄임말 유형에서는 더 높다. 넘지 못하면 결과와 원인을 기록하고 PRD를 닫지 않는다.
4. 기본 모델이 올라간 process의 메모리 증가량과, 색인이 끝난 상태에서 질문 하나의 검색 시간(10회 중앙값)을 기록한다.
5. `memory_search` tool과 `memory search` CLI가 같은 질문에 같은 결과를 낸다.
6. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다.

## Closure Evidence

1. 검색·tool·CLI·skill·config 구현과 테스트.
2. fixture, 질문별 결과와 유형별 적중률 표, 메모리·시간 기록(로컬 evidence).
3. 고친 사용자 문서와 Cargo 검증 기록.

PRD 010은 PRD 000의 닫힌 결과만 소비한다.

## 구현 메모 및 검증 근거 (2026-11-02)

- 구현:
  - `crates/shacs-core/src/memory_search/`:
    - `query_terms.rs`: 질문 낱말과 FTS 질의.
    - `search_ranking.rs`: 동점 순서 T, 글자·뜻 순위, RRF. RRF 점수는 정확한 분수로 비교한다.
    - `search_result.rs`: 결과 JSON, 처음 걸린 이유, 500자 상한, 한도 맞추기와 고정 결과.
    - `search_settings.rs`: 모델 id와 `maxToolResultChars` 확인.
    - `search.rs`의 `MemorySearch`: 구현 결정 2의 흐름과 실패 표, 잠금.
  - `memory_index.rs`에는 다음을 더했다.
    - 한 read transaction의 `search_view`.
    - 검색이 쓰는 엄격한 meta 읽기(`read_checked_model`, `checked_status`). 빠지거나 망가진 meta, 다른 schema·나누기 규칙 판은 `InvalidMeta`이고 결과는 `index_unavailable`이다. 온전한 다른 모델만 `index_superseded`다. PRD 000의 `status()`와 벡터 계산은 그대로다.
  - tool `memory_search`(`crates/shacs-core/src/tools/memory_search_tool.rs`). `production_tool_registry`에만 등록한다.
  - config `agents.defaults.memorySearch`, `ConfigError::Invalid`.
  - CLI `memory search`, 설정한 모델을 따르는 `memory index`.
  - `memory` skill 안내, `docs/USAGE.md`의 "기억 검색" 절, README 한 줄.
- 시험:
  - 가짜 임베딩 시험 35개(`crates/shacs-core/tests/it/spec047_memory_search/`): AC 1·2, 구현 결정 6의 시험 목록, 평가 계산, 실제 `AgentLoop` turn, 맞추기 직후 망가진 meta 7가지.
  - FTS 질의 단위 시험 1개.
  - CLI 시험 5개: tool과 CLI가 같은 결과(AC 5), 사람이 읽는 형식이 JSON의 모든 사실을 담음, 인자와 설정 거절, `enable=false`의 `memory index`, production 목록의 등록과 설정 거절.
  - subagent 목록에 없음.
  - 변이 시험 70개를 최종 코드와 최종 시험으로 돌려 모두 잡았다.
- 평가 자료: 허구의 일상·업무 기록(history 48개, `MEMORY.md` 항목 11개)과 질문 35개(조사·줄임말·다른 표현·두 글자 단어·날짜 각 7개). 측정 전에 세 파일의 sha256을 기록했고, 측정 뒤에 고치지 않았다.
- 실제 모델 측정(AC 3·4, 구현 결정 12의 명령, macOS aarch64 release, 기본 모델):

  | 유형 | 이 검색 | grep 기준선 |
  |---|---|---|
  | 전체 | 31/35 (88.6%) | 19/35 (54.3%) |
  | 조사 | 7/7 | 4/7 |
  | 줄임말 | 6/7 | 1/7 |
  | 다른 표현 | 4/7 | 2/7 |
  | 두 글자 단어 | 7/7 | 7/7 |
  | 날짜 | 7/7 | 5/7 |

  - 전체는 기준선 이상이고, 다른 표현·줄임말은 기준선보다 높다. 모든 질문에서 뜻 검색을 썼다.
  - 메모리: 모델을 올린 뒤 RSS가 1,316,960 KiB(약 1,286 MiB) 늘었다. 불러오기는 1.83 s였다.
  - 첫 검색: 남은 벡터 59개를 계산하는 것까지 480 ms였다.
  - 색인이 끝난 뒤 질문 하나: 10회 중앙값 25.6 ms(25.1–26.3 ms).
  - 구현 결정 3: 검색당 256개는 지나치게 느리지 않다고 보고 줄이지 않았다.
- 실행 기록은 로컬 `.omo/evidence/spec047/prd010/`(추적하지 않음)에 있고, `implementation.md`에 정리했다.

## 종료 확인 (2026-11-02)

- 범위: 범위 1–12와 수용 조건 1–6. 모델 실행·설치·색인·원본 맞추기는 [PRD 000](000-local-embedding-model-and-index.md)의 닫힌 결과를 썼다.
- 최종 검증(최종 소스):
  - Google 옵션을 붙인 `cargo fmt`와 workspace clippy `--all-targets -D warnings`가 통과했다.
  - 변경한 package(`shacs-config`·`shacs-core`·`shacs-cli`·`shacs-skills`)의 테스트가 통과했다.
  - Docker `rust:1.88-bookworm` linux/arm64 builder 단계의 `cargo build -p shacs-cli --release --locked`(arm64 flag)가 통과했다.
- 전체 workspace 테스트: 사용자 승인을 받아 `cargo test --workspace --locked -q`를 실행했다. exit 0, 23분이다(`closure/test-workspace.log`).
  - 걸러진 시험이 없는 결과 줄 188개: 3,384 passed / 0 failed / 8 ignored.
  - 테스트가 띄운 자식 process의 결과 줄 22개: 22 passed.
  - 결과 줄 없이 끝나는 자식 2개가 있다. 046의 충돌 시험이 일부러 도중에 끝내는 것이다.
- 034 runner: 다시 돌리지 않았다(사용자 동의). 이 PRD는 의존성을 더하지 않았고, 모든 cargo 명령을 `--locked`로 돌려 `Cargo.lock`도 바뀌지 않았다. runner의 의존성 트리는 PRD 000 종료 때와 같다.
- Oracle 독립 검증: 구현은 1차 BLOCKED(중대 1, 경미 3)였고, 고친 뒤 2차에 **PASS**를 받았다. 지적과 조치는 `implementation.md`에 있다.
- 남은 제한:
  - 평가는 허구의 작은 fixture로 했다. 다른 표현 질문 7개 중 3개는 상위 3개에 들지 못했다.
  - 시점으로 묻는 질문은 글에 날짜가 적혀 있을 때만 잘 찾는다(날짜 거르기 없음).
  - 벡터가 없는 항목이 많으면 첫 검색들이 한 번에 최대 256개를 계산하느라 몇 초 걸릴 수 있다.
  - `memory search`는 결과를 냈으면 `error`가 있어도 종료 코드 0이다.
  - 검색 도중 다른 process가 meta를 바꾸는 경쟁은 SQLite와 엄격한 meta 읽기로 다룬다. 그 밖의 의도적 변조는 막지 않는다.
