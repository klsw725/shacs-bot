use super::*;

fn words(tokens: &[ShellToken]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|token| match token {
            ShellToken::Word(word) => Some(word.text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn quotes_and_escapes_become_plain_words_and_operators_split_commands() -> Result<(), Unanalyzable>
{
    // Given
    let command = "git commit -m 'a b' && echo \"x \\\"y\\\"\" | tee out\\ log";

    // When
    let tokens = tokenize(command)?;

    // Then
    assert_eq!(
        words(&tokens),
        ["git", "commit", "-m", "a b", "echo", "x \"y\"", "tee", "out log"]
    );
    assert!(tokens.contains(&ShellToken::Separator(Separator::And)));
    assert!(tokens.contains(&ShellToken::Separator(Separator::Pipe)));
    Ok(())
}

#[test]
fn redirects_keep_file_targets_and_drop_descriptor_duplication() -> Result<(), Unanalyzable> {
    // Given
    let command = "cmd < in.txt > out.txt 2>&1 >> log &> all 2> err";

    // When
    let tokens = tokenize(command)?;

    // Then
    let targets = tokens
        .iter()
        .filter_map(|token| match token {
            ShellToken::Redirect { kind, target } => {
                Some((*kind, target.as_ref().map(|word| word.text.as_str())))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [
            (RedirectKind::Input, Some("in.txt")),
            (RedirectKind::Output, Some("out.txt")),
            (RedirectKind::Output, None),
            (RedirectKind::Output, Some("log")),
            (RedirectKind::Output, Some("all")),
            (RedirectKind::Output, Some("err")),
        ]
    );
    Ok(())
}

#[test]
fn here_doc_body_is_input_and_unquoted_bodies_with_substitution_are_rejected(
) -> Result<(), Unanalyzable> {
    // Given
    let quoted = "python3 - <<'EOF'\nprint(open('auth.json').read())\nEOF\necho done";
    let unquoted = "cat <<EOF\n$(rm -rf ~)\nEOF";

    // When
    let tokens = tokenize(quoted)?;

    // Then
    assert!(tokens.contains(&ShellToken::HereDoc {
        body: "print(open('auth.json').read())\n".to_owned()
    }));
    assert_eq!(words(&tokens), ["python3", "-", "echo", "done"]);
    assert_eq!(tokenize(unquoted), Err(Unanalyzable));
    Ok(())
}

#[test]
fn parameters_are_kept_in_text_and_removed_for_empty_substitution() -> Result<(), Unanalyzable> {
    // Given
    let command = "rm -rf \"$DIR\"/* \"${1}\"/ $HOME/x";

    // When
    let tokens = tokenize(command)?;

    // Then
    let parameter_words = tokens
        .iter()
        .filter_map(|token| match token {
            ShellToken::Word(word) if word.has_parameter => Some((
                word.text.as_str(),
                word.without_parameters.as_str(),
                word.has_positional,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        parameter_words,
        [
            ("$DIR/*", "/*", false),
            ("${1}/", "/", true),
            ("$HOME/x", "/x", false),
        ]
    );
    Ok(())
}

#[test]
fn syntax_the_reader_does_not_follow_is_unanalyzable() {
    for command in [
        "echo $(id)",
        "echo `id`",
        "(cd /tmp && ls)",
        "{ echo a; }",
        "diff <(ls a) <(ls b)",
        "npm test &&",
        "a && ; b",
        "case x in a) ;; esac",
        "echo 'open",
        "cat <<EOF\nno end",
        "echo $'\\x41'",
    ] {
        assert_eq!(tokenize(command), Err(Unanalyzable), "{command}");
    }
    assert_eq!(
        tokenize(&"a".repeat(MAX_COMMAND_CHARS + 1)),
        Err(Unanalyzable)
    );
}

#[test]
fn line_continuation_after_operator_and_comments_are_followed() -> Result<(), Unanalyzable> {
    // Given
    let command = "mkdir -p out &&\n  python3 run.py # fetch\nls";

    // When
    let tokens = tokenize(command)?;

    // Then
    assert_eq!(
        words(&tokens),
        ["mkdir", "-p", "out", "python3", "run.py", "ls"]
    );
    Ok(())
}
