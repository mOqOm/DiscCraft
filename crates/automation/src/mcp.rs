//! DiscCraft's MCP tools (M9.61): the engine's commands for AI agents, served over stdio by craft-libs'
//! `craft-mcp-server` (the MCP machinery the Craft apps share: JSON-RPC 2.0 framing, the lifecycle and protocol
//! version choice, `tools/list` and `tools/call`, the check of a call's arguments against the tool's schema, and
//! bounded reading of request lines). DiscCraft gives it the three tools and their instructions.

use craft_mcp_server::{Effect, Info, Server, Tools, object, string, text_result, tool};
use disccraft_engine::{Session, commands};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// What every client is told at `initialize`.
const INSTRUCTIONS: &str = "DiscCraft is a disc authoring project editor. Every edit is an engine command; call disccraft_commands first to discover command ids and parameter descriptions, then call disccraft_execute with a command id and params. Numeric ids returned by commands identify assets, timelines, chapters, menus, and other project objects.";

/// The longest request line read (16 MiB): a longer one is refused without holding it in memory.
const MAX_LINE: usize = 16 << 20;

/// Who the server says it is.
const INFO: Info = Info {
    name: "disccraft",
    title: "DiscCraft",
    version: env!("CARGO_PKG_VERSION"),
    max_line: MAX_LINE,
};

/// MCP server that owns one engine session.
pub struct McpServer {
    server: Server<EngineTools>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self {
            server: Server::new(INFO, EngineTools::default()),
        }
    }
}

impl McpServer {
    /// Creates a server with an empty project session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one JSON-RPC line (a message or a batch) and returns one response line, or `None` when nothing is
    /// answered (notifications).
    #[must_use]
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        self.server.handle_line(line)
    }

    /// Serves JSON-RPC lines from `input` until EOF, writing one response line per request.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reading input or writing output fails.
    pub fn serve(&mut self, input: impl BufRead, output: impl Write) -> std::io::Result<()> {
        self.server.serve(input, output)
    }
}

/// The engine session behind the tools.
#[derive(Default)]
struct EngineTools {
    session: Session,
}

impl Tools for EngineTools {
    fn instructions(&self) -> String {
        INSTRUCTIONS.to_owned()
    }

    fn list(&mut self) -> Vec<Value> {
        vec![
            tool(
                "disccraft_commands",
                "List commands",
                "List all DiscCraft engine commands with ids, labels, menu paths, shortcuts, parameter descriptions, and journal flags.",
                object(json!({}), &[]),
                Effect::ReadOnly,
            ),
            tool(
                "disccraft_execute",
                "Execute a command",
                "Execute one DiscCraft engine command with optional params.",
                object(
                    json!({
                        "command": string("The command id, as disccraft_commands lists it (for example timeline.addChapter)."),
                        "params": {
                            "type": "object",
                            "description": "The command's parameters, as disccraft_commands describes them (left out: none).",
                        },
                    }),
                    &["command"],
                ),
                // A command may save, build, write a disc image or burn a disc.
                Effect::Files,
            ),
            tool(
                "disccraft_inspect",
                "Inspect the project",
                "Inspect the current project and run project validation.",
                object(json!({}), &[]),
                Effect::ReadOnly,
            ),
        ]
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        match name {
            "disccraft_commands" => success(&command_list()),
            "disccraft_inspect" => inspect_tool(&mut self.session),
            "disccraft_execute" => execute_tool(&mut self.session, args),
            _ => text_result(format!("unknown tool: {name}"), true),
        }
    }
}

/// The command list returned by `disccraft_commands`.
fn command_list() -> Value {
    Value::Array(
        commands()
            .iter()
            .map(|spec| {
                json!({
                    "id": spec.id,
                    "label": spec.label,
                    "menu": spec.menu,
                    "shortcut": spec.shortcut,
                    "params": spec.params,
                    "journal": spec.journal,
                })
            })
            .collect(),
    )
}

/// Runs `disccraft_execute`: its arguments were checked against the tool's schema (a string `command`, `params` an
/// object or left out).
fn execute_tool(session: &mut Session, args: &Value) -> Value {
    let Some(command) = args.get("command").and_then(Value::as_str) else {
        return text_result("command required", true);
    };
    let params = args.get("params").cloned().unwrap_or(Value::Null);
    match session.execute(command, &params) {
        Ok(value) => success(&value),
        Err(message) => text_result(message, true),
    }
}

/// Runs `disccraft_inspect`: the project's outline and its check.
fn inspect_tool(session: &mut Session) -> Value {
    let inspect = match session.execute("project.inspect", &Value::Null) {
        Ok(value) => value,
        Err(message) => return text_result(message, true),
    };
    let check = match session.execute("project.check", &Value::Null) {
        Ok(value) => value,
        Err(message) => return text_result(message, true),
    };
    success(&json!({ "inspect": inspect, "check": check }))
}

/// A successful tool result: `value` as pretty JSON text, flagged `isError: false` as DiscCraft's results always
/// were.
fn success(value: &Value) -> Value {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            let mut result = text_result(text, false);
            if let Some(fields) = result.as_object_mut() {
                fields.insert("isError".to_owned(), Value::Bool(false));
            }
            result
        }
        Err(error) => text_result(error.to_string(), true),
    }
}
