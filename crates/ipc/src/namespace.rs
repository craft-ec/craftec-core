//! Namespace routing for IPC handlers.
//!
//! Routes JSON-RPC methods by prefix: `"tunnel.connect"` → finds the `"tunnel"`
//! handler and calls `handle("connect", params)`. Methods without a `.` are
//! routed to an optional default handler (backwards compat for standalone daemons).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::server::IpcHandler;

/// Routes incoming method calls to namespace-specific handlers.
///
/// A method like `"tunnel.connect"` is split on the first `.` into
/// namespace `"tunnel"` and method `"connect"`. If no handler is found
/// for the namespace (or the method has no `.`), the default handler
/// is tried.
pub struct NamespacedHandler {
    handlers: HashMap<String, Arc<dyn IpcHandler>>,
    default: Option<Arc<dyn IpcHandler>>,
}

impl NamespacedHandler {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            default: None,
        }
    }

    /// Register a handler for the given namespace prefix.
    pub fn add_namespace(&mut self, prefix: &str, handler: Arc<dyn IpcHandler>) {
        self.handlers.insert(prefix.to_string(), handler);
    }

    /// Set the default handler for methods without a recognized namespace.
    pub fn set_default(&mut self, handler: Arc<dyn IpcHandler>) {
        self.default = Some(handler);
    }
}

impl Default for NamespacedHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl IpcHandler for NamespacedHandler {
    fn handle(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
        // Resolve handler + stripped method synchronously to avoid lifetime issues
        let (handler, stripped): (Option<&Arc<dyn IpcHandler>>, String) =
            if let Some((ns, rest)) = method.split_once('.') {
                if let Some(h) = self.handlers.get(ns) {
                    (Some(h), rest.to_string())
                } else {
                    (self.default.as_ref(), method.to_string())
                }
            } else {
                (self.default.as_ref(), method.to_string())
            };

        match handler {
            Some(h) => h.handle(&stripped, params),
            None => {
                let method = method.to_string();
                Box::pin(async move { Err(format!("unknown method: {}", method)) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoHandler {
        prefix: String,
    }

    impl IpcHandler for EchoHandler {
        fn handle(
            &self,
            method: &str,
            _params: Option<Value>,
        ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
            let result = serde_json::json!({
                "handler": self.prefix,
                "method": method,
            });
            Box::pin(async move { Ok(result) })
        }
    }

    #[tokio::test]
    async fn test_namespace_routing() {
        let mut router = NamespacedHandler::new();
        router.add_namespace("tunnel", Arc::new(EchoHandler { prefix: "tunnel".into() }));
        router.add_namespace("data", Arc::new(EchoHandler { prefix: "data".into() }));

        let result = router.handle("tunnel.connect", None).await.unwrap();
        assert_eq!(result["handler"], "tunnel");
        assert_eq!(result["method"], "connect");

        let result = router.handle("data.publish", None).await.unwrap();
        assert_eq!(result["handler"], "data");
        assert_eq!(result["method"], "publish");
    }

    #[tokio::test]
    async fn test_default_handler() {
        let mut router = NamespacedHandler::new();
        router.set_default(Arc::new(EchoHandler { prefix: "default".into() }));

        let result = router.handle("status", None).await.unwrap();
        assert_eq!(result["handler"], "default");
        assert_eq!(result["method"], "status");
    }

    #[tokio::test]
    async fn test_unknown_namespace_falls_to_default() {
        let mut router = NamespacedHandler::new();
        router.add_namespace("tunnel", Arc::new(EchoHandler { prefix: "tunnel".into() }));
        router.set_default(Arc::new(EchoHandler { prefix: "default".into() }));

        let result = router.handle("unknown.method", None).await.unwrap();
        assert_eq!(result["handler"], "default");
        assert_eq!(result["method"], "unknown.method");
    }

    #[tokio::test]
    async fn test_no_handler_returns_error() {
        let router = NamespacedHandler::new();
        let result = router.handle("tunnel.connect", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown method"));
    }
}
