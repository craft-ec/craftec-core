use sha2::{Digest, Sha256};

/// Compute the Merkle root of a list of leaf hashes.
///
/// Uses SHA-256 for internal nodes. Empty input returns all zeros.
/// Single leaf returns that leaf's hash. Odd-length levels duplicate the last element.
pub fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current_level = leaves.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));

        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() == 2 {
                &chunk[1]
            } else {
                &chunk[0] // duplicate last element for odd count
            };
            let mut hasher = Sha256::new();
            hasher.update(left);
            hasher.update(right);
            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            next_level.push(hash);
        }

        current_level = next_level;
    }

    current_level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_merkle_root() {
        assert_eq!(compute_merkle_root(&[]), [0u8; 32]);
    }

    #[test]
    fn test_single_leaf() {
        let leaf = [42u8; 32];
        assert_eq!(compute_merkle_root(&[leaf]), leaf);
    }

    #[test]
    fn test_two_leaves() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let root = compute_merkle_root(&[a, b]);
        // Manually compute expected
        let mut hasher = Sha256::new();
        hasher.update(a);
        hasher.update(b);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(root, expected);
    }

    #[test]
    fn test_deterministic() {
        let leaves: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
        let r1 = compute_merkle_root(&leaves);
        let r2 = compute_merkle_root(&leaves);
        assert_eq!(r1, r2);
    }
}
