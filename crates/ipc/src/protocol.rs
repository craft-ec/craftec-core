//! JSON-RPC 2.0 protocol types

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    pub id: serde_json::Value,
}

impl RpcRequest {
    pub fn new(method: &str, params: Option<serde_json::Value>, id: impl Into<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: id.into(),
        }
    }
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: serde_json::Value,
}

impl RpcResponse {
    pub fn success(id: impl Into<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id: id.into(),
        }
    }

    pub fn error(id: impl Into<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(RpcError { code, message }),
            id: id.into(),
        }
    }
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Server-sent event notification (no id field — JSON-RPC 2.0 notification)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl RpcNotification {
    pub fn new(method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(serde_json::to_value(params).unwrap_or_default()),
        }
    }
}

/// Legacy event type (kept for backward compatibility with existing code).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEvent {
    pub event: String,
    pub data: serde_json::Value,
}

impl RpcEvent {
    pub fn new(event: &str, data: serde_json::Value) -> Self {
        Self {
            event: event.to_string(),
            data,
        }
    }
}

/// Serialize an event into a JSON-RPC 2.0 notification string.
pub fn event_to_notification(method: &str, data: &impl Serialize) -> String {
    let notification = RpcNotification {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params: Some(serde_json::to_value(data).unwrap_or_default()),
    };
    serde_json::to_string(&notification).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_serialization() {
        let req = RpcRequest::new("status", None, 1u64);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"status\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_rpc_request_string_id() {
        let req = RpcRequest::new("status", None, serde_json::Value::String("abc".into()));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"id\":\"abc\""));
    }

    #[test]
    fn test_rpc_request_null_id() {
        let req = RpcRequest::new("status", None, serde_json::Value::Null);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"id\":null"));
    }

    #[test]
    fn test_rpc_response_success() {
        let resp = RpcResponse::success(1u64, serde_json::json!({"state": "ready"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_rpc_response_error() {
        let resp = RpcResponse::error(1u64, -32000, "Not found".to_string());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32000"));
    }

    #[test]
    fn test_rpc_notification() {
        let notif = RpcNotification::new("peer_connected", serde_json::json!({"peer": "abc"}));
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"peer_connected\""));
        assert!(json.contains("\"params\""));
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn test_event_to_notification() {
        let json = event_to_notification("state_change", &serde_json::json!({"state": "connected"}));
        assert!(json.contains("\"method\":\"state_change\""));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_rpc_event() {
        let event = RpcEvent::new("state_change", serde_json::json!({"state": "connected"}));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"state_change\""));
    }
}
