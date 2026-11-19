//! Proof of Reserves report generation.
//!
//! Generates a full PoR report containing:
//! - Merkle root of all user liabilities
//! - Total liabilities per asset
//! - Total reserves per asset (on-chain wallet balances)
//! - Reserve ratio (reserves / liabilities)
//! - Individual proofs for each user

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::merkle::{hash_leaf, MerkleProof, MerkleTree};

#[allow(unused_imports)]
use tracing::error;

/// A single user's liability (what the exchange owes them).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLiability {
    pub user_id: Uuid,
    pub asset: String,
    pub balance: Decimal,
}

/// Reserve info for an asset (what the exchange actually holds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReserve {
    pub asset: String,
    pub total_reserve: Decimal,
    pub total_liability: Decimal,
    /// reserve / liability — must be >= 1.0 for full backing.
    pub ratio: Decimal,
    pub wallet_addresses: Vec<String>,
}

/// Full Proof of Reserves report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReserveReport {
    pub report_id: Uuid,
    pub generated_at: DateTime<Utc>,
    /// Merkle root committing to all user liabilities.
    pub merkle_root: String,
    /// Total number of users included.
    pub total_users: usize,
    /// Total number of liability entries (user × asset).
    pub total_entries: usize,
    /// Reserve status per asset.
    pub assets: Vec<AssetReserve>,
    /// Overall reserve ratio (weighted by USD value).
    pub overall_status: ReserveStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReserveStatus {
    /// All assets fully backed (ratio >= 1.0).
    FullyBacked,
    /// Some assets under-collateralized.
    UnderCollateralized,
}

/// Generates Proof of Reserves reports.
pub struct ReportGenerator;

impl ReportGenerator {
    /// Generate a full PoR report.
    ///
    /// Returns (report, merkle_tree) — the tree is needed to generate individual proofs.
    ///
    /// `expected_account_count` — when `Some(n)`, an error is logged if the
    /// number of unique users in `liabilities` does not equal `n`.  This acts
    /// as a completeness check that all user accounts were included.
    pub fn generate(
        liabilities: &[UserLiability],
        reserves: &HashMap<String, (Decimal, Vec<String>)>, // asset → (balance, wallet_addresses)
        expected_account_count: Option<usize>,
    ) -> (ReserveReport, MerkleTree) {
        // Build leaves
        let leaves: Vec<_> = liabilities
            .iter()
            .map(|l| hash_leaf(&l.user_id.to_string(), &l.asset, &l.balance.to_string()))
            .collect();

        let tree = if leaves.is_empty() {
            MerkleTree::build(vec![hash_leaf("empty", "none", "0")])
        } else {
            MerkleTree::build(leaves)
        };

        // Aggregate liabilities per asset
        let mut liability_totals: HashMap<String, Decimal> = HashMap::new();
        let mut user_set = std::collections::HashSet::new();
        for l in liabilities {
            *liability_totals.entry(l.asset.clone()).or_default() += l.balance;
            user_set.insert(l.user_id);
        }

        // Completeness check.
        if let Some(expected) = expected_account_count {
            let actual = user_set.len();
            if actual != expected {
                error!(
                    expected,
                    actual,
                    "PoR completeness check FAILED: not all user accounts included in report"
                );
            }
        }

        // Build asset reserves
        let mut assets = Vec::new();
        let mut all_backed = true;

        for (asset, &total_liability) in &liability_totals {
            let (total_reserve, wallet_addresses) = reserves
                .get(asset)
                .cloned()
                .unwrap_or((Decimal::ZERO, vec![]));

            let ratio = if total_liability.is_zero() {
                Decimal::MAX
            } else {
                total_reserve / total_liability
            };

            if ratio < Decimal::ONE {
                all_backed = false;
            }

            assets.push(AssetReserve {
                asset: asset.clone(),
                total_reserve,
                total_liability,
                ratio,
                wallet_addresses,
            });
        }

        assets.sort_by(|a, b| a.asset.cmp(&b.asset));

        let root_hex: String = tree.root().iter().map(|b| format!("{:02x}", b)).collect();

        let report = ReserveReport {
            report_id: Uuid::new_v4(),
            generated_at: Utc::now(),
            merkle_root: root_hex,
            total_users: user_set.len(),
            total_entries: liabilities.len(),
            assets,
            overall_status: if all_backed {
                ReserveStatus::FullyBacked
            } else {
                ReserveStatus::UnderCollateralized
            },
        };

        (report, tree)
    }

