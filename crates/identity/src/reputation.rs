use serde::{Deserialize, Serialize};

/// Stub reputation score for a peer. Computation logic will come with the aggregator.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReputationScore {
    pub storage_receipts_count: u64,
    pub uptime_ratio: f64,
    pub challenges_passed: u64,
    pub challenges_failed: u64,
    pub peer_endorsements: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let score = ReputationScore {
            storage_receipts_count: 42,
            uptime_ratio: 0.99,
            challenges_passed: 100,
            challenges_failed: 1,
            peer_endorsements: 5,
        };
        let json = serde_json::to_string(&score).unwrap();
        let back: ReputationScore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, score);
    }
}
