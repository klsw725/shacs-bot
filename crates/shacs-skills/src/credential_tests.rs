use super::*;
use shacs_config::EnvFile;
use std::error::Error;

fn entry(fields: &str) -> SkillRegistryEntry {
    entry_from_raw(
        format!("---\nname: original\n{fields}\n---\nReadable body."),
        "directory".to_owned(),
        SkillSourceKind::WorkspaceLocal,
        None,
    )
}

#[test]
fn credential_declarations_normalize_supported_structures() {
    let cases = [
        "prerequisites:\n  env_vars: [SECOND_KEY, 'FIRST_KEY']",
        "prerequisites.env_vars:\n  - \"SECOND_KEY\"\n  - FIRST_KEY",
        "prerequisites:\n  'env_vars': [\"SECOND_KEY\", 'FIRST_KEY',] # keys",
        "required_environment_variables:\n  - name: SECOND_KEY\n    prompt: Enter a key\n    help: See documentation\n    required_for: [terminal]\n  - name: FIRST_KEY",
        "requires.env: SECOND_KEY, FIRST_KEY",
        "requires:\n  env:\n    - SECOND_KEY\n    - FIRST_KEY",
        "metadata.nanobot.requires.env: [SECOND_KEY, FIRST_KEY]",
        "metadata:\n  openclaw:\n    requires:\n      env: [SECOND_KEY, FIRST_KEY]",
        "metadata: '{\"nanobot\":{\"requires\":{\"env\":\"SECOND_KEY\"}},\"openclaw\":{\"requires\":{\"env\":[\"FIRST_KEY\"]}}}'",
        "prerequisites:\n  env_vars: [FIRST_KEY]\nrequired_environment_variables:\n  - name: FIRST_KEY\nrequires.env: SECOND_KEY",
    ];
    for fields in cases {
        let skill = entry(fields);
        assert_eq!(skill.descriptor.name, "original", "{fields}");
        assert_eq!(
            skill.descriptor.credentials.required,
            ["FIRST_KEY", "SECOND_KEY"],
            "{fields}"
        );
        assert!(
            skill.descriptor.credentials.diagnostics.is_empty(),
            "{fields}"
        );
    }
    let unrelated = entry("env_vars: [IGNORED_KEY]\nmetadata:\n  other:\n    env: [IGNORED_KEY]");
    assert!(
        unrelated.descriptor.credentials.required.is_empty(),
        "unscoped names are not declarations"
    );
}

#[test]
fn unindented_credential_list_preserves_following_disabled_field() {
    let skill = entry("requires.env:\n- TOKEN\ndisabled: true");

    assert_eq!(skill.descriptor.credentials.required, ["TOKEN"]);
    assert!(
        skill.descriptor.credentials.diagnostics.is_empty(),
        "an unindented list followed by a sibling mapping is valid"
    );
    assert!(
        skill.descriptor.disabled,
        "the sibling disabled field must remain part of the skill mapping"
    );
}

#[test]
fn credential_declarations_read_five_builtin_sources() -> Result<(), Box<dyn Error>> {
    for (name, names) in [
        ("airtable", vec!["AIRTABLE_API_KEY"]),
        ("gif-search", vec!["TENOR_API_KEY"]),
        ("linear", vec!["LINEAR_API_KEY"]),
        ("notion", vec!["NOTION_API_KEY"]),
        (
            "teams-meeting-pipeline",
            vec![
                "MSGRAPH_CLIENT_ID",
                "MSGRAPH_CLIENT_SECRET",
                "MSGRAPH_TENANT_ID",
            ],
        ),
    ] {
        let raw = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(name)
                .join("SKILL.md"),
        )?;
        let skill = entry_from_raw(raw, name.to_owned(), SkillSourceKind::WorkspaceLocal, None);
        assert_eq!(skill.descriptor.credentials.required, names, "{name}");
        assert!(
            skill.descriptor.credentials.diagnostics.is_empty(),
            "{name}"
        );
        assert_eq!(skill.descriptor.disabled, name == "teams-meeting-pipeline");
    }
    Ok(())
}

