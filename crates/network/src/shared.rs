//! Shared swarm communication primitives.
//!
//! Provides the `SharedSwarmCommand` and `SharedSwarmEvent` enums used to decouple
//! application event loops from the ownership of the `libp2p::Swarm`. This allows
//! multiple application protocols (e.g. CraftOBJ and CraftNet) to multiplex over
//! a single underlying `Swarm` instance driven by a central coordinator task.

use libp2p::{Multiaddr, PeerId};
use libp2p::gossipsub::TopicHash;
use libp2p::kad::{Record, RecordKey};

/// Defines commands that an application can send to the shared swarm coordinator.
#[derive(Debug)]
pub enum SharedSwarmCommand {
    /// Dial a specific peer.
    Dial(PeerId),
    /// Disconnect from a specific peer.
    Disconnect(PeerId),
    /// Add an address for a peer to the Kademlia routing table.
    AddAddress(PeerId, Multiaddr),
    /// Publish a message to a gossipsub topic.
    PublishGossipsub { topic: String, data: Vec<u8> },
    /// Subscribe to a gossipsub topic.
    SubscribeGossipsub(String),
    /// Unsubscribe from a gossipsub topic.
    UnsubscribeGossipsub(String),
    /// Get a record from the secondary Kademlia DHT.
    GetRecordSecondary(RecordKey),
    /// Retrieve the providers for a key on the secondary Kademlia DHT.
    GetProvidersSecondary(RecordKey),
    /// Bootstrap the secondary Kademlia DHT.
    BootstrapSecondary,
    /// Put a record into the secondary Kademlia DHT.
    PutRecordSecondary(Record),
    /// Start providing a key in the secondary DHT.
    StartProvidingSecondary(RecordKey),
    /// Stop providing a key in the secondary DHT.
    StopProvidingSecondary(RecordKey),
    /// Return the list of connected peers via a oneshot channel.
    GetConnectedPeers(tokio::sync::oneshot::Sender<Vec<PeerId>>),
    /// Check if we are connected to a specific peer via a oneshot channel.
    IsConnected(PeerId, tokio::sync::oneshot::Sender<bool>),
    /// Tell the swarm to start listening on a specific address (standalone mode).
    ListenOn(Multiaddr),
}

/// Defines events that the shared swarm coordinator dispatches to applications.
#[derive(Debug, Clone)]
pub enum SharedSwarmEvent {
    /// Connection to a peer was established.
    ConnectionEstablished(PeerId),
    /// Connection to a peer was closed.
    ConnectionClosed(PeerId),
    /// A Gossipsub message was received.
    GossipsubMessage {
        topic: TopicHash,
        data: Vec<u8>,
        propagation_source: Option<PeerId>,
    },
    /// Discovered peers via mDNS.
    MdnsDiscovered(Vec<(PeerId, Multiaddr)>),
    /// Peer records expired via mDNS.
    MdnsExpired(Vec<(PeerId, Multiaddr)>),
    /// An event from the secondary Kademlia DHT.
    /// Since libp2p::kad::Event has non-cloneable inner structures, we serialize
    /// or represent the critical aspects of the event we care about.
    /// E.g. Query progressing for records.
    KademliaSecondaryRecordFound {
        key: RecordKey,
        value: Vec<u8>,
    },
    KademliaSecondaryProvidersFound {
        key: RecordKey,
        providers: Vec<PeerId>,
    },
    /// AutoNAT inferred the node's NAT status.
    AutoNatStatusChanged(AutoNatStatus),
}

/// A simplified NAT status that is easily cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoNatStatus {
    Public,
    Private,
    Unknown,
}
