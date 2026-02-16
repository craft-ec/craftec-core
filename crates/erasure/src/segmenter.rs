//! Segment-aware RLNC encoding and decoding
//!
//! Content is split into segments of configurable size (default 10MB).
//! Each segment is independently RLNC-encoded into pieces.

use std::collections::BTreeMap;

use crate::{CodedPiece, ErasureConfig, ErasureError, Result, RlncDecoder, RlncEncoder};

/// Split content into segments and encode each with RLNC.
///
/// Returns Vec<(segment_index, Vec<CodedPiece>)> where each segment's pieces
/// include source pieces + initial parity coded pieces.
pub fn segment_and_encode(data: &[u8], config: &ErasureConfig) -> Result<Vec<(u32, Vec<CodedPiece>)>> {
    if data.is_empty() {
        return Err(ErasureError::EmptyData);
    }

    let num_segments = data.len().div_ceil(config.segment_size);
    let mut result = Vec::with_capacity(num_segments);

    for i in 0..num_segments {
        let start = i * config.segment_size;
        let end = (start + config.segment_size).min(data.len());
        let segment_data = &data[start..end];

        let encoder = RlncEncoder::new(segment_data, config.piece_size)?;
        let k = encoder.k();
        let parity_count = config.parity_count(k);
        let pieces = encoder.encode(parity_count);

        result.push((i as u32, pieces));
    }

    Ok(result)
}

/// Decode segments back to original content.
///
/// `segments` maps segment_index → Vec<CodedPiece> (at least k pieces per segment).
/// `config` provides piece_size and segment_size.
/// `original_len` is the total content length for final truncation.
pub fn decode_and_reassemble(
    segments: &BTreeMap<u32, Vec<CodedPiece>>,
    total_segments: u32,
    config: &ErasureConfig,
    original_len: usize,
) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(original_len);

    for i in 0..total_segments {
        let pieces = segments.get(&i).ok_or_else(|| {
            ErasureError::DecodingFailed(format!("missing segment {}", i))
        })?;

        // Determine segment size (last segment may be smaller)
        let segment_start = i as usize * config.segment_size;
        let segment_end = ((i as usize + 1) * config.segment_size).min(original_len);
        let segment_len = segment_end - segment_start;
        let k = config.k_for_segment(segment_len);

        let decoder = RlncDecoder::new(k, config.piece_size, segment_len);
        let decoded = decoder.decode(pieces)?;
        result.extend_from_slice(&decoded);
    }

    result.truncate(original_len);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_config() -> ErasureConfig {
        ErasureConfig {
            piece_size: 100,
            segment_size: 500,
            initial_parity: 20,
        }
    }

    #[test]
    fn test_single_segment_roundtrip() {
        let config = small_config();
        let data: Vec<u8> = (0..400).map(|i| (i % 256) as u8).collect();

        let encoded = segment_and_encode(&data, &config).unwrap();
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].0, 0);

        let mut segments = BTreeMap::new();
        segments.insert(0u32, encoded[0].1.clone());

        let decoded = decode_and_reassemble(&segments, 1, &config, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_multi_segment_roundtrip() {
        let config = small_config();
        let data: Vec<u8> = (0..1200).map(|i| (i % 256) as u8).collect();

        let encoded = segment_and_encode(&data, &config).unwrap();
        assert_eq!(encoded.len(), 3); // 1200/500 = 2.4 → 3 segments

        let mut segments = BTreeMap::new();
        for (idx, pieces) in &encoded {
            segments.insert(*idx, pieces.clone());
        }

        let decoded = decode_and_reassemble(&segments, encoded.len() as u32, &config, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_partial_last_segment() {
        let config = small_config();
        // 550 bytes = 1 full segment (500) + 1 partial (50, k=1)
        let data: Vec<u8> = (0..550).map(|i| (i % 256) as u8).collect();

        let encoded = segment_and_encode(&data, &config).unwrap();
        assert_eq!(encoded.len(), 2);

        let mut segments = BTreeMap::new();
        for (idx, pieces) in &encoded {
            segments.insert(*idx, pieces.clone());
        }

        let decoded = decode_and_reassemble(&segments, 2, &config, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_with_only_coded_pieces() {
        let config = small_config();
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();

        let encoded = segment_and_encode(&data, &config).unwrap();
        let all_pieces = &encoded[0].1;
        let k = config.k_for_segment(data.len()); // 5

        // Use only the coded (parity) pieces, skip source pieces
        // Use a mix: skip first source piece, use rest + parity
        let mut selection: Vec<CodedPiece> = all_pieces[1..].to_vec();
        selection.truncate(k);

        let mut segments = BTreeMap::new();
        segments.insert(0u32, selection);

        let decoded = decode_and_reassemble(&segments, 1, &config, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_empty_data_error() {
        let config = small_config();
        let result = segment_and_encode(b"", &config);
        assert!(matches!(result, Err(ErasureError::EmptyData)));
    }

    #[test]
    fn test_missing_segment_error() {
        let config = small_config();
        let segments = BTreeMap::new();
        let result = decode_and_reassemble(&segments, 1, &config, 100);
        assert!(result.is_err());
    }
}
