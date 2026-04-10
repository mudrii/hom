use serde_json::Value;

pub fn tool_list() -> Value {
    serde_json::json!({ "tools": [] })
}

pub fn server_capabilities() -> Value {
    serde_json::json!({})
}

pub fn server_info() -> Value {
    serde_json::json!({})
}
