# 21. Using DiscCraft from AI Agents (MCP)

[Contents](README.md) · Previous: [20. Command Line and Scripts](20-command-line.md#20-command-line-and-scripts) · Next: [22. Limits and Rules](22-limits.md#22-limits-and-rules)

> **In this chapter**
>
> - Use DiscCraft from AI agents

`disccraft-cli mcp` runs as an MCP server (JSON-RPC over standard input/output). There are three tools.

| Tool | Description |
|---|---|
| `disccraft_commands` | List of commands (ID, display name, menu, shortcut, parameters) |
| `disccraft_execute` | Execute one command with parameters |
| `disccraft_inspect` | Inspect the current project and check results |

The server runs on the MCP machinery the Craft apps share (craft-libs' craft-mcp-server). It speaks protocol versions
2025-06-18, 2025-03-26 and 2024-11-05, and answers a version it does not know with 2025-06-18. `disccraft_commands` and
`disccraft_inspect` are announced as read-only tools, `disccraft_execute` as one that may write files (saving, building
and writing discs are commands too). A tool's arguments must match its listing (`command` present, no unknown
arguments, `params` an object), or the call is an error.

In your MCP client, register `<repository path>/target/release/disccraft-cli` as the command with `mcp` as its
argument (connected over stdio).
