use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[test]
fn mcp_prompts_and_resources_list_empty_ok() {
    let exe = env!("CARGO_BIN_EXE_mcp_gateway");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp_gateway");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let frames = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"prompts/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"resources/list","params":{}}"#,
    ];
    for f in frames {
        writeln!(stdin, "{}", f).unwrap();
    }
    drop(stdin);

    let mut buf = String::new();
    stdout.read_to_string(&mut buf).unwrap();
    let lines: Vec<&str> = buf.lines().collect();
    assert!(
        lines.len() >= 3,
        "expected at least 3 responses, got {}",
        lines.len()
    );

    // prompts/list
    let prompts: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(prompts["jsonrpc"], "2.0");
    assert_eq!(prompts["id"], 2);
    assert!(prompts["result"]["prompts"].is_array());

    // resources/list
    let resources: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(resources["jsonrpc"], "2.0");
    assert_eq!(resources["id"], 3);
    assert!(resources["result"]["resources"].is_array());
}
