use hom_core::types::{McpCommand, McpRequest};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

/// Parse the `arguments` field from a tools/call request and dispatch
/// to the app via the McpRequest channel.
///
/// Returns the JSON value to include in the MCP tool result.
pub async fn handle_tool_call(
    tool_name: &str,
    args: &Value,
    tx: &mpsc::Sender<McpRequest>,
) -> Result<Value, String> {
    let command = parse_command(tool_name, args)?;
    let (reply_tx, reply_rx) = oneshot::channel();
    let req = McpRequest {
        command,
        reply: reply_tx,
    };
    tx.send(req)
        .await
        .map_err(|_| "App channel closed".to_string())?;
    let response = reply_rx
        .await
        .map_err(|_| "App dropped reply channel".to_string())?;
    Ok(serde_json::to_value(response).unwrap_or(Value::Null))
}

fn parse_command(tool_name: &str, args: &Value) -> Result<McpCommand, String> {
    match tool_name {
        "spawn_pane" => {
            let harness = args["harness"]
                .as_str()
                .ok_or("spawn_pane: 'harness' argument is required and must be a string")?
                .to_string();
            let model = args["model"].as_str().map(|s| s.to_string());
            Ok(McpCommand::SpawnPane { harness, model })
        }
        "send_to_pane" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("send_to_pane: 'pane_id' required")?
                .to_string();
            let text = args["text"]
                .as_str()
                .ok_or("send_to_pane: 'text' required")?
                .to_string();
            Ok(McpCommand::SendToPane { pane_id, text })
        }
        "run_workflow" => {
            let path = args["path"]
                .as_str()
                .ok_or("run_workflow: 'path' required")?
                .to_string();
            let vars: HashMap<String, String> = args["vars"]
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            Ok(McpCommand::RunWorkflow { path, vars })
        }
        "list_panes" => Ok(McpCommand::ListPanes),
        "get_pane_output" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("get_pane_output: 'pane_id' required")?
                .to_string();
            let lines = args["lines"].as_u64().unwrap_or(20).min(200) as usize;
            Ok(McpCommand::GetPaneOutput { pane_id, lines })
        }
        "kill_pane" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("kill_pane: 'pane_id' required")?
                .to_string();
            Ok(McpCommand::KillPane { pane_id })
        }
        unknown => Err(format!("Unknown tool: {unknown}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_spawn_pane_with_model() {
        let args = json!({"harness": "claude", "model": "claude-opus-4-5"});
        let cmd = parse_command("spawn_pane", &args).unwrap();
        assert!(
            matches!(cmd, McpCommand::SpawnPane { ref harness, ref model }
            if harness == "claude" && model.as_deref() == Some("claude-opus-4-5"))
        );
    }

    #[test]
    fn parse_spawn_pane_without_model() {
        let args = json!({"harness": "codex"});
        let cmd = parse_command("spawn_pane", &args).unwrap();
        assert!(matches!(cmd, McpCommand::SpawnPane { ref model, .. } if model.is_none()));
    }

    #[test]
    fn parse_spawn_pane_missing_harness_returns_error() {
        let args = json!({});
        let err = parse_command("spawn_pane", &args).unwrap_err();
        assert!(err.contains("harness"));
    }

    #[test]
    fn parse_send_to_pane() {
        let args = json!({"pane_id": "p1", "text": "hello world"});
        let cmd = parse_command("send_to_pane", &args).unwrap();
        assert!(matches!(cmd, McpCommand::SendToPane { ref text, .. } if text == "hello world"));
    }

    #[test]
    fn parse_run_workflow_with_vars() {
        let args = json!({"path": "workflows/tdd.yaml", "vars": {"planner": "claude"}});
        let cmd = parse_command("run_workflow", &args).unwrap();
        if let McpCommand::RunWorkflow { path, vars } = cmd {
            assert_eq!(path, "workflows/tdd.yaml");
            assert_eq!(vars.get("planner").map(|s| s.as_str()), Some("claude"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn parse_get_pane_output_default_lines() {
        let args = json!({"pane_id": "p1"});
        let cmd = parse_command("get_pane_output", &args).unwrap();
        assert!(matches!(cmd, McpCommand::GetPaneOutput { lines: 20, .. }));
    }

    #[test]
    fn parse_get_pane_output_caps_at_200() {
        let args = json!({"pane_id": "p1", "lines": 999});
        let cmd = parse_command("get_pane_output", &args).unwrap();
        assert!(matches!(cmd, McpCommand::GetPaneOutput { lines: 200, .. }));
    }

    #[test]
    fn parse_unknown_tool_returns_error() {
        let err = parse_command("fly_to_moon", &json!({})).unwrap_err();
        assert!(err.contains("Unknown tool"));
    }
}
