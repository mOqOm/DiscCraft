use disccraft_automation::McpServer;
use serde_json::{Map, Value};
use std::error::Error;
use std::io::Cursor;

/// Builds a JSON object from key/value pairs.
fn object(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), value.clone());
    }

    Value::Object(map)
}

/// Builds a JSON-RPC request value.
fn request(id: Value, method: &str, params: Value) -> Value {
    object(&[
        ("jsonrpc", Value::String("2.0".to_string())),
        ("id", id),
        ("method", Value::String(method.to_string())),
        ("params", params),
    ])
}

/// Builds a JSON-RPC notification value.
fn notification(method: &str, params: Value) -> Value {
    object(&[
        ("jsonrpc", Value::String("2.0".to_string())),
        ("method", Value::String(method.to_string())),
        ("params", params),
    ])
}

/// Parses a response line.
fn parse_response(line: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(line)?)
}

/// Extracts the first text content from a tool result.
fn content_text(value: &Value) -> Result<&str, Box<dyn Error>> {
    match value
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
    {
        Some(text) => Ok(text),
        None => Err("missing content text".into()),
    }
}

/// Extracts the tool error flag.
fn is_error(value: &Value) -> Result<bool, Box<dyn Error>> {
    match value
        .get("result")
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
    {
        Some(flag) => Ok(flag),
        None => Err("missing isError".into()),
    }
}

/// Extracts a JSON-RPC error code.
fn error_code(value: &Value) -> Result<i64, Box<dyn Error>> {
    match value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
    {
        Some(code) => Ok(code),
        None => Err("missing error code".into()),
    }
}

/// Sends a request and parses the response.
fn send(server: &mut McpServer, line: &str) -> Result<Value, Box<dyn Error>> {
    let Some(response) = server.handle_line(line) else {
        return Err("expected response".into());
    };

    parse_response(&response)
}

#[test]
fn initialize_echoes_protocol_version() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = request(
        Value::from(1_u64),
        "initialize",
        object(&[("protocolVersion", Value::String("2024-11-05".to_string()))]),
    )
    .to_string();

    let value = send(&mut server, &line)?;
    assert_eq!(value.get("jsonrpc").and_then(Value::as_str), Some("2.0"));
    assert_eq!(value.get("id").and_then(Value::as_u64), Some(1));
    assert_eq!(
        value
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_str),
        Some("2024-11-05")
    );

    Ok(())
}

#[test]
fn tools_list_has_three_tools() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = request(Value::from(2_u64), "tools/list", object(&[])).to_string();
    let value = send(&mut server, &line)?;

    let Some(tools) = value
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
    else {
        return Err("missing tools".into());
    };

    assert_eq!(tools.len(), 3);

    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();

    assert_eq!(
        names,
        [
            "disccraft_commands",
            "disccraft_execute",
            "disccraft_inspect"
        ]
    );

    Ok(())
}

#[test]
fn commands_tool_contains_timeline_add_chapter() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let params = object(&[
        ("name", Value::String("disccraft_commands".to_string())),
        ("arguments", object(&[])),
    ]);
    let line = request(Value::from(3_u64), "tools/call", params).to_string();
    let value = send(&mut server, &line)?;

    let text = content_text(&value)?;
    assert!(text.contains("timeline.addChapter"));
    assert!(!is_error(&value)?);

    Ok(())
}

#[test]
fn execute_menu_create_tool_contains_menu() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let params = object(&[
        ("name", Value::String("disccraft_execute".to_string())),
        (
            "arguments",
            object(&[
                ("command", Value::String("menu.create".to_string())),
                (
                    "params",
                    object(&[("name", Value::String("menu".to_string()))]),
                ),
            ]),
        ),
    ]);
    let line = request(Value::from(4_u64), "tools/call", params).to_string();
    let value = send(&mut server, &line)?;

    let text = content_text(&value)?;
    assert!(text.to_lowercase().contains("menu"));
    assert!(!is_error(&value)?);

    Ok(())
}

#[test]
fn unknown_engine_command_is_tool_error() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let params = object(&[
        ("name", Value::String("disccraft_execute".to_string())),
        (
            "arguments",
            object(&[
                ("command", Value::String("bogus.command".to_string())),
                ("params", object(&[])),
            ]),
        ),
    ]);
    let line = request(Value::from(5_u64), "tools/call", params).to_string();
    let value = send(&mut server, &line)?;

    assert!(is_error(&value)?);
    let text = content_text(&value)?;
    assert!(text.contains("unknown command"));

    Ok(())
}

#[test]
fn unknown_method_returns_method_not_found() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = request(
        Value::String("abc".to_string()),
        "bogus/method",
        object(&[]),
    )
    .to_string();
    let value = send(&mut server, &line)?;

    assert_eq!(value.get("id").and_then(Value::as_str), Some("abc"));
    assert_eq!(error_code(&value)?, -32601);

    Ok(())
}

#[test]
fn garbage_returns_parse_error() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let value = send(&mut server, "not json")?;

    assert_eq!(value.get("id"), Some(&Value::Null));
    assert_eq!(error_code(&value)?, -32700);

    Ok(())
}

#[test]
fn notification_has_no_response() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = notification("notifications/initialized", object(&[])).to_string();

    assert!(server.handle_line(&line).is_none());

    Ok(())
}

#[test]
fn serve_writes_two_lines() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let first = request(Value::from(1_u64), "ping", object(&[])).to_string();
    let second = request(Value::from(2_u64), "ping", object(&[])).to_string();
    let input = format!("{first}\n{second}\n");
    let mut output = Cursor::new(Vec::new());

    server.serve(Cursor::new(input.into_bytes()), &mut output)?;

    let text = String::from_utf8(output.into_inner()).unwrap_or_default();
    assert_eq!(text.lines().count(), 2);

    Ok(())
}
