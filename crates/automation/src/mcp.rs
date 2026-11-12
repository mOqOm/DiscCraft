use disccraft_engine::{Session, commands};
use serde_json::{Map, Value};
use std::io::{BufRead, Write};

const JSONRPC_VERSION: &str = "2.0";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const INSTRUCTIONS: &str = "DiscCraft is a disc authoring project editor. Every edit is an engine command; call disccraft_commands first to discover command ids and parameter descriptions, then call disccraft_execute with a command id and params. Numeric ids returned by commands identify assets, timelines, chapters, menus, and other project objects.";

/// MCP server that owns one engine session.
#[derive(Default)]
pub struct McpServer {
    session: Session,
}

impl McpServer {
    /// Creates a server with an empty project session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one JSON-RPC line and returns one response line, or `None` for notifications.
    #[must_use]
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let request = match serde_json::from_str::<Value>(trimmed) {
            Ok(request) => request,
            Err(_) => return Some(error_line(None, PARSE_ERROR, "parse error")),
        };

        if !request.is_object() {
            return Some(error_line(None, INVALID_REQUEST, "invalid request"));
        }

        let id = request.get("id").cloned();
        id.as_ref()?;

        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return Some(error_line(id, INVALID_REQUEST, "invalid request"));
        };

        let null = Value::Null;
        let params = request.get("params").unwrap_or(&null);

        match dispatch(&mut self.session, method, params) {
            Ok(result) => Some(success_line(id, result)),
            Err((code, message)) => Some(error_line(id, code, &message)),
        }
    }

    /// Serves JSON-RPC lines from `input` until EOF, writing one response line per request.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reading input or writing output fails.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if let Some(response) = self.handle_line(&line) {
                output.write_all(response.as_bytes())?;
                output.write_all(b"\n")?;
            }
            output.flush()?;
        }

        Ok(())
    }
}

/// Dispatches a JSON-RPC method to the corresponding MCP handler.
fn dispatch(session: &mut Session, method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(initialize_result(params)),
        "ping" => Ok(object(&[])),
        "tools/list" => Ok(tools_list()),
        "tools/call" => handle_tool_call(session, params),
        _ => Err((METHOD_NOT_FOUND, "method not found".to_string())),
    }
}

/// Builds the `initialize` result.
fn initialize_result(params: &Value) -> Value {
    let protocol_version = match params.get("protocolVersion").and_then(Value::as_str) {
        Some(version) => version.to_string(),
        None => DEFAULT_PROTOCOL_VERSION.to_string(),
    };

    let capabilities = object(&[("tools", object(&[]))]);
    let server_info = object(&[
        ("name", Value::String("disccraft".to_string())),
        (
            "version",
            Value::String(env!("CARGO_PKG_VERSION").to_string()),
        ),
    ]);

    object(&[
        ("protocolVersion", Value::String(protocol_version)),
        ("capabilities", capabilities),
        ("serverInfo", server_info),
        ("instructions", Value::String(INSTRUCTIONS.to_string())),
    ])
}

/// Builds a JSON-RPC success response line.
fn success_line(id: Option<Value>, result: Value) -> String {
    let null = Value::Null;
    let response = object(&[
        ("jsonrpc", Value::String(JSONRPC_VERSION.to_string())),
        ("id", id.unwrap_or(null)),
        ("result", result),
    ]);

    to_line(response)
}

/// Builds a JSON-RPC error response line.
fn error_line(id: Option<Value>, code: i64, message: &str) -> String {
    let null = Value::Null;
    let error = object(&[
        ("code", Value::from(code)),
        ("message", Value::String(message.to_string())),
    ]);

    let response = object(&[
        ("jsonrpc", Value::String(JSONRPC_VERSION.to_string())),
        ("id", id.unwrap_or(null)),
        ("error", error),
    ]);

    to_line(response)
}

/// Serializes a JSON value as a single line.
fn to_line(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Builds a JSON object from key/value pairs.
fn object(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), value.clone());
    }

    Value::Object(map)
}

/// Builds the MCP tool list result.
fn tools_list() -> Value {
    let tools = Value::Array(vec![
        tool(
            "disccraft_commands",
            "List all DiscCraft engine commands with ids, labels, menu paths, shortcuts, parameter descriptions, and journal flags.",
            empty_schema(),
        ),
        tool(
            "disccraft_execute",
            "Execute one DiscCraft engine command with optional params.",
            execute_schema(),
        ),
        tool(
            "disccraft_inspect",
            "Inspect the current project and run project validation.",
            empty_schema(),
        ),
    ]);

    object(&[("tools", tools)])
}

