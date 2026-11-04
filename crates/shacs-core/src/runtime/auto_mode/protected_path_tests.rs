use super::*;

#[test]
fn protected_paths_cover_claude_code_names_and_shacs_bot_owned_files() {
    // Given
    let protected = ProtectedPaths::new(ProtectedPathsInput {
        workspace: Path::new("/ws"),
        data_folder: Path::new("/data"),
        config_file: Path::new("/data/config.json"),
        extra_skill_roots: &[PathBuf::from("/plugins/demo/skills")],
        extra_context_files: &[PathBuf::from("/ws/docs/RULES.md")],
        configured_targets: &["/ws/private".to_owned()],
    });

    // When / Then
    for path in [
        "/ws/.git/config",
        "/ws/a/.config/git/ignore",
        "/home/u/.zshrc",
        "/ws/pkg/.npmrc",
        "/ws/.mcp.json",
        "/data/config.json",
        "/data/runtime/state.json",
        "/data/plugins/demo/plugin.json",
        "/ws/.shacs-bot/plugins/x/plugin.toml",
        "/ws/sessions/cli_direct.jsonl",
        "/ws/AGENTS.md",
        "/ws/sub/CLAUDE.md",
        "/ws/docs/RULES.md",
        "/ws/skills/youtube/SKILL.md",
        "/ws/builtin_skills/cron/SKILL.md",
        "/plugins/demo/skills/x/SKILL.md",
        "/ws/workflows/daily.toml",
        "/ws/private/notes.txt",
    ] {
        assert!(protected.contains(Path::new(path)), "{path}");
    }
    for path in ["/ws/notes.md", "/ws/sub/SOUL.md", "/ws/out/report.txt"] {
        assert!(!protected.contains(Path::new(path)), "{path}");
    }
}
