//! Generic network behaviour for Craftec
//!
//! Combines Kademlia DHT, Identify, mDNS, Gossipsub, Rendezvous,
//! Relay, DCUtR, AutoNAT, and persistent stream protocols.
//! All protocol names are parameterized by a prefix (e.g., "craftnet", "craftobj").

use libp2p::{
    autonat, dcutr, gossipsub, identify, kad, mdns, relay, rendezvous,
    swarm::{NetworkBehaviour, behaviour::toggle::Toggle},
    Multiaddr, PeerId, StreamProtocol,
};
use std::time::Duration;

/// Combined network behaviour for any Craftec service.
///
/// Protocol names are configured via the `protocol_prefix` passed to [`CraftBehaviour::build`].
/// For example, prefix `"craftnet"` yields Kademlia protocol `/craftnet/kad/1.0.0`.
///
/// Dual-Kademlia support: when `kademlia_secondary` is enabled (via [`CraftBehaviour::build_dual_kad`]),
/// two independent Kademlia DHT instances run on the same swarm with separate protocol IDs.
/// This allows e.g. CraftOBJ (`/craftobj/kad/1.0.0`) and CraftNet (`/craftnet/kad/1.0.0`)
/// to share peer connections while keeping DHT records isolated.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "CraftBehaviourEvent")]
pub struct CraftBehaviour {
    /// Primary Kademlia DHT for peer and content discovery
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    /// Secondary Kademlia DHT (optional, for dual-craft swarm)
    pub kademlia_secondary: Toggle<kad::Behaviour<kad::store::MemoryStore>>,
    /// Identify protocol for peer info exchange
    pub identify: identify::Behaviour,
    /// mDNS for local network discovery (disabled via Toggle when not needed)
    pub mdns: Toggle<mdns::tokio::Behaviour>,
    /// Gossipsub for pub/sub messaging
    pub gossipsub: gossipsub::Behaviour,
    /// Rendezvous client for decentralized discovery
    pub rendezvous_client: rendezvous::client::Behaviour,
    /// Rendezvous server (nodes can act as rendezvous points)
    pub rendezvous_server: rendezvous::server::Behaviour,
    /// Relay client for NAT traversal
    pub relay_client: relay::client::Behaviour,
    /// DCUtR for direct connection upgrade
    pub dcutr: dcutr::Behaviour,
    /// AutoNAT for NAT detection
    pub autonat: autonat::Behaviour,
    /// Persistent stream protocol for application-level streams
    pub stream: libp2p_stream::Behaviour,
}

/// Events emitted by CraftBehaviour
#[derive(Debug)]
pub enum CraftBehaviourEvent {
    Kademlia(kad::Event),
    KademliaSecondary(kad::Event),
    Identify(identify::Event),
    Mdns(mdns::Event),
    Gossipsub(gossipsub::Event),
    RendezvousClient(rendezvous::client::Event),
    RendezvousServer(rendezvous::server::Event),
    RelayClient(relay::client::Event),
    Dcutr(dcutr::Event),
    AutoNat(autonat::Event),
    Stream(()),
}

impl From<kad::Event> for CraftBehaviourEvent {
    fn from(e: kad::Event) -> Self {
        CraftBehaviourEvent::Kademlia(e)
    }
}

impl From<identify::Event> for CraftBehaviourEvent {
    fn from(e: identify::Event) -> Self {
        CraftBehaviourEvent::Identify(e)
    }
}

impl From<mdns::Event> for CraftBehaviourEvent {
    fn from(e: mdns::Event) -> Self {
        CraftBehaviourEvent::Mdns(e)
    }
}

impl From<gossipsub::Event> for CraftBehaviourEvent {
    fn from(e: gossipsub::Event) -> Self {
        CraftBehaviourEvent::Gossipsub(e)
    }
}

impl From<rendezvous::client::Event> for CraftBehaviourEvent {
    fn from(e: rendezvous::client::Event) -> Self {
        CraftBehaviourEvent::RendezvousClient(e)
    }
}

impl From<rendezvous::server::Event> for CraftBehaviourEvent {
    fn from(e: rendezvous::server::Event) -> Self {
        CraftBehaviourEvent::RendezvousServer(e)
    }
}

impl From<relay::client::Event> for CraftBehaviourEvent {
    fn from(e: relay::client::Event) -> Self {
        CraftBehaviourEvent::RelayClient(e)
    }
}

impl From<dcutr::Event> for CraftBehaviourEvent {
    fn from(e: dcutr::Event) -> Self {
        CraftBehaviourEvent::Dcutr(e)
    }
}

