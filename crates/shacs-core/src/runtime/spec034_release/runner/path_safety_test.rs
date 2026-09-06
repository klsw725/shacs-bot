use super::*;

#[test]
fn host_path_detection_preserves_complete_web_transport_uris() {
    let urls = [
        "https://example.com/docs/reference?next=relative#section",
        "HTTP://example.com/a?next=docs/readme",
        "ws://example.com/socket?root=portable",
        "wss://example.com/socket#events",
        "http://localhost:8080/docs",
        "https://user:pass@example.com/docs",
        "https://user%20name@example.com/a",
        "ws://127.0.0.1/socket",
        "wss://[2001:db8::1]:443/socket",
    ];

    for url in urls {
        assert!(!contains_host_path(url), "web URI rejected: {url}");
    }
}

#[test]
fn host_path_detection_rejects_uri_payload_paths() {
    let urls = [
        "https://example.com/a?next=/Users/alice/private.txt",
        "HTTP://example.com/a?next=C:/Users/alice/private.txt",
        "ws://example.com/socket?root=/home/alice",
        "wss://example.com/socket?root=//server/share",
        "https://example.com/a#/private/tmp/private.txt",
        "https://example.com/a?next=%2FUsers%2Falice%2Fprivate.txt",
    ];

    for url in urls {
        assert!(contains_host_path(url), "URI host path accepted: {url}");
    }
}

#[test]
fn host_path_detection_uses_general_token_boundaries() {
    let paths = [
        "</Users/alice/private.txt",
        "|/home/alice/private.txt",
        "!/private/tmp/private.txt",
        "#/Users/alice/private.txt",
        "?/home/alice/private.txt",
        "@file:///private/tmp/private.txt",
        "@FiLe:///private/tmp/private.txt",
        "|//server/share/private.txt",
        "///Users/alice/private.txt",
        "////server/share/private.txt",
        "https://?/Users/alice/private.txt",
        "http://:80/C:/Users/alice/private.txt",
        "https://%@example.com/a?next=/Users/alice/private.txt",
        "https://%2@example.com/a?next=/Users/alice/private.txt",
        "https://%GG@example.com/a?next=/Users/alice/private.txt",
        "https://exa%mple.com/a?next=/Users/alice/private.txt",
        r"|\\server\share\private.txt",
        "unix:///private/tmp/private.txt",
        "custom:///Users/alice/private.txt",
        "\u{1b}[31m/Users/alice/private.txt",
        "\u{1b}[?25l/Users/alice/private.txt",
        "\u{1b}[>0c/home/alice/private.txt",
        "\u{1b}[1$z/Users/alice/private.txt",
        "\u{1b}[2 qC:\\Users\\alice\\private.txt",
        r"C:\Users\alice\private.txt",
    ];

    for path in paths {
        assert!(contains_host_path(path), "host path accepted: {path:?}");
    }
}

#[test]
fn host_path_detection_handles_unbounded_csi_parameters() {
    let over_sixty_four = format!("\u{1b}[{}l/Users/alice/private.txt", "?".repeat(80));
    let multi_kilobyte = format!("\u{1b}[{}lC:\\Users\\alice\\private.txt", ">".repeat(4_096));
    let malformed = format!("\u{1b}[{}/Users/alice/private.txt", "?".repeat(4_096));

    assert!(contains_host_path(&over_sixty_four));
    assert!(contains_host_path(&multi_kilobyte));
    assert!(contains_host_path(&malformed));
}

#[test]
fn host_path_detection_treats_escape_and_c1_controls_as_segment_boundaries() {
    let paths = [
        "\u{1b}]8;;https://example.com\u{1b}\\/Users/alice/private.txt",
        "\u{1b}]8;;https://example.com\u{1b}\\C:\\Users\\alice\\private.txt",
        "\u{1b}]8;;https://example.com\u{1b}\\\\\\server\\share\\private.txt",
        "\u{1b}]8;;https://example.com\u{7}/Users/alice/private.txt",
        "\u{1b}c/Users/alice/private.txt",
        "\u{9b}31m/Users/alice/private.txt",
        "\u{9d}8;;https://example.com\u{9c}/Users/alice/private.txt",
        "\u{1b}malformed/Users/alice/private.txt",
    ];

    for path in paths {
        assert!(contains_host_path(path), "host path accepted: {path:?}");
    }
}

#[test]
fn host_path_detection_preserves_control_state_across_sequence_spaces() {
    let paths = [
        "\u{1b}[2 q/Users/alice/private.txt",
        "\u{1b}[2 qC:\\Users\\alice\\private.txt",
        "\u{1b}[2 q\\\\server\\share\\private.txt",
        "\u{9b}2 q/Users/alice/private.txt",
        "\u{1b}malformed q/Users/alice/private.txt",
        "\u{1b}]8;;label\u{1b}\\\u{1b}[2 q/Users/alice/private.txt",
    ];

    for path in paths {
        assert!(contains_host_path(path), "host path accepted: {path:?}");
    }
    assert!(!contains_host_path("ordinary whitespace separated text"));
    assert!(contains_host_path("ordinary label /Users/alice/private.txt"));
    assert!(!contains_host_path("\u{1b}c\nlabel/Users/alice/private.txt"));
}

#[test]
fn host_path_detection_decodes_percent_encoding_for_validation() {
    let text = "%2FUsers%2Falice%2Fprivate.txt";

    assert_eq!(redact_host_paths(text), text);
    assert!(contains_host_path(text));
}

#[test]
fn host_path_detection_rejects_nested_posix_drive_and_unc_encoding() {
    for text in [
        "%252FUsers%252Falice%252Fprivate.txt",
        "%2543%253A%252FUsers%252Falice%252Fprivate.txt",
        "%255C%255Cserver%255Cshare%255Cprivate.txt",
        "%25255C%25255Cserver%25255Cshare",
    ] {
        assert!(contains_host_path(text), "nested host path accepted: {text}");
    }
}

#[test]
fn excessive_percent_layers_fail_closed() {
    assert!(checked_contains_host_path("%25252525252FUsers").is_err());
}

#[test]
fn artifact_scan_rejects_secret_shaped_prose() -> Result<(), Box<dyn std::error::Error>> {
    for payload in [
        "Authorization: Basic dXNlcjpwYXNz",
        "https://user:password@example.com/resource",
        "Cookie: session=opaque-value",
        "session: opaque-value",
        "token=opaque-value",
        "unknown secret: opaque-value",
    ] {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("command.stdout"), payload)?;
    let snapshot = ArtifactSnapshot::capture(&root.path().canonicalize()?)?;
        assert!(validate_snapshot(&snapshot).is_err(), "secret accepted: {payload}");
    }
    Ok(())
}

#[test]
fn json_path_detection_decodes_json_escapes() -> Result<(), Box<dyn std::error::Error>> {
    let text = r#"{"value":"\u002fUsers\u002falice\u002fprivate.txt"}"#;

    assert!(decoded_json_contains_host_path("artifact.json", text)?);
    Ok(())
}

#[test]
fn json_path_detection_decodes_percent_encoded_uri_payloads(
) -> Result<(), Box<dyn std::error::Error>> {
    let text = r#"{"value":"https://example.com/a?next=%2FUsers%2Falice"}"#;

    assert!(decoded_json_contains_host_path("artifact.json", text)?);
    Ok(())
}
