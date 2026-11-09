//! Spec 052 PRD 030 acceptance 1–3: the error text, the check on real
//! folders with a fake access answer, and which commands are checked.

use super::super::parse_cli_args;
use super::{data_folder, mode_allows, probe, read_groups, render, write_target, Denied, Ids};
use crate::provider_status_cli::CommandForm;
use std::error::Error;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::{fs, io};

type TestResult = Result<(), Box<dyn Error>>;

const RUNNING: Ids = Ids {
    uid: 1001,
    gid: 1001,
};
const ROOT: Ids = Ids { uid: 0, gid: 0 };
const OTHER: Ids = Ids {
    uid: 1000,
    gid: 1000,
};

fn denied(data_folder: &str, exists: bool, owner: Ids, mode: u32) -> Denied {
    let data_folder = PathBuf::from(data_folder);
    let asked = if exists {
        data_folder.clone()
    } else {
        data_folder
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    Denied {
        data_folder,
        asked,
        exists,
        link_target: None,
        owner,
        mode,
        mode_allows: false,
        default_folder: true,
    }
}

#[test]
fn the_message_names_the_folder_the_owners_and_a_fix_for_each_case() {
    let home = "/home/u/.shacs-bot";
    let host = CommandForm::Host;
    let container = CommandForm::OfficialContainer;
    let root_line = "A folder owned by root is usually one Docker made because it did not exist \
                     when a container mounted it.";
    let acl_line =
        "Its mode lets this user write, so an ACL or a security policy such as SELinux blocks it.";
    let chown = "fix: sudo chown -R 1001:1001 /home/u/.shacs-bot && chmod u+rwx /home/u/.shacs-bot";
    let mkdir = "fix: sudo mkdir -p /home/u/.shacs-bot && sudo chown 1001:1001 /home/u/.shacs-bot \
                 && chmod u+rwx /home/u/.shacs-bot";
    let script_creates = "fix: on the host, run ./scripts/docker-setup.sh in the shacs-bot \
                          repository; it creates ~/.shacs-bot as your user and sets SHACS_UID and \
                          SHACS_GID in the repository .env";
    let other_folder = Denied {
        default_folder: false,
        ..denied("/etc", true, ROOT, 0o755)
    };
    let missing_other_folder = Denied {
        default_folder: false,
        ..denied("/srv/shacs/data", false, ROOT, 0o755)
    };
    let link = Denied {
        link_target: Some(PathBuf::from("/mnt/data")),
        default_folder: false,
        ..denied(home, true, OTHER, 0o755)
    };
    let acl = Denied {
        mode_allows: true,
        ..denied(home, true, RUNNING, 0o755)
    };
    let cases: [(&str, Denied, CommandForm, &[&str]); 20] = [
        (
            "own folder without write",
            denied(home, true, RUNNING, 0o555),
            host,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 1001:1001 \
                 (mode 0555) and this command runs as 1001:1001",
                "fix: chmod u+rwx /home/u/.shacs-bot",
            ],
        ),
        (
            "root's folder",
            denied(home, true, ROOT, 0o755),
            host,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                root_line,
                chown,
            ],
        ),
        (
            "another user's folder",
            denied(home, true, OTHER, 0o755),
            host,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 1000:1000 \
                 (mode 0755) and this command runs as 1001:1001",
                chown,
            ],
        ),
        (
            "missing folder under root's folder",
            denied(home, false, ROOT, 0o755),
            host,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                mkdir,
            ],
        ),
        (
            "missing folder under another user's folder",
            denied(home, false, OTHER, 0o755),
            host,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 1000:1000 \
                 (mode 0755) and this command runs as 1001:1001",
                mkdir,
            ],
        ),
        (
            "missing folder under the user's own folder without write",
            denied(home, false, RUNNING, 0o555),
            host,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 1001:1001 \
                 (mode 0555) and this command runs as 1001:1001",
                mkdir,
            ],
        ),
        (
            "own folder in the container",
            denied(home, true, RUNNING, 0o555),
            container,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 1001:1001 \
                 (mode 0555) and this command runs as 1001:1001",
                "fix: on the host, run chmod u+rwx ~/.shacs-bot",
            ],
        ),
        (
            "root's folder in the container",
            denied(home, true, ROOT, 0o755),
            container,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                root_line,
                "fix: on the host, run sudo chown -R \"$(id -u):$(id -g)\" ~/.shacs-bot && chmod \
                 u+rwx ~/.shacs-bot, then ./scripts/docker-setup.sh in the shacs-bot repository; \
                 it sets SHACS_UID and SHACS_GID in the repository .env so the containers run as \
                 you",
            ],
        ),
        (
            "another user's folder in the container",
            denied(home, true, OTHER, 0o755),
            container,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 1000:1000 \
                 (mode 0755) and this command runs as 1001:1001",
                "fix: on the host, run ./scripts/docker-setup.sh in the shacs-bot repository as \
                 the user who owns ~/.shacs-bot (1000:1000), or set SHACS_UID=1000 and \
                 SHACS_GID=1000 in the repository .env; then run this again",
            ],
        ),
        (
            "missing folder under root's folder in the container",
            denied(home, false, ROOT, 0o755),
            container,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                script_creates,
            ],
        ),
        (
            "missing folder under another user's folder in the container",
            denied(home, false, OTHER, 0o755),
            container,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 1000:1000 \
                 (mode 0755) and this command runs as 1001:1001",
                script_creates,
            ],
        ),
        (
            "missing folder under the user's own folder in the container",
            denied(home, false, RUNNING, 0o555),
            container,
            &[
                "cannot create the data folder /home/u/.shacs-bot: /home/u belongs to 1001:1001 \
                 (mode 0555) and this command runs as 1001:1001",
                script_creates,
            ],
        ),
        (
            "mode already allows",
            acl.clone(),
            host,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 1001:1001 \
                 (mode 0755) and this command runs as 1001:1001",
                acl_line,
                "fix: check the folder's ACL (ls -le on macOS, getfacl on Linux) and its security \
                 labels",
            ],
        ),
        (
            "mode already allows in the container",
            Denied { owner: ROOT, ..acl },
            container,
            &[
                "cannot write to the data folder /home/u/.shacs-bot: it belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                acl_line,
                "fix: on the host, check the ACL and security labels of the folder mounted at \
                 /home/u/.shacs-bot; with SELinux, add :z to that volume in docker-compose.yml",
            ],
        ),
        (
            "another folder owned by root",
            other_folder.clone(),
            host,
            &[
                "cannot write to the data folder /etc: it belongs to 0:0 (mode 0755) and this \
                 command runs as 1001:1001",
                root_line,
                "fix: make /etc writable for 1001:1001, or use --config with a path in a folder \
                 you own (the default data folder is ~/.shacs-bot)",
            ],
        ),
        (
            "another folder in the container",
            other_folder,
            container,
            &[
                "cannot write to the data folder /etc: it belongs to 0:0 (mode 0755) and this \
                 command runs as 1001:1001",
                root_line,
                "fix: on the host, make the folder mounted at /etc writable for 1001:1001, or run \
                 the containers as its owner with SHACS_UID and SHACS_GID in the repository .env",
            ],
        ),
        (
            "another missing folder",
            missing_other_folder.clone(),
            host,
            &[
                "cannot create the data folder /srv/shacs/data: /srv/shacs belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                "fix: create /srv/shacs/data for 1001:1001, or use --config with a path in a \
                 folder you own (the default data folder is ~/.shacs-bot)",
            ],
        ),
        (
            "another missing folder in the container",
            missing_other_folder,
            container,
            &[
                "cannot create the data folder /srv/shacs/data: /srv/shacs belongs to 0:0 (mode \
                 0755) and this command runs as 1001:1001",
                "fix: use --config with a path in a folder this container can write, such as the \
                 mounted data folder",
            ],
        ),
        (
            "a link",
            link,
            host,
            &[
                "cannot write to the data folder /home/u/.shacs-bot, a link to /mnt/data: it \
                 belongs to 1000:1000 (mode 0755) and this command runs as 1001:1001",
                "fix: make /mnt/data writable for 1001:1001, or use --config with a path in a \
                 folder you own (the default data folder is ~/.shacs-bot)",
            ],
        ),
        (
            "quoted paths",
            denied("/home/o'neil x/.shacs-bot", true, OTHER, 0o755),
            host,
            &[
                "cannot write to the data folder /home/o'neil x/.shacs-bot: it belongs to \
                 1000:1000 (mode 0755) and this command runs as 1001:1001",
                r"fix: sudo chown -R 1001:1001 '/home/o'\''neil x/.shacs-bot' && chmod u+rwx '/home/o'\''neil x/.shacs-bot'",
            ],
        ),
    ];
    for (name, denied, form, expected) in cases {
        // When
        let message = render(&denied, RUNNING, form);

        // Then
        assert_eq!(message, expected.join("\n"), "{name}");
        if !denied.default_folder {
            assert!(
                !message.contains("chown") && !message.contains("sudo"),
                "{name}"
            );
        }
    }

    // A space alone is quoted too.
    let message = render(
        &denied("/home/a b/.shacs-bot", true, RUNNING, 0o555),
        RUNNING,
        host,
    );
    assert!(
        message.ends_with("fix: chmod u+rwx '/home/a b/.shacs-bot'"),
        "{message}"
    );
}

