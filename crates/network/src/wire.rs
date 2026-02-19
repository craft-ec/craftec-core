//! Generic wire framing for Craftec stream protocols.
//!
//! Wire format: `[seq_id: u64 BE][type: u8][len: u32 BE][payload: bytes]`
//!
//! Flush after every write — yamux buffers data, flush pushes it to the wire.

use std::io;
use futures::prelude::*;

/// Maximum frame payload (50 MB).
pub const MAX_FRAME_PAYLOAD: usize = 50 * 1024 * 1024;

/// A raw parsed frame header + payload from a stream.
#[derive(Debug)]
pub struct RawFrame {
    pub seq_id: u64,
    pub msg_type: u8,
    pub payload: Vec<u8>,
}

/// Read a single raw frame from a stream.
///
/// Returns the seq_id, message type byte, and payload.
/// Craft-specific code maps msg_type to request/response variants.
pub async fn read_raw_frame<T: AsyncRead + Unpin>(io: &mut T) -> io::Result<RawFrame> {
    // seq_id (8 bytes)
    let mut seq_bytes = [0u8; 8];
    io.read_exact(&mut seq_bytes).await?;
    let seq_id = u64::from_be_bytes(seq_bytes);

    // type (1 byte)
    let mut ty = [0u8; 1];
    io.read_exact(&mut ty).await?;

    // length (4 bytes)
    let mut len_bytes = [0u8; 4];
    io.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    if len > MAX_FRAME_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Frame payload too large: {} > {}", len, MAX_FRAME_PAYLOAD),
        ));
    }

    let mut payload = vec![0u8; len];
    if len > 0 {
        io.read_exact(&mut payload).await?;
    }

    Ok(RawFrame { seq_id, msg_type: ty[0], payload })
}

/// Write a raw frame: `[seq_id:8][type:1][len:4][payload]` + flush.
pub async fn write_raw_frame<T: AsyncWrite + Unpin>(
    io: &mut T,
    seq_id: u64,
    msg_type: u8,
    payload: &[u8],
) -> io::Result<()> {
    if payload.len() > MAX_FRAME_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Frame payload too large: {} > {}", payload.len(), MAX_FRAME_PAYLOAD),
        ));
    }

    let frame_len = 8 + 1 + 4 + payload.len();
    let mut buf = Vec::with_capacity(frame_len);
    buf.extend_from_slice(&seq_id.to_be_bytes());
    buf.push(msg_type);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);

    io.write_all(&buf).await?;
    io.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_raw_frame_roundtrip() {
        let payload = b"hello world";
        let mut buf = Vec::new();
        write_raw_frame(&mut futures::io::Cursor::new(&mut buf), 42, 0x01, payload)
            .await
            .unwrap();

        let frame = read_raw_frame(&mut futures::io::Cursor::new(&buf)).await.unwrap();
        assert_eq!(frame.seq_id, 42);
        assert_eq!(frame.msg_type, 0x01);
        assert_eq!(frame.payload, payload);
    }

    #[tokio::test]
    async fn test_empty_payload() {
        let mut buf = Vec::new();
        write_raw_frame(&mut futures::io::Cursor::new(&mut buf), 0, 0xFF, &[])
            .await
            .unwrap();

        let frame = read_raw_frame(&mut futures::io::Cursor::new(&buf)).await.unwrap();
        assert_eq!(frame.seq_id, 0);
        assert_eq!(frame.msg_type, 0xFF);
        assert!(frame.payload.is_empty());
    }
}
