use super::*;

#[test]
fn the_third_block_in_a_row_escalates_and_an_allow_starts_the_row_over() {
    // Given
    let blocks = AutoModeBlocks::default();

    // When
    let first = blocks.blocked();
    let second = blocks.blocked();
    let allowed = blocks.allowed();
    let after_allow = blocks.blocked();
    blocks.blocked();
    let third_in_row = blocks.blocked();

    // Then
    assert_eq!(first.1, None);
    assert_eq!(second.1, None);
    assert_eq!(
        allowed,
        AutoModeBlockCounts {
            consecutive: 0,
            total: 2
        }
    );
    assert_eq!(after_allow.1, None);
    assert_eq!(
        third_in_row,
        (
            AutoModeBlockCounts {
                consecutive: 3,
                total: 5
            },
            Some(AutoModeEscalation::Consecutive)
        )
    );
}

#[test]
fn the_twentieth_block_escalates_and_starts_only_the_total_over() {
    // Given
    let blocks = AutoModeBlocks::new(AutoModeBlockCounts {
        consecutive: 2,
        total: 19,
    });

    // When
    let twentieth = blocks.blocked();
    let next = blocks.blocked();

    // Then
    assert_eq!(
        twentieth,
        (
            AutoModeBlockCounts {
                consecutive: 3,
                total: 0
            },
            Some(AutoModeEscalation::Total)
        )
    );
    assert_eq!(
        next,
        (
            AutoModeBlockCounts {
                consecutive: 4,
                total: 1
            },
            Some(AutoModeEscalation::Consecutive)
        )
    );
}

#[test]
fn a_row_past_the_limit_keeps_escalating_until_an_allow() {
    // Given
    let blocks = AutoModeBlocks::new(AutoModeBlockCounts {
        consecutive: 3,
        total: 3,
    });

    // When
    let again = blocks.blocked().1;
    blocks.allowed();
    let after_allow = blocks.blocked().1;

    // Then
    assert_eq!(again, Some(AutoModeEscalation::Consecutive));
    assert_eq!(after_allow, None);
}

#[test]
fn calls_not_run_are_remembered_for_one_dispatch() {
    // Given
    let blocks = AutoModeBlocks::default();
    blocks.mark_not_run("call_0");

    // When
    let during = blocks.did_not_run("call_0");
    blocks.begin_dispatch();
    let next_dispatch = blocks.did_not_run("call_0");

    // Then
    assert!(during);
    assert!(!next_dispatch);
    assert!(!blocks.did_not_run("call_1"));
}

#[test]
fn counts_read_back_from_the_session_shape() {
    // Given
    let stored = serde_json::json!({"consecutive": 2, "total": 7});

    // When
    let counts: AutoModeBlockCounts = serde_json::from_value(stored).unwrap_or_default();

    // Then
    assert_eq!(
        counts,
        AutoModeBlockCounts {
            consecutive: 2,
            total: 7
        }
    );
}

#[test]
fn an_id_reused_by_a_call_that_ran_gets_no_special_handling() {
    // Given
    let blocks = AutoModeBlocks::default();

    // When
    blocks.mark_not_run("call_0");
    blocks.mark_ran("call_0");

    // Then
    assert!(!blocks.did_not_run("call_0"));
}