#[test]
fn the_mode_class_that_applies_decides_whether_the_mode_allows_writing() {
    // Given
    let owner = Ids { uid: 10, gid: 20 };
    let cases = [
        ("owner rwx", Ids { uid: 10, gid: 99 }, &[][..], 0o700, true),
        ("owner r-x", Ids { uid: 10, gid: 99 }, &[][..], 0o577, false),
        ("group rwx", Ids { uid: 11, gid: 20 }, &[][..], 0o070, true),
        ("group r-x", Ids { uid: 11, gid: 20 }, &[][..], 0o757, false),
        (
            "supplementary group",
            Ids { uid: 11, gid: 99 },
            &[20][..],
            0o070,
            true,
        ),
        ("other rwx", Ids { uid: 11, gid: 99 }, &[][..], 0o007, true),
        ("other r-x", Ids { uid: 11, gid: 99 }, &[][..], 0o775, false),
    ];
    for (name, running, groups, mode, allows) in cases {
        // When
        let answer = mode_allows(owner, mode, running, groups);

        // Then
        assert_eq!(answer, allows, "{name}");
    }
}

#[test]
fn the_group_list_is_read_again_when_it_grows_between_count_and_read() {
    let einval = || Err(io::Error::from_raw_os_error(libc::EINVAL));
    let fills = [
        (
            "an empty list that grew to two",
            &[0, 2][..],
            Some(vec![7, 8]),
        ),
        ("more groups than slots", &[1, 2], Some(vec![7, 8])),
        ("a list that keeps growing", &[1, 2, 3], None),
        ("an error other than EINVAL", &[2], None),
    ];
    for (name, counts, expected) in fills {
        // Given
        let mut counted = counts.iter().copied();
        let mut calls = 0;

        // When
        let groups = read_groups(
            || counted.next().ok_or_else(|| io::Error::other("no count")),
            |slots| {
                calls += 1;
                match name {
                    "an error other than EINVAL" => Err(io::Error::from_raw_os_error(libc::EPERM)),
                    "more groups than slots" if calls == 1 => Ok(slots.len() + 1),
                    "a list that keeps growing" => einval(),
                    _ if slots.len() < 2 => einval(),
                    _ => {
                        slots[..2].copy_from_slice(&[7, 8]);
                        Ok(2)
                    }
                }
            },
        );

        // Then
        assert_eq!(groups, expected, "{name}");
    }
}

