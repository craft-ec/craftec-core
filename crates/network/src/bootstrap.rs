//! Bootstrap node utilities
//!
//! Parsing and construction of bootstrap multiaddr strings.
//! Each craft provides its own bootstrap node list; this module
//! supplies the parsing infrastructure.

use libp2p::{Multiaddr, PeerId};

/// Parse a single bootstrap address.
///
/// Expected format: `/ip4/<IP>/tcp/<PORT>/p2p/<PEER_ID>`
///
/// Returns `(PeerId, dial_addr)` where `dial_addr` has the `/p2p/...` suffix removed.
pub fn parse_bootstrap_addr(addr_str: &str) -> Option<(PeerId, Multiaddr)> {
    let addr: Multiaddr = addr_str.parse().ok()?;

    let peer_id = addr.iter().find_map(|proto| {
        if let libp2p::multiaddr::Protocol::P2p(peer_id) = proto {
            Some(peer_id)
        } else {
            None
        }
    })?;

    let dial_addr: Multiaddr = addr
        .iter()
        .filter(|proto| !matches!(proto, libp2p::multiaddr::Protocol::P2p(_)))
        .collect();

    Some((peer_id, dial_addr))
}

/// Parse multiple bootstrap addresses, skipping any that fail to parse.
pub fn parse_bootstrap_nodes(addrs: &[&str]) -> Vec<(PeerId, Multiaddr)> {
    addrs
        .iter()
        .filter_map(|addr_str| parse_bootstrap_addr(addr_str))
        .collect()
}

/// Construct a bootstrap multiaddr string from components.
pub fn make_bootstrap_addr(ip: &str, port: u16, peer_id: &str) -> String {
    format!("/ip4/{}/tcp/{}/p2p/{}", ip, port, peer_id)
}

/// Returns the default bootstrap nodes for craftec-core (empty).
///
/// Each craft (craftnet, craftobj, etc.) provides its own bootstrap list.
pub fn default_bootstrap_nodes() -> &'static [&'static str] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bootstrap_addr() {
        let addr = "/ip4/127.0.0.1/tcp/9000/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
        let result = parse_bootstrap_addr(addr);
        assert!(result.is_some());

        let (peer_id, dial_addr) = result.unwrap();
        assert_eq!(dial_addr.to_string(), "/ip4/127.0.0.1/tcp/9000");
        assert!(peer_id.to_string().starts_with("12D3KooW"));
    }

    #[test]
    fn test_parse_invalid_addr() {
        assert!(parse_bootstrap_addr("invalid").is_none());
        assert!(parse_bootstrap_addr("/ip4/127.0.0.1/tcp/9000").is_none());
    }

    #[test]
    fn test_make_bootstrap_addr() {
        let addr = make_bootstrap_addr(
            "123.45.67.89",
            9000,
            "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN",
        );
        assert_eq!(
            addr,
            "/ip4/123.45.67.89/tcp/9000/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        );
    }

    #[test]
    fn test_parse_bootstrap_nodes() {
        let addrs = &[
            "/ip4/127.0.0.1/tcp/9000/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN",
        ];
        let peers = parse_bootstrap_nodes(addrs);
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn test_empty_bootstrap() {
        let peers = parse_bootstrap_nodes(&[]);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_default_bootstrap_nodes_empty() {
        assert!(default_bootstrap_nodes().is_empty());
    }
}
