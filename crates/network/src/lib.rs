//! Craftec Network
//!
//! Protocol-agnostic libp2p networking stack. Provides:
//! - Swarm builder with configurable protocol prefix
//! - Kademlia DHT, gossipsub, mDNS, NAT traversal
//! - Generic methods: subscribe, publish, put_record, get_record, start_providing
//! - Bootstrap utilities

pub mod behaviour;
pub mod node;
pub mod bootstrap;
pub mod error;
pub mod pex;
pub mod pex_wire;
pub mod wire;
pub mod shared;

pub use behaviour::CraftBehaviour;
pub use node::{build_swarm, NetworkConfig, CraftSwarm};
pub use bootstrap::{parse_bootstrap_addr, default_bootstrap_nodes};
pub use error::NetworkError;
pub use shared::{SharedSwarmCommand, SharedSwarmEvent, AutoNatStatus};

/// Re-export libp2p types commonly used by craft crates
pub use libp2p::{PeerId, Multiaddr, StreamProtocol};
pub use libp2p::kad::RecordKey;
