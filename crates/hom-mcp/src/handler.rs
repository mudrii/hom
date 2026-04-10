use serde_json::Value;
use tokio::sync::mpsc;
use hom_core::types::McpRequest;

pub async fn handle_tool_call(
    _tool_name: &str,
    _args: &Value,
    _tx: &mpsc::Sender<McpRequest>,
) -> Result<Value, String> {
    Ok(Value::Null)
}
