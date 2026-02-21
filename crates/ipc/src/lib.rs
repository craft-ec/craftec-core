//! Craftec IPC
//!
//! Generic JSON-RPC 2.0 IPC server and client over Unix sockets (macOS/Linux)
//! or Named Pipes (Windows). Optionally supports WebSocket transport via the
//! `websocket` feature, and namespaced method routing for multi-service daemons.

pub mod server;
pub mod client;
pub mod protocol;
pub mod namespace;

#[cfg(feature = "websocket")]
pub mod ws;

pub use server::{IpcServer, IpcHandler, ServerBuilder};
pub use client::IpcClient;
pub use protocol::{RpcRequest, RpcResponse, RpcError, RpcNotification, event_to_notification};
pub use namespace::NamespacedHandler;

#[cfg(feature = "websocket")]
pub use ws::WsAuth;

use std::path::PathBuf;

/// Get the default socket path for a service.
///
/// - macOS: `/tmp/{service}.sock`
/// - Linux: `$XDG_RUNTIME_DIR/{service}.sock` or `/tmp/{service}.sock`
/// - Windows: `\\.\pipe\{service}`
pub fn default_socket_path(service: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!(r"\\.\pipe\{}", service)
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_RUNTIME_DIR")
            .map(|dir| format!("{}/{}.sock", dir, service))
            .unwrap_or_else(|_| format!("/tmp/{}.sock", service))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        format!("/tmp/{}.sock", service)
    }
}

/// Get the socket path as a PathBuf.
pub fn default_socket_path_buf(service: &str) -> PathBuf {
    PathBuf::from(default_socket_path(service))
}
