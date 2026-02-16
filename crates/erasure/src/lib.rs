//! Craftec Erasure Coding — RLNC (Random Linear Network Coding) over GF(2^8)
//!
//! Content is split into segments (default 10MB), each segment into k source pieces
//! (default 100KB each). Coded pieces are random linear combinations of source pieces
//! with coefficient vectors enabling reconstruction via Gaussian elimination.
//!
//! ## TODO: Performance Optimizations
//! - **SIMD first** (biggest win): GF(2^8) multiply-accumulate via SSE/AVX2/NEON intrinsics.
//!   Single-threaded SIMD beats naive rayon parallelism for RLNC — the inner loop is
//!   sequential memory access over ~10MB (fits L3 cache), thread coordination overhead
//!   and cache thrashing negate rayon's benefit at this granularity. See `rlnc` crate's
//!   `gf256_inplace_mul_vec_by_scalar` for reference implementation.
//! - **Rayon across segments** (outer loop only): Parallelize encoding/decoding of independent
//!   segments. NOT within a single piece's GF(2^8) computation — that's SIMD territory.
//! - **Evaluate `rlnc` crate** (https://crates.io/crates/rlnc): Published GF(2^8) RLNC
//!   with SIMD support. Consider adopting their GF(2^8) primitives or the full crate.

pub mod gf256;
pub mod segmenter;

use gf256::GF256;
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default piece size: 100 KB
pub const DEFAULT_PIECE_SIZE: usize = 102_400;
/// Default segment size: 10 MB
pub const DEFAULT_SEGMENT_SIZE: usize = 10_240_000;
/// Default initial parity: 20% extra coded pieces
pub const DEFAULT_INITIAL_PARITY: usize = 20;

/// Configurable erasure coding parameters for RLNC
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureConfig {
    /// Size of each piece in bytes (default 100KB)
    pub piece_size: usize,
    /// Size of each segment in bytes (default 10MB)
    pub segment_size: usize,
    /// Number of extra coded pieces per segment (percentage, e.g. 20 = 1.2x)
    pub initial_parity: usize,
}

impl Default for ErasureConfig {
    fn default() -> Self {
        Self {
            piece_size: DEFAULT_PIECE_SIZE,
            segment_size: DEFAULT_SEGMENT_SIZE,
            initial_parity: DEFAULT_INITIAL_PARITY,
        }
    }
}

impl ErasureConfig {
    /// Number of source pieces for a full segment
    pub fn k(&self) -> usize {
        self.segment_size / self.piece_size
    }

    /// Number of source pieces for a segment of given byte size
    pub fn k_for_segment(&self, segment_bytes: usize) -> usize {
        segment_bytes.div_ceil(self.piece_size).max(1)
    }

    /// Number of extra coded pieces per segment
    pub fn parity_count(&self, k: usize) -> usize {
        (k * self.initial_parity) / 100
    }
}

#[derive(Error, Debug)]
pub enum ErasureError {
    #[error("Empty data")]
    EmptyData,
    #[error("Encoding failed: {0}")]
    EncodingFailed(String),
    #[error("Decoding failed: {0}")]
    DecodingFailed(String),
    #[error("Insufficient pieces: have {have}, need {need}")]
    InsufficientPieces { have: usize, need: usize },
    #[error("Pieces not linearly independent")]
    LinearlyDependent,
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, ErasureError>;

/// A coded piece: data bytes + coefficient vector
#[derive(Debug, Clone)]
pub struct CodedPiece {
    /// The piece data (piece_size bytes)
    pub data: Vec<u8>,
    /// Coefficient vector (k bytes, one per source piece)
    pub coefficients: Vec<u8>,
}

/// RLNC Encoder: takes source data for a segment and produces coded pieces
pub struct RlncEncoder {
    /// Source pieces (k pieces, each piece_size bytes, last may be zero-padded)
    source_pieces: Vec<Vec<u8>>,
    piece_size: usize,
}

impl RlncEncoder {
    /// Create encoder from raw segment data
    pub fn new(segment_data: &[u8], piece_size: usize) -> Result<Self> {
        if segment_data.is_empty() {
            return Err(ErasureError::EmptyData);
        }
        if piece_size == 0 {
            return Err(ErasureError::InvalidConfig("piece_size must be > 0".into()));
        }

        let k = segment_data.len().div_ceil(piece_size).max(1);
        let mut source_pieces = Vec::with_capacity(k);

        for i in 0..k {
            let start = i * piece_size;
            let end = (start + piece_size).min(segment_data.len());
            let mut piece = vec![0u8; piece_size];
            if start < segment_data.len() {
                piece[..end - start].copy_from_slice(&segment_data[start..end]);
            }
            source_pieces.push(piece);
        }

        Ok(Self {
            source_pieces,
            piece_size,
        })
    }

