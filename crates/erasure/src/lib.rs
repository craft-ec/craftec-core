//! Craftec Erasure Coding
//!
//! Reed-Solomon erasure coding with configurable parameters.
//! Each craft configures its own parameters:
//! - TunnelCraft: 3/2 at 18KB (low-latency streaming)
//! - DataCraft: 4/4 at 64KB (high-redundancy storage)

pub mod chunker;

use reed_solomon_erasure::galois_8::ReedSolomon;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default data shards (TunnelCraft-compatible defaults)
pub const DATA_SHARDS: usize = 3;
/// Default parity shards
pub const PARITY_SHARDS: usize = 2;
/// Default total shards
pub const TOTAL_SHARDS: usize = DATA_SHARDS + PARITY_SHARDS;

/// Configurable erasure coding parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureConfig {
    pub data_shards: usize,
    pub parity_shards: usize,
    pub chunk_size: usize,
}

impl Default for ErasureConfig {
    fn default() -> Self {
        Self {
            data_shards: DATA_SHARDS,
            parity_shards: PARITY_SHARDS,
            chunk_size: 18_432, // 18 KB (TunnelCraft default)
        }
    }
}

impl ErasureConfig {
    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }
}

#[derive(Error, Debug)]
pub enum ErasureError {
    #[error("Failed to create encoder: {0}")]
    EncoderCreationFailed(String),
    #[error("Encoding failed: {0}")]
    EncodingFailed(String),
    #[error("Decoding failed: {0}")]
    DecodingFailed(String),
    #[error("Insufficient shards: have {0}, need at least data_shards")]
    InsufficientShards(usize),
    #[error("Invalid shard size")]
    InvalidShardSize,
    #[error("Empty data")]
    EmptyData,
}

pub type Result<T> = std::result::Result<T, ErasureError>;

/// Erasure coder with configurable parameters
pub struct ErasureCoder {
    rs: ReedSolomon,
    data_shards: usize,
    parity_shards: usize,
}

impl ErasureCoder {
    /// Create a new erasure coder with default parameters (3/2)
    pub fn new() -> Result<Self> {
        Self::with_config_params(DATA_SHARDS, PARITY_SHARDS)
    }

    /// Create a new erasure coder from an ErasureConfig
    pub fn with_config(config: &ErasureConfig) -> Result<Self> {
        Self::with_config_params(config.data_shards, config.parity_shards)
    }

