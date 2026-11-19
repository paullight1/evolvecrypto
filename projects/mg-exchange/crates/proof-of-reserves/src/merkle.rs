//! Merkle tree for Proof of Reserves.
//!
//! Each leaf is a hash of (user_id, asset, balance).
//! Users can verify their inclusion without knowing other users' data.
//!
//! Based on the Kraken/OKX approach:
//! - Leaf = SHA256(user_id || asset || balance)
//! - Node = SHA256(left || right)
//! - Root = single hash committing to all liabilities

use sha2::{Digest, Sha256};

/// A node hash in the Merkle tree (32 bytes).
pub type Hash = [u8; 32];

/// Hash a leaf: SHA256(user_id || ":" || asset || ":" || balance).
pub fn hash_leaf(user_id: &str, asset: &str, balance: &str) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    hasher.update(b":");
    hasher.update(asset.as_bytes());
    hasher.update(b":");
    hasher.update(balance.as_bytes());
    hasher.finalize().into()
}

/// Hash two child nodes: SHA256(left || right).
/// Always sorts left < right to make the tree order-independent.
pub fn hash_node(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Sha256::new();
    if left <= right {
        hasher.update(left);
        hasher.update(right);
    } else {
        hasher.update(right);
        hasher.update(left);
    }
    hasher.finalize().into()
}

/// Merkle proof for inclusion of a single leaf.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleProof {
    /// The leaf hash being proven.
    pub leaf_hash: String,
    /// Sibling hashes from leaf to root, with position (left/right).
    pub siblings: Vec<ProofSibling>,
    /// The Merkle root this proof verifies against.
    pub root: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProofSibling {
    pub hash: String,
    pub position: SiblingPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SiblingPosition {
    Left,
    Right,
}

/// Merkle tree built from leaf hashes.
pub struct MerkleTree {
    /// All levels of the tree. Level 0 = leaves, last = root.
    levels: Vec<Vec<Hash>>,
}

impl MerkleTree {
    /// Build a Merkle tree from leaf hashes.
    ///
    /// If the number of leaves is odd, the last leaf is duplicated.
    pub fn build(leaves: Vec<Hash>) -> Self {
        assert!(!leaves.is_empty(), "Cannot build Merkle tree with zero leaves");

        let mut levels = vec![leaves];

        loop {
            let current = levels.last().unwrap();
            if current.len() == 1 {
                break;
            }

            let mut next_level = Vec::with_capacity((current.len() + 1) / 2);
            let mut i = 0;
            while i < current.len() {
                let left = &current[i];
                let right = if i + 1 < current.len() {
                    &current[i + 1]
                } else {
                    left // duplicate last if odd
                };
                next_level.push(hash_node(left, right));
                i += 2;
            }
            levels.push(next_level);
        }

        Self { levels }
    }

    /// Get the Merkle root hash.
    pub fn root(&self) -> Hash {
        *self.levels.last().unwrap().first().unwrap()
    }

    /// Get the number of leaves.
    pub fn leaf_count(&self) -> usize {
        self.levels[0].len()
    }

    /// Get the tree height (number of levels).
    pub fn height(&self) -> usize {
        self.levels.len()
    }

    /// Generate a Merkle proof for a leaf at the given index.
    pub fn proof(&self, leaf_index: usize) -> Option<MerkleProof> {
        if leaf_index >= self.levels[0].len() {
            return None;
        }

        let leaf_hash = self.levels[0][leaf_index];
        let mut siblings = Vec::new();
        let mut index = leaf_index;

        for level in 0..self.levels.len() - 1 {
            let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };

            let sibling_hash = if sibling_index < self.levels[level].len() {
                self.levels[level][sibling_index]
            } else {
                self.levels[level][index] // duplicate for odd count
            };

            let position = if index % 2 == 0 {
                SiblingPosition::Right
            } else {
                SiblingPosition::Left
            };

            siblings.push(ProofSibling {
                hash: hex::encode(&sibling_hash),
                position,
            });

            index /= 2;
        }

        Some(MerkleProof {
            leaf_hash: hex::encode(&leaf_hash),
            siblings,
            root: hex::encode(&self.root()),
        })
    }
}