    /// Number of source pieces (k)
    pub fn k(&self) -> usize {
        self.source_pieces.len()
    }

    /// Generate the k source pieces as coded pieces (identity coefficient vectors)
    pub fn source_pieces(&self) -> Vec<CodedPiece> {
        let k = self.k();
        (0..k)
            .map(|i| {
                let mut coeffs = vec![0u8; k];
                coeffs[i] = 1;
                CodedPiece {
                    data: self.source_pieces[i].clone(),
                    coefficients: coeffs,
                }
            })
            .collect()
    }

    /// Generate n coded pieces with random coefficient vectors
    pub fn generate_coded_pieces(&self, n: usize) -> Vec<CodedPiece> {
        let mut rng = rand::thread_rng();
        (0..n).map(|_| self.generate_one_piece(&mut rng)).collect()
    }

    /// Generate a single random coded piece
    fn generate_one_piece(&self, rng: &mut impl Rng) -> CodedPiece {
        let k = self.k();
        let coeffs: Vec<u8> = (0..k).map(|_| rng.gen()).collect();
        let data = Self::linear_combine(&self.source_pieces, &coeffs, self.piece_size);
        CodedPiece {
            data,
            coefficients: coeffs,
        }
    }

    /// Compute a linear combination of pieces with given coefficients over GF(2^8)
    fn linear_combine(pieces: &[Vec<u8>], coeffs: &[u8], piece_size: usize) -> Vec<u8> {
        let mut result = vec![0u8; piece_size];
        for (piece, &coeff) in pieces.iter().zip(coeffs.iter()) {
            if coeff == 0 {
                continue;
            }
            for (r, &p) in result.iter_mut().zip(piece.iter()) {
                *r = GF256::add(*r, GF256::mul(coeff, p));
            }
        }
        result
    }

    /// Encode a segment: returns source pieces + parity coded pieces
    pub fn encode(&self, initial_parity_count: usize) -> Vec<CodedPiece> {
        let mut pieces = self.source_pieces();
        pieces.extend(self.generate_coded_pieces(initial_parity_count));
        pieces
    }
}

/// RLNC Decoder: collects coded pieces and reconstructs source data via Gaussian elimination
pub struct RlncDecoder {
    k: usize,
    piece_size: usize,
    original_size: usize,
}

impl RlncDecoder {
    /// Create decoder for a segment with k source pieces
    pub fn new(k: usize, piece_size: usize, original_segment_size: usize) -> Self {
        Self {
            k,
            piece_size,
            original_size: original_segment_size,
        }
    }