impl From<autonat::Event> for CraftBehaviourEvent {
    fn from(e: autonat::Event) -> Self {
        CraftBehaviourEvent::AutoNat(e)
    }
}

impl From<()> for CraftBehaviourEvent {
    fn from(_: ()) -> Self {
        CraftBehaviourEvent::Stream(())
    }
}

impl CraftBehaviour {
    /// Build a new CraftBehaviour with the given protocol prefix.
    ///
    /// Called from [`build_swarm`](crate::node::build_swarm) inside the SwarmBuilder callback.
    /// The `relay_client` is provided by SwarmBuilder's `with_relay_client`.
    ///
    /// `protocol_prefix` sets protocol names: e.g. `"craftobj"` yields
    /// Kademlia `/craftobj/kad/1.0.0`, identify `/craftobj/id/1.0.0`.
    /// Build with mDNS enabled (default).
    pub fn build(
        protocol_prefix: &str,
        local_peer_id: PeerId,
        keypair: &libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::build_with_options(protocol_prefix, local_peer_id, keypair, relay_client, true)
    }

    /// Build with optional mDNS.
    pub fn build_with_options(
        protocol_prefix: &str,
        local_peer_id: PeerId,
        keypair: &libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
        enable_mdns: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::build_full(protocol_prefix, None, local_peer_id, keypair, relay_client, enable_mdns)
    }

    /// Build with dual Kademlia DHTs (separate protocol IDs on one swarm).
    ///
    /// `primary_prefix` — Kademlia protocol for the primary DHT (e.g. `"craftobj"`).
    /// `secondary_prefix` — Kademlia protocol for the secondary DHT (e.g. `"craftnet"`).
    /// Both share the same identify, mDNS, gossipsub, relay, etc.
    pub fn build_dual_kad(
        primary_prefix: &str,
        secondary_prefix: &str,
        local_peer_id: PeerId,
        keypair: &libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
        enable_mdns: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::build_full(primary_prefix, Some(secondary_prefix), local_peer_id, keypair, relay_client, enable_mdns)
    }