/// Hex encode helper.
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn decode(s: &str) -> Option<Vec<u8>> {
        if s.len() % 2 != 0 {
            return None;
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_leaf_deterministic() {
        let h1 = hash_leaf("user1", "BTC", "1.5");
        let h2 = hash_leaf("user1", "BTC", "1.5");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_leaf_different_inputs() {
        let h1 = hash_leaf("user1", "BTC", "1.5");
        let h2 = hash_leaf("user2", "BTC", "1.5");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_node_order_independent() {
        let a = hash_leaf("a", "BTC", "1");
        let b = hash_leaf("b", "BTC", "2");
        assert_eq!(hash_node(&a, &b), hash_node(&b, &a));
    }

    #[test]
    fn test_single_leaf_tree() {
        let leaf = hash_leaf("user1", "BTC", "1.0");
        let tree = MerkleTree::build(vec![leaf]);
        assert_eq!(tree.root(), leaf);
        assert_eq!(tree.leaf_count(), 1);
        assert_eq!(tree.height(), 1);
    }

    #[test]
    fn test_two_leaf_tree() {
        let a = hash_leaf("alice", "BTC", "1.0");
        let b = hash_leaf("bob", "BTC", "2.0");
        let tree = MerkleTree::build(vec![a, b]);

        assert_eq!(tree.leaf_count(), 2);
        assert_eq!(tree.height(), 2);
        assert_eq!(tree.root(), hash_node(&a, &b));
    }

    #[test]
    fn test_four_leaf_tree() {
        let leaves: Vec<Hash> = (0..4)
            .map(|i| hash_leaf(&format!("user{}", i), "BTC", &format!("{}", i)))
            .collect();

        let tree = MerkleTree::build(leaves.clone());
        assert_eq!(tree.leaf_count(), 4);
        assert_eq!(tree.height(), 3); // 4 → 2 → 1

        let left = hash_node(&leaves[0], &leaves[1]);
        let right = hash_node(&leaves[2], &leaves[3]);
        assert_eq!(tree.root(), hash_node(&left, &right));
    }

    #[test]
    fn test_odd_leaf_count() {
        let leaves: Vec<Hash> = (0..3)
            .map(|i| hash_leaf(&format!("u{}", i), "ETH", &format!("{}", i)))
            .collect();

        let tree = MerkleTree::build(leaves.clone());
        assert_eq!(tree.leaf_count(), 3);
        // 3 leaves: [L0, L1, L2] → [H(L0,L1), H(L2,L2)] → [root]
        assert_eq!(tree.height(), 3);
    }

    #[test]
    fn test_proof_generation() {
        let leaves: Vec<Hash> = (0..4)
            .map(|i| hash_leaf(&format!("user{}", i), "BTC", &format!("{}", i)))
            .collect();

        let tree = MerkleTree::build(leaves);

        let proof = tree.proof(0).unwrap();
        assert_eq!(proof.siblings.len(), 2); // log2(4) = 2
        assert_eq!(proof.root, hex::encode(&tree.root()));
    }

    #[test]
    fn test_proof_out_of_bounds() {
        let tree = MerkleTree::build(vec![hash_leaf("u", "B", "1")]);
        assert!(tree.proof(1).is_none());
    }

    #[test]
    fn test_large_tree() {
        let leaves: Vec<Hash> = (0..1000)
            .map(|i| hash_leaf(&format!("user{}", i), "USDT", &format!("{}", i * 100)))
            .collect();

        let tree = MerkleTree::build(leaves);
        assert_eq!(tree.leaf_count(), 1000);
        // All proofs should be valid
        for i in [0, 1, 499, 500, 999] {
            let proof = tree.proof(i).unwrap();
            assert!(!proof.siblings.is_empty());
            assert_eq!(proof.root, hex::encode(&tree.root()));
        }
    }
}
