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
pub mod homomorphic;
pub mod segmenter;

use gf256::GF256;

// Re-export homomorphic hashing functionality
pub use homomorphic::{
    generate_segment_hashes, verify_piece, ContentVerificationRecord, SegmentHashes,
    HASH_PROJECTIONS,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default piece size: 256 KiB (matches IPFS block size)
pub const DEFAULT_PIECE_SIZE: usize = 262_144;
/// Default segment size: 10 MiB = 40 × 256 KiB, k=40
pub const DEFAULT_SEGMENT_SIZE: usize = 10_485_760;
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

/// Given a set of existing coefficient vectors over GF(2^8), generate a new coefficient vector
/// that is guaranteed to be linearly independent from all of them.
///
/// Returns None if the existing vectors already span the full space (rank == k),
/// meaning no independent vector can be created.
///
/// The returned vector can be used to create a new coded piece via recombination
/// that is guaranteed to increase the rank by 1.
/// `offset` selects which free column to target. Nodes ranked by independence
/// contribution use their rank position as offset — each targets a different
/// dimension of the null space. Offset 0 = first free column, 1 = second, etc.
/// Wraps around if offset >= number of free columns.
pub fn generate_orthogonal_vector(existing: &[Vec<u8>], k: usize, offset: usize) -> Option<Vec<u8>> {
    if k == 0 {
        return None;
    }
    if existing.is_empty() {
        // Return e_0
        let mut v = vec![0u8; k];
        v[0] = 1;
        return Some(v);
    }

    // Gaussian elimination to find pivot columns
    let mut matrix: Vec<Vec<u8>> = existing.to_vec();
    let rows = matrix.len();
    let mut rank = 0;
    let mut pivot_cols: Vec<usize> = Vec::new();

    for col in 0..k {
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

        for v in matrix[rank].iter_mut() {
            *v = GF256::mul(*v, inv);
        }

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

        pivot_cols.push(col);
        rank += 1;
        if rank == k {
            break;
        }
    }

    if rank >= k {
        return None; // Full rank, no independent vector possible
    }

    // Find all free (non-pivot) columns
    let free_cols: Vec<usize> = (0..k).filter(|c| !pivot_cols.contains(c)).collect();

    if free_cols.is_empty() {
        return None;
    }

    // Use offset to select which free column this node targets.
    // Different nodes use different offsets → each creates a piece in a different
    // dimension of the null space → zero collisions even with simultaneous repair.
    let col_index = offset % free_cols.len();
    let free_col = free_cols[col_index];

    // Construct vector: e_{free_col} — a standard basis vector at the free column.
    // This is independent because no existing vector (after elimination) has a pivot there.
    let mut result = vec![0u8; k];
    result[free_col] = 1;
    Some(result)
}

/// Create a new coded piece that is guaranteed linearly independent from existing pieces.
/// Uses generate_orthogonal_vector to find the right coefficients, then linearly combines
/// the given local pieces using those target coefficients.
///
/// `local_pieces` — pieces this node holds (used as basis for recombination)
/// `all_existing_coefficients` — coefficient vectors from ALL network providers (from PieceMap)
/// `k` — number of source pieces for this segment
///
/// Returns None if existing vectors already span the full space or local pieces are insufficient.
/// `offset` — this node's rank position among providers. Each node targets a different
/// free column, so simultaneous repairs create pieces in different null space dimensions.
pub fn create_orthogonal_piece(
    local_pieces: &[CodedPiece],
    all_existing_coefficients: &[Vec<u8>],
    k: usize,
    offset: usize,
) -> Option<CodedPiece> {
    if local_pieces.is_empty() || k == 0 {
        return None;
    }

    let target = generate_orthogonal_vector(all_existing_coefficients, k, offset)?;

    // We need to find alphas such that: sum(alpha_i * local_pieces[i].coefficients) = target
    // This is a linear system over GF(2^8).
    // Build matrix where columns are local piece coefficient vectors, solve for alphas.

    let n = local_pieces.len();
    // Build augmented matrix [local_coeffs^T | target] — but easier to think row-wise.
    // We have k equations (one per coefficient position), n unknowns (alphas).
    // Row j: sum_i(alpha_i * local_pieces[i].coefficients[j]) = target[j]

    let mut aug: Vec<Vec<u8>> = (0..k)
        .map(|j| {
            let mut row = Vec::with_capacity(n + 1);
            for piece in local_pieces.iter().take(n) {
                row.push(piece.coefficients[j]);
            }
            row.push(target[j]);
            row
        })
        .collect();

    // Gaussian elimination on this k×(n+1) augmented matrix
    let mut rank = 0;
    let mut pivot_cols: Vec<usize> = Vec::new();

    for col in 0..n {
        let pivot = (rank..k).find(|&r| aug[r][col] != 0);
        let pivot = match pivot {
            Some(p) => p,
            None => continue,
        };

        aug.swap(rank, pivot);

        let inv = GF256::inv(aug[rank][col])?;
        for v in aug[rank].iter_mut() {
            *v = GF256::mul(*v, inv);
        }

        for row in 0..k {
            if row == rank {
                continue;
            }
            let factor = aug[row][col];
            if factor == 0 {
                continue;
            }
            let pivot_row: Vec<u8> = aug[rank].clone();
            for (mv, &pv) in aug[row].iter_mut().zip(pivot_row.iter()) {
                *mv = GF256::add(*mv, GF256::mul(factor, pv));
            }
        }

        pivot_cols.push(col);
        rank += 1;
        if rank == k {
            break;
        }
    }

    // Check consistency: any row with all-zero LHS but non-zero RHS means no solution
    for aug_row in aug.iter().take(k).skip(rank) {
        if aug_row[n] != 0 {
            return None; // Local pieces can't express the target vector
        }
    }

    // Extract alphas (set free variables to 0)
    let mut alphas = vec![0u8; n];
    for (idx, &col) in pivot_cols.iter().enumerate() {
        alphas[col] = aug[idx][n];
    }

    // Compute the new piece
    let piece_size = local_pieces[0].data.len();
    let mut new_data = vec![0u8; piece_size];
    let mut new_coeffs = vec![0u8; k];

    for (piece, &alpha) in local_pieces.iter().zip(alphas.iter()) {
        if alpha == 0 {
            continue;
        }
        for (nd, &pd) in new_data.iter_mut().zip(piece.data.iter()) {
            *nd = GF256::add(*nd, GF256::mul(alpha, pd));
        }
        for (nc, &pc) in new_coeffs.iter_mut().zip(piece.coefficients.iter()) {
            *nc = GF256::add(*nc, GF256::mul(alpha, pc));
        }
    }

    Some(CodedPiece {
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
        assert_eq!(config.piece_size, 262_144);
        assert_eq!(config.segment_size, 10_485_760);
        assert_eq!(config.initial_parity, 20);
        assert_eq!(config.k(), 40); // 10_485_760 / 262_144 = 40
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
    fn test_generate_orthogonal_vector_rank_less_than_k() {
        let existing = vec![
            vec![1u8, 0, 0, 0],
            vec![0u8, 1, 0, 0],
        ];
        let result = generate_orthogonal_vector(&existing, 4, 0).unwrap();
        assert_eq!(result.len(), 4);
        // Adding the new vector should increase rank
        let mut all = existing.clone();
        all.push(result);
        assert_eq!(check_independence(&all), 3);
    }

    #[test]
    fn test_generate_orthogonal_vector_full_rank() {
        let existing = vec![
            vec![1u8, 0, 0],
            vec![0u8, 1, 0],
            vec![0u8, 0, 1],
        ];
        assert!(generate_orthogonal_vector(&existing, 3, 0).is_none());
    }

    #[test]
    fn test_generate_orthogonal_sequential() {
        let k = 5;
        let mut existing: Vec<Vec<u8>> = Vec::new();
        for i in 0..k {
            let v = generate_orthogonal_vector(&existing, k, 0).unwrap();
            assert_eq!(v.len(), k);
            existing.push(v);
            assert_eq!(check_independence(&existing), i + 1);
        }
        // Now full rank, should return None
        assert!(generate_orthogonal_vector(&existing, k, 0).is_none());
    }

    #[test]
    fn test_generate_orthogonal_empty_existing() {
        let v = generate_orthogonal_vector(&[], 3, 0).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(check_independence(&[v]), 1);
    }

    #[test]
    fn test_generate_orthogonal_k1() {
        let v = generate_orthogonal_vector(&[], 1, 0).unwrap();
        assert_eq!(v, vec![1]);
        assert!(generate_orthogonal_vector(&[vec![1]], 1, 0).is_none());
    }

    #[test]
    fn test_generate_orthogonal_k2() {
        let existing = vec![vec![1u8, 1]];
        let v = generate_orthogonal_vector(&existing, 2, 0).unwrap();
        let mut all = existing;
        all.push(v);
        assert_eq!(check_independence(&all), 2);
    }

    #[test]
    fn test_create_orthogonal_piece() {
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let encoder = RlncEncoder::new(&data, piece_size).unwrap();
        let k = encoder.k(); // 5

        let source = encoder.source_pieces();
        // Pretend we have source pieces 0,1,2 locally, and the network has pieces 0,1
        let local = vec![source[0].clone(), source[1].clone(), source[2].clone()];
        let existing_coeffs: Vec<Vec<u8>> = vec![
            source[0].coefficients.clone(),
            source[1].coefficients.clone(),
        ];

        let new_piece = create_orthogonal_piece(&local, &existing_coeffs, k, 0).unwrap();
        assert_eq!(new_piece.coefficients.len(), k);
        assert_eq!(new_piece.data.len(), piece_size);

        // Verify independence
        let mut all = existing_coeffs;
        all.push(new_piece.coefficients.clone());
        assert_eq!(check_independence(&all), 3);

        // Verify data correctness: decode using all 5 source pieces replaced with our new piece
        let pieces_for_decode = vec![
            source[0].clone(),
            source[1].clone(),
            new_piece,
            source[3].clone(),
            source[4].clone(),
        ];
        let decoder = RlncDecoder::new(k, piece_size, data.len());
        let decoded = decoder.decode(&pieces_for_decode).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_create_orthogonal_piece_insufficient_local() {
        // Local pieces don't span the needed direction
        let k = 3;
        let local = vec![CodedPiece {
            data: vec![0u8; 10],
            coefficients: vec![1, 0, 0],
        }];
        // Existing already has e_0, so orthogonal would be e_1 or e_2
        // But local only has e_0, can't produce e_1 or e_2
        let existing = vec![vec![1u8, 0, 0]];
        assert!(create_orthogonal_piece(&local, &existing, k, 0).is_none());
    }

    #[test]
    fn test_orthogonal_offset_targets_different_columns() {
        // Two nodes repair simultaneously with different offsets → different pieces
        let existing = vec![
            vec![1u8, 0, 0, 0, 0],
            vec![0u8, 1, 0, 0, 0],
        ];
        let k = 5;
        // 3 free columns (2,3,4). Each offset targets a different one.
        let v0 = generate_orthogonal_vector(&existing, k, 0).unwrap();
        let v1 = generate_orthogonal_vector(&existing, k, 1).unwrap();
        let v2 = generate_orthogonal_vector(&existing, k, 2).unwrap();

        // All three should be different
        assert_ne!(v0, v1);
        assert_ne!(v1, v2);
        assert_ne!(v0, v2);

        // All should be independent from existing AND from each other
        let mut all = existing.clone();
        all.push(v0.clone());
        all.push(v1);
        all.push(v2);
        assert_eq!(check_independence(&all), 5); // full rank

        // Offset wraps: offset=3 should equal offset=0
        let v3 = generate_orthogonal_vector(&existing, k, 3).unwrap();
        assert_eq!(v3, v0);
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