#[test]
fn invalid_credentials_reject_entire_declaration_without_echoing_input() {
    for invalid in [
        "requires.env: [VALID_KEY, 'secret-canary-value']",
        "requires.env: [VALID_KEY, 123]",
        "requires.env: {name: secret-canary-value}",
        "requires.env: {install: secret-canary-value}",
        "required_environment_variables:\n  - prompt: secret-canary-value",
        "required_environment_variables:\n  - name: VALID_KEY\n    name: OTHER_KEY\n    help: secret-canary-value",
        "requires.env: VALID_KEY\nrequires.env: secret-canary-value",
        "requires:\n  env: VALID_KEY\nrequires:\n  env: secret-canary-value",
        "requires.env: &secret-canary-value [VALID_KEY]",
        "requires.env: *secret-canary-value",
        "requires.env: !!secret-canary-value VALID_KEY",
        "requires:\n  <<: *secret-canary-value\n  env: VALID_KEY",
        "metadata: '{\"nanobot\":{\"requires\":{\"env\":\"VALID_KEY\",\"env\":\"secret-canary-value\"}}}'",
        "prerequisites.env_vars: [VALID_KEY, 'secret-canary-value'",
        "requires:\n  env: VALID_KEY\n    - secret-canary-value",
        "required_environment_variables:\n  - name: VALID_KEY\n    required_for: [*secret-canary-value]",
    ] {
        let skill = entry(&format!("prerequisites.env_vars: [GOOD_KEY]\n{invalid}"));
        let resolution = resolve_skill_credentials(&skill, &[], SkillCredentialSources {
            process: &BTreeMap::<String, String>::new(),
            file: &EnvFile::default(),
            explicit: &BTreeMap::new(),
        });
        assert!(resolution.values().is_empty(), "{invalid}");
        assert!(!resolution.status.diagnostics.is_empty(), "{invalid}");
        assert!(!format!("{resolution:?}").contains("secret-canary-value"), "{invalid}");
        assert!(!skill.descriptor.requirements.join(" ").contains("secret-canary-value"), "{invalid}");
        assert!(!skill.descriptor.install_metadata.as_deref().unwrap_or_default().contains("secret-canary-value"), "invalid credentials must not become install metadata: {invalid}");
        assert_eq!(skill.status, SkillRegistryStatus::Active);
        assert!(skill.raw.as_deref().is_some_and(|raw| raw.ends_with("Readable body.")), "invalid declaration keeps the body");
    }
}

#[test]
fn credential_resolution_obeys_precedence_and_excludes_reserved_names() -> Result<(), Box<dyn Error>>
{
    let skill = entry("requires.env: [PROCESS, FALLBACK, ABSENT, EMPTY_FILE, EXPLICIT, EMPTY_OVERRIDE, SPACES, LITERAL, OPENAI_API_KEY, HOME, PATH, LANG, TERM]");
    let process = BTreeMap::from([
        ("PROCESS".to_owned(), "process-canary".to_owned()),
        ("FALLBACK".to_owned(), String::new()),
        ("EXPLICIT".to_owned(), "process-overridden".to_owned()),
        ("EMPTY_OVERRIDE".to_owned(), "process-hidden".to_owned()),
    ]);
    let file = EnvFile::parse(br#"{"PROCESS":"file-overridden","FALLBACK":"file-canary","EMPTY_FILE":"","EXPLICIT":"file-hidden","EMPTY_OVERRIDE":"file-hidden","SPACES":" ","LITERAL":"${NOT_EXPANDED}","OPENAI_API_KEY":"provider-canary","HOME":"wrong-home"}"#)
        .map_err(|error| error.to_string())?;
    let explicit = BTreeMap::from([
        ("EXPLICIT".to_owned(), "explicit-canary".to_owned()),
        ("EMPTY_OVERRIDE".to_owned(), String::new()),
    ]);
    let resolution = resolve_skill_credentials(
        &skill,
        &[],
        SkillCredentialSources {
            process: &process,
            file: &file,
            explicit: &explicit,
        },
    );
    assert_eq!(
        resolution.values(),
        &BTreeMap::from([
            ("PROCESS".to_owned(), "process-canary".to_owned()),
            ("FALLBACK".to_owned(), "file-canary".to_owned()),
            ("EXPLICIT".to_owned(), "explicit-canary".to_owned()),
            ("SPACES".to_owned(), " ".to_owned()),
            ("LITERAL".to_owned(), "${NOT_EXPANDED}".to_owned()),
            ("OPENAI_API_KEY".to_owned(), "provider-canary".to_owned()),
        ])
    );
    assert_eq!(
        resolution.status.missing,
        ["ABSENT", "EMPTY_FILE", "EMPTY_OVERRIDE"]
    );
    assert_eq!(resolution.status.overridden_empty, ["EMPTY_OVERRIDE"]);
    assert_eq!(resolution.status.reserved, ["HOME", "LANG", "PATH", "TERM"]);
    assert!(
        !format!("{resolution:?}").contains("canary"),
        "Debug only contains names"
    );
    Ok(())
}
