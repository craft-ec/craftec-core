//! Fixed-size chunking for erasure-coded payloads
//!
//! Data is split into configurable-size chunks before erasure coding.
//! Each chunk is independently erasure coded.

use std::collections::BTreeMap;

use crate::{ErasureConfig, ErasureCoder, ErasureError, Result};

/// Split data into chunks and erasure code each independently.
///
/// Returns `Vec<(chunk_index, shard_payloads)>` where each `shard_payloads`
/// contains exactly `total_shards` payload buffers.
pub fn chunk_and_encode(data: &[u8], config: &ErasureConfig) -> Result<Vec<(u16, Vec<Vec<u8>>)>> {
    if data.is_empty() {
        return Err(ErasureError::EmptyData);
    }

    let coder = ErasureCoder::with_config(config)?;
    let num_chunks = data.len().div_ceil(config.chunk_size);
    let mut result = Vec::with_capacity(num_chunks);

    for i in 0..num_chunks {
        let start = i * config.chunk_size;
        let end = std::cmp::min(start + config.chunk_size, data.len());
        let chunk = &data[start..end];

        let shard_payloads = coder.encode(chunk)?;
        result.push((i as u16, shard_payloads));
    }

    Ok(result)
}

/// Reassemble reconstructed chunks into original data.
///
/// `chunks` maps `chunk_index → reconstructed chunk data`.
/// `total_chunks` is the expected number of chunks.
/// `original_len` is the total original data length (for trimming the last chunk's padding).
pub fn reassemble(
    chunks: &BTreeMap<u16, Vec<u8>>,
    total_chunks: u16,
    original_len: usize,
) -> Result<Vec<u8>> {
    if chunks.len() < total_chunks as usize {
        return Err(ErasureError::DecodingFailed(format!(
            "Missing chunks: have {}, need {}",
            chunks.len(),
            total_chunks
        )));
    }

    let mut data = Vec::with_capacity(original_len);
    for i in 0..total_chunks {
        let chunk = chunks.get(&i).ok_or_else(|| {
            ErasureError::DecodingFailed(format!("Missing chunk {}", i))
        })?;
        data.extend_from_slice(chunk);
    }

    data.truncate(original_len);

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ErasureConfig {
        ErasureConfig::default()
    }

    fn datacraft_config() -> ErasureConfig {
        ErasureConfig {
            data_shards: 4,
            parity_shards: 4,
            chunk_size: 65536,
        }
    }

    #[test]
    fn test_small_data_single_chunk() {
        let config = default_config();
        let data = b"Hello, Craftec!";
        let chunks = chunk_and_encode(data, &config).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks[0].1.len(), config.total_shards());
    }

    #[test]
    fn test_roundtrip_default_config() {
        let config = default_config();
        let data = b"Small payload under chunk size";
        let encoded = chunk_and_encode(data, &config).unwrap();

        let coder = ErasureCoder::with_config(&config).unwrap();
        let mut chunks_map = BTreeMap::new();

        for (chunk_idx, shard_payloads) in &encoded {
            let mut opts: Vec<Option<Vec<u8>>> =
                shard_payloads.iter().map(|p| Some(p.clone())).collect();
            let shard_size = shard_payloads[0].len();
            let max_len = shard_size * config.data_shards;
            let chunk_data = coder.decode(&mut opts, max_len).unwrap();
            chunks_map.insert(*chunk_idx, chunk_data);
        }

        let total_chunks = encoded.len() as u16;
        let result = reassemble(&chunks_map, total_chunks, data.len()).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_roundtrip_datacraft_config() {
        let config = datacraft_config();
        let data: Vec<u8> = (0..200_000).map(|i| (i % 256) as u8).collect();
        let encoded = chunk_and_encode(&data, &config).unwrap();

        let coder = ErasureCoder::with_config(&config).unwrap();
        let mut chunks_map = BTreeMap::new();

        for (chunk_idx, shard_payloads) in &encoded {
            let mut opts: Vec<Option<Vec<u8>>> =
                shard_payloads.iter().map(|p| Some(p.clone())).collect();
            let shard_size = shard_payloads[0].len();
            let max_len = shard_size * config.data_shards;
            let chunk_data = coder.decode(&mut opts, max_len).unwrap();
            chunks_map.insert(*chunk_idx, chunk_data);
        }

        let total_chunks = encoded.len() as u16;
        let result = reassemble(&chunks_map, total_chunks, data.len()).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_roundtrip_with_missing_shards() {
        let config = datacraft_config();
        let data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
        let encoded = chunk_and_encode(&data, &config).unwrap();

        let coder = ErasureCoder::with_config(&config).unwrap();
        let mut chunks_map = BTreeMap::new();

        for (chunk_idx, shard_payloads) in &encoded {
            let mut opts: Vec<Option<Vec<u8>>> =
                shard_payloads.iter().map(|p| Some(p.clone())).collect();
            // Drop 4 shards per chunk (max tolerable for 4/4)
            opts[0] = None;
            opts[2] = None;
            opts[5] = None;
            opts[7] = None;

            let shard_size = shard_payloads[0].len();
            let max_len = shard_size * config.data_shards;
            let chunk_data = coder.decode(&mut opts, max_len).unwrap();
            chunks_map.insert(*chunk_idx, chunk_data);
        }

        let total_chunks = encoded.len() as u16;
        let result = reassemble(&chunks_map, total_chunks, data.len()).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_empty_data_error() {
        let config = default_config();
        let result = chunk_and_encode(b"", &config);
        assert!(matches!(result, Err(ErasureError::EmptyData)));
    }

    #[test]
    fn test_reassemble_missing_chunk() {
        let mut chunks = BTreeMap::new();
        chunks.insert(0u16, vec![0u8; 100]);

        let result = reassemble(&chunks, 2, 200);
        assert!(result.is_err());
    }
}
