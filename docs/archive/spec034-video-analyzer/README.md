# Retired Spec 034 video analyzer test source

`runtime_agent_video_injection_test.rs` preserves, byte for byte, lines 535–656 of `crates/shacs-core/tests/runtime_agent.rs` at commit `95254eb`: the `RuntimeVideoAnalyzer` test double and the `runtime_context_routes_stored_video_with_injected_analyzer` test. [Spec 044](../../specs/044-video-analyzer-retirement/SPEC.md) removed the video analyzer and this test.

Spec031 release runner results with schema `spec031.release_runner.v2` or `spec031.release_runner.v3` cite that test as the `spec034` external owner fact. When those results are read later, the runner checks the cited test name in this snapshot instead of the live test file. The snapshot digest is pinned in `crates/shacs-projection/src/spec031/release_runner/external_audit_facts.rs`; `SHA256SUMS` records the same digest.

This file is not compiled, is not an active test, and does not certify any historical execution. It only lets recorded results be read with the composition they were produced under.
