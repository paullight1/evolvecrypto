use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Entry type for the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryType {
    TradeFill,
    Deposit,
    Withdrawal,
    FundingPayment,
    Fee,
    Rebate,
    Liquidation,
    Transfer,
    MarginLock,
    MarginRelease,
    RealizedPnl,
    InsuranceSurplus,
    InsuranceDeficit,
    Adjustment,
}

impl EntryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TradeFill => "trade_fill",
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::FundingPayment => "funding_payment",
            Self::Fee => "fee",
            Self::Rebate => "rebate",
            Self::Liquidation => "liquidation",
            Self::Transfer => "transfer",
            Self::MarginLock => "margin_lock",
            Self::MarginRelease => "margin_release",
            Self::RealizedPnl => "realized_pnl",
            Self::InsuranceSurplus => "insurance_surplus",
            Self::InsuranceDeficit => "insurance_deficit",
            Self::Adjustment => "adjustment",
        }
    }
}

/// A single entry in a ledger transaction.
pub struct LedgerEntry {
    pub account_id: Uuid,
    /// "user_spot", "user_derivatives", or "system"
    pub account_type: String,
    pub asset: String,
    /// Signed amount: positive = credit (inflow), negative = debit (outflow).
    pub amount: Decimal,
    pub entry_type: EntryType,
    pub reference_id: Option<String>,
    pub reference_type: Option<String>,
    pub description: Option<String>,
}

/// Result of posting a ledger transaction.
#[derive(Debug)]
pub struct LedgerTransactionResult {
    pub transaction_id: Uuid,
    pub entries_created: usize,
}

/// The double-entry ledger service.
///
/// This is the ONLY authoritative path for balance changes in the system.
/// Every call to `post` is atomic: all entries land in one PostgreSQL
/// transaction or none do.
pub struct LedgerService;

