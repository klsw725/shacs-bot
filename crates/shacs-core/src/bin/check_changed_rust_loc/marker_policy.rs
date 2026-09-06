use std::path::Path;

const MAX_PURE_LOC: usize = 250;
const MAX_MARKED_SEMANTIC_ADDITIONS: usize = 5;

pub(super) struct MarkerPolicyInput<'a> {
    pub path: &'a Path,
    pub markers: &'a [&'a str],
    pub existed_at_base: bool,
    pub semantic_additions: usize,
    pub pure_loc: usize,
}

pub(super) fn marker_is_valid(input: MarkerPolicyInput<'_>) -> bool {
    match input.markers {
        [] => input.pure_loc <= MAX_PURE_LOC,
        [reason] => {
            input.existed_at_base
                && input.semantic_additions <= MAX_MARKED_SEMANTIC_ADDITIONS
                && approved_reason(input.path).is_some_and(|approved| approved == *reason)
        }
        [_, _, ..] => false,
    }
}

fn approved_reason(path: &Path) -> Option<&'static str> {
    APPROVED_MARKERS
        .iter()
        .find_map(|(approved_path, reason)| (path == Path::new(approved_path)).then_some(*reason))
}

const APPROVED_MARKERS: &[(&str, &str)] = &[
    (
        "crates/shacs-api/src/lib.rs",
        "preexisting API facade; new media behavior lives in focused media_api modules",
    ),
    (
        "crates/shacs-channels/src/lib.rs",
        "preexisting channel catalog; Spec034 diff only registers and re-exports the focused spec035 media adapter",
    ),
    (
        "crates/shacs-cli/src/lib.rs",
        "preexisting CLI composition root; Spec034 retains five typed module/field/diagnostics fixture hooks",
    ),
    (
        "crates/shacs-core/src/runtime/agent_loop.rs",
        "preexisting agent loop; Spec034 turn control is delegated through five facade hooks",
    ),
    (
        "crates/shacs-core/src/runtime/context.rs",
        "preexisting context facade; Spec034 video state and routing live in a focused child module",
    ),
    (
        "crates/shacs-core/src/runtime/file_context.rs",
        "preexisting file-context facade; Spec034 analyzer routing lives in bounded child modules",
    ),
    (
        "crates/shacs-core/src/runtime/loop_control.rs",
        "preexisting stream coalescer; Spec034 diff is one exhaustive media lifecycle integration arm",
    ),
    (
        "crates/shacs-core/src/runtime/mod.rs",
        "preexisting runtime API index; Spec034 declarations and re-exports expand from one focused module macro",
    ),
    (
        "crates/shacs-core/src/runtime/runner.rs",
        "preexisting agent runner; Spec034 delegates media result semantics through five focused module hooks",
    ),
    (
        "crates/shacs-core/src/runtime/subagent.rs",
        "preexisting subagent runtime; Spec034 diff is one ToolExecutionContext fixture field hook",
    ),
    (
        "crates/shacs-core/src/runtime/tool_execution.rs",
        "preexisting tool executor; Spec034 keeps two context fields and delegates provider invocation assembly",
    ),
    (
        "crates/shacs-core/tests/runtime.rs",
        "preexisting runtime integration suite; Spec034 diff is one test-fixture deadline field hook",
    ),
    (
        "crates/shacs-core/tests/runtime_agent.rs",
        "preexisting runtime-agent suite; Spec034 diff is three compile-contract media fixture hooks",
    ),
    (
        "crates/shacs-core/tests/runtime_loop.rs",
        "preexisting runtime-loop suite; Spec034 diff is one test-fixture deadline field hook",
    ),
    (
        "crates/shacs-providers/src/registry.rs",
        "preexisting provider catalog; Spec034 diff is one Codex image-generation capability index flag",
    ),
    (
        "crates/shacs-tui/src/view.rs",
        "preexisting TUI renderer; Spec034 diff is one focused media-view projection hook",
    ),
    (
        "crates/shacs-tui/tests/spec031_tui.rs",
        "preexisting TUI fixture suite; Spec034 diff is one media-view fixture field hook",
    ),
    (
        "crates/shacs-utils/src/attachments.rs",
        "preexisting attachment catalog; Spec034 diff is two canonical analyzer terminal-status variants",
    ),
];

#[cfg(test)]
mod tests {
    use super::{marker_is_valid, MarkerPolicyInput, APPROVED_MARKERS};
    use std::collections::BTreeSet;
    use std::path::Path;

    const PATH: &str = "crates/shacs-api/src/lib.rs";
    const REASON: &str =
        "preexisting API facade; new media behavior lives in focused media_api modules";

    fn input<'a>(markers: &'a [&'a str]) -> MarkerPolicyInput<'a> {
        MarkerPolicyInput {
            path: Path::new(PATH),
            markers,
            existed_at_base: true,
            semantic_additions: 4,
            pure_loc: 5_149,
        }
    }

    #[test]
    fn exact_approved_reason_passes_when_path_matches() {
        // Given
        let policy_input = input(&[REASON]);

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(valid);
    }

    #[test]
    fn arbitrary_reason_fails_when_path_is_approved() {
        // Given
        let policy_input = input(&["arbitrary but nonempty"]);

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn approved_reason_fails_when_path_is_not_approved() {
        // Given
        let policy_input = MarkerPolicyInput {
            path: Path::new("crates/shacs-core/src/generated_media/new.rs"),
            ..input(&[REASON])
        };

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn missing_reason_fails_when_marker_is_present() {
        // Given
        let policy_input = input(&[""]);

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn duplicate_marker_fails_when_both_reasons_are_exact() {
        // Given
        let policy_input = input(&[REASON, REASON]);

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn new_file_marker_fails_when_reason_is_exact() {
        // Given
        let policy_input = MarkerPolicyInput {
            existed_at_base: false,
            ..input(&[REASON])
        };

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn six_additions_fail_when_reason_is_exact() {
        // Given
        let policy_input = MarkerPolicyInput {
            semantic_additions: 6,
            ..input(&[REASON])
        };

        // When
        let valid = marker_is_valid(policy_input);

        // Then
        assert!(!valid);
    }

    #[test]
    fn approved_marker_table_contains_eighteen_unique_paths() {
        // Given
        let paths = APPROVED_MARKERS
            .iter()
            .map(|(path, _)| *path)
            .collect::<BTreeSet<_>>();

        // When
        let unique_path_count = paths.len();

        // Then
        assert_eq!(APPROVED_MARKERS.len(), 18);
        assert_eq!(unique_path_count, APPROVED_MARKERS.len());
    }
}
