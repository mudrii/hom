use serde_json::{json, Value};

/// Returns the list of tools exposed by the HOM MCP server.
/// Each entry matches the MCP tools/list response format.
pub fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "spawn_pane",
                "description": "Spawn a new pane running a harness. Returns the pane_id.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "harness": {
                            "type": "string",
                            "description": "Harness name: claude, codex, gemini, pi, kimi, opencode, copilot",
                            "enum": ["claude", "codex", "gemini", "pi", "kimi", "opencode", "copilot"]
                        },
                        "model": {
                            "type": "string",
                            "description": "Optional model override (e.g. claude-opus-4-5)"
                        }
                    },
                    "required": ["harness"]
                }
            },
            {
                "name": "send_to_pane",
                "description": "Send text to a pane's stdin. Use to issue prompts or commands.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID from spawn_pane or list_panes" },
                        "text": { "type": "string", "description": "Text to send (newline appended automatically)" }
                    },
                    "required": ["pane_id", "text"]
                }
            },
            {
                "name": "run_workflow",
                "description": "Execute a YAML workflow file. Returns the workflow_id for tracking.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the workflow YAML file" },
                        "vars": {
                            "type": "object",
                            "description": "Template variables passed to the workflow (key-value pairs)",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "list_panes",
                "description": "List all open panes with their IDs, harness names, and status.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "get_pane_output",
                "description": "Read the last N lines of visible output from a pane.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID" },
                        "lines": {
                            "type": "integer",
                            "description": "Number of lines to return (default 20, max 200)",
                            "default": 20,
                            "minimum": 1,
                            "maximum": 200
                        }
                    },
                    "required": ["pane_id"]
                }
            },
            {
                "name": "kill_pane",
                "description": "Kill a pane and its harness process.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID to kill" }
                    },
                    "required": ["pane_id"]
                }
            }
        ]
    })
}

/// Returns the MCP capabilities advertised during initialize.
pub fn server_capabilities() -> Value {
    json!({
        "tools": { "listChanged": false }
    })
}

/// Returns the server info block sent in the initialize response.
pub fn server_info() -> Value {
    json!({
        "name": "hom",
        "version": env!("CARGO_PKG_VERSION")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_six_tools() {
        let list = tool_list();
        assert_eq!(list["tools"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn all_tools_have_required_fields() {
        let list = tool_list();
        for tool in list["tools"].as_array().unwrap() {
            assert!(tool["name"].is_string(), "tool missing name");
            assert!(tool["description"].is_string(), "tool missing description");
            assert!(tool["inputSchema"].is_object(), "tool missing inputSchema");
        }
    }

    #[test]
    fn spawn_pane_requires_harness() {
        let list = tool_list();
        let spawn = list["tools"].as_array().unwrap()
            .iter().find(|t| t["name"] == "spawn_pane").unwrap();
        let required = spawn["inputSchema"]["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "harness"));
    }
}
