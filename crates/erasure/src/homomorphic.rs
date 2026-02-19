//! Homomorphic Hashing for RLNC Piece Verification
//!
//! Uses random projection vectors over GF(2^8) to create homomorphic hashes that work
//! with both original and recombined pieces. If `piece = c1*p1 + c2*p2 + ... + ck*pk`,
//! then `dot(r, piece) = c1*dot(r, p1) + c2*dot(r, p2) + ... + ck*dot(r, pk)`.

use crate::{gf256::GF256, CodedPiece};
use serde::{Deserialize, Serialize};

/// Number of hash projections for 128-bit security (forgery probability = (1/256)^16)
pub const HASH_PROJECTIONS: usize = 16;

/// Homomorphic hash parameters for a segment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentHashes {
    /// Hash seed (32 bytes) — used to regenerate random projection vectors
    pub seed: [u8; 32],
    /// Per-piece hashes: piece_hashes[i] is a 16-byte hash for original piece i
    /// piece_hashes.len() == k (number of original pieces in this segment)
    pub piece_hashes: Vec<[u8; HASH_PROJECTIONS]>,
}

/// Complete verification record for a content ID (replaces manifest).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentVerificationRecord {
    /// Original file size in bytes (used to derive segment_count and last segment k)
    pub file_size: u64,
    /// Per-segment homomorphic hashes
    pub segment_hashes: Vec<SegmentHashes>,
}

/// Generate homomorphic hashes for all original pieces in a segment.
/// Called during encoding/publish.
///
/// # Arguments
/// * `seed` - Random 32-byte seed for this segment
/// * `original_pieces` - The k original pieces (before any recombination)
///
/// # Returns
/// SegmentHashes containing the seed and per-piece hash values
pub fn generate_segment_hashes(seed: [u8; 32], original_pieces: &[CodedPiece]) -> SegmentHashes {
    if original_pieces.is_empty() {
        return SegmentHashes {
            seed,
            piece_hashes: Vec::new(),
        };
    }

    let k = original_pieces.len();
    let piece_size = original_pieces[0].data.len();
    let mut piece_hashes = Vec::with_capacity(k);

    for piece in original_pieces {
        let mut hash = [0u8; HASH_PROJECTIONS];
        
        // Compute hash for this piece using all projection vectors
        for (projection_index, hash_element) in hash.iter_mut().enumerate() {
            let projection = generate_projection(&seed, projection_index as u8, piece_size);
            *hash_element = gf256_dot(&piece.data, &projection);
        }
        
        piece_hashes.push(hash);
    }

    SegmentHashes { seed, piece_hashes }
}

/// Verify a single piece (original or recombined) against segment hashes.
///
/// # Arguments
/// * `piece` - The piece to verify (data + coefficients)
/// * `hashes` - The SegmentHashes for this segment
///
/// # Returns
/// `true` if valid, `false` if corrupted/forged
pub fn verify_piece(piece: &CodedPiece, hashes: &SegmentHashes) -> bool {
    let k = hashes.piece_hashes.len();
    let piece_size = piece.data.len();
    
    // Check basic constraints
    if piece.coefficients.len() != k {
        return false;
    }
    
    if k == 0 {
        return piece.data.is_empty() && piece.coefficients.is_empty();
    }

    // For each projection, verify the homomorphic property
    for projection_index in 0..HASH_PROJECTIONS {
        let projection = generate_projection(&hashes.seed, projection_index as u8, piece_size);
        
        // Compute actual hash of the piece data
        let actual = gf256_dot(&piece.data, &projection);
        
        // Compute expected hash using homomorphic property:
        // expected = sum(coefficients[i] * piece_hashes[i][projection_index]) over GF(2^8)
        let mut expected = 0u8;
        for (i, &coeff) in piece.coefficients.iter().enumerate() {
            if coeff != 0 && i < k {
                let contribution = GF256::mul(coeff, hashes.piece_hashes[i][projection_index]);
                expected = GF256::add(expected, contribution);
            }
        }
        
        if actual != expected {
            return false;
        }
    }
    
    true
}

/// Generate a single random projection vector from seed + projection index.
/// Uses blake3 as PRG: blake3(seed || projection_index || chunk_index) for each 32-byte block
fn generate_projection(seed: &[u8; 32], projection_index: u8, piece_size: usize) -> Vec<u8> {
    let mut projection = Vec::with_capacity(piece_size);
    let chunk_size = 32; // blake3 produces 32 bytes per hash
    let num_chunks = piece_size.div_ceil(chunk_size);
    
    for chunk_index in 0..num_chunks {
        // Create input: seed || projection_index || chunk_index
        let mut input = Vec::with_capacity(32 + 1 + 4);
        input.extend_from_slice(seed);
        input.push(projection_index);
        input.extend_from_slice(&(chunk_index as u32).to_le_bytes());
        
        // Generate 32 random bytes using blake3
        let hash = blake3::hash(&input);
        let hash_bytes = hash.as_bytes();
        
        // Take up to 32 bytes or remaining bytes needed
        let bytes_needed = (piece_size - projection.len()).min(chunk_size);
        projection.extend_from_slice(&hash_bytes[..bytes_needed]);
        
        if projection.len() >= piece_size {
            break;
        }
    }
    
    // Ensure we have exactly piece_size bytes
    projection.truncate(piece_size);
    projection
}