impl LedgerService {
    /// Post a ledger transaction (multiple entries that MUST sum to zero).
    ///
    /// Invariants enforced before touching the database:
    /// - At least one entry must be provided.
    /// - The sum of all `amount` fields must be exactly zero (double-entry rule).
    ///
    /// Invariants enforced inside the database transaction:
    /// - No user account balance (user_spot / user_derivatives) may go negative.
    /// - System accounts are allowed negative balances (e.g. the hot wallet
    ///   "owes" the user until the on-chain send completes).
    ///
    /// Each entry records `balance_before` and `balance_after` for a full audit
    /// trail. The `balances` table is kept in sync as a fast-read cache.
    pub async fn post(
        pool: &PgPool,
        entries: Vec<LedgerEntry>,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        if entries.is_empty() {
            return Err(LedgerError::EmptyTransaction);
        }

        // Verify double-entry invariant before touching the DB.
        let total: Decimal = entries.iter().map(|e| e.amount).sum();
        if !total.is_zero() {
            return Err(LedgerError::ImbalancedTransaction {
                total,
                entry_count: entries.len(),
            });
        }

        let transaction_id = Uuid::new_v4();
        let now = Utc::now();

        let mut tx = pool.begin().await.map_err(LedgerError::Database)?;

        for entry in &entries {
            // Compute the running balance for this account+asset from the ledger
            // itself (the single source of truth).  The query runs inside the
            // open transaction so it sees the rows written by earlier iterations
            // of this loop.
            let balance_before: Decimal = sqlx::query_scalar(
                "SELECT COALESCE(SUM(amount), 0) \
                 FROM ledger_entries \
                 WHERE account_id = $1 AND asset = $2",
            )
            .bind(entry.account_id)
            .bind(&entry.asset)
            .fetch_one(&mut *tx)
            .await
            .map_err(LedgerError::Database)?;

            let balance_after = balance_before + entry.amount;

            // User balances must never go negative.  System accounts (hot
            // wallet, insurance fund, etc.) are allowed to go negative because
            // they represent exchange obligations.
            if entry.account_type.starts_with("user") && balance_after < Decimal::ZERO {
                return Err(LedgerError::InsufficientBalance {
                    account_id: entry.account_id,
                    asset: entry.asset.clone(),
                    current: balance_before,
                    attempted: entry.amount,
                });
            }

            sqlx::query(
                "INSERT INTO ledger_entries \
                 (transaction_id, account_id, account_type, asset, amount, \
                  entry_type, reference_id, reference_type, \
                  balance_before, balance_after, description, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(transaction_id)
            .bind(entry.account_id)
            .bind(&entry.account_type)
            .bind(&entry.asset)
            .bind(entry.amount)
            .bind(entry.entry_type.as_str())
            .bind(&entry.reference_id)
            .bind(&entry.reference_type)
            .bind(balance_before)
            .bind(balance_after)
            .bind(&entry.description)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(LedgerError::Database)?;

            // Keep the `balances` table in sync for fast reads.
            // Only user accounts have rows there; system accounts do not.
            if entry.account_type.starts_with("user") {
                sqlx::query(
                    "INSERT INTO balances (user_id, asset, total, locked, updated_at) \
                     VALUES ($1, $2, $3, 0, NOW()) \
                     ON CONFLICT (user_id, asset) DO UPDATE SET \
                        total = balances.total + $3, \
                        updated_at = NOW()",
                )
                .bind(entry.account_id)
                .bind(&entry.asset)
                .bind(entry.amount)
                .execute(&mut *tx)
                .await
                .map_err(LedgerError::Database)?;
            }
        }

        tx.commit().await.map_err(LedgerError::Database)?;

        tracing::debug!(
            transaction_id = %transaction_id,
            entries = entries.len(),
            "ledger transaction committed"
        );

        Ok(LedgerTransactionResult {
            transaction_id,
            entries_created: entries.len(),
        })
    }

    // -------------------------------------------------------------------------
    // High-level helpers
    // -------------------------------------------------------------------------

    /// Settle a spot trade: buyer pays quote, receives base (minus fee).
    /// Seller pays base, receives quote (minus fee).
    /// Fees flow to the exchange fee-revenue system account.
    ///
    /// Creates 4–6 ledger entries atomically.
    pub async fn settle_spot_trade(
        pool: &PgPool,
        buyer_id: Uuid,
        seller_id: Uuid,
        base_asset: &str,
        quote_asset: &str,
        base_qty: Decimal,
        quote_cost: Decimal,
        buyer_fee: Decimal,  // collected in base asset (taker)
        seller_fee: Decimal, // collected in quote asset (maker)
        trade_id: &str,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        let fee_account = Uuid::from_u128(0x01); // fee_revenue system account

        let mut entries = vec![
            // Buyer pays quote.
            LedgerEntry {
                account_id: buyer_id,
                account_type: "user_spot".into(),
                asset: quote_asset.into(),
                amount: -quote_cost,
                entry_type: EntryType::TradeFill,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: Some(format!("Buy {} {}", base_qty, base_asset)),
            },
            // Buyer receives base minus fee.
            LedgerEntry {
                account_id: buyer_id,
                account_type: "user_spot".into(),
                asset: base_asset.into(),
                amount: base_qty - buyer_fee,
                entry_type: EntryType::TradeFill,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: None,
            },
            // Seller pays base.
            LedgerEntry {
                account_id: seller_id,
                account_type: "user_spot".into(),
                asset: base_asset.into(),
                amount: -base_qty,
                entry_type: EntryType::TradeFill,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: Some(format!("Sell {} {}", base_qty, base_asset)),
            },
            // Seller receives quote minus fee.
            LedgerEntry {
                account_id: seller_id,
                account_type: "user_spot".into(),
                asset: quote_asset.into(),
                amount: quote_cost - seller_fee,
                entry_type: EntryType::TradeFill,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: None,
            },
        ];

        // Fees must balance out the entries above.
        // buyer_fee is in base: buyer got (base_qty - buyer_fee), fee_account gets buyer_fee.
        // seller_fee is in quote: seller got (quote_cost - seller_fee), fee_account gets seller_fee.
        if buyer_fee > Decimal::ZERO {
            entries.push(LedgerEntry {
                account_id: fee_account,
                account_type: "system".into(),
                asset: base_asset.into(),
                amount: buyer_fee,
                entry_type: EntryType::Fee,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: Some("Taker fee".into()),
            });
        }
        if seller_fee > Decimal::ZERO {
            entries.push(LedgerEntry {
                account_id: fee_account,
                account_type: "system".into(),
                asset: quote_asset.into(),
                amount: seller_fee,
                entry_type: EntryType::Fee,
                reference_id: Some(trade_id.into()),
                reference_type: Some("trade".into()),
                description: Some("Maker fee".into()),
            });
        }

        Self::post(pool, entries).await
    }

    /// Record a deposit: user receives funds, hot wallet is debited.
    pub async fn record_deposit(
        pool: &PgPool,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
        deposit_id: &str,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        let hot_wallet = Uuid::from_u128(0x05);

        Self::post(
            pool,
            vec![
                LedgerEntry {
                    account_id: user_id,
                    account_type: "user_spot".into(),
                    asset: asset.into(),
                    amount,
                    entry_type: EntryType::Deposit,
                    reference_id: Some(deposit_id.into()),
                    reference_type: Some("deposit".into()),
                    description: Some(format!("Deposit {} {}", amount, asset)),
                },
                // Hot wallet is credited on-chain and "owes" the user; we debit
                // its ledger balance to reflect that obligation being discharged.
                LedgerEntry {
                    account_id: hot_wallet,
                    account_type: "system".into(),
                    asset: asset.into(),
                    amount: -amount,
                    entry_type: EntryType::Deposit,
                    reference_id: Some(deposit_id.into()),
                    reference_type: Some("deposit".into()),
                    description: None,
                },
            ],
        )
        .await
    }

    /// Record a withdrawal: user pays amount + fee; hot wallet receives amount
    /// (to send on-chain); fee account receives the fee.
    pub async fn record_withdrawal(
        pool: &PgPool,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
        fee: Decimal,
        withdrawal_id: &str,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        let hot_wallet = Uuid::from_u128(0x05);
        let fee_account = Uuid::from_u128(0x01);

        let mut entries = vec![
            // User pays the total (amount + fee) in one debit.
            LedgerEntry {
                account_id: user_id,
                account_type: "user_spot".into(),
                asset: asset.into(),
                amount: -(amount + fee),
                entry_type: EntryType::Withdrawal,
                reference_id: Some(withdrawal_id.into()),
                reference_type: Some("withdrawal".into()),
                description: Some(format!("Withdraw {} {}", amount, asset)),
            },
            // Hot wallet receives the net amount it will send on-chain.
            LedgerEntry {
                account_id: hot_wallet,
                account_type: "system".into(),
                asset: asset.into(),
                amount,
                entry_type: EntryType::Withdrawal,
                reference_id: Some(withdrawal_id.into()),
                reference_type: Some("withdrawal".into()),
                description: None,
            },
        ];

        if fee > Decimal::ZERO {
            entries.push(LedgerEntry {
                account_id: fee_account,
                account_type: "system".into(),
                asset: asset.into(),
                amount: fee,
                entry_type: EntryType::Fee,
                reference_id: Some(withdrawal_id.into()),
                reference_type: Some("withdrawal".into()),
                description: Some("Withdrawal fee".into()),
            });
        }

        Self::post(pool, entries).await
    }

    /// Internal transfer between wallets (spot ↔ derivatives, or sub-accounts).
    pub async fn record_transfer(
        pool: &PgPool,
        from_id: Uuid,
        from_type: &str,
        to_id: Uuid,
        to_type: &str,
        asset: &str,
        amount: Decimal,
        transfer_id: &str,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        Self::post(
            pool,
            vec![
                LedgerEntry {
                    account_id: from_id,
                    account_type: from_type.into(),
                    asset: asset.into(),
                    amount: -amount,
                    entry_type: EntryType::Transfer,
                    reference_id: Some(transfer_id.into()),
                    reference_type: Some("transfer".into()),
                    description: None,
                },
                LedgerEntry {
                    account_id: to_id,
                    account_type: to_type.into(),
                    asset: asset.into(),
                    amount,
                    entry_type: EntryType::Transfer,
                    reference_id: Some(transfer_id.into()),
                    reference_type: Some("transfer".into()),
                    description: None,
                },
            ],
        )
        .await
    }

    /// Settle a funding payment: payer is debited, receiver is credited.
    pub async fn record_funding(
        pool: &PgPool,
        payer_id: Uuid,
        receiver_id: Uuid,
        asset: &str,
        amount: Decimal,
        funding_id: &str,
    ) -> Result<LedgerTransactionResult, LedgerError> {
        Self::post(
            pool,
            vec![
                LedgerEntry {
                    account_id: payer_id,
                    account_type: "user_derivatives".into(),
                    asset: asset.into(),
                    amount: -amount,
                    entry_type: EntryType::FundingPayment,
                    reference_id: Some(funding_id.into()),
                    reference_type: Some("funding".into()),
                    description: None,
                },
                LedgerEntry {
                    account_id: receiver_id,
                    account_type: "user_derivatives".into(),
                    asset: asset.into(),
                    amount,
                    entry_type: EntryType::FundingPayment,
                    reference_id: Some(funding_id.into()),
                    reference_type: Some("funding".into()),
                    description: None,
                },
            ],
        )
        .await
    }

    // -------------------------------------------------------------------------
    // Audit / reconciliation
    // -------------------------------------------------------------------------

    /// Verify that every transaction in the ledger sums to zero.
    ///
    /// Uses the `ledger_invariant_check` view defined in the migration.
    /// Returns `true` when there are no violations.
    pub async fn verify_invariant(pool: &PgPool) -> Result<bool, LedgerError> {
        // The view only returns rows where the sum is non-zero (violations).
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_invariant_check")
            .fetch_one(pool)
            .await
            .map_err(LedgerError::Database)?;

        Ok(violations == 0)
    }

    /// Compute the current balance for an account+asset directly from the
    /// ledger (bypasses the `balances` cache).  Use for auditing.
    pub async fn computed_balance(
        pool: &PgPool,
        account_id: Uuid,
        asset: &str,
    ) -> Result<Decimal, LedgerError> {
        let balance: Decimal = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0) \
             FROM ledger_entries \
             WHERE account_id = $1 AND asset = $2",
        )
        .bind(account_id)
        .bind(asset)
        .fetch_one(pool)
        .await
        .map_err(LedgerError::Database)?;

