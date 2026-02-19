//! PEX Wire Protocol Types
//!
//! Serializable message types for Peer Exchange (PEX) protocol.
//! Generic — usable by any Craftec craft.

use serde::{Deserialize, Serialize};
use libp2p::{PeerId, Multiaddr};
use std::convert::TryFrom;

/// A PEX message containing a list of peers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PexMessage {
    pub peers: Vec<PexPeer>,
}

/// A single peer entry in a PEX message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PexPeer {
    /// Peer ID as raw bytes.
    pub peer_id_bytes: Vec<u8>,
    /// List of multiaddresses as raw bytes.
    pub addrs: Vec<Vec<u8>>,
}

impl PexMessage {
    pub fn new() -> Self {
        Self { peers: Vec::new() }
    }

    /// Create from (PeerId, Vec<Multiaddr>) pairs.
    pub fn from_peers(peers: Vec<(PeerId, Vec<Multiaddr>)>) -> Self {
        Self {
            peers: peers.into_iter()
                .map(|(peer_id, addrs)| PexPeer::from_peer(peer_id, addrs))
                .collect(),
        }
    }

    /// Convert back to (PeerId, Vec<Multiaddr>) pairs.
    pub fn to_peers(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>, PexWireError> {
        self.peers.iter().map(|p| p.to_peer()).collect()
    }

    /// Serialize to bytes (JSON).
    pub fn to_bytes(&self) -> Result<Vec<u8>, PexWireError> {
        serde_json::to_vec(self)
            .map_err(|e| PexWireError::SerializationError(e.to_string()))
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, PexWireError> {
        serde_json::from_slice(data)
            .map_err(|e| PexWireError::DeserializationError(e.to_string()))
    }
}

impl Default for PexMessage {
    fn default() -> Self {
        Self::new()
    }
}

impl PexPeer {
    pub fn from_peer(peer_id: PeerId, addrs: Vec<Multiaddr>) -> Self {
        Self {
            peer_id_bytes: peer_id.to_bytes(),
            addrs: addrs.into_iter().map(|a| a.to_vec()).collect(),
        }
    }

    pub fn to_peer(&self) -> Result<(PeerId, Vec<Multiaddr>), PexWireError> {
        let peer_id = PeerId::from_bytes(&self.peer_id_bytes)
            .map_err(|e| PexWireError::InvalidPeerId(e.to_string()))?;
        let mut addrs = Vec::new();
        for addr_bytes in &self.addrs {
            let addr = Multiaddr::try_from(addr_bytes.clone())
                .map_err(|e| PexWireError::InvalidAddress(e.to_string()))?;
            addrs.push(addr);
        }
        Ok((peer_id, addrs))
    }
}

/// Errors during PEX wire operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PexWireError {
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    #[error("Invalid peer ID: {0}")]
    InvalidPeerId(String),
    #[error("Invalid multiaddress: {0}")]
    InvalidAddress(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer_id() -> PeerId {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    fn test_multiaddr() -> Multiaddr {
        "/ip4/192.168.1.100/tcp/4001".parse().unwrap()
    }

    #[test]
    fn test_pex_peer_round_trip() {
        let peer_id = test_peer_id();
        let addrs = vec![test_multiaddr(), "/ip6/::1/tcp/4001".parse().unwrap()];
        let pex_peer = PexPeer::from_peer(peer_id, addrs.clone());
        let (decoded_id, decoded_addrs) = pex_peer.to_peer().unwrap();
        assert_eq!(decoded_id, peer_id);
        assert_eq!(decoded_addrs, addrs);
    }

    #[test]
    fn test_pex_message_serialization() {
        let peer1 = test_peer_id();
        let peer2 = test_peer_id();
        let addr1 = test_multiaddr();
        let addr2: Multiaddr = "/ip4/10.0.0.1/tcp/8080".parse().unwrap();
        let peers = vec![(peer1, vec![addr1.clone()]), (peer2, vec![addr2, addr1])];
        let message = PexMessage::from_peers(peers);
        let bytes = message.to_bytes().unwrap();
        let decoded = PexMessage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, message);
        let decoded_peers = decoded.to_peers().unwrap();
        assert_eq!(decoded_peers.len(), 2);
        assert_eq!(decoded_peers[0].0, peer1);
        assert_eq!(decoded_peers[1].0, peer2);
    }

    #[test]
    fn test_empty_pex_message() {
        let msg = PexMessage::new();
        let bytes = msg.to_bytes().unwrap();
        let decoded = PexMessage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.peers.len(), 0);
    }

    #[test]
    fn test_invalid_deserialization() {
        let result = PexMessage::from_bytes(b"invalid");
        assert!(matches!(result, Err(PexWireError::DeserializationError(_))));
    }

    #[test]
    fn test_peer_with_no_addresses() {
        let peer_id = test_peer_id();
        let pex_peer = PexPeer::from_peer(peer_id, vec![]);
        let (decoded_id, decoded_addrs) = pex_peer.to_peer().unwrap();
        assert_eq!(decoded_id, peer_id);
        assert!(decoded_addrs.is_empty());
    }

    #[test]
    fn test_message_size_reasonable() {
        let mut peers = vec![];
        for _ in 0..50 {
            peers.push((test_peer_id(), vec![
                test_multiaddr(),
                "/ip6/::1/tcp/4001".parse().unwrap(),
                "/ip4/172.16.0.1/tcp/9000".parse().unwrap(),
            ]));
        }
        let message = PexMessage::from_peers(peers);
        let bytes = message.to_bytes().unwrap();
        assert!(bytes.len() < 50_000);
    }
}