/// Builds one MCP tool descriptor.
fn tool(name: &str, description: &str, schema: Value) -> Value {
    object(&[
        ("name", Value::String(name.to_string())),
        ("description", Value::String(description.to_string())),
        ("inputSchema", schema),
    ])
}

/// Builds an empty JSON Schema object.
fn empty_schema() -> Value {
    object(&[
        ("type", Value::String("object".to_string())),
        ("properties", object(&[])),
        ("additionalProperties", Value::Bool(false)),
    ])
}

/// Builds the JSON Schema for `disccraft_execute`.
fn execute_schema() -> Value {
    let properties = object(&[
        (
            "command",
            object(&[("type", Value::String("string".to_string()))]),
        ),
        (
            "params",
            object(&[("type", Value::String("object".to_string()))]),
        ),
    ]);

    let required = Value::Array(vec![Value::String("command".to_string())]);

    object(&[
        ("type", Value::String("object".to_string())),
        ("properties", properties),
        ("required", required),
        ("additionalProperties", Value::Bool(false)),
    ])
}

/// Builds the command list returned by `disccraft_commands`.
fn command_list() -> Value {
    let mut items = Vec::new();

    for spec in commands() {
        let menu = Value::Array(
            spec.menu
                .iter()
                .map(|path| Value::String(path.to_string()))
                .collect(),
        );
        let shortcut = match spec.shortcut {
            Some(shortcut) => Value::String(shortcut.to_string()),
            None => Value::Null,
        };

        items.push(object(&[
            ("id", Value::String(spec.id.to_string())),
            ("label", Value::String(spec.label.to_string())),
            ("menu", menu),
            ("shortcut", shortcut),
            ("params", Value::String(spec.params.to_string())),
            ("journal", Value::Bool(spec.journal)),
        ]));
    }

    Value::Array(items)
}

/// Handles a `tools/call` request.
fn handle_tool_call(session: &mut Session, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (INVALID_PARAMS, "tool name required".to_string()))?;

    match name {
        "disccraft_commands" => {
            tool_result(&command_list()).map_err(|error| (INTERNAL_ERROR, error))
        }
        "disccraft_inspect" => inspect_tool(session),
        "disccraft_execute" => {
            let null = Value::Null;
            let arguments = params.get("arguments").unwrap_or(&null);
            execute_tool(session, arguments)
        }
        _ => Err((INVALID_PARAMS, format!("unknown tool: {name}"))),
    }
}

/// Executes the `disccraft_execute` tool.
fn execute_tool(session: &mut Session, arguments: &Value) -> Result<Value, (i64, String)> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| (INVALID_PARAMS, "command required".to_string()))?;

    let null = Value::Null;
    let params = match arguments.get("params") {
        Some(value) if value.is_object() || value.is_null() => value,
        Some(_) => return Err((INVALID_PARAMS, "params must be an object".to_string())),
        None => &null,
    };

    match session.execute(command, params) {
        Ok(value) => tool_result(&value).map_err(|error| (INTERNAL_ERROR, error)),
        Err(message) => Ok(tool_error(&message)),
    }
}

/// Executes the `disccraft_inspect` tool.
fn inspect_tool(session: &mut Session) -> Result<Value, (i64, String)> {
    let inspect = match session.execute("project.inspect", &Value::Null) {
        Ok(value) => value,
        Err(message) => return Ok(tool_error(&message)),
    };

    let check = match session.execute("project.check", &Value::Null) {
        Ok(value) => value,
        Err(message) => return Ok(tool_error(&message)),
    };

    let combined = object(&[("inspect", inspect), ("check", check)]);
    tool_result(&combined).map_err(|error| (INTERNAL_ERROR, error))
}

/// Wraps a successful tool result as MCP content.
fn tool_result(value: &Value) -> Result<Value, String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    Ok(tool_content(&text, false))
}

/// Wraps a tool error as MCP content.
fn tool_error(message: &str) -> Value {
    tool_content(message, true)
}

/// Builds MCP content with text and error flag.
fn tool_content(text: &str, is_error: bool) -> Value {
    let content = Value::Array(vec![object(&[
        ("type", Value::String("text".to_string())),
        ("text", Value::String(text.to_string())),
    ])]);

    object(&[("content", content), ("isError", Value::Bool(is_error))])
}