    /// Decode k linearly independent coded pieces back to original segment data
    pub fn decode(&self, pieces: &[CodedPiece]) -> Result<Vec<u8>> {
        if pieces.len() < self.k {
            return Err(ErasureError::InsufficientPieces {
                have: pieces.len(),
                need: self.k,
            });
        }

        // Build augmented matrix: [coefficients | data]
        // We'll do Gaussian elimination on the coefficient part and apply same ops to data
        let k = self.k;
        let mut coeffs: Vec<Vec<u8>> = pieces.iter().take(k).map(|p| p.coefficients.clone()).collect();
        let mut data: Vec<Vec<u8>> = pieces.iter().take(k).map(|p| p.data.clone()).collect();

        // Gaussian elimination with partial pivoting
        for col in 0..k {
            // Find pivot
            let pivot_row = (col..k)
                .find(|&r| coeffs[r][col] != 0)
                .ok_or(ErasureError::LinearlyDependent)?;

            if pivot_row != col {
                coeffs.swap(col, pivot_row);
                data.swap(col, pivot_row);
            }

            // Scale pivot row
            let inv = GF256::inv(coeffs[col][col])
                .ok_or_else(|| ErasureError::DecodingFailed("zero pivot".into()))?;

            for v in coeffs[col].iter_mut() {
                *v = GF256::mul(*v, inv);
            }
            for v in data[col].iter_mut() {
                *v = GF256::mul(*v, inv);
            }

            // Eliminate column in all other rows
            for row in 0..k {
                if row == col {
                    continue;
                }
                let factor = coeffs[row][col];
                if factor == 0 {
                    continue;
                }
                // Must clone pivot row to avoid borrow issues
                let pivot_coeffs: Vec<u8> = coeffs[col].clone();
                let pivot_data: Vec<u8> = data[col].clone();
                for (cv, &pv) in coeffs[row].iter_mut().zip(pivot_coeffs.iter()) {
                    *cv = GF256::add(*cv, GF256::mul(factor, pv));
                }
                for (dv, &pv) in data[row].iter_mut().zip(pivot_data.iter()) {
                    *dv = GF256::add(*dv, GF256::mul(factor, pv));
                }
            }
        }

        // After elimination, data[i] = source_piece[i]
        let mut result = Vec::with_capacity(k * self.piece_size);
        for row in &data {
            result.extend_from_slice(row);
        }
        result.truncate(self.original_size);
        Ok(result)
    }
}

/// Create a new coded piece by linearly combining existing coded pieces with random coefficients.
///
/// Takes 2+ coded pieces (data + coefficient vector each), returns a new piece
/// that is a random linear combination of the inputs.
pub fn create_piece_from_existing(pieces: &[CodedPiece]) -> Result<CodedPiece> {
    if pieces.len() < 2 {
        return Err(ErasureError::EncodingFailed(
            "need at least 2 pieces to combine".into(),
        ));
    }

    let k = pieces[0].coefficients.len();
    let piece_size = pieces[0].data.len();

    let mut rng = rand::thread_rng();
    let alphas: Vec<u8> = (0..pieces.len())
        .map(|_| {
            // Avoid zero coefficients
            loop {
                let v: u8 = rng.gen();
                if v != 0 {
                    return v;
                }
            }
        })
        .collect();

    let mut new_data = vec![0u8; piece_size];
    let mut new_coeffs = vec![0u8; k];

    for (piece, &alpha) in pieces.iter().zip(alphas.iter()) {
        for (nd, &pd) in new_data.iter_mut().zip(piece.data.iter()) {
            *nd = GF256::add(*nd, GF256::mul(alpha, pd));
        }
        for (nc, &pc) in new_coeffs.iter_mut().zip(piece.coefficients.iter()) {
            *nc = GF256::add(*nc, GF256::mul(alpha, pc));
        }
    }

    Ok(CodedPiece {
        data: new_data,
        coefficients: new_coeffs,
    })
}

/// Check the rank (number of linearly independent vectors) of a set of coefficient vectors.
pub fn check_independence(vectors: &[Vec<u8>]) -> usize {
    if vectors.is_empty() {
        return 0;
    }

    let k = vectors[0].len();
    let mut matrix: Vec<Vec<u8>> = vectors.to_vec();
    let rows = matrix.len();
    let mut rank = 0;

    for col in 0..k {
        // Find pivot in rows[rank..]
        let pivot = (rank..rows).find(|&r| matrix[r][col] != 0);
        let pivot = match pivot {
            Some(p) => p,
            None => continue,
        };

        matrix.swap(rank, pivot);

        let inv = match GF256::inv(matrix[rank][col]) {
            Some(v) => v,
            None => continue,
        };

        // Scale pivot row
        for v in matrix[rank].iter_mut() {
            *v = GF256::mul(*v, inv);
        }

        // Eliminate
        for row in 0..rows {
            if row == rank {
                continue;
            }
            let factor = matrix[row][col];
            if factor == 0 {
                continue;
            }
            let pivot_row: Vec<u8> = matrix[rank].clone();
            for (mv, &pv) in matrix[row].iter_mut().zip(pivot_row.iter()) {
                *mv = GF256::add(*mv, GF256::mul(factor, pv));
            }
        }

        rank += 1;
        if rank == k {
            break;
        }
    }

    rank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erasure_config_default() {
        let config = ErasureConfig::default();
        assert_eq!(config.piece_size, 102_400);
        assert_eq!(config.segment_size, 10_240_000);
        assert_eq!(config.initial_parity, 20);
        assert_eq!(config.k(), 100); // 10_240_000 / 102_400 = 100
    }

    #[test]
    fn test_erasure_config_serde() {
        let config = ErasureConfig {
            piece_size: 1024,
            segment_size: 10240,
            initial_parity: 20,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ErasureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_encode_decode_small() {
        let data = b"Hello, Craftec RLNC!";
        let piece_size = 8;
        let encoder = RlncEncoder::new(data, piece_size).unwrap();
        let k = encoder.k(); // ceil(20/8) = 3

        let pieces = encoder.source_pieces();
        assert_eq!(pieces.len(), k);

        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&pieces).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_with_coded_pieces() {
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 5

        // Generate only coded (random) pieces, no source pieces
        let coded = encoder.generate_coded_pieces(k + 5);
        // Take first k coded pieces (should be enough if independent)
        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&coded[..k]).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_mixed_pieces() {
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let piece_size = 200;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 5

        let all = encoder.encode(3); // 5 source + 3 coded = 8
        assert_eq!(all.len(), 8);

        // Use a mix: skip source piece 0 and 2, use coded pieces instead
        let mut selection: Vec<CodedPiece> = Vec::new();
        selection.push(all[1].clone()); // source 1
        selection.push(all[3].clone()); // source 3
        selection.push(all[4].clone()); // source 4
        selection.push(all[5].clone()); // coded 0
        selection.push(all[6].clone()); // coded 1

        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&selection).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_single_piece_k1() {
        let data = b"tiny";
        let piece_size = 100;
        let encoder = RlncEncoder::new(data, piece_size).unwrap();
        assert_eq!(encoder.k(), 1);

        let pieces = encoder.source_pieces();
        let decoder = RlncDecoder::new(1, piece_size, data.len());
        let decoded = decoder.decode(&pieces).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_create_piece_from_existing() {
        let data: Vec<u8> = (0..300).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 3

        let source = encoder.source_pieces();
        let combined = create_piece_from_existing(&source[0..2]).unwrap();

        // The combined piece should have k coefficients
        assert_eq!(combined.coefficients.len(), k);
        assert_eq!(combined.data.len(), piece_size);

        // Use combined piece + source[2] + source[0] to decode
        let pieces_for_decode = vec![source[0].clone(), source[2].clone(), combined];
        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&pieces_for_decode).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_check_independence_full_rank() {
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();

        let source = encoder.source_pieces();
        let vecs: Vec<Vec<u8>> = source.iter().map(|p| p.coefficients.clone()).collect();
        assert_eq!(check_independence(&vecs), 5);
    }

    #[test]
    fn test_check_independence_dependent() {
        // Create dependent vectors
        let v1 = vec![1u8, 0, 0];
        let v2 = vec![0u8, 1, 0];
        // v3 = v1 XOR v2 (since add is XOR in GF(2^8))
        let v3 = vec![1u8, 1, 0];
        assert_eq!(check_independence(&[v1, v2, v3]), 2);
    }

    #[test]
    fn test_check_independence_empty() {
        assert_eq!(check_independence(&[]), 0);
    }

    #[test]
    fn test_insufficient_pieces() {
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();

        let source = encoder.source_pieces();
        let decoder = RlncDecoder::new(5, piece_size, data.len());
        let result = decoder.decode(&source[..3]);
        assert!(matches!(
            result,
            Err(ErasureError::InsufficientPieces { have: 3, need: 5 })
        ));
    }

    #[test]
    fn test_empty_data_error() {
        let result = RlncEncoder::new(b"", 100);
        assert!(matches!(result, Err(ErasureError::EmptyData)));
    }

    #[test]
    fn test_large_k_roundtrip() {
        // Simulate a realistic scenario with larger k
        let data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 100
        assert_eq!(k, 100);

        let coded = encoder.generate_coded_pieces(k);
        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&coded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_piece_size_not_dividing_evenly() {
        let data: Vec<u8> = (0..250).map(|i| (i % 256) as u8).collect();
        let piece_size = 100; // 250 / 100 = 2.5, ceil = 3
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        assert_eq!(encoder.k(), 3);

        let pieces = encoder.source_pieces();
        let decoder = RlncDecoder::new(3, piece_size, data.len());
        let decoded = decoder.decode(&pieces).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_create_piece_from_three_existing() {
        let data: Vec<u8> = (0..400).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 4

        let source = encoder.source_pieces();
        let combined = create_piece_from_existing(&source[0..3]).unwrap();

        // Decode using combined + source[1] + source[2] + source[3]
        let pieces_for_decode = vec![
            combined,
            source[1].clone(),
            source[2].clone(),
            source[3].clone(),
        ];
        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&pieces_for_decode).unwrap();
        assert_eq!(decoded, data);
    }
}
