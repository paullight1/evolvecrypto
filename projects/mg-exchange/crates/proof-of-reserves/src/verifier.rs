//! Standalone verifier — users run this to check their inclusion.
//!
//! Given: leaf data (user_id, asset, balance) + MerkleProof + published root
//! Verify: the leaf is included in the tree with that root.

use crate::merkle::{hash_leaf, hash_node, MerkleProof, SiblingPosition};

/// Verify that a user's balance is included in a published Merkle root.
///
/// This function can be distributed as a standalone binary or WASM module
/// so users don't need to trust the exchange's verification page.
pub fn verify_inclusion(
    user_id: &str,
    asset: &str,
    balance: &str,
    proof: &MerkleProof,
    published_root: &str,
) -> VerifyResult {
    // Step 1: Compute leaf hash
    let leaf = hash_leaf(user_id, asset, balance);
    let leaf_hex = hex_encode(&leaf);

    if leaf_hex != proof.leaf_hash {
        return VerifyResult::Failed {
            reason: format!(
                "Leaf hash mismatch: computed {} but proof says {}",
                leaf_hex, proof.leaf_hash
            ),
        };
    }

    // Step 2: Walk up the tree using sibling hashes
    let mut current = leaf;
    for sibling in &proof.siblings {
        let sibling_bytes = match hex_decode(&sibling.hash) {
            Some(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                arr
            }
            _ => {
                return VerifyResult::Failed {
                    reason: format!("Invalid sibling hash: {}", sibling.hash),
                };
            }
        };

        current = match sibling.position {
            SiblingPosition::Left => hash_node(&sibling_bytes, &current),
            SiblingPosition::Right => hash_node(&current, &sibling_bytes),
        };
    }

    // Step 3: Compare computed root with published root
    let computed_root = hex_encode(&current);
    if computed_root == published_root {
        VerifyResult::Verified { root: computed_root }
    } else {
        VerifyResult::Failed {
            reason: format!(
                "Root mismatch: computed {} but published root is {}",
                computed_root, published_root
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Verified { root: String },
    Failed { reason: String },
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::MerkleTree;

    fn build_test_tree() -> (MerkleTree, Vec<(String, String, String)>) {
        let users = vec![
            ("alice".to_string(), "BTC".to_string(), "1.50000000".to_string()),
            ("bob".to_string(), "BTC".to_string(), "2.00000000".to_string()),
            ("carol".to_string(), "BTC".to_string(), "0.75000000".to_string()),
            ("dave".to_string(), "BTC".to_string(), "3.25000000".to_string()),
        ];

        let leaves: Vec<_> = users
            .iter()
            .map(|(u, a, b)| hash_leaf(u, a, b))
            .collect();

        (MerkleTree::build(leaves), users)
    }

    #[test]
    fn test_valid_proof() {
        let (tree, users) = build_test_tree();
        let root = hex_encode(&tree.root());

        // Verify Alice's inclusion
        let proof = tree.proof(0).unwrap();
        let result = verify_inclusion(&users[0].0, &users[0].1, &users[0].2, &proof, &root);
        assert_eq!(result, VerifyResult::Verified { root: root.clone() });
    }

    #[test]
    fn test_all_users_verify() {
        let (tree, users) = build_test_tree();
        let root = hex_encode(&tree.root());

        for (i, (user, asset, balance)) in users.iter().enumerate() {
            let proof = tree.proof(i).unwrap();
            let result = verify_inclusion(user, asset, balance, &proof, &root);
            assert!(matches!(result, VerifyResult::Verified { .. }), "User {} failed verification", user);
        }
    }

    #[test]
    fn test_wrong_balance_fails() {
        let (tree, users) = build_test_tree();
        let root = hex_encode(&tree.root());
        let proof = tree.proof(0).unwrap();

        // Alice claims 999 BTC instead of 1.5
        let result = verify_inclusion(&users[0].0, &users[0].1, "999.00000000", &proof, &root);
        assert!(matches!(result, VerifyResult::Failed { .. }));
    }

    #[test]
    fn test_wrong_root_fails() {
        let (tree, users) = build_test_tree();
        let proof = tree.proof(0).unwrap();

        let result = verify_inclusion(&users[0].0, &users[0].1, &users[0].2, &proof, "0000000000000000000000000000000000000000000000000000000000000000");
        assert!(matches!(result, VerifyResult::Failed { .. }));
    }

    #[test]
    fn test_wrong_user_fails() {
        let (tree, users) = build_test_tree();
        let root = hex_encode(&tree.root());
        let proof = tree.proof(0).unwrap();

        // Eve tries to use Alice's proof
        let result = verify_inclusion("eve", &users[0].1, &users[0].2, &proof, &root);
        assert!(matches!(result, VerifyResult::Failed { .. }));
    }

    #[test]
    fn test_large_tree_verification() {
        let n = 10_000;
        let users: Vec<_> = (0..n)
            .map(|i| (format!("user{}", i), "USDT".to_string(), format!("{}.00", i * 100)))
            .collect();

        let leaves: Vec<_> = users.iter().map(|(u, a, b)| hash_leaf(u, a, b)).collect();
        let tree = MerkleTree::build(leaves);
        let root = hex_encode(&tree.root());

        // Verify random users
        for i in [0, 42, 999, 5000, 9999] {
            let proof = tree.proof(i).unwrap();
            let result = verify_inclusion(&users[i].0, &users[i].1, &users[i].2, &proof, &root);
            assert!(matches!(result, VerifyResult::Verified { .. }));
        }
    }
}
