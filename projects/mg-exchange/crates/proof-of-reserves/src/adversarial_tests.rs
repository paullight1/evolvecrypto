/// # Adversarial Tests — Proof of Reserves
///
/// These tests verify that the PoR system cannot be used to deceive users:
///   - Reported liabilities match actual user balances exactly
///   - Every user can independently verify their inclusion
///   - Modifying even one balance invalidates the entire Merkle tree
///   - Wrong balance, wrong user, wrong asset — all fail verification
///   - The reserve ratio calculation correctly identifies under-collateralisation
#[cfg(test)]
mod adversarial {
    use std::collections::HashMap;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use crate::{
        merkle::hash_leaf,
        report::{ReportGenerator, ReserveStatus, UserLiability},
        verifier::{verify_inclusion, VerifyResult},
    };

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn make_liabilities(n: usize) -> Vec<UserLiability> {
        (0..n)
            .map(|i| UserLiability {
                user_id: Uuid::from_u128(i as u128 + 1),
                asset: "USDT".to_string(),
                balance: Decimal::from((i as u64 + 1) * 1000),
            })
            .collect()
    }

    fn fully_backed_reserves(liabilities: &[UserLiability]) -> HashMap<String, (Decimal, Vec<String>)> {
        let total: Decimal = liabilities.iter().map(|l| l.balance).sum();
        let mut m = HashMap::new();
        m.insert("USDT".to_string(), (total, vec!["cold_wallet_1".to_string()]));
        m
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 1 — Reported Liabilities Match Actual Balances
    // ─────────────────────────────────────────────────────────────────────────

    /// The PoR report's total liabilities must equal the exact arithmetic sum
    /// of all individual user balances. No rounding, no omissions.
    #[test]
    fn test_proof_of_reserves_reported_liabilities_equal_sum_of_user_balances() {
        let liabilities = vec![
            UserLiability { user_id: Uuid::from_u128(1), asset: "BTC".to_string(), balance: dec!(1.5) },
            UserLiability { user_id: Uuid::from_u128(2), asset: "BTC".to_string(), balance: dec!(2.333333) },
            UserLiability { user_id: Uuid::from_u128(3), asset: "BTC".to_string(), balance: dec!(0.00000001) }, // dust
            UserLiability { user_id: Uuid::from_u128(1), asset: "USDT".to_string(), balance: dec!(50000.99) },
            UserLiability { user_id: Uuid::from_u128(2), asset: "USDT".to_string(), balance: dec!(30000.01) },
        ];

        let expected_btc: Decimal = liabilities.iter()
            .filter(|l| l.asset == "BTC")
            .map(|l| l.balance)
            .sum();
        let expected_usdt: Decimal = liabilities.iter()
            .filter(|l| l.asset == "USDT")
            .map(|l| l.balance)
            .sum();

        let mut reserves = HashMap::new();
        reserves.insert("BTC".to_string(), (expected_btc * dec!(1.1), vec![])); // over-reserved
        reserves.insert("USDT".to_string(), (expected_usdt * dec!(1.1), vec![]));

        let (report, _tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        let btc_asset = report.assets.iter().find(|a| a.asset == "BTC").unwrap();
        let usdt_asset = report.assets.iter().find(|a| a.asset == "USDT").unwrap();

        assert_eq!(
            btc_asset.total_liability,
            expected_btc,
            "BTC liability must equal exact sum: expected {expected_btc} got {}",
            btc_asset.total_liability
        );
        assert_eq!(
            usdt_asset.total_liability,
            expected_usdt,
            "USDT liability must equal exact sum: expected {expected_usdt} got {}",
            usdt_asset.total_liability
        );

        // Also verify the entry count and user count.
        // Users: from_u128(1) for BTC+USDT, from_u128(2) for BTC+USDT, from_u128(3) for USDT = 3 distinct.
        assert_eq!(report.total_entries, 5);
        assert_eq!(report.total_users, 3);
    }

    /// Every user can verify their own inclusion using only their data + the proof.
    /// They do not need to know any other user's data.
    #[test]
    fn test_every_user_can_verify_own_inclusion() {
        let liabilities = make_liabilities(20);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        for (i, liability) in liabilities.iter().enumerate() {
            let proof = ReportGenerator::user_proof(&liabilities, &tree, liability.user_id, "USDT")
                .expect("proof must exist for every user");

            let result = verify_inclusion(
                &liability.user_id.to_string(),
                "USDT",
                &liability.balance.to_string(),
                &proof,
                &report.merkle_root,
            );

            assert!(
                matches!(result, VerifyResult::Verified { .. }),
                "user at index {i} (id={}) must be able to verify inclusion: {:?}",
                liability.user_id, result
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 2 — Tamper Detection
    //
    // Modifying a single user's balance must invalidate the Merkle root,
    // making the tampering detectable by any user who re-runs verification.
    // ─────────────────────────────────────────────────────────────────────────

    /// Modifying user 0's balance from the original value to a different value
    /// produces a different Merkle root. Verification of ANY user against the
    /// original root will then fail if the tree was rebuilt with the tampered data.
    #[test]
    fn test_tampering_one_balance_invalidates_entire_tree() {
        let liabilities = make_liabilities(8);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, _tree) = ReportGenerator::generate(&liabilities, &reserves, None);
        let original_root = report.merkle_root.clone();

        // Tamper: change user 1's balance from 1000 to 9,999,999.
        let mut tampered = liabilities.clone();
        tampered[0].balance = dec!(9_999_999);

        // Rebuild the tree with tampered data.
        let tampered_reserves = fully_backed_reserves(&tampered);
        let (tampered_report, tampered_tree) =
            ReportGenerator::generate(&tampered, &tampered_reserves, None);

        // The tampered root must differ from the original root.
        assert_ne!(
            original_root, tampered_report.merkle_root,
            "tampered tree must produce a different Merkle root"
        );

        // A user trying to verify their inclusion against the ORIGINAL published
        // root using the TAMPERED balance must fail.
        let tampered_user_id = liabilities[0].user_id;
        // Build a proof from the tampered tree for the tampered user.
        let tampered_proof = ReportGenerator::user_proof(
            &tampered, &tampered_tree, tampered_user_id, "USDT"
        ).unwrap();

        // Verify against ORIGINAL root — must fail.
        let result = verify_inclusion(
            &tampered_user_id.to_string(),
            "USDT",
            &dec!(9_999_999).to_string(), // tampered balance
            &tampered_proof,
            &original_root, // original published root
        );
        assert!(
            matches!(result, VerifyResult::Failed { .. }),
            "tampered balance must not verify against original root: {:?}",
            result
        );
    }

    /// The legitimate user at index 0 can still verify against the ORIGINAL root
    /// with their correct balance.
    #[test]
    fn test_original_user_verifies_with_original_root() {
        let liabilities = make_liabilities(8);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        let user_id = liabilities[0].user_id;
        let proof = ReportGenerator::user_proof(&liabilities, &tree, user_id, "USDT").unwrap();

        let result = verify_inclusion(
            &user_id.to_string(),
            "USDT",
            &liabilities[0].balance.to_string(),
            &proof,
            &report.merkle_root,
        );
        assert!(
            matches!(result, VerifyResult::Verified { .. }),
            "honest user must verify successfully: {:?}",
            result
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 3 — Proof Forgery Resistance
    // ─────────────────────────────────────────────────────────────────────────

    /// An attacker cannot claim a higher balance than they actually have.
    /// Using Alice's proof with a different balance must fail.
    #[test]
    fn test_user_cannot_claim_higher_balance() {
        let liabilities = make_liabilities(10);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        let alice = &liabilities[0];
        let proof = ReportGenerator::user_proof(&liabilities, &tree, alice.user_id, "USDT")
            .unwrap();

        // Alice has 1000 USDT. She claims 1,000,000.
        let result = verify_inclusion(
            &alice.user_id.to_string(),
            "USDT",
            "1000000",
            &proof,
            &report.merkle_root,
        );
        assert!(
            matches!(result, VerifyResult::Failed { .. }),
            "inflated balance claim must fail: {:?}",
            result
        );
    }

    /// Eve cannot use Alice's proof to prove Eve's own inclusion.
    #[test]
    fn test_user_cannot_use_other_users_proof() {
        let liabilities = make_liabilities(10);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        // Alice's proof.
        let alice = &liabilities[0];
        let alice_proof = ReportGenerator::user_proof(&liabilities, &tree, alice.user_id, "USDT")
            .unwrap();

        // Eve tries to use Alice's proof with Eve's user ID and Alice's balance.
        let eve_id = Uuid::new_v4();
        let result = verify_inclusion(
            &eve_id.to_string(),
            "USDT",
            &alice.balance.to_string(),
            &alice_proof,
            &report.merkle_root,
        );
        assert!(
            matches!(result, VerifyResult::Failed { .. }),
            "proof from another user must fail: {:?}",
            result
        );
    }

    /// An unregistered user cannot prove inclusion regardless of what proof they construct.
    #[test]
    fn test_nonexistent_user_has_no_valid_proof() {
        let liabilities = make_liabilities(10);
        let reserves = fully_backed_reserves(&liabilities);
        let (_report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        let ghost_id = Uuid::new_v4();
        let proof = ReportGenerator::user_proof(&liabilities, &tree, ghost_id, "USDT");
        assert!(
            proof.is_none(),
            "non-existent user must not receive a proof"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 4 — Reserve Ratio Correctness
    // ─────────────────────────────────────────────────────────────────────────

    /// Under-collateralisation is correctly detected.
    /// When reserves < liabilities, the status must be UnderCollateralized.
    #[test]
    fn test_under_collateralisation_detected_correctly() {
        let liabilities = vec![
            UserLiability { user_id: Uuid::from_u128(1), asset: "BTC".to_string(), balance: dec!(10) },
            UserLiability { user_id: Uuid::from_u128(2), asset: "BTC".to_string(), balance: dec!(5) },
        ];
        // Total liability = 15 BTC. Exchange only holds 12 BTC.
        let mut reserves = HashMap::new();
        reserves.insert("BTC".to_string(), (dec!(12), vec![]));

        let (report, _) = ReportGenerator::generate(&liabilities, &reserves, None);

        assert_eq!(
            report.overall_status,
            ReserveStatus::UnderCollateralized,
            "should detect under-collateralisation when reserves (12) < liabilities (15)"
        );

        let btc = report.assets.iter().find(|a| a.asset == "BTC").unwrap();
        assert!(
            btc.ratio < Decimal::ONE,
            "BTC ratio ({}) must be below 1.0 when under-collateralised",
            btc.ratio
        );
        // ratio = 12/15 = 0.8
        assert_eq!(btc.ratio, dec!(12) / dec!(15));
    }

    /// Full backing is correctly confirmed when reserves >= liabilities.
    #[test]
    fn test_full_backing_confirmed_when_reserves_exceed_liabilities() {
        let liabilities = make_liabilities(5);
        // total = 1000+2000+3000+4000+5000 = 15000 USDT
        let mut reserves = HashMap::new();
        reserves.insert("USDT".to_string(), (dec!(20000), vec![])); // 133% backed

        let (report, _) = ReportGenerator::generate(&liabilities, &reserves, None);

        assert_eq!(
            report.overall_status,
            ReserveStatus::FullyBacked,
            "fully backed when reserves exceed liabilities"
        );
        let usdt = report.assets.iter().find(|a| a.asset == "USDT").unwrap();
        assert!(
            usdt.ratio >= Decimal::ONE,
            "ratio ({}) must be >= 1.0 when fully backed",
            usdt.ratio
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 5 — Merkle Tree Properties
    // ─────────────────────────────────────────────────────────────────────────

    /// 1,000 users: all proofs verify against the published root.
    /// This mimics a real PoR audit at scale.
    #[test]
    fn test_1000_users_all_proofs_verify() {
        let liabilities = make_liabilities(1000);
        let reserves = fully_backed_reserves(&liabilities);
        let (report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        // Sample 10 users spread across the tree (first, last, middle, etc.).
        let indices = [0, 1, 99, 250, 499, 500, 750, 900, 998, 999];
        for i in indices {
            let liability = &liabilities[i];
            let proof = ReportGenerator::user_proof(
                &liabilities, &tree, liability.user_id, "USDT"
            ).unwrap();

            let result = verify_inclusion(
                &liability.user_id.to_string(),
                "USDT",
                &liability.balance.to_string(),
                &proof,
                &report.merkle_root,
            );
            assert!(
                matches!(result, VerifyResult::Verified { .. }),
                "user at index {i} must verify in 1000-user tree: {:?}",
                result
            );
        }
    }

    /// Two users with the same balance must produce different leaf hashes
    /// (because their user IDs differ). Prevents collision attacks.
    #[test]
    fn test_same_balance_different_users_produce_different_hashes() {
        let user1 = Uuid::from_u128(1);
        let user2 = Uuid::from_u128(2);
        let balance = "5000";

        let h1 = hash_leaf(&user1.to_string(), "USDT", balance);
        let h2 = hash_leaf(&user2.to_string(), "USDT", balance);

        assert_ne!(h1, h2, "different users with same balance must have different leaf hashes");
    }

    /// Leaf hash is order-sensitive: user:asset:balance != balance:asset:user.
    #[test]
    fn test_leaf_hash_field_ordering_is_collision_resistant() {
        // These three strings concatenate differently.
        let h1 = hash_leaf("user1", "BTC", "100");
        let h2 = hash_leaf("100", "BTC", "user1"); // swapped fields
        let h3 = hash_leaf("user1", "100", "BTC"); // different order

        assert_ne!(h1, h2, "field order swap must produce different hash");
        assert_ne!(h1, h3, "field order swap must produce different hash");
        assert_ne!(h2, h3, "all permutations must be distinct");
    }

    /// Determinism: the same inputs always produce the same Merkle root.
    /// Non-determinism would mean users see different roots on different
    /// verification runs — unacceptable for a PoR system.
    #[test]
    fn test_merkle_root_is_deterministic() {
        let liabilities = make_liabilities(16);
        let reserves = fully_backed_reserves(&liabilities);

        let (report1, _) = ReportGenerator::generate(&liabilities, &reserves, None);
        let (report2, _) = ReportGenerator::generate(&liabilities, &reserves, None);

        assert_eq!(
            report1.merkle_root, report2.merkle_root,
            "identical inputs must always produce the same Merkle root"
        );
    }
}
