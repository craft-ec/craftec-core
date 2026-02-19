use std::collections::HashSet;

/// Capabilities that can be granted to an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Capability {
    ReadOwnDb,
    WriteOwnDb,
    StorageRead,
    StorageWrite,
    NetworkSend,
    DhtAccess,
    SpawnAgent,
}

/// A set of capabilities granted to an agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionSet {
    capabilities: HashSet<Capability>,
}

impl PermissionSet {
    pub fn new(caps: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            capabilities: caps.into_iter().collect(),
        }
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn check(&self, cap: Capability) -> Result<(), PermissionError> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(PermissionError::Denied(cap))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("permission denied: {0:?}")]
    Denied(Capability),
}

/// Full access for whitelisted Tier 1 infrastructure agents.
pub const TIER1_PERMISSIONS: &[Capability] = &[
    Capability::ReadOwnDb,
    Capability::WriteOwnDb,
    Capability::StorageRead,
    Capability::StorageWrite,
    Capability::NetworkSend,
    Capability::DhtAccess,
    Capability::SpawnAgent,
];

/// Restricted access for sandboxed Tier 2 user agents.
pub const TIER2_PERMISSIONS: &[Capability] = &[
    Capability::ReadOwnDb,
    Capability::WriteOwnDb,
];
