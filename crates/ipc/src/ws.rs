//! WebSocket JSON-RPC 2.0 transport.
//!
//! Accepts WebSocket connections at `/ws` and routes JSON-RPC requests
//! through the shared `IpcHandler`. Connections authenticate via
//! `?key=<api_key>` query parameter (when `WsAuth.api_key` is set).
//!
//! Server-push: each connection receives broadcast events as JSON-RPC
//! notifications (messages with `method` but no `id`).

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::protocol::{RpcRequest, RpcResponse};
use crate::server::IpcHandler;

/// WebSocket authentication configuration.
#[derive(Debug, Clone, Default)]
pub struct WsAuth {
    /// Optional API key required in the `?key=` query parameter.
    /// When `None`, no authentication is required.
    pub api_key: Option<String>,
}

/// Run the WebSocket JSON-RPC 2.0 server.
///
/// Listens on the given port and upgrades HTTP connections at `/ws` to WebSocket.
/// Each incoming text message is parsed as a JSON-RPC 2.0 request and dispatched
/// to the shared `handler`. Broadcast events from `event_tx` are forwarded as
/// JSON-RPC notifications to all connected clients.
pub async fn run_ws_server(
    port: u16,
    handler: Arc<dyn IpcHandler>,
    auth: WsAuth,
    event_tx: broadcast::Sender<String>,
) -> std::io::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!("WebSocket server listening on ws://0.0.0.0:{}/ws", port);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let handler = handler.clone();
                let auth = auth.clone();
                let event_rx = event_tx.subscribe();
                tokio::spawn(async move {
                    let ws_stream = match accept_ws(stream, &auth).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            debug!("WebSocket handshake failed from {}: {}", peer, e);
                            return;
                        }
                    };

                    debug!("WebSocket client connected: {}", peer);
                    handle_ws_connection(ws_stream, handler, peer, event_rx).await;
                    debug!("WebSocket client disconnected: {}", peer);
                });
            }
            Err(e) => {
                error!("Failed to accept TCP connection: {}", e);
            }
        }
    }
}

/// Perform the WebSocket handshake with path and optional API key validation.
async fn accept_ws(
    stream: tokio::net::TcpStream,
    auth: &WsAuth,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    tokio_tungstenite::tungstenite::Error,
> {
    let api_key = auth.api_key.clone();
    tokio_tungstenite::accept_hdr_async(
        stream,
        move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
              resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
            let uri = req.uri();
            let path = uri.path();
            if path != "/ws" && path != "/ws/" {
                let mut err =
                    tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(Some(
                        "Not Found".into(),
                    ));
                *err.status_mut() =
                    tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
                return Err(err);
            }

            // Validate API key if configured
            if let Some(ref expected_key) = api_key {
                let query = uri.query().unwrap_or("");
                let key_value = query.split('&').find_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    if k == "key" {
                        Some(v.to_string())
                    } else {
                        None
                    }
                });

                match key_value {
                    Some(ref k) if k == expected_key => Ok(resp),
                    _ => {
                        let mut err =
                            tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(
                                Some("Unauthorized".into()),
                            );
                        *err.status_mut() =
                            tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
                        Err(err)
                    }
                }
            } else {
                // No auth required
                Ok(resp)
            }
        },
    )
    .await
}

async fn handle_ws_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    handler: Arc<dyn IpcHandler>,
    peer: std::net::SocketAddr,
    mut event_rx: broadcast::Receiver<String>,
) {
    let (mut sink, mut stream) = ws_stream.split();

    loop {
        tokio::select! {
            msg = stream.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        debug!("WebSocket read error from {}: {}", peer, e);
                        break;
                    }
                    None => break,
                };

                match msg {
                    Message::Text(text) => {
                        let response = match serde_json::from_str::<RpcRequest>(&text) {
                            Ok(req) => {
                                debug!("WS RPC: {} (id={})", req.method, req.id);
                                match handler.handle(&req.method, req.params).await {
                                    Ok(result) => RpcResponse::success(req.id, result),
                                    Err(msg) => RpcResponse::error(req.id, -32000, msg),
                                }
                            }
                            Err(e) => {
                                warn!("Invalid JSON-RPC from {}: {}", peer, e);
                                RpcResponse::error(
                                    serde_json::Value::Null,
                                    -32700,
                                    format!("Parse error: {}", e),
                                )
                            }
                        };

                        let json = serde_json::to_string(&response).unwrap_or_default();
                        if sink.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Message::Ping(data) => {
                        if sink.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {} // Ignore binary, pong, etc.
                }
            }

            event = event_rx.recv() => {
                match event {
                    Ok(notification_json) => {
                        // Events are pre-serialized JSON-RPC notification strings
                        if sink.send(Message::Text(notification_json)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client {} lagged, dropped {} events", peer, n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }
}
