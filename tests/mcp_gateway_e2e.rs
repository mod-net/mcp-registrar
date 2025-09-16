use std::io::{Write, Read};
use std::process::{Command, Stdio};

#[test]
fn mcp_initialize_list_call_echo() {
    // Spawn the compiled mcp_gateway binary
    let exe = env!("CARGO_BIN_EXE_mcp_gateway");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp_gateway");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // Send initialize, notification, tools/list, tools/call
    let frames = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.0.1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hello mcp"}}}"#,
    ];
    for f in frames {
        writeln!(stdin, "{}", f).unwrap();
    }
    drop(stdin);

    // Read three responses (initialize, list, call)
    let mut buf = String::new();
    stdout.read_to_string(&mut buf).unwrap();
    let lines: Vec<&str> = buf.lines().collect();
    assert!(lines.len() >= 3, "expected at least 3 responses, got {}", lines.len());

    let init: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["id"], 1);
    assert!(init["result"]["serverInfo"]["name"].is_string());
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");

    let list: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(list["jsonrpc"], "2.0");
    assert_eq!(list["id"], 2);
    assert!(list["result"]["tools"].is_array());

    let call: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(call["jsonrpc"], "2.0");
    assert_eq!(call["id"], 3);
    let content = call["result"]["content"].as_array().unwrap();
    assert_eq!(content[0]["text"], "hello mcp");
}

