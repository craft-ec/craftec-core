use sha2::{Digest, Sha256};

/// Compute the Merkle root of a list of leaf hashes.
///
/// Pads to next power-of-2 with `[0u8; 32]`, then bottom-up `SHA256(left || right)`.
/// Matches the guest program and CraftNet's Merkle tree algorithm.
pub fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    // Pad to next power of 2
    let n = leaves.len().next_power_of_two();
    let mut nodes: Vec<[u8; 32]> = Vec::with_capacity(n);
    nodes.extend_from_slice(leaves);
    while nodes.len() < n {
        nodes.push([0u8; 32]);
    }

    // Bottom-up merge
    while nodes.len() > 1 {
        let mut next = Vec::with_capacity(nodes.len() / 2);
        for i in (0..nodes.len()).step_by(2) {
            let mut hasher = Sha256::new();
            hasher.update(nodes[i]);
            hasher.update(nodes[i + 1]);
            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            next.push(hash);
        }
        nodes = next;
    }

    nodes[0]
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
        let mut hasher = Sha256::new();
        hasher.update(a);
        hasher.update(b);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(root, expected);
    }

    #[test]
    fn test_three_leaves_pads_to_four() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        let root = compute_merkle_root(&[a, b, c]);

        // Manually: pad with [0;32], so leaves = [a, b, c, zero]
        let zero = [0u8; 32];
        let left = {
            let mut h = Sha256::new();
            h.update(a);
            h.update(b);
            let r: [u8; 32] = h.finalize().into();
            r
        };
        let right = {
            let mut h = Sha256::new();
            h.update(c);
            h.update(zero);
            let r: [u8; 32] = h.finalize().into();
            r
        };
        let expected = {
            let mut h = Sha256::new();
            h.update(left);
            h.update(right);
            let r: [u8; 32] = h.finalize().into();
            r
        };
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