    /// Generate a proof for a specific user + asset.
    pub fn user_proof(
        liabilities: &[UserLiability],
        tree: &MerkleTree,
        user_id: Uuid,
        asset: &str,
    ) -> Option<MerkleProof> {
        let index = liabilities
            .iter()
            .position(|l| l.user_id == user_id && l.asset == asset)?;
        tree.proof(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verifier::{verify_inclusion, VerifyResult};
    use rust_decimal_macros::dec;

    fn sample_liabilities() -> Vec<UserLiability> {
        vec![
            UserLiability { user_id: Uuid::from_u128(1), asset: "BTC".into(), balance: dec!(1.5) },
            UserLiability { user_id: Uuid::from_u128(2), asset: "BTC".into(), balance: dec!(2.0) },
            UserLiability { user_id: Uuid::from_u128(1), asset: "USDT".into(), balance: dec!(50000) },
            UserLiability { user_id: Uuid::from_u128(3), asset: "USDT".into(), balance: dec!(30000) },
        ]
    }

    fn sample_reserves() -> HashMap<String, (Decimal, Vec<String>)> {
        let mut m = HashMap::new();
        m.insert("BTC".into(), (dec!(5.0), vec!["bc1q_cold_wallet".into()]));
        m.insert("USDT".into(), (dec!(100000), vec!["0x_cold_wallet".into()]));
        m
    }

    #[test]
    fn test_generate_report_fully_backed() {
        let (report, _tree) = ReportGenerator::generate(&sample_liabilities(), &sample_reserves(), None);

        assert_eq!(report.total_users, 3);
        assert_eq!(report.total_entries, 4);
        assert_eq!(report.overall_status, ReserveStatus::FullyBacked);
        assert!(!report.merkle_root.is_empty());

        let btc = report.assets.iter().find(|a| a.asset == "BTC").unwrap();
        assert_eq!(btc.total_liability, dec!(3.5));
        assert_eq!(btc.total_reserve, dec!(5.0));
        assert!(btc.ratio > Decimal::ONE);
    }

    #[test]
    fn test_under_collateralized() {
        let mut reserves = sample_reserves();
        reserves.insert("BTC".into(), (dec!(2.0), vec![])); // Only 2 BTC, owe 3.5

        let (report, _) = ReportGenerator::generate(&sample_liabilities(), &reserves, None);
        assert_eq!(report.overall_status, ReserveStatus::UnderCollateralized);
    }

    #[test]
    fn test_user_proof_verifies() {
        let liabilities = sample_liabilities();
        let (report, tree) = ReportGenerator::generate(&liabilities, &sample_reserves(), None);

        let user_id = Uuid::from_u128(1);
        let proof = ReportGenerator::user_proof(&liabilities, &tree, user_id, "BTC").unwrap();

        let result = verify_inclusion(
            &user_id.to_string(),
            "BTC",
            &dec!(1.5).to_string(),
            &proof,
            &report.merkle_root,
        );
        assert!(matches!(result, VerifyResult::Verified { .. }));
    }

    #[test]
    fn test_user_proof_wrong_balance_fails() {
        let liabilities = sample_liabilities();
        let (report, tree) = ReportGenerator::generate(&liabilities, &sample_reserves(), None);

        let user_id = Uuid::from_u128(1);
        let proof = ReportGenerator::user_proof(&liabilities, &tree, user_id, "BTC").unwrap();

        let result = verify_inclusion(
            &user_id.to_string(),
            "BTC",
            "999.0", // wrong balance
            &proof,
            &report.merkle_root,
        );
        assert!(matches!(result, VerifyResult::Failed { .. }));
    }

    #[test]
    fn test_nonexistent_user_no_proof() {
        let liabilities = sample_liabilities();
        let (_, tree) = ReportGenerator::generate(&liabilities, &sample_reserves(), None);

        let proof = ReportGenerator::user_proof(&liabilities, &tree, Uuid::from_u128(999), "BTC");
        assert!(proof.is_none());
    }

    #[test]
    fn test_empty_liabilities() {
        let (report, _) = ReportGenerator::generate(&[], &HashMap::new(), None);
        assert_eq!(report.total_users, 0);
        assert_eq!(report.total_entries, 0);
    }

    #[test]
    fn test_report_serializes_to_json() {
        let (report, _) = ReportGenerator::generate(&sample_liabilities(), &sample_reserves(), None);
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("merkle_root"));
        assert!(json.contains("FullyBacked"));
    }
}
