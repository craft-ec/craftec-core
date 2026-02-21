//! Generic IPC server
//!
//! Binds a Unix socket (or Windows named pipe), accepts connections,
//! and dispatches JSON-RPC requests to an IpcHandler implementation.
//!
//! `IpcServer` provides the low-level single-transport server.
//! `ServerBuilder` provides a high-level builder that can run Unix socket +
//! WebSocket transports simultaneously with namespaced method routing.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::protocol::{RpcRequest, RpcResponse};

/// Trait that services implement to handle IPC requests.
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>>;
}

/// Generic IPC server that accepts connections and dispatches to a handler.
pub struct IpcServer {
    socket_path: String,
    event_tx: broadcast::Sender<String>,
}

impl IpcServer {
    pub fn new(socket_path: &str) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            socket_path: socket_path.to_string(),
            event_tx,
        }
    }

    /// Get a clone of the event sender for broadcasting events to all clients.
    pub fn event_sender(&self) -> broadcast::Sender<String> {
        self.event_tx.clone()
    }

    /// Run the IPC server, accepting connections and dispatching to the handler.
    #[cfg(unix)]
    pub async fn run(&self, handler: Arc<dyn IpcHandler>) -> std::io::Result<()> {
        use tokio::net::UnixListener;

        // Remove stale socket
        let _ = std::fs::remove_file(&self.socket_path);

        let listener = UnixListener::bind(&self.socket_path)?;
        info!("IPC server listening on {}", self.socket_path);

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let handler = handler.clone();
                    let event_rx = self.event_tx.subscribe();
                    tokio::spawn(Self::handle_connection(stream, handler, event_rx));
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    #[cfg(unix)]
    async fn handle_connection(
        stream: tokio::net::UnixStream,
        handler: Arc<dyn IpcHandler>,
        mut event_rx: broadcast::Receiver<String>,
    ) {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Spawn event forwarder
        let (local_tx, mut local_rx) = tokio::sync::mpsc::channel::<String>(64);
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(msg) => {
                        if local_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });

        loop {
            let mut line = String::new();

            tokio::select! {
                result = reader.read_line(&mut line) => {
                    match result {
                        Ok(0) => {
                            debug!("IPC client disconnected");
                            break;
                        }
                        Ok(_) => {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }

                            let response = match serde_json::from_str::<RpcRequest>(line) {
                                Ok(req) => {
                                    match handler.handle(&req.method, req.params).await {
                                        Ok(result) => RpcResponse::success(req.id, result),
                                        Err(msg) => RpcResponse::error(req.id, -32000, msg),
                                    }
                                }
                                Err(e) => {
                                    warn!("Invalid JSON-RPC request: {}", e);
                                    RpcResponse::error(Value::Null, -32700, format!("Parse error: {}", e))
                                }
                            };

                            let json = serde_json::to_string(&response).unwrap_or_default();
                            if writer.write_all(format!("{}\n", json).as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("IPC read error: {}", e);
                            break;
                        }
                    }
                }
                Some(event) = local_rx.recv() => {
                    if writer.write_all(format!("{}\n", event).as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    /// Broadcast an event to all connected clients.
    pub fn send_event(&self, event: &str, data: serde_json::Value) {
        let msg = serde_json::json!({
            "event": event,
            "data": data,
        });
        let _ = self.event_tx.send(msg.to_string());
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

// ---------------------------------------------------------------------------
// ServerBuilder — high-level builder for multi-transport IPC servers
// ---------------------------------------------------------------------------

#[cfg(feature = "websocket")]
use crate::ws::WsAuth;
use crate::namespace::NamespacedHandler;

/// High-level builder that runs Unix socket + optional WebSocket transports
/// with namespaced method routing.
///
/// ```rust,no_run
/// # use std::sync::Arc;
/// # use craftec_ipc::ServerBuilder;
/// # async fn example(handler: Arc<dyn craftec_ipc::server::IpcHandler>) {
/// let ipc = ServerBuilder::new("/tmp/craftec.sock")
///     .namespace("data", handler.clone())
///     .default_handler(handler);
///
/// ipc.run().await.unwrap();
/// # }
/// ```
pub struct ServerBuilder {
    socket_path: String,
    #[cfg(feature = "websocket")]
    ws_port: Option<u16>,
    #[cfg(feature = "websocket")]
    ws_auth: Option<WsAuth>,
    handler: NamespacedHandler,
    event_tx: broadcast::Sender<String>,
}

impl ServerBuilder {
    pub fn new(socket_path: &str) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            socket_path: socket_path.to_string(),
            #[cfg(feature = "websocket")]
            ws_port: None,
            #[cfg(feature = "websocket")]
            ws_auth: None,
            handler: NamespacedHandler::new(),
            event_tx,
        }
    }

    /// Enable the WebSocket transport on the given port.
    #[cfg(feature = "websocket")]
    pub fn with_websocket(mut self, port: u16) -> Self {
        self.ws_port = Some(port);
        self
    }

    /// Set the API key required for WebSocket connections.
    #[cfg(feature = "websocket")]
    pub fn with_api_key(mut self, key: String) -> Self {
        self.ws_auth = Some(WsAuth { api_key: Some(key) });
        self
    }

    /// Register a handler for a method namespace (e.g. `"tunnel"` handles `"tunnel.*"` methods).
    pub fn namespace(mut self, prefix: &str, handler: Arc<dyn IpcHandler>) -> Self {
        self.handler.add_namespace(prefix, handler);
        self
    }

    /// Register a default handler for methods without a namespace prefix.
    pub fn default_handler(mut self, handler: Arc<dyn IpcHandler>) -> Self {
        self.handler.set_default(handler);
        self
    }

    /// Get a clone of the event broadcast sender.
    ///
    /// Events sent through this sender are forwarded to all connected clients
    /// (both Unix socket and WebSocket).
    pub fn event_sender(&self) -> broadcast::Sender<String> {
        self.event_tx.clone()
    }

    /// Run all configured transports. Blocks until shutdown.
    #[cfg(unix)]
    pub async fn run(self) -> std::io::Result<()> {
        use tokio::net::UnixListener;

        let handler: Arc<dyn IpcHandler> = Arc::new(self.handler);

        // Remove stale socket
        let _ = std::fs::remove_file(&self.socket_path);
        let listener = UnixListener::bind(&self.socket_path)?;
        info!("IPC server listening on {}", self.socket_path);

        let socket_path = self.socket_path.clone();
        let event_tx = self.event_tx.clone();
        let handler_clone = handler.clone();

        let unix_fut = async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let h = handler_clone.clone();
                        let rx = event_tx.subscribe();
                        tokio::spawn(IpcServer::handle_connection(stream, h, rx));
                    }
                    Err(e) => {
                        error!("Failed to accept IPC connection: {}", e);
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), std::io::Error>(())
        };

        #[cfg(feature = "websocket")]
        {
            if let Some(port) = self.ws_port {
                let auth = self.ws_auth.unwrap_or_default();
                let ws_event_tx = self.event_tx.clone();
                let ws_handler = handler.clone();

                let ws_fut = crate::ws::run_ws_server(port, ws_handler, auth, ws_event_tx);

                tokio::select! {
                    res = unix_fut => { let _ = std::fs::remove_file(&socket_path); res }
                    res = ws_fut => { let _ = std::fs::remove_file(&socket_path); res }
                }
            } else {
                let res = unix_fut.await;
                let _ = std::fs::remove_file(&socket_path);
                res
            }
        }

        #[cfg(not(feature = "websocket"))]
        {
            let res = unix_fut.await;
            let _ = std::fs::remove_file(&socket_path);
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_server_creation() {
        let server = IpcServer::new("/tmp/test-craftec.sock");
        assert_eq!(server.socket_path, "/tmp/test-craftec.sock");
    }

    #[test]
    fn test_event_broadcast() {
        let server = IpcServer::new("/tmp/test-craftec-event.sock");
        let mut rx = server.event_sender().subscribe();

        server.send_event("test", serde_json::json!({"key": "value"}));

        let msg = rx.try_recv().unwrap();
        assert!(msg.contains("test"));
    }

    #[test]
    fn test_server_builder_creation() {
        let builder = ServerBuilder::new("/tmp/test-builder.sock");
        assert_eq!(builder.socket_path, "/tmp/test-builder.sock");
    }

    #[test]
    fn test_server_builder_event_sender() {
        let builder = ServerBuilder::new("/tmp/test-builder-event.sock");
        let mut rx = builder.event_sender().subscribe();
        let tx = builder.event_sender();

        let _ = tx.send("test event".to_string());
        let msg = rx.try_recv().unwrap();
        assert_eq!(msg, "test event");
    }
}
