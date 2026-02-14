//! Generic IPC client
//!
//! Connects to a daemon's Unix socket (or Windows named pipe)
//! and sends JSON-RPC 2.0 requests.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::debug;

use crate::protocol::{RpcRequest, RpcResponse};

#[derive(Error, Debug)]
pub enum IpcError {
    #[error("Daemon not running")]
    DaemonNotRunning,
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Daemon error: code={code}, message={message}")]
    DaemonError { code: i32, message: String },
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, IpcError>;

/// Generic IPC client for communicating with a Craftec daemon.
pub struct IpcClient {
    socket_path: String,
    next_id: AtomicU64,
}

impl IpcClient {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a JSON-RPC request and return the result.
    #[cfg(unix)]
    pub async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value> {
        use tokio::net::UnixStream;

        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|_| IpcError::DaemonNotRunning)?;

        let (reader, mut writer) = stream.into_split();

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = RpcRequest::new(method, params, id);
        let json = serde_json::to_string(&request)
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;

        debug!("IPC request: {}", json);

        writer
            .write_all(format!("{}\n", json).as_bytes())
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;

        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| IpcError::ConnectionFailed(e.to_string()))?;

        debug!("IPC response: {}", line.trim());

        let response: RpcResponse = serde_json::from_str(line.trim())
            .map_err(|e| IpcError::InvalidResponse(e.to_string()))?;

        if let Some(err) = response.error {
            return Err(IpcError::DaemonError {
                code: err.code,
                message: err.message,
            });
        }

        response
            .result
            .ok_or_else(|| IpcError::InvalidResponse("No result in response".to_string()))
    }

    /// Check if the daemon is running by attempting a connection.
    #[cfg(unix)]
    pub async fn is_daemon_running(&self) -> bool {
        tokio::net::UnixStream::connect(&self.socket_path)
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = IpcClient::new("/tmp/test.sock");
        assert_eq!(client.socket_path, "/tmp/test.sock");
    }

    #[tokio::test]
    async fn test_daemon_not_running() {
        let client = IpcClient::new("/tmp/nonexistent-craftec-test.sock");
        let result = client.send_request("status", None).await;
        assert!(matches!(result, Err(IpcError::DaemonNotRunning)));
    }
}