/// Answers `EACCES` for the folders in `denied`, `EROFS` for those in
/// `read_only`, and allows the rest.
fn answers<'a>(
    denied: &'a [&'a Path],
    read_only: &'a [&'a Path],
) -> impl Fn(&Path) -> io::Result<()> + 'a {
    move |path| {
        if denied.contains(&path) {
            Err(io::Error::from_raw_os_error(libc::EACCES))
        } else if read_only.contains(&path) {
            Err(io::Error::from_raw_os_error(libc::EROFS))
        } else {
            Ok(())
        }
    }
}

fn owner_of(path: &Path) -> Result<Ids, Box<dyn Error>> {
    let metadata = fs::metadata(path)?;
    Ok(Ids {
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

#[test]
fn the_probe_asks_the_folder_or_the_first_existing_folder_above_it() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let parent = root.path().join("home");
    let data = parent.join(".shacs-bot");
    fs::create_dir_all(&data)?;
    fs::set_permissions(&data, fs::Permissions::from_mode(0o555))?;
    let running = owner_of(&data)?;
    let missing = parent.join(".missing");
    let deep = parent.join("a/b/.shacs-bot");
    let file = parent.join("file");
    fs::write(&file, "not a folder")?;
    let target = root.path().join("target");
    fs::create_dir(&target)?;
    let link = parent.join("link");
    std::os::unix::fs::symlink(&target, &link)?;
    let dangling = parent.join("dangling");
    std::os::unix::fs::symlink(root.path().join("nowhere"), &dangling)?;
    let other_default = root.path().join("default");

    let (data_path, parent_path) = (data.as_path(), parent.as_path());

    // When / Then: a writable folder passes.
    assert_eq!(probe(&data, &data, running, &[], &answers(&[], &[])), None);

    // A denied folder is reported with its owner and mode.
    let found = probe(&data, &data, running, &[], &answers(&[data_path], &[]))
        .ok_or("expected a denial")?;
    assert_eq!(
        found,
        Denied {
            data_folder: data.clone(),
            asked: data.clone(),
            exists: true,
            link_target: None,
            owner: running,
            mode: 0o555,
            mode_allows: false,
            default_folder: true,
        }
    );
    let elsewhere = probe(
        &data,
        &other_default,
        running,
        &[],
        &answers(&[data_path], &[]),
    )
    .ok_or("expected a denial")?;
    assert!(!elsewhere.default_folder);

    // A missing folder is reported through the first existing folder above.
    for folder in [&missing, &deep] {
        let found = probe(folder, folder, running, &[], &answers(&[parent_path], &[]))
            .ok_or("expected a denial")?;
        assert_eq!(
            (
                found.exists,
                found.asked.as_path(),
                found.data_folder.as_path()
            ),
            (false, parent_path, folder.as_path())
        );
    }

    // A file, a read-only answer and a dangling link pass.
    assert_eq!(
        probe(&file, &file, running, &[], &answers(&[file.as_path()], &[])),
        None
    );
    assert_eq!(
        probe(&data, &data, running, &[], &answers(&[], &[data_path])),
        None
    );
    assert_eq!(
        probe(
            &dangling,
            &dangling,
            running,
            &[],
            &answers(&[dangling.as_path(), parent_path], &[]),
        ),
        None
    );

    // A link to a folder is asked through and names its target.
    let found = probe(&link, &link, running, &[], &answers(&[link.as_path()], &[]))
        .ok_or("expected a denial")?;
    assert_eq!(found.link_target, Some(fs::canonicalize(&target)?));
    assert!(found.exists && !found.default_folder);

    // A relative path is reported as an absolute one.
    let relative = Path::new("spec052-prd030-missing/.shacs-bot");
    let current = std::env::current_dir()?;
    let found = probe(
        relative,
        relative,
        running,
        &[],
        &answers(&[current.as_path()], &[]),
    )
    .ok_or("expected a denial")?;
    assert_eq!(found.data_folder, current.join(relative));
    assert_eq!(found.asked, current);

    fs::set_permissions(&data, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[test]
fn a_relative_config_path_gives_the_runtime_data_folder_made_absolute() -> TestResult {
    // Given
    let current = std::env::current_dir()?;
    let cases = [
        ("config.json", current.clone()),
        ("nested/config.json", current.join("nested")),
        ("/srv/shacs/config.json", PathBuf::from("/srv/shacs")),
    ];
    for (config_path, expected) in cases {
        // When
        let folder = data_folder(Path::new(config_path));

        // Then
        assert_eq!(folder, expected, "{config_path}");
    }
    Ok(())
}

#[test]
fn only_commands_that_write_the_data_folder_are_checked() -> TestResult {
    let default = shacs_config::default_config_path();
    let recovery = [
        "--owner",
        "recovery",
        "--action",
        "recover",
        "--locator",
        "runtime:recovery",
    ];
    let mut checked: Vec<Vec<&str>> = [
        &["onboard"][..],
        &["onboard", "--wizard"],
        &["run"],
        &["serve"],
        &["api", "serve"],
        &["gateway"],
        &["web"],
        &["ask", "hi"],
        &["agent"],
        &["runtime", "start"],
        &["runtime", "stop"],
        &["runtime", "restart"],
        &["runtime", "recover"],
        &["runtime", "update", "--target-version", "1.0.0"],
        &["runtime", "migrate", "--apply"],
        &["runtime", "migrate", "--resume"],
        &["runtime", "config-migrate", "--apply"],
        &["runtime", "config-migrate", "--recover"],
        &["provider", "codex", "login"],
        &["provider", "codex", "import-token", "--token-stdin"],
        &["provider", "copilot", "import-token", "--token-stdin"],
        &["provider", "anthropic", "import-key", "--token-stdin"],
        &["channels", "connect", "discord", "--token-stdin"],
        &["channels", "connect", "discord", "--allow-anyone"],
        &["plugins", "enable", "demo"],
        &["plugins", "disable", "demo"],
        &["permissions", "revoke", "abc"],
        &["permissions", "auto-mode", "reset"],
        &["permissions", "auto-mode", "setup"],
        &["memory", "model", "install"],
        &["memory", "index"],
    ]
    .iter()
    .map(|args| args.to_vec())
    .collect();
    checked.push([&["tasks", "--json"][..], &recovery].concat());
    for args in checked {
        // When
        let command = parse_cli_args(args.clone())?;

        // Then
        assert_eq!(write_target(&command), Some(default.clone()), "{args:?}");
    }

    for (args, config) in [
        (
            &["onboard", "--config", "/x/config.json"][..],
            "/x/config.json",
        ),
        (&["--config", "/y/config.json", "run"], "/y/config.json"),
        (
            &[
                "runtime",
                "config-migrate",
                "--apply",
                "--config",
                "/z/config.json",
            ],
            "/z/config.json",
        ),
    ] {
        let command = parse_cli_args(args.to_vec())?;
        assert_eq!(
            write_target(&command),
            Some(PathBuf::from(config)),
            "{args:?}"
        );
    }

    let mut unchecked: Vec<Vec<&str>> = [
        &["provider", "status"][..],
        &["channels", "connect", "discord", "--status"],
        &["runtime", "config-migrate"],
        &["runtime", "migrate"],
        &["runtime", "inspect"],
        &["status"],
        &["channels", "list"],
        &["memory", "search", "q"],
        &["session", "create", "--session", "s"],
        &["plugins", "list"],
        &["permissions", "list"],
        &[
            "tasks",
            "--json",
            "--owner",
            "goal",
            "--action",
            "pause",
            "--locator",
            "goal:demo",
        ],
        &["--help"],
    ]
    .iter()
    .map(|args| args.to_vec())
    .collect();
    unchecked.push([&["tasks", "--json", "--workspace", "/w"][..], &recovery].concat());
    unchecked.push([&["tasks", "--json", "--data-dir", "/d"][..], &recovery].concat());
    for args in unchecked {
        let command = parse_cli_args(args.clone())?;
        assert_eq!(write_target(&command), None, "{args:?}");
    }
    Ok(())
}
