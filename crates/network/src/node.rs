//! Network node builder
//!
//! Builds a libp2p swarm with the full Craftec behaviour stack.
//! Protocol names are parameterized by the `protocol_prefix` in [`NetworkConfig`].

use libp2p::{
    identity::Keypair,
    noise, tcp, yamux,
    Multiaddr, PeerId, SwarmBuilder,
};
use tracing::info;

use crate::behaviour::CraftBehaviour;
use crate::error::NetworkError;

/// Type alias for a swarm using the generic CraftBehaviour.
pub type CraftSwarm = libp2p::Swarm<CraftBehaviour>;

/// Network configuration for building a swarm.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Protocol prefix for Kademlia, Identify, etc.
    /// E.g. `"tunnelcraft"` yields `/tunnelcraft/kad/1.0.0`.
    pub protocol_prefix: String,
    /// Addresses to listen on.
    pub listen_addrs: Vec<Multiaddr>,
    /// Bootstrap peers to connect to on startup.
    pub bootstrap_peers: Vec<(PeerId, Multiaddr)>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            protocol_prefix: "craftec".to_string(),
            listen_addrs: vec!["/ip4/0.0.0.0/tcp/0".parse().expect("valid multiaddr")],
            bootstrap_peers: Vec::new(),
        }
    }
}

#[allow(deprecated)]
fn yamux_config() -> yamux::Config {
    let mut cfg = yamux::Config::default();
    cfg.set_max_num_streams(4096);
    cfg.set_receive_window_size(1024 * 1024);
    cfg.set_max_buffer_size(1024 * 1024);
    cfg
}

/// Build a libp2p swarm with the full CraftBehaviour stack.
///
/// Returns `(swarm, local_peer_id)`. The swarm is already listening on
/// `config.listen_addrs` and has bootstrap peers added to Kademlia.
///
/// To register application-level stream protocols, use
/// `swarm.behaviour().stream_control().accept(protocol)` after building.
pub async fn build_swarm(
    keypair: Keypair,
    config: NetworkConfig,
) -> Result<(CraftSwarm, PeerId), NetworkError> {
    let local_peer_id = PeerId::from(keypair.public());
    info!("Local peer ID: {}", local_peer_id);

    let protocol_prefix = config.protocol_prefix.clone();

    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux_config,
        )
        .map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?
        .with_relay_client(noise::Config::new, yamux_config)
        .map_err(|e| NetworkError::SwarmBuildError(e.to_string()))?
        .with_behaviour(|key, relay_behaviour| {
            let peer_id = PeerId::from(key.public());
            CraftBehaviour::build(&protocol_prefix, peer_id, key, relay_behaviour)
        })
        .map_err(|e| NetworkError::SwarmBuildError(format!("{:?}", e)))?
        .with_swarm_config(|c| {
            c.with_idle_connection_timeout(std::time::Duration::from_secs(300))
        })
        .build();

    // Start listening
    for addr in config.listen_addrs {
        swarm
            .listen_on(addr)
            .map_err(|e| NetworkError::ListenError(e.to_string()))?;
    }

    // Add bootstrap peers to Kademlia
    for (peer_id, addr) in config.bootstrap_peers {
        swarm.behaviour_mut().add_address(&peer_id, addr);
    }

    Ok((swarm, local_peer_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.protocol_prefix, "craftec");
        assert!(!config.listen_addrs.is_empty());
        assert!(config.bootstrap_peers.is_empty());
    }

    #[test]
    fn test_config_custom_prefix() {
        let config = NetworkConfig {
            protocol_prefix: "datacraft".to_string(),
            listen_addrs: vec!["/ip4/0.0.0.0/tcp/9100".parse().unwrap()],
            bootstrap_peers: Vec::new(),
        };
        assert_eq!(config.protocol_prefix, "datacraft");
    }

    #[tokio::test]
    async fn test_build_swarm() {
        let keypair = Keypair::generate_ed25519();
        let expected_peer_id = PeerId::from(keypair.public());
        let config = NetworkConfig::default();

        let result = build_swarm(keypair, config).await;
        assert!(result.is_ok());

        let (swarm, peer_id) = result.unwrap();
        assert_eq!(peer_id, expected_peer_id);
        assert_eq!(swarm.connected_peers().count(), 0);
    }
}