    fn build_full(
        protocol_prefix: &str,
        secondary_prefix: Option<&str>,
        local_peer_id: PeerId,
        keypair: &libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
        enable_mdns: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Primary Kademlia
        let kad_protocol = StreamProtocol::try_from_owned(
            format!("/{}/kad/1.0.0", protocol_prefix),
        )?;
        let mut kad_config = kad::Config::new(kad_protocol);
        kad_config.set_query_timeout(Duration::from_secs(60));
        kad_config.set_max_packet_size(512 * 1024);
        let mut store_config = kad::store::MemoryStoreConfig::default();
        store_config.max_value_bytes = 256 * 1024;
        store_config.max_records = 4096;
        let store = kad::store::MemoryStore::with_config(local_peer_id, store_config);
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

        // Secondary Kademlia (optional)
        let kademlia_secondary = if let Some(sec_prefix) = secondary_prefix {
            let kad2_protocol = StreamProtocol::try_from_owned(
                format!("/{}/kad/1.0.0", sec_prefix),
            )?;
            let mut kad2_config = kad::Config::new(kad2_protocol);
            kad2_config.set_query_timeout(Duration::from_secs(60));
            let store2 = kad::store::MemoryStore::new(local_peer_id);
            let mut kad2 = kad::Behaviour::with_config(local_peer_id, store2, kad2_config);
            // Server mode: respond to DHT queries from other peers.
            // Without this, Kademlia acts as Client-only and won't respond
            // to FIND_NODE / GET_PROVIDERS, leaving routing tables empty.
            kad2.set_mode(Some(kad::Mode::Server));
            Toggle::from(Some(kad2))
        } else {
            Toggle::from(None)
        };

        // Identify
        let identify_config = identify::Config::new(
            format!("/{}/id/1.0.0", protocol_prefix),
            keypair.public(),
        )
        .with_agent_version(format!("{}/{}", protocol_prefix, env!("CARGO_PKG_VERSION")));
        let identify = identify::Behaviour::new(identify_config);

        // mDNS (optional — disabled in test environments to prevent cross-test discovery)
        let mdns = if enable_mdns {
            Toggle::from(Some(mdns::tokio::Behaviour::new(
                mdns::Config::default(),
                local_peer_id,
            )?))
        } else {
            Toggle::from(None)
        };

        // Gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Permissive)
            .message_id_fn(|msg: &gossipsub::Message| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                msg.data.hash(&mut hasher);
                if let Some(peer) = &msg.source {
                    Hash::hash(peer, &mut hasher);
                }
                gossipsub::MessageId::from(hasher.finish().to_be_bytes().to_vec())
            })
            .build()
            .map_err(|e| format!("gossipsub config: {}", e))?;
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| format!("gossipsub behaviour: {}", e))?;

        // Rendezvous
        let rendezvous_client = rendezvous::client::Behaviour::new(keypair.clone());
        let rendezvous_server = rendezvous::server::Behaviour::new(
            rendezvous::server::Config::default(),
        );

        // DCUtR
        let dcutr = dcutr::Behaviour::new(local_peer_id);

        // AutoNAT
        let autonat = autonat::Behaviour::new(
            local_peer_id,
            autonat::Config {
                retry_interval: Duration::from_secs(60),
                refresh_interval: Duration::from_secs(300),
                confidence_max: 3,
                ..Default::default()
            },
        );

        // Stream
        let stream = libp2p_stream::Behaviour::new();

        Ok(Self {
            kademlia,
            kademlia_secondary,
            identify,
            mdns,
            gossipsub,
            rendezvous_client,
            rendezvous_server,
            relay_client,
            dcutr,
            autonat,
            stream,
        })
    }

    // =========================================================================
    // Generic gossipsub methods
    // =========================================================================

    /// Subscribe to a gossipsub topic by name.
    pub fn subscribe_topic(&mut self, topic: &str) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = gossipsub::IdentTopic::new(topic);
        self.gossipsub.subscribe(&topic)
    }

    /// Unsubscribe from a gossipsub topic by name.
    pub fn unsubscribe_topic(&mut self, topic: &str) -> bool {
        let topic = gossipsub::IdentTopic::new(topic);
        self.gossipsub.unsubscribe(&topic)
    }

    /// Publish data to a gossipsub topic.
    pub fn publish_to_topic(
        &mut self,
        topic: &str,
        data: Vec<u8>,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        let topic = gossipsub::IdentTopic::new(topic);
        self.gossipsub.publish(topic, data)
    }

    // =========================================================================
    // Generic DHT methods
    // =========================================================================

    /// Store a record in the DHT.
    pub fn put_record(
        &mut self,
        key: &[u8],
        value: Vec<u8>,
        publisher: &PeerId,
        ttl: Option<Duration>,
    ) -> Result<kad::QueryId, kad::store::Error> {
        let record = kad::Record {
            key: kad::RecordKey::new(&key),
            value,
            publisher: Some(*publisher),
            expires: ttl.map(|d| std::time::Instant::now() + d),
        };
        self.kademlia.put_record(record, kad::Quorum::One)
    }

    /// Retrieve a record from the DHT.
    pub fn get_record(&mut self, key: &[u8]) -> kad::QueryId {
        let key = kad::RecordKey::new(&key);
        self.kademlia.get_record(key)
    }

    /// Announce this node as a provider for the given key.
    pub fn start_providing(&mut self, key: &[u8]) -> Result<kad::QueryId, kad::store::Error> {
        let key = kad::RecordKey::new(&key);
        self.kademlia.start_providing(key)
    }

    /// Stop providing for the given key.
    pub fn stop_providing(&mut self, key: &[u8]) {
        let key = kad::RecordKey::new(&key);
        self.kademlia.stop_providing(&key);
    }

    /// Query the DHT for providers of the given key.
    pub fn get_providers(&mut self, key: &[u8]) -> kad::QueryId {
        let key = kad::RecordKey::new(&key);
        self.kademlia.get_providers(key)
    }

    // =========================================================================
    // Peer / Kademlia routing
    // =========================================================================

    /// Add a known peer address to both Kademlia routing tables.
    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        self.kademlia.add_address(peer_id, addr.clone());
        if let Some(ref mut kad2) = self.kademlia_secondary.as_mut() {
            kad2.add_address(peer_id, addr);
        }
    }

    /// Bootstrap the primary Kademlia DHT.
    pub fn bootstrap(&mut self) -> Result<kad::QueryId, kad::NoKnownPeers> {
        self.kademlia.bootstrap()
    }

    /// Bootstrap the secondary Kademlia DHT (if enabled).
    pub fn bootstrap_secondary(&mut self) -> Option<Result<kad::QueryId, kad::NoKnownPeers>> {
        self.kademlia_secondary.as_mut().map(|kad2| kad2.bootstrap())
    }

    // =========================================================================
    // Secondary Kademlia DHT methods
    // =========================================================================

    /// Get a mutable reference to the secondary Kademlia DHT (if enabled).
    pub fn kademlia_secondary_mut(&mut self) -> Option<&mut kad::Behaviour<kad::store::MemoryStore>> {
        self.kademlia_secondary.as_mut()
    }

    /// Get an immutable reference to the secondary Kademlia DHT (if enabled).
    pub fn kademlia_secondary_ref(&self) -> Option<&kad::Behaviour<kad::store::MemoryStore>> {
        self.kademlia_secondary.as_ref()
    }

    /// Store a record in the secondary DHT.
    pub fn put_record_secondary(
        &mut self,
        key: &[u8],
        value: Vec<u8>,
        publisher: &PeerId,
        ttl: Option<Duration>,
    ) -> Option<Result<kad::QueryId, kad::store::Error>> {
        self.kademlia_secondary.as_mut().map(|kad2| {
            let record = kad::Record {
                key: kad::RecordKey::new(&key),
                value,
                publisher: Some(*publisher),
                expires: ttl.map(|d| std::time::Instant::now() + d),
            };
            kad2.put_record(record, kad::Quorum::One)
        })
    }

    /// Retrieve a record from the secondary DHT.
    pub fn get_record_secondary(&mut self, key: &[u8]) -> Option<kad::QueryId> {
        self.kademlia_secondary.as_mut().map(|kad2| {
            let key = kad::RecordKey::new(&key);
            kad2.get_record(key)
        })
    }

    /// Announce as provider in the secondary DHT.
    pub fn start_providing_secondary(&mut self, key: &[u8]) -> Option<Result<kad::QueryId, kad::store::Error>> {
        self.kademlia_secondary.as_mut().map(|kad2| {
            let key = kad::RecordKey::new(&key);
            kad2.start_providing(key)
        })
    }

    /// Stop providing in the secondary DHT.
    pub fn stop_providing_secondary(&mut self, key: &[u8]) {
        if let Some(kad2) = self.kademlia_secondary.as_mut() {
            let key = kad::RecordKey::new(&key);
            kad2.stop_providing(&key);
        }
    }

    /// Query providers in the secondary DHT.
    pub fn get_providers_secondary(&mut self, key: &[u8]) -> Option<kad::QueryId> {
        self.kademlia_secondary.as_mut().map(|kad2| {
            let key = kad::RecordKey::new(&key);
            kad2.get_providers(key)
        })
    }

    /// Whether the secondary Kademlia DHT is enabled.
    pub fn has_secondary_kad(&self) -> bool {
        self.kademlia_secondary.is_enabled()
    }

    // =========================================================================
    // Rendezvous
    // =========================================================================

    /// Register with a rendezvous server under the given namespace.
    pub fn register_with_rendezvous(
        &mut self,
        namespace: &str,
        rendezvous_peer: PeerId,
    ) -> Result<(), rendezvous::client::RegisterError> {
        let ns = rendezvous::Namespace::new(namespace.to_string())
            .expect("namespace too long");
        self.rendezvous_client.register(ns, rendezvous_peer, None)
    }

    /// Discover peers from a rendezvous server.
    pub fn discover_from_rendezvous(
        &mut self,
        namespace: &str,
        rendezvous_peer: PeerId,
        cookie: Option<rendezvous::Cookie>,
    ) {
        let ns = rendezvous::Namespace::new(namespace.to_string())
            .expect("namespace too long");
        self.rendezvous_client.discover(Some(ns), cookie, None, rendezvous_peer);
    }

    // =========================================================================
    // Stream protocol
    // =========================================================================

    /// Get a Control handle for the persistent stream protocol.
    ///
    /// Use this to register application-level stream protocols:
    /// ```ignore
    /// let control = behaviour.stream_control();
    /// let incoming = control.accept(StreamProtocol::new("/myapp/transfer/1.0.0"))?;
    /// ```
    pub fn stream_control(&self) -> libp2p_stream::Control {
        self.stream.new_control()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_conversions() {
        // Verify From impls compile
        fn _kad(e: kad::Event) -> CraftBehaviourEvent { e.into() }
        fn _id(e: identify::Event) -> CraftBehaviourEvent { e.into() }
        fn _mdns(e: mdns::Event) -> CraftBehaviourEvent { e.into() }
        fn _gossip(e: gossipsub::Event) -> CraftBehaviourEvent { e.into() }
        fn _rv_c(e: rendezvous::client::Event) -> CraftBehaviourEvent { e.into() }
        fn _rv_s(e: rendezvous::server::Event) -> CraftBehaviourEvent { e.into() }
        fn _relay(e: relay::client::Event) -> CraftBehaviourEvent { e.into() }
        fn _dcutr(e: dcutr::Event) -> CraftBehaviourEvent { e.into() }
        fn _autonat(e: autonat::Event) -> CraftBehaviourEvent { e.into() }
    }
}
