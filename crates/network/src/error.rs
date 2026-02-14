use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Failed to build swarm: {0}")]
    SwarmBuildError(String),
    #[error("Failed to listen: {0}")]
    ListenError(String),
    #[error("Failed to dial: {0}")]
    DialError(String),
    #[error("DHT error: {0}")]
    DhtError(String),
    #[error("Gossipsub error: {0}")]
    GossipsubError(String),
    #[error("Bootstrap error: {0}")]
    BootstrapError(String),
    #[error("Transport error: {0}")]
    TransportError(String),
}
