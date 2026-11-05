use super::*;
use std::collections::BTreeMap;

const CANARY: &str = "canary-value-0451";

#[test]
fn names_must_be_upper_case_letters_digits_and_underscores() {
    for name in ["A", "_", "SHACS_X1", "_A_9"] {
        assert!(is_env_name(name), "{name}");
    }
    for name in ["", "a", "1A", "A-B", "A B", "Ä"] {
        assert!(!is_env_name(name), "{name}");
    }
}

#[test]
fn a_valid_file_reads_its_names_and_values() {
    // Given
    let bytes = br#"{"SHACS_B": "two", "SHACS_A": "", "_X": "x"}"#;

    // When
    let file = EnvFile::parse(bytes).expect("valid file");

    // Then
    assert_eq!(file.get("SHACS_B"), Some("two"));
    assert_eq!(file.get("SHACS_A"), Some(""));
    assert_eq!(
        file.entries().map(|(name, _)| name).collect::<Vec<_>>(),
        vec!["SHACS_A", "SHACS_B", "_X"]
    );
}

#[test]
fn each_malformed_shape_fails_without_the_file_text() {
    let cases: Vec<(Vec<u8>, EnvFileProblem)> = vec![
        (
            format!("{{\"A\": \"{CANARY}\", ").into_bytes(),
            EnvFileProblem::NotJson { line: 0, column: 0 },
        ),
        (
            format!("[\"{CANARY}\"]").into_bytes(),
            EnvFileProblem::NotObject,
        ),
        (
            format!("\"{CANARY}\"").into_bytes(),
            EnvFileProblem::NotObject,
        ),
        (
            format!("{{\"A\": \"x\", \"{CANARY}\": \"y\"}}").into_bytes(),
            EnvFileProblem::BadName { entry: 2 },
        ),
        (
            br#"{"A": 451}"#.to_vec(),
            EnvFileProblem::NotText { entry: 1 },
        ),
        (
            format!("{{\"A\": {{\"inner\": \"{CANARY}\"}}}}").into_bytes(),
            EnvFileProblem::NotText { entry: 1 },
        ),
        (
            format!("{{\"A\": \"{CANARY}\", \"A\": \"other\"}}").into_bytes(),
            EnvFileProblem::RepeatedName { entry: 2 },
        ),
    ];
    for (bytes, expected) in cases {
        // When
        let problem = EnvFile::parse(&bytes).expect_err("malformed file");

        // Then
        match expected {
            EnvFileProblem::NotJson { .. } => {
                assert!(
                    matches!(problem, EnvFileProblem::NotJson { .. }),
                    "{problem:?}"
                );
            }
            expected => assert_eq!(problem, expected),
        }
        let shown = ConfigError::EnvFile(problem).to_string();
        assert!(!shown.contains(CANARY), "{shown}");
        assert!(
            !shown.contains("inner") && !shown.contains("other"),
            "{shown}"
        );
    }
}

#[test]
fn debug_shows_names_but_not_values() {
    // Given
    let file = EnvFile::parse(format!("{{\"SHACS_TOKEN\": \"{CANARY}\"}}").as_bytes())
        .expect("valid file");

    // When
    let shown = format!("{file:?}");

    // Then
    assert!(shown.contains("SHACS_TOKEN"), "{shown}");
    assert!(!shown.contains(CANARY), "{shown}");
}

#[test]
fn a_non_empty_process_value_wins_then_the_file_then_an_empty_process_value() {
    // Given
    let file = EnvFile::parse(br#"{"BOTH": "file", "EMPTY": "file", "FILE": "file"}"#)
        .expect("valid file");
    let process = BTreeMap::from([
        ("BOTH".to_owned(), "process".to_owned()),
        ("EMPTY".to_owned(), String::new()),
        ("ONLY_EMPTY".to_owned(), String::new()),
    ]);
    let env = ConfigEnv {
        first: &process,
        file,
    };

    // When / Then
    assert_eq!(env.var("BOTH").as_deref(), Some("process"));
    assert_eq!(env.var("EMPTY").as_deref(), Some("file"));
    assert_eq!(env.var("FILE").as_deref(), Some("file"));
    assert_eq!(env.var("ONLY_EMPTY").as_deref(), Some(""));
    assert_eq!(env.var("MISSING"), None);
}

#[test]
fn the_file_is_written_back_in_name_order() {
    // Given
    let mut file = EnvFile::default();
    file.insert("SHACS_Z".to_owned(), "z".to_owned());
    file.insert("SHACS_A".to_owned(), "a \"quoted\"".to_owned());

    // When
    let bytes = file.to_bytes().expect("bytes");

    // Then
    assert_eq!(EnvFile::parse(&bytes).expect("round trip"), file);
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.find("SHACS_A") < text.find("SHACS_Z"), "{text}");
    assert!(text.ends_with("}\n"), "{text}");
}