/// Compute GF(2^8) dot product of two byte slices.
fn gf256_dot(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "Dot product requires equal length vectors");
    
    let mut result = 0u8;
    for (&a_byte, &b_byte) in a.iter().zip(b.iter()) {
        result = GF256::add(result, GF256::mul(a_byte, b_byte));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RlncEncoder;

    fn create_test_pieces(data: &[u8], piece_size: usize) -> Vec<CodedPiece> {
        let encoder = RlncEncoder::new(data, piece_size).unwrap();
        encoder.source_pieces()
    }

    #[test]
    fn test_generate_projection_deterministic() {
        let seed = [42u8; 32];
        let projection1 = generate_projection(&seed, 0, 100);
        let projection2 = generate_projection(&seed, 0, 100);
        assert_eq!(projection1, projection2);
    }

    #[test]
    fn test_generate_projection_different_seeds() {
        let seed1 = [42u8; 32];
        let seed2 = [43u8; 32];
        let projection1 = generate_projection(&seed1, 0, 100);
        let projection2 = generate_projection(&seed2, 0, 100);
        assert_ne!(projection1, projection2);
    }

    #[test]
    fn test_generate_projection_different_indices() {
        let seed = [42u8; 32];
        let projection1 = generate_projection(&seed, 0, 100);
        let projection2 = generate_projection(&seed, 1, 100);
        assert_ne!(projection1, projection2);
    }

    #[test]
    fn test_gf256_dot_basic() {
        let a = vec![1, 2, 3, 4];
        let b = vec![5, 6, 7, 8];
        let result = gf256_dot(&a, &b);
        
        // Manual calculation: 1*5 + 2*6 + 3*7 + 4*8 in GF(2^8)
        let expected = GF256::add(
            GF256::add(GF256::mul(1, 5), GF256::mul(2, 6)),
            GF256::add(GF256::mul(3, 7), GF256::mul(4, 8))
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_gf256_dot_zero_vector() {
        let a = vec![1, 2, 3, 4];
        let b = vec![0, 0, 0, 0];
        assert_eq!(gf256_dot(&a, &b), 0);
        assert_eq!(gf256_dot(&b, &a), 0);
    }

    #[test]
    fn test_original_pieces_verify_correctly() {
        let data = b"Hello, homomorphic world!";
        let piece_size = 8;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [123u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // All original pieces should verify
        for piece in &pieces {
            assert!(verify_piece(piece, &hashes));
        }
    }

    #[test]
    fn test_recombined_pieces_verify_correctly() {
        let data: Vec<u8> = (0..400).map(|i| (i % 256) as u8).collect();
        let piece_size = 100;
        let pieces = create_test_pieces(&data, piece_size);
        
        let seed = [42u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Create a recombined piece using the first two pieces
        let recombined = crate::create_piece_from_existing(&pieces[0..2]).unwrap();
        
        // Recombined piece should verify
        assert!(verify_piece(&recombined, &hashes));
    }

    #[test]
    fn test_corrupted_data_fails_verification() {
        let data = b"Test data for corruption check";
        let piece_size = 10;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [99u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Corrupt the first piece's data
        let mut corrupted = pieces[0].clone();
        corrupted.data[0] ^= 1; // Flip one bit
        
        assert!(!verify_piece(&corrupted, &hashes));
    }

    #[test]
    fn test_wrong_coefficients_fail_verification() {
        let data = b"Test data for coefficient check";
        let piece_size = 10;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [77u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Create a piece with wrong coefficients
        let mut wrong_coeffs = pieces[0].clone();
        wrong_coeffs.coefficients[0] = 42; // Should be 1 for first original piece
        
        assert!(!verify_piece(&wrong_coeffs, &hashes));
    }

    #[test]
    fn test_random_garbage_fails_verification() {
        let data = b"Test data for garbage check";
        let piece_size = 10;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [88u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Create completely random garbage
        let garbage = CodedPiece {
            data: vec![0xFFu8; piece_size],
            coefficients: vec![0xAAu8; pieces.len()],
        };
        
        assert!(!verify_piece(&garbage, &hashes));
    }

    #[test]
    fn test_multiple_segments_independent() {
        // Create segments with different k values
        let data1 = b"Small"; // k=1 with piece_size=8  
        let data2 = b"This is longer than 8 chars for k=5"; // k=5 with piece_size=8
        let piece_size = 8;
        
        let pieces1 = create_test_pieces(data1, piece_size);
        let pieces2 = create_test_pieces(data2, piece_size);
        
        // Verify k values are different
        assert_ne!(pieces1.len(), pieces2.len());
        
        let seed1 = [11u8; 32];
        let seed2 = [22u8; 32];
        
        let hashes1 = generate_segment_hashes(seed1, &pieces1);
        let hashes2 = generate_segment_hashes(seed2, &pieces2);
        
        // Each segment's pieces should verify against their own hashes
        assert!(verify_piece(&pieces1[0], &hashes1));
        assert!(verify_piece(&pieces2[0], &hashes2));
        
        // But not against the other segment's hashes
        // This should fail because different k values (coefficient vector lengths don't match)
        assert!(!verify_piece(&pieces1[0], &hashes2));
        assert!(!verify_piece(&pieces2[0], &hashes1));
    }

    #[test]
    fn test_edge_case_last_segment_fewer_pieces() {
        // Create a small piece that results in k < than normal
        let data = b"Small";
        let piece_size = 10; // k = 1
        let pieces = create_test_pieces(data, piece_size);
        assert_eq!(pieces.len(), 1);
        
        let seed = [33u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        assert!(verify_piece(&pieces[0], &hashes));
    }

    #[test]
    fn test_empty_segment_edge_case() {
        let pieces: Vec<CodedPiece> = Vec::new();
        let seed = [44u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        assert_eq!(hashes.piece_hashes.len(), 0);
        
        // Empty piece should verify against empty hashes
        let empty_piece = CodedPiece {
            data: Vec::new(),
            coefficients: Vec::new(),
        };
        assert!(verify_piece(&empty_piece, &hashes));
    }

    #[test]
    fn test_mismatched_coefficient_length_fails() {
        let data = b"Test mismatched length";
        let piece_size = 8;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [55u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Create piece with wrong coefficient vector length
        let mut wrong_length = pieces[0].clone();
        wrong_length.coefficients.push(99); // Extra coefficient
        
        assert!(!verify_piece(&wrong_length, &hashes));
    }

    #[test]
    fn test_large_k_segment() {
        // Test with a larger segment similar to real usage
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let piece_size = 100; // k = 100
        let pieces = create_test_pieces(&data, piece_size);
        
        let seed = [66u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Test several original pieces
        for i in [0, 25, 50, 75, 99] {
            assert!(verify_piece(&pieces[i], &hashes));
        }
        
        // Test a recombined piece
        let recombined = crate::create_piece_from_existing(&pieces[10..13]).unwrap();
        assert!(verify_piece(&recombined, &hashes));
    }

    #[test]
    fn test_segment_hashes_serialization() {
        let data = b"Serialization test data";
        let piece_size = 8;
        let pieces = create_test_pieces(data, piece_size);
        
        let seed = [123u8; 32];
        let hashes = generate_segment_hashes(seed, &pieces);
        
        // Test JSON serialization
        let json = serde_json::to_string(&hashes).unwrap();
        let deserialized: SegmentHashes = serde_json::from_str(&json).unwrap();
        
        assert_eq!(hashes, deserialized);
        
        // Verify functionality is preserved
        assert!(verify_piece(&pieces[0], &deserialized));
    }

    #[test]
    fn test_content_verification_record_serialization() {
        let record = ContentVerificationRecord {
            file_size: 12345,
            segment_hashes: vec![
                SegmentHashes {
                    seed: [1u8; 32],
                    piece_hashes: vec![[0u8; HASH_PROJECTIONS], [1u8; HASH_PROJECTIONS]],
                },
                SegmentHashes {
                    seed: [2u8; 32],
                    piece_hashes: vec![[2u8; HASH_PROJECTIONS]],
                },
            ],
        };
        
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: ContentVerificationRecord = serde_json::from_str(&json).unwrap();
        
        assert_eq!(record, deserialized);
    }

    #[test]
    fn test_homomorphic_property_manual() {
        // Manual test of the homomorphic property
        let data1 = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let data2 = vec![9u8, 10, 11, 12, 13, 14, 15, 16];
        let piece_size = 8;
        
        // Create two pieces with identity coefficient vectors
        let piece1 = CodedPiece {
            data: data1.clone(),
            coefficients: vec![1, 0], // First original piece
        };
        let piece2 = CodedPiece {
            data: data2.clone(),
            coefficients: vec![0, 1], // Second original piece
        };
        
        let original_pieces = vec![piece1.clone(), piece2.clone()];
        let seed = [200u8; 32];
        let hashes = generate_segment_hashes(seed, &original_pieces);
        
        // Create a linear combination: 5*piece1 + 7*piece2
        let alpha1 = 5u8;
        let alpha2 = 7u8;
        let mut combined_data = vec![0u8; piece_size];
        for i in 0..piece_size {
            combined_data[i] = GF256::add(
                GF256::mul(alpha1, data1[i]),
                GF256::mul(alpha2, data2[i])
            );
        }
        
        let combined_piece = CodedPiece {
            data: combined_data,
            coefficients: vec![alpha1, alpha2],
        };
        
        // The combined piece should verify
        assert!(verify_piece(&combined_piece, &hashes));
    }
}