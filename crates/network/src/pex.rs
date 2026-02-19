//! Peer Exchange (PEX) Protocol
//!
//! Generic peer exchange implementation for Craftec crafts.
//! Nodes share their known peer lists with connected peers to reduce
//! reliance on bootstrap nodes and DHT for peer discovery.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use libp2p::{PeerId, Multiaddr};

/// Manages peer exchange for a Craftec node.
///
/// Tracks known peers and coordinates periodic exchange of peer information
/// with connected nodes to improve network connectivity.
pub struct PexManager {
    /// Known peers with their addresses
    known_peers: HashMap<PeerId, Vec<Multiaddr>>,
    /// When we last sent PEX to each peer
    last_sent: HashMap<PeerId, Instant>,
    /// Interval between PEX exchanges
    interval: Duration,
    /// Max peers to share per exchange
    max_share: usize,
}

impl PexManager {
    /// Create a new PEX manager with default settings (60s interval, 20 max share).
    pub fn new() -> Self {
        Self {
            known_peers: HashMap::new(),
            last_sent: HashMap::new(),
            interval: Duration::from_secs(60),
            max_share: 20,
        }
    }

    /// Create a new PEX manager with custom settings.
    pub fn with_config(interval: Duration, max_share: usize) -> Self {
        Self {
            known_peers: HashMap::new(),
            last_sent: HashMap::new(),
            interval,
            max_share,
        }
    }

    /// Record a discovered peer.
    pub fn add_peer(&mut self, peer_id: PeerId, addrs: Vec<Multiaddr>) {
        if addrs.is_empty() {
            return;
        }
        self.known_peers.insert(peer_id, addrs);
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.known_peers.remove(peer_id);
        self.last_sent.remove(peer_id);
    }

    /// Get peers to share with a given peer (excludes that peer).
    pub fn peers_to_share(&self, exclude: &PeerId) -> Vec<(PeerId, Vec<Multiaddr>)> {
        self.known_peers
            .iter()
            .filter(|(peer_id, _)| *peer_id != exclude)
            .take(self.max_share)
            .map(|(peer_id, addrs)| (*peer_id, addrs.clone()))
            .collect()
    }

    /// Process received PEX data — returns new peers we should try connecting to.
    pub fn receive_pex(
        &mut self,
        _from: &PeerId,
        peers: Vec<(PeerId, Vec<Multiaddr>)>,
    ) -> Vec<(PeerId, Vec<Multiaddr>)> {
        let mut new_peers = Vec::new();
        for (peer_id, addrs) in peers {
            if addrs.is_empty() {
                continue;
            }
            if !self.known_peers.contains_key(&peer_id) {
                new_peers.push((peer_id, addrs.clone()));
            }
            self.add_peer(peer_id, addrs);
        }
        new_peers
    }

    /// Check if it's time to send PEX to a peer.
    pub fn should_send(&self, peer: &PeerId) -> bool {
        match self.last_sent.get(peer) {
            Some(last_time) => last_time.elapsed() >= self.interval,
            None => true,
        }
    }

    /// Mark PEX as sent to peer.
    pub fn mark_sent(&mut self, peer: &PeerId) {
        self.last_sent.insert(*peer, Instant::now());
    }

    /// Get total number of known peers.
    pub fn peer_count(&self) -> usize {
        self.known_peers.len()
    }

    /// Get the configured exchange interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Get the configured max peers to share.
    pub fn max_share(&self) -> usize {
        self.max_share
    }

    /// Clean up old peers that haven't been seen recently.
    pub fn cleanup_stale_peers(&mut self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        let stale_peers: Vec<PeerId> = self.last_sent
            .iter()
            .filter(|(_, &timestamp)| timestamp < cutoff)
            .map(|(peer_id, _)| *peer_id)
            .collect();
        for peer_id in stale_peers {
            self.remove_peer(&peer_id);
        }
    }
}

impl Default for PexManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer_id() -> PeerId {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    fn test_multiaddr() -> Multiaddr {
        "/ip4/127.0.0.1/tcp/8080".parse().unwrap()
    }

    #[test]
    fn test_add_and_remove_peers() {
        let mut manager = PexManager::new();
        let peer_id = test_peer_id();
        manager.add_peer(peer_id, vec![test_multiaddr()]);
        assert_eq!(manager.peer_count(), 1);
        manager.remove_peer(&peer_id);
        assert_eq!(manager.peer_count(), 0);
    }

    #[test]
    fn test_peers_to_share_excludes_self() {
        let mut manager = PexManager::new();
        let peer1 = test_peer_id();
        let peer2 = test_peer_id();
        let addrs = vec![test_multiaddr()];
        manager.add_peer(peer1, addrs.clone());
        manager.add_peer(peer2, addrs);
        let shared = manager.peers_to_share(&peer1);
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].0, peer2);
    }

    #[test]
    fn test_receive_pex_returns_only_new_peers() {
        let mut manager = PexManager::new();
        let known_peer = test_peer_id();
        let new_peer = test_peer_id();
        let from_peer = test_peer_id();
        let addrs = vec![test_multiaddr()];
        manager.add_peer(known_peer, addrs.clone());
        let pex_data = vec![(known_peer, addrs.clone()), (new_peer, addrs)];
        let new_peers = manager.receive_pex(&from_peer, pex_data);
        assert_eq!(new_peers.len(), 1);
        assert_eq!(new_peers[0].0, new_peer);
        assert_eq!(manager.peer_count(), 2);
    }

    #[test]
    fn test_should_send_rate_limiting() {
        let interval = Duration::from_millis(100);
        let mut manager = PexManager::with_config(interval, 20);
        let peer_id = test_peer_id();
        assert!(manager.should_send(&peer_id));
        manager.mark_sent(&peer_id);
        assert!(!manager.should_send(&peer_id));
        std::thread::sleep(Duration::from_millis(150));
        assert!(manager.should_send(&peer_id));
    }

    #[test]
    fn test_max_share_limit() {
        let max_share = 3;
        let mut manager = PexManager::with_config(Duration::from_secs(60), max_share);
        let exclude_peer = test_peer_id();
        let addrs = vec![test_multiaddr()];
        for _ in 0..5 {
            manager.add_peer(test_peer_id(), addrs.clone());
        }
        let shared = manager.peers_to_share(&exclude_peer);
        assert!(shared.len() <= max_share);
    }
}
