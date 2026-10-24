use super::*;
use shacs_core::tools::{
    EditFileTool, GlobTool, GrepTool, ListDirTool, PathContext, ReadFileTool, WriteFileTool,
};

#[test]
#[cfg(unix)]
fn file_tools_reach_the_store_neither_by_walking_nor_under_another_name(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let store = dir.path().join(".shacs-methods/methods.json");
    std::fs::create_dir(dir.path().join(".shacs-methods"))?;
    std::fs::write(&store, "needle from the store\n")?;
    std::os::unix::fs::symlink(dir.path().join(".shacs-methods"), dir.path().join("alias"))?;
    std::fs::hard_link(&store, dir.path().join("copy.json"))?;
    let context = PathContext::workspace(dir.path());
    let mut tools = ToolRegistry::new();
    tools.register(ListDirTool::new(context.clone()));
    tools.register(GlobTool::new(context.clone()));
    tools.register(GrepTool::new(context.clone()));
    tools.register(ReadFileTool::new(context.clone()));
    tools.register(WriteFileTool::new(context.clone()));
    tools.register(EditFileTool::new(context));
    let run = |name: &str, arguments: Value| tools.execute(name, arguments).into_text();
    // When
    let walked = [
        run("list_dir", json!({"path": ".", "recursive": true})),
        run("glob", json!({"pattern": "**/*"})),
        run(
            "grep",
            json!({"pattern": "needle", "output_mode": "content"}),
        ),
    ];
    let aliased = [
        run("read_file", json!({"path": "alias/methods.json"})),
        run("list_dir", json!({"path": "alias"})),
        run("grep", json!({"pattern": "needle", "path": "alias"})),
        run("read_file", json!({"path": "copy.json"})),
        run(
            "write_file",
            json!({"path": "copy.json", "content": "forged"}),
        ),
        run(
            "edit_file",
            json!({"path": "copy.json", "op": "delete_line", "line_tag": "L1#00000000"}),
        ),
    ];
    // Then
    for output in &walked {
        assert!(
            !output.contains("methods.json") && !output.contains("needle from"),
            "{output}"
        );
    }
    for output in &aliased {
        assert!(
            output.contains("leads into the learned-method store"),
            "{output}"
        );
    }
    assert_eq!(std::fs::read_to_string(&store)?, "needle from the store\n");
    Ok(())
}
