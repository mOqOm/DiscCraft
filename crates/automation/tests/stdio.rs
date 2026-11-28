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

/// M9.61: a protocol version the server does not know is answered with the newest it speaks.
#[test]
fn initialize_offers_its_newest_version_for_an_unknown_one() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = request(
        Value::from(1_u64),
        "initialize",
        object(&[("protocolVersion", Value::String("1999-01-01".to_string()))]),
    )
    .to_string();
    let value = send(&mut server, &line)?;
    let result = value.get("result").ok_or("missing result")?;
    assert_eq!(
        result.get("protocolVersion").and_then(Value::as_str),
        Some("2025-06-18")
    );
    assert_eq!(
        result
            .get("serverInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str),
        Some("disccraft")
    );
    assert!(
        result
            .get("instructions")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("disccraft_commands"))
    );
    Ok(())
}

/// M9.61: the tools say what they may do: listing and inspecting only read, executing may write files.
#[test]
fn tools_carry_their_effects() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let line = request(Value::from(2_u64), "tools/list", object(&[])).to_string();
    let value = send(&mut server, &line)?;
    let tools = value
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or("missing tools")?;
    let read_only = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|tool| tool.get("annotations"))
            .and_then(|annotations| annotations.get("readOnlyHint"))
            .and_then(Value::as_bool)
    };
    assert_eq!(read_only("disccraft_commands"), Some(true));
    assert_eq!(read_only("disccraft_inspect"), Some(true));
    assert_eq!(read_only("disccraft_execute"), Some(false));
    Ok(())
}

/// M9.61: a call whose arguments do not fit the tool's schema is refused with -32602 and runs nothing.
#[test]
fn arguments_are_checked_against_the_schema() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    for (id, arguments) in [
        (10_u64, object(&[("params", object(&[]))])),
        (11, object(&[("command", Value::from(5))])),
        (
            12,
            object(&[
                ("command", Value::String("menu.create".to_string())),
                ("params", Value::String("not an object".to_string())),
            ]),
        ),
        (
            13,
            object(&[
                ("command", Value::String("menu.create".to_string())),
                ("extra", Value::Bool(true)),
            ]),
        ),
    ] {
        let params = object(&[
            ("name", Value::String("disccraft_execute".to_string())),
            ("arguments", arguments),
        ]);
        let line = request(Value::from(id), "tools/call", params).to_string();
        let value = send(&mut server, &line)?;
        assert_eq!(error_code(&value)?, -32602, "{value}");
    }
    let params = object(&[("name", Value::String("bogus_tool".to_string()))]);
    let line = request(Value::from(14_u64), "tools/call", params).to_string();
    assert_eq!(error_code(&send(&mut server, &line)?)?, -32602);
    // Nothing ran: the project still has no menu, until a call that fits makes one.
    assert_eq!(menus(&mut server, 15)?, 0);
    let params = object(&[
        ("name", Value::String("disccraft_execute".to_string())),
        (
            "arguments",
            object(&[("command", Value::String("menu.create".to_string()))]),
        ),
    ]);
    let line = request(Value::from(16_u64), "tools/call", params).to_string();
    assert!(!is_error(&send(&mut server, &line)?)?);
    assert_eq!(menus(&mut server, 17)?, 1);
    Ok(())
}

/// The number of menus `disccraft_inspect` shows.
fn menus(server: &mut McpServer, id: u64) -> Result<usize, Box<dyn Error>> {
    let params = object(&[("name", Value::String("disccraft_inspect".to_string()))]);
    let line = request(Value::from(id), "tools/call", params).to_string();
    let value = send(server, &line)?;
    assert!(!is_error(&value)?);
    let inspected: Value = serde_json::from_str(content_text(&value)?)?;
    Ok(inspected
        .get("inspect")
        .and_then(|inspect| inspect.get("menus"))
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len))
}

/// M9.61: a message without `"jsonrpc": "2.0"` is refused; a batch gets one reply per request in it.
#[test]
fn envelopes_and_batches() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let bare = object(&[
        ("id", Value::from(1_u64)),
        ("method", Value::String("ping".to_string())),
    ])
    .to_string();
    assert_eq!(error_code(&send(&mut server, &bare)?)?, -32600);

    let batch = Value::Array(vec![
        request(Value::from(2_u64), "ping", object(&[])),
        notification("notifications/initialized", object(&[])),
        request(Value::from(3_u64), "tools/list", object(&[])),
    ])
    .to_string();
    let value = send(&mut server, &batch)?;
    let replies = value.as_array().ok_or("a batch reply is an array")?;
    let ids: Vec<u64> = replies
        .iter()
        .filter_map(|reply| reply.get("id").and_then(Value::as_u64))
        .collect();
    assert_eq!(ids, [2, 3]);
    Ok(())
}

/// M9.61: a line longer than the server reads (16 MiB) is refused without being held, and serving goes on.
#[test]
fn an_overlong_line_is_refused_and_serving_goes_on() -> Result<(), Box<dyn Error>> {
    let mut server = McpServer::new();
    let long = format!("{{\"padding\":\"{}\"}}", "x".repeat(17 << 20));
    let ping = request(Value::from(7_u64), "ping", object(&[])).to_string();
    let input = format!("{long}\n{ping}\n");
    let mut output = Cursor::new(Vec::new());
    server.serve(Cursor::new(input.into_bytes()), &mut output)?;
    let text = String::from_utf8(output.into_inner())?;
    let lines: Vec<Value> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(lines.len(), 2);
    assert_eq!(error_code(&lines[0])?, -32600);
    assert_eq!(lines[1].get("id").and_then(Value::as_u64), Some(7));
    Ok(())
}
