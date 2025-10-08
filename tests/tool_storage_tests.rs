use mcp_registrar::models::tool::Tool;
use mcp_registrar::utils::tool_storage::{FileToolStorage, ToolStorage};
use std::path::PathBuf;
use tempfile::tempdir;

#[tokio::test]
async fn atomic_save_and_read_back() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tools.json");
    let storage = FileToolStorage::new(PathBuf::from(&path));
    storage.initialize().await.unwrap_or(());

    let tool = Tool::new(
        "t1".to_string(),
        "T1".to_string(),
        "d".to_string(),
        "0.1.0".to_string(),
        "s1".to_string(),
        vec!["test".into()],
        None,
        None,
    );

    storage.save_tool(tool.clone()).await.unwrap();
    let listed = storage.list_tools().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "t1");
}
