use super::*;
use crate::runtime::auto_mode::shell_command;

fn paths_of(command: &str) -> Vec<SubcommandPaths> {
    let analysis = shell_command::analyze(command, Path::new("/ws"), Path::new("/home/u"));
    analysis
        .subcommands
        .iter()
        .map(|subcommand| subcommand_paths(subcommand, Path::new("/home/u")))
        .collect()
}

fn resolved(words: &[PathWord]) -> Vec<(String, PathSource)> {
    words
        .iter()
        .flat_map(|word| {
            word.paths
                .iter()
                .map(|path| (path.display().to_string(), word.source))
        })
        .collect()
}

#[test]
fn writers_redirects_and_tee_are_writes_and_the_rest_are_reads() {
    // Given
    let command = "cp src/a.txt ~/b.txt && sort < in.txt > /tmp/out 2>/dev/null | tee log";

    // When
    let paths = paths_of(command);

    // Then
    assert_eq!(
        resolved(&paths[0].reads),
        [("/ws/src/a.txt".to_owned(), PathSource::Argument)]
    );
    assert_eq!(
        resolved(&paths[0].writes),
        [("/home/u/b.txt".to_owned(), PathSource::Argument)]
    );
    assert_eq!(
        resolved(&paths[1].reads),
        [("/ws/in.txt".to_owned(), PathSource::InputRedirect)]
    );
    assert_eq!(
        resolved(&paths[1].writes),
        [("/tmp/out".to_owned(), PathSource::OutputRedirect)]
    );
    assert_eq!(
        resolved(&paths[2].writes),
        [("/ws/log".to_owned(), PathSource::Tee)]
    );
}

#[test]
fn option_values_parameters_and_unknown_cd_bases_are_kept_for_later_checks() {
    // Given
    let command = "tool --config=~/.npmrc && rm -f \"$OUT\" && cd \"$D\" && cat notes";

    // When
    let paths = paths_of(command);

    // Then
    assert!(
        resolved(&paths[0].reads).contains(&("/home/u/.npmrc".to_owned(), PathSource::Argument))
    );
    assert!(paths[1].writes[0].word.has_parameter);
    assert!(paths[1].writes[0].paths.is_empty());
    assert!(paths[3].reads[0].unknown_base);
}