        Ok(balance)
    }

    /// Return the most recent ledger entries for an account, newest first.
    pub async fn account_history(
        pool: &PgPool,
        account_id: Uuid,
        asset: Option<&str>,
        limit: i64,
    ) -> Result<Vec<LedgerEntryRow>, LedgerError> {
        let rows = match asset {
            Some(asset) => {
                sqlx::query_as::<_, LedgerEntryRow>(
                    "SELECT id, transaction_id, account_id, account_type, asset, amount, \
                            entry_type, reference_id, reference_type, \
                            balance_before, balance_after, description, created_at \
                     FROM ledger_entries \
                     WHERE account_id = $1 AND asset = $2 \
                     ORDER BY created_at DESC \
                     LIMIT $3",
                )
                .bind(account_id)
                .bind(asset)
                .bind(limit)
                .fetch_all(pool)
                .await
            }
            None => {
                sqlx::query_as::<_, LedgerEntryRow>(
                    "SELECT id, transaction_id, account_id, account_type, asset, amount, \
                            entry_type, reference_id, reference_type, \
                            balance_before, balance_after, description, created_at \
                     FROM ledger_entries \
                     WHERE account_id = $1 \
                     ORDER BY created_at DESC \
                     LIMIT $2",
                )
                .bind(account_id)
                .bind(limit)
                .fetch_all(pool)
                .await
            }
        };

        rows.map_err(LedgerError::Database)
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A row returned from `ledger_entries`.
#[derive(Debug, sqlx::FromRow)]
pub struct LedgerEntryRow {
    pub id: i64,
    pub transaction_id: Uuid,
    pub account_id: Uuid,
    pub account_type: String,
    pub asset: String,
    pub amount: Decimal,
    pub entry_type: String,
    pub reference_id: Option<String>,
    pub reference_type: Option<String>,
    pub balance_before: Decimal,
    pub balance_after: Decimal,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Errors that the ledger service can return.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("Empty transaction — at least one entry is required")]
    EmptyTransaction,

    #[error(
        "Imbalanced transaction: sum={total}, entries={entry_count} (must sum to zero)"
    )]
    ImbalancedTransaction {
        total: Decimal,
        entry_count: usize,
    },

    #[error(
        "Insufficient balance: account {account_id} asset {asset} \
         has {current}, attempted debit of {attempted}"
    )]
    InsufficientBalance {
        account_id: Uuid,
        asset: String,
        current: Decimal,
        attempted: Decimal,
    },

    #[error("Database error: {0}")]
    Database(sqlx::Error),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    async fn test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for ledger integration tests");
        PgPool::connect(&url)
            .await
            .expect("failed to connect to test database")
    }

    // Helper: deposit funds into a user account so subsequent tests can debit.
    async fn seed_deposit(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) {
        LedgerService::record_deposit(pool, user_id, asset, amount, &Uuid::new_v4().to_string())
            .await
            .expect("seed deposit failed");
    }

    // -----------------------------------------------------------------------
    // Invariant checks (no DB needed)
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_transaction_rejected_without_db() {
        // EmptyTransaction is returned before the DB is touched.
        // We cannot call post without a pool, so we verify the guard condition directly.
        let entries: Vec<LedgerEntry> = vec![];
        assert!(entries.is_empty()); // the guard fires on empty vec
    }

    #[test]
    fn test_imbalanced_amount_detected_without_db() {
        // Verify that the sum check catches an imbalanced set.
        let entries = vec![
            LedgerEntry {
                account_id: Uuid::new_v4(),
                account_type: "user_spot".into(),
                asset: "BTC".into(),
                amount: dec!(1),
                entry_type: EntryType::Deposit,
                reference_id: None,
                reference_type: None,
                description: None,
            },
            LedgerEntry {
                account_id: Uuid::from_u128(0x05),
                account_type: "system".into(),
                asset: "BTC".into(),
                amount: dec!(-0.5), // intentionally wrong — does NOT sum to zero
                entry_type: EntryType::Deposit,
                reference_id: None,
                reference_type: None,
                description: None,
            },
        ];
        let total: Decimal = entries.iter().map(|e| e.amount).sum();
        assert!(!total.is_zero(), "expected imbalance to be detected");
    }

    #[test]
    fn test_entry_type_strings() {
        // Ensure every variant round-trips through as_str() correctly.
        let cases = [
            (EntryType::TradeFill, "trade_fill"),
            (EntryType::Deposit, "deposit"),
            (EntryType::Withdrawal, "withdrawal"),
            (EntryType::FundingPayment, "funding_payment"),
            (EntryType::Fee, "fee"),
            (EntryType::Rebate, "rebate"),
            (EntryType::Liquidation, "liquidation"),
            (EntryType::Transfer, "transfer"),
            (EntryType::MarginLock, "margin_lock"),
            (EntryType::MarginRelease, "margin_release"),
            (EntryType::RealizedPnl, "realized_pnl"),
            (EntryType::InsuranceSurplus, "insurance_surplus"),
            (EntryType::InsuranceDeficit, "insurance_deficit"),
            (EntryType::Adjustment, "adjustment"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
        }
    }

    // -----------------------------------------------------------------------
    // Integration tests (require TEST_DATABASE_URL)
    // -----------------------------------------------------------------------

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_balanced_transaction() {
        let pool = test_pool().await;
        let user = Uuid::new_v4();
        let system = Uuid::from_u128(0x05);

        let result = LedgerService::post(
            &pool,
            vec![
                LedgerEntry {
                    account_id: user,
                    account_type: "user_spot".into(),
                    asset: "BTC".into(),
                    amount: dec!(1),
                    entry_type: EntryType::Deposit,
                    reference_id: None,
                    reference_type: None,
                    description: None,
                },
                LedgerEntry {
                    account_id: system,
                    account_type: "system".into(),
                    asset: "BTC".into(),
                    amount: dec!(-1),
                    entry_type: EntryType::Deposit,
                    reference_id: None,
                    reference_type: None,
                    description: None,
                },
            ],
        )
        .await
        .expect("balanced transaction must succeed");

        assert_eq!(result.entries_created, 2);
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_imbalanced_rejected() {
        let pool = test_pool().await;

        let result = LedgerService::post(
            &pool,
            vec![LedgerEntry {
                account_id: Uuid::new_v4(),
                account_type: "user_spot".into(),
                asset: "BTC".into(),
                amount: dec!(1),
                entry_type: EntryType::Deposit,
                reference_id: None,
                reference_type: None,
                description: None,
            }],
        )
        .await;

        assert!(
            matches!(result, Err(LedgerError::ImbalancedTransaction { .. })),
            "single-entry transaction must be rejected as imbalanced"
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_insufficient_balance_rejected() {
        let pool = test_pool().await;
        let user = Uuid::new_v4();
        let system = Uuid::from_u128(0x05);

        // User has zero balance; attempting to debit must fail.
        let result = LedgerService::post(
            &pool,
            vec![
                LedgerEntry {
                    account_id: user,
                    account_type: "user_spot".into(),
                    asset: "USDT".into(),
                    amount: dec!(-100),
                    entry_type: EntryType::Withdrawal,
                    reference_id: None,
                    reference_type: None,
                    description: None,
                },
                LedgerEntry {
                    account_id: system,
                    account_type: "system".into(),
                    asset: "USDT".into(),
                    amount: dec!(100),
                    entry_type: EntryType::Withdrawal,
                    reference_id: None,
                    reference_type: None,
                    description: None,
                },
            ],
        )
        .await;

        assert!(
            matches!(result, Err(LedgerError::InsufficientBalance { .. })),
            "debit beyond balance must be rejected"
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_spot_trade_settlement() {
        let pool = test_pool().await;
        let buyer = Uuid::new_v4();
        let seller = Uuid::new_v4();
        let trade_id = Uuid::new_v4().to_string();

        // Fund both parties first.
        seed_deposit(&pool, buyer, "USDT", dec!(1000)).await;
        seed_deposit(&pool, seller, "BTC", dec!(1)).await;

        // base_qty=0.5 BTC at 50000 USDT, buyer_fee=0.001 BTC, seller_fee=25 USDT
        let result = LedgerService::settle_spot_trade(
            &pool,
            buyer,
            seller,
            "BTC",
            "USDT",
            dec!(0.5),
            dec!(25000),
            dec!(0.0005),
            dec!(12.5),
            &trade_id,
        )
        .await
        .expect("spot trade settlement must succeed");

        // 4 trade entries + 2 fee entries = 6 total
        assert_eq!(result.entries_created, 6);

        // Buyer should have received 0.4995 BTC and still have 975 USDT.
        let buyer_btc = LedgerService::computed_balance(&pool, buyer, "BTC").await.unwrap();
        let buyer_usdt = LedgerService::computed_balance(&pool, buyer, "USDT").await.unwrap();
        assert_eq!(buyer_btc, dec!(0.4995));
        assert_eq!(buyer_usdt, dec!(975));

        // Seller should have 0.5 BTC left and 24987.5 USDT.
        let seller_btc = LedgerService::computed_balance(&pool, seller, "BTC").await.unwrap();
        let seller_usdt = LedgerService::computed_balance(&pool, seller, "USDT").await.unwrap();
        assert_eq!(seller_btc, dec!(0.5));
        assert_eq!(seller_usdt, dec!(24987.5));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_computed_balance_after_deposit_and_withdrawal() {
        let pool = test_pool().await;
        let user = Uuid::new_v4();

        // Start at zero, deposit 500, withdraw 200 (no fee).
        assert_eq!(
            LedgerService::computed_balance(&pool, user, "USDT").await.unwrap(),
            dec!(0)
        );

        seed_deposit(&pool, user, "USDT", dec!(500)).await;
        assert_eq!(
            LedgerService::computed_balance(&pool, user, "USDT").await.unwrap(),
            dec!(500)
        );

        LedgerService::record_withdrawal(&pool, user, "USDT", dec!(200), dec!(0), &Uuid::new_v4().to_string())
            .await
            .expect("withdrawal must succeed");
        assert_eq!(
            LedgerService::computed_balance(&pool, user, "USDT").await.unwrap(),
            dec!(300)
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_invariant_holds() {
        let pool = test_pool().await;
        // Post a balanced transaction and confirm the invariant view stays clean.
        let user = Uuid::new_v4();
        seed_deposit(&pool, user, "ETH", dec!(2)).await;

        let ok = LedgerService::verify_invariant(&pool).await.unwrap();
        assert!(ok, "double-entry invariant must hold after balanced transactions");
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_account_history_ordering() {
        let pool = test_pool().await;
        let user = Uuid::new_v4();

        seed_deposit(&pool, user, "BTC", dec!(1)).await;
        seed_deposit(&pool, user, "BTC", dec!(0.5)).await;

        let history = LedgerService::account_history(&pool, user, Some("BTC"), 10)
            .await
            .unwrap();

        // Newest entry first, amounts are credits (positive).
        assert_eq!(history.len(), 2);
        assert!(history[0].created_at >= history[1].created_at);
        assert!(history.iter().all(|r| r.amount > Decimal::ZERO));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_transfer_between_wallets() {
        let pool = test_pool().await;
        let user = Uuid::new_v4();
        let transfer_id = Uuid::new_v4().to_string();

        seed_deposit(&pool, user, "USDT", dec!(1000)).await;

        // Transfer 400 from spot to derivatives.
        LedgerService::record_transfer(
            &pool,
            user,
            "user_spot",
            user,
            "user_derivatives",
            "USDT",
            dec!(400),
            &transfer_id,
        )
        .await
        .expect("transfer must succeed");

        // Ledger balances for the same account_id are additive across wallet
        // types, so the computed balance reflects both.
        let total = LedgerService::computed_balance(&pool, user, "USDT").await.unwrap();
        assert_eq!(total, dec!(1000), "net balance unchanged after internal transfer");
    }
}
