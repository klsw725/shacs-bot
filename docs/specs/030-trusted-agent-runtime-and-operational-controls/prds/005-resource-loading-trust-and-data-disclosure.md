# PRD 005. resource loading trust and data disclosure

Status: Planned

## Goal

Skill, extension, prompt, context, package를 trusted resource로 발견·로드하고 source·precedence·diagnostics와 데이터 노출 범위를 명확히 한다.

## Scope

1. Global, project, package, explicit resource discovery.
2. Canonical path, source kind, precedence, collision, parse error diagnostics.
3. Markdown skill metadata와 model-visible instruction loading.
4. Python skill package install과 kernel import.
5. TypeScript/JavaScript extension import와 process-local host API.
6. Package source install/update/pin status.
7. Session JSONL, logs, traces, tool output과 custom extension data disclosure.

## Activation and precedence

1. Executable resource는 explicit config 또는 trusted workspace assertion 없이 활성화하지 않는다.
2. 전체 precedence는 `explicit > project-configured > trusted-project-auto > user-configured > user-auto > package > builtin`이다.
3. 같은 precedence에서는 canonical source path의 byte-order가 빠른 resource를 선택하고 collision diagnostics에 winner와 loser를 모두 남긴다.
4. Markdown-only skill은 현재 005 baseline을 소비한다. Python skill과 in-process extension은 030 trusted executable resource다.
5. Command-backed manifest plugin은 025 baseline을 소비하며 in-process extension과 동일 package에서 충돌하면 explicit activation이 없는 한 command-backed baseline을 우선한다.

## Invariants

1. Project-local resource는 configured precedence에 따라 builtin/package resource보다 우선할 수 있다.
2. Resource hash는 identity/cache/provenance evidence이며 authorization proof가 아니다.
3. Python skill과 extension은 trusted executable code다.
4. Parse warning과 collision diagnostics는 malicious content detection이 아니다.
5. Session, log, trace에는 prompt, tool result, error, extension data의 raw content가 남을 수 있다.
6. Remote trace upload는 opt-in 상태와 전송 대상·크기·endpoint summary를 표시한다.
7. Auto-discovered executable resource는 trusted workspace assertion이 사라지면 다음 session activation에서 제외한다.

## Acceptance Criteria

1. Source precedence와 collision winner가 inspect output에 나타난다.
2. Malformed skill/extension은 path와 reason을 진단한다.
3. Python skill install/import와 extension load failure가 실제 runtime에서 검증된다.
4. Trusted-code disclosure가 install/load/inspect surface에 나타난다.
5. Trace opt-in/off와 export preview가 검증되고 raw-content 가능성이 문서화된다.
6. Resource diagnostics가 trust authorization이나 sandbox evidence로 오인되지 않는다.
7. 모든 source family와 동순위 collision fixture가 결정적 winner를 검증한다.
