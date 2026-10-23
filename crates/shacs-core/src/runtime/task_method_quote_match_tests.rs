use super::*;

fn quoted(file: &str, line: &str) -> bool {
    Folded::new(file).quotes(line)
}

#[test]
fn a_quote_is_whole_sentences_or_lines_of_the_file_word_for_word() {
    // Given / When / Then
    assert!(quoted("Revenue grew 10%.", "Revenue grew 10%."));
    assert!(
        quoted("Revenue grew 10%.", "Revenue grew 10%"),
        "a final . may go"
    );
    assert!(quoted("Revenue  grew\t10%.", "Revenue grew 10%."));
    assert!(quoted("Revenue grew. Costs fell.", "Costs fell."));
    assert!(quoted("\"Revenue grew 10%.\"", "- \"Revenue grew 10%.\""));
    assert!(quoted(
        "The artwork is blue\nin the photo.",
        "The artwork is blue in the photo."
    ));
    assert!(quoted(
        "- Revenue grew 10%\n- Costs fell",
        "Revenue grew 10%"
    ));
    assert!(quoted("# Notes\nRevenue grew 10%.", "Revenue grew 10%."));
    assert!(quoted("Is the artwork blue?", "Is the artwork blue?"));
    assert!(
        !quoted("Revenue grew 10%.", "revenue grew 10%."),
        "case counts"
    );
    assert!(
        !quoted("Revenue grew 10%.", "Revenue grew"),
        "not a whole sentence"
    );
    assert!(
        !quoted("Is the artwork blue?", "Is the artwork blue"),
        "a ? may not go"
    );
    assert!(!quoted(
        "The artwork is blue in the photo, not in the image.",
        "The artwork is blue"
    ));
    assert!(!quoted(
        "The artwork is blue in the photo, not in the image.",
        "not in the image."
    ));
    assert!(!quoted(
        "The artwork is blue\nin the photo.",
        "The artwork is blue"
    ));
    assert!(!quoted("U.S. revenue grew.", "revenue grew."));
    assert!(
        !quoted("U.S. revenue fell.", "U.S."),
        "an abbreviation ends no sentence"
    );
    assert!(!quoted("U.S. revenue fell.", "U.S"));
    assert!(quoted(
        "Sales rose in the U.S. Revenue fell.",
        "Revenue fell."
    ));
    assert!(!quoted("Revenue grew.", " ... "));
}

#[test]
fn a_quote_never_cuts_a_word_or_a_number() {
    // Given / When / Then
    assert!(!quoted("Revenue grew 100%.", "Revenue grew 10"));
    assert!(!quoted("Revenue grew 10%.", "Revenue grew 10"));
    assert!(!quoted("It is untrue.", "true."));
    assert!(!quoted("It fell to -10 degrees.", "10 degrees."));
    assert!(!quoted("-10 degrees.", "10 degrees."));
    assert!(!quoted("It is 3.5 m.", "It is 3."));
    assert!(!quoted("It is 3.5 m.", "It is 3"));
    assert!(!quoted("It is 1e5 m.", "It is 1"));
    assert!(!quoted("It costs 1,000 won.", "It costs 1"));
}