    /// Create a new erasure coder with explicit parameters
    pub fn with_config_params(data_shards: usize, parity_shards: usize) -> Result<Self> {
        let rs = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|e| ErasureError::EncoderCreationFailed(e.to_string()))?;
        Ok(Self {
            rs,
            data_shards,
            parity_shards,
        })
    }

    pub fn data_shards(&self) -> usize {
        self.data_shards
    }

    pub fn parity_shards(&self) -> usize {
        self.parity_shards
    }

    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    /// Encode data into shards
    pub fn encode(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        if data.is_empty() {
            return Err(ErasureError::EmptyData);
        }

        let shard_size = data.len().div_ceil(self.data_shards);
        let total = self.total_shards();

        let mut shards: Vec<Vec<u8>> = Vec::with_capacity(total);
        for i in 0..self.data_shards {
            let start = i * shard_size;
            let end = std::cmp::min(start + shard_size, data.len());
            let mut shard = vec![0u8; shard_size];
            if start < data.len() {
                shard[..end - start].copy_from_slice(&data[start..end]);
            }
            shards.push(shard);
        }

        for _ in 0..self.parity_shards {
            shards.push(vec![0u8; shard_size]);
        }

        self.rs
            .encode(&mut shards)
            .map_err(|e| ErasureError::EncodingFailed(e.to_string()))?;

        Ok(shards)
    }

    /// Decode data from shards
    pub fn decode(&self, shards: &mut [Option<Vec<u8>>], original_len: usize) -> Result<Vec<u8>> {
        let total = self.total_shards();

        if shards.len() != total {
            return Err(ErasureError::InvalidShardSize);
        }

        // Count present shards and validate sizes
        let mut present = 0;
        let mut shard_size = 0;
        for s in shards.iter().flatten() {
            if shard_size == 0 {
                shard_size = s.len();
            } else if s.len() != shard_size {
                return Err(ErasureError::InvalidShardSize);
            }
            present += 1;
        }

        if present < self.data_shards {
            return Err(ErasureError::InsufficientShards(present));
        }

        self.rs
            .reconstruct(shards)
            .map_err(|e| ErasureError::DecodingFailed(e.to_string()))?;

        // Reassemble data from data shards
        let mut result = Vec::with_capacity(shard_size * self.data_shards);
        for s in shards.iter().take(self.data_shards).flatten() {
            result.extend_from_slice(s);
        }

        result.truncate(original_len);
        Ok(result)
    }

    /// Verify that enough shards are present for reconstruction
    pub fn verify(&self, shards: &[Option<Vec<u8>>]) -> bool {
        let present = shards.iter().filter(|s| s.is_some()).count();
        present >= self.data_shards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erasure_config_default() {
        let config = ErasureConfig::default();
        assert_eq!(config.data_shards, 3);
        assert_eq!(config.parity_shards, 2);
        assert_eq!(config.chunk_size, 18_432);
        assert_eq!(config.total_shards(), 5);
    }

    #[test]
    fn test_erasure_config_serde() {
        let config = ErasureConfig {
            data_shards: 4,
            parity_shards: 4,
            chunk_size: 65536,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ErasureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_encode_decode_default() {
        let coder = ErasureCoder::new().unwrap();
        let data = b"Hello, Craftec!";

        let shards = coder.encode(data).unwrap();
        assert_eq!(shards.len(), TOTAL_SHARDS);

        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        let decoded = coder.decode(&mut shard_opts, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_with_config() {
        let config = ErasureConfig {
            data_shards: 4,
            parity_shards: 4,
            chunk_size: 65536,
        };
        let coder = ErasureCoder::with_config(&config).unwrap();
        let data = b"DataCraft erasure coding test";

        let shards = coder.encode(data).unwrap();
        assert_eq!(shards.len(), 8);

        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        let decoded = coder.decode(&mut shard_opts, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_with_missing_shards() {
        let coder = ErasureCoder::new().unwrap();
        let data = b"Test data for reconstruction";

        let shards = coder.encode(data).unwrap();
        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        // Remove 2 shards (maximum tolerable loss for 3/2)
        shard_opts[0] = None;
        shard_opts[3] = None;

        let decoded = coder.decode(&mut shard_opts, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_insufficient_shards() {
        let coder = ErasureCoder::new().unwrap();
        let data = b"Test data";

        let shards = coder.encode(data).unwrap();
        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        // Remove 3 shards (too many for 3/2)
        shard_opts[0] = None;
        shard_opts[1] = None;
        shard_opts[2] = None;

        let result = coder.decode(&mut shard_opts, data.len());
        assert!(matches!(result, Err(ErasureError::InsufficientShards(_))));
    }

    #[test]
    fn test_verify() {
        let coder = ErasureCoder::new().unwrap();
        let data = b"Verify test";

        let shards = coder.encode(data).unwrap();
        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        assert!(coder.verify(&shard_opts));

        shard_opts[0] = None;
        shard_opts[1] = None;
        assert!(coder.verify(&shard_opts));

        shard_opts[2] = None;
        assert!(!coder.verify(&shard_opts));
    }

    #[test]
    fn test_empty_data() {
        let coder = ErasureCoder::new().unwrap();
        let result = coder.encode(b"");
        assert!(matches!(result, Err(ErasureError::EmptyData)));
    }

    #[test]
    fn test_4_4_config() {
        let coder = ErasureCoder::with_config_params(4, 4).unwrap();
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        let shards = coder.encode(&data).unwrap();
        assert_eq!(shards.len(), 8);

        let mut shard_opts: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        // Remove 4 shards (maximum tolerable for 4/4)
        shard_opts[0] = None;
        shard_opts[2] = None;
        shard_opts[5] = None;
        shard_opts[7] = None;

        let decoded = coder.decode(&mut shard_opts, data.len()).unwrap();
        assert_eq!(decoded, data);
    }
}
