use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

use exchange_common::UserId;

use crate::account_manager::AccountManager;
use crate::error::AccountError;
use crate::sharded::ShardedAccounts;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransferError {
    #[error("Insufficient available balance: need {needed}, have {available}")]
    InsufficientBalance { needed: Decimal, available: Decimal },

    #[error("Cannot transfer to the same account")]
    SameAccount,

    #[error("Amount must be positive, got {0}")]
    InvalidAmount(Decimal),

    #[error("Source account not found: {0}")]
    SourceAccountNotFound(UserId),

    #[error("Destination account not found: {0}")]
    DestinationAccountNotFound(UserId),
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Classification of a transfer for audit and reporting purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    /// Between two sub-accounts belonging to the same master.
    SubToSub,
    /// From master to one of its own sub-accounts.
    MasterToSub,
    /// From a sub-account back to its master.
    SubToMaster,
    /// Between two unrelated users (P2P transfer).
    UserToUser,
}

/// Immutable record of a completed transfer.
#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub id: Uuid,
    pub from_user: UserId,
    pub to_user: UserId,
    pub asset: String,
    pub amount: Decimal,
    pub transfer_type: TransferType,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Executes zero-fee instant transfers and maintains transfer history.
///
/// `TransferManager` does not own `AccountManager` — it mutably borrows it
/// only during `transfer`, keeping the two concerns cleanly separated.
pub struct TransferManager {
    history: Vec<TransferRecord>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self { history: Vec::new() }
    }

    /// Execute a transfer from `from` to `to` for `amount` of `asset`.
    ///
    /// The operation is atomic from the `AccountManager`'s perspective:
    /// the withdraw is validated first; only on success is the deposit applied.
    ///
    /// Returns the completed `TransferRecord` on success.
    pub fn transfer(
        &mut self,
        accounts: &mut AccountManager,
        from: UserId,
        to: UserId,
        asset: &str,
        amount: Decimal,
        transfer_type: TransferType,
    ) -> Result<TransferRecord, TransferError> {
        // --- Validate inputs ---

        if from == to {
            return Err(TransferError::SameAccount);
        }

        if amount <= Decimal::ZERO {
            return Err(TransferError::InvalidAmount(amount));
        }

        // Check that `from` has sufficient available balance before touching anything.
        // This produces a clean error instead of propagating AccountError.
        let available = accounts
            .get_balance(&from, asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);

        if available < amount {
            return Err(TransferError::InsufficientBalance {
                needed: amount,
                available,
            });
        }

        // --- Execute: withdraw then deposit ---
        // Both accounts must exist. We checked available balance above so the
        // withdraw cannot fail due to insufficient funds; AccountNotFound is the
        // only remaining failure mode.

        accounts.withdraw(from, asset, amount).map_err(|_| {
            // withdraw returned AccountNotFound (available check passed, so balance exists)
            TransferError::SourceAccountNotFound(from)
        })?;

        if let Err(e) = accounts.deposit(to, asset, amount) {
            // Deposit failed (destination account not found).
            // Roll back the withdrawal so the source is made whole.
            accounts
                .deposit(from, asset, amount)
                .expect("rollback deposit into source must succeed");

            tracing::error!(
                from = %from,
                to = %to,
                asset,
                amount = %amount,
                err = %e,
                "Transfer deposit failed, withdrawal rolled back"
            );

            return Err(TransferError::DestinationAccountNotFound(to));
        }

        // --- Record ---

        let record = TransferRecord {
            id: Uuid::new_v4(),
            from_user: from,
            to_user: to,
            asset: asset.to_string(),
            amount,
            transfer_type,
            timestamp: Utc::now(),
        };

        tracing::info!(
            transfer_id = %record.id,
            from = %from,
            to = %to,
            asset,
            amount = %amount,
            kind = ?transfer_type,
            "Transfer completed"
        );

        self.history.push(record.clone());
        Ok(record)
    }

    /// S18 Phase B: variant of `transfer` that operates on the 64-shard
    /// `ShardedAccounts` store used by the api-gateway in production.
    ///
    /// Behaviour is identical to `transfer`: same validation, same error
    /// mapping, same `TransferRecord` emitted on success and appended to
    /// history. The only difference is that balance inspection and the
    /// atomic cross-shard withdraw/deposit pair go through the sharded
    /// store's built-in 2-shard lock ordering (see
    /// `ShardedAccounts::transfer`), eliminating the per-operation global
    /// account mutex.
    pub fn transfer_sharded(
        &mut self,
        accounts: &ShardedAccounts,
        from: UserId,
        to: UserId,
        asset: &str,
        amount: Decimal,
        transfer_type: TransferType,
    ) -> Result<TransferRecord, TransferError> {
        if from == to {
            return Err(TransferError::SameAccount);
        }
        if amount <= Decimal::ZERO {
            return Err(TransferError::InvalidAmount(amount));
        }

        let available = accounts
            .get_balance(from, asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if available < amount {
            return Err(TransferError::InsufficientBalance {
                needed: amount,
                available,
            });
        }

        // Map AccountError → TransferError. `ShardedAccounts::transfer` already
        // rolls back the source debit if the destination deposit fails.
        accounts
            .transfer(from, to, asset, amount)
            .map_err(|e| match e {
                AccountError::AccountNotFound(uid) if uid == from => {
                    TransferError::SourceAccountNotFound(from)
                }
                AccountError::AccountNotFound(_) => TransferError::DestinationAccountNotFound(to),
                _ => TransferError::DestinationAccountNotFound(to),
            })?;

        let record = TransferRecord {
            id: Uuid::new_v4(),
            from_user: from,
            to_user: to,
            asset: asset.to_string(),
            amount,
            transfer_type,
            timestamp: Utc::now(),
        };

        tracing::info!(
            transfer_id = %record.id,
            from = %from,
            to = %to,
            asset,
            amount = %amount,
            kind = ?transfer_type,
            "Transfer completed (sharded)"
        );

        self.history.push(record.clone());
        Ok(record)
    }

    /// Return the most recent `limit` transfers involving `user_id`
    /// (either as sender or receiver), newest first.
    ///
    /// Passing `limit = 0` returns all matching records.
    pub fn user_transfers(&self, user_id: UserId, limit: usize) -> Vec<&TransferRecord> {
        let iter = self
            .history
            .iter()
            .rev()
            .filter(|r| r.from_user == user_id || r.to_user == user_id);

        if limit == 0 {
            iter.collect()
        } else {
            iter.take(limit).collect()
        }
    }
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup_two_accounts() -> (AccountManager, UserId, UserId) {
        let mut accounts = AccountManager::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        accounts.create_account(alice);
        accounts.create_account(bob);
        accounts.deposit(alice, "USDT", dec!(1000)).unwrap();
        accounts.deposit(bob, "USDT", dec!(500)).unwrap();
        (accounts, alice, bob)
    }

    // -----------------------------------------------------------------------
    // Happy-path transfers
    // -----------------------------------------------------------------------

    #[test]
    fn test_user_to_user_transfer() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();

        let record = tm
            .transfer(&mut accounts, alice, bob, "USDT", dec!(200), TransferType::UserToUser)
            .unwrap();

        assert_eq!(record.from_user, alice);
        assert_eq!(record.to_user, bob);
        assert_eq!(record.amount, dec!(200));
        assert_eq!(record.transfer_type, TransferType::UserToUser);

        assert_eq!(accounts.get_balance(&alice, "USDT").unwrap().total(), dec!(800));
        assert_eq!(accounts.get_balance(&bob, "USDT").unwrap().total(), dec!(700));
    }

    #[test]
    fn test_master_to_sub_transfer() {
        let mut accounts = AccountManager::new();
        let master = Uuid::new_v4();
        let sub = Uuid::new_v4();
        accounts.create_account(master);
        accounts.create_account(sub);
        accounts.deposit(master, "BTC", dec!(2)).unwrap();

        let mut tm = TransferManager::new();
        tm.transfer(&mut accounts, master, sub, "BTC", dec!(1), TransferType::MasterToSub)
            .unwrap();

        assert_eq!(accounts.get_balance(&master, "BTC").unwrap().total(), dec!(1));
        assert_eq!(accounts.get_balance(&sub, "BTC").unwrap().total(), dec!(1));
    }

    #[test]
    fn test_sub_to_master_transfer() {
        let mut accounts = AccountManager::new();
        let master = Uuid::new_v4();
        let sub = Uuid::new_v4();
        accounts.create_account(master);
        accounts.create_account(sub);
        accounts.deposit(sub, "USDT", dec!(300)).unwrap();

        let mut tm = TransferManager::new();
        tm.transfer(&mut accounts, sub, master, "USDT", dec!(300), TransferType::SubToMaster)
            .unwrap();

        assert_eq!(accounts.get_balance(&sub, "USDT").unwrap().total(), dec!(0));
        assert_eq!(accounts.get_balance(&master, "USDT").unwrap().total(), dec!(300));
    }

    #[test]
    fn test_sub_to_sub_transfer() {
        let mut accounts = AccountManager::new();
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        accounts.create_account(s1);
        accounts.create_account(s2);
        accounts.deposit(s1, "ETH", dec!(5)).unwrap();

        let mut tm = TransferManager::new();
        tm.transfer(&mut accounts, s1, s2, "ETH", dec!(2), TransferType::SubToSub).unwrap();

        assert_eq!(accounts.get_balance(&s1, "ETH").unwrap().total(), dec!(3));
        assert_eq!(accounts.get_balance(&s2, "ETH").unwrap().total(), dec!(2));
    }

    #[test]
    fn test_transfer_exact_available_balance() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();
        // Transfer exactly what alice has (no partial lock)
        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(1000), TransferType::UserToUser)
            .unwrap();
        assert_eq!(accounts.get_balance(&alice, "USDT").unwrap().total(), dec!(0));
        assert_eq!(accounts.get_balance(&bob, "USDT").unwrap().total(), dec!(1500));
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_same_account_rejected() {
        let (mut accounts, alice, _) = setup_two_accounts();
        let mut tm = TransferManager::new();
        let err = tm
            .transfer(&mut accounts, alice, alice, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap_err();
        assert_eq!(err, TransferError::SameAccount);
    }

    #[test]
    fn test_transfer_zero_amount_rejected() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();
        let err = tm
            .transfer(&mut accounts, alice, bob, "USDT", dec!(0), TransferType::UserToUser)
            .unwrap_err();
        assert!(matches!(err, TransferError::InvalidAmount(_)));
    }

    #[test]
    fn test_transfer_negative_amount_rejected() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();
        let err = tm
            .transfer(&mut accounts, alice, bob, "USDT", dec!(-50), TransferType::UserToUser)
            .unwrap_err();
        assert!(matches!(err, TransferError::InvalidAmount(_)));
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();
        // Alice has 1000, tries to send 1001
        let err = tm
            .transfer(&mut accounts, alice, bob, "USDT", dec!(1001), TransferType::UserToUser)
            .unwrap_err();
        assert!(matches!(
            err,
            TransferError::InsufficientBalance { needed, available }
            if needed == dec!(1001) && available == dec!(1000)
        ));
        // Source balance must be untouched
        assert_eq!(accounts.get_balance(&alice, "USDT").unwrap().total(), dec!(1000));
    }

    #[test]
    fn test_transfer_destination_not_found_rolls_back_source() {
        let mut accounts = AccountManager::new();
        let alice = Uuid::new_v4();
        accounts.create_account(alice);
        accounts.deposit(alice, "USDT", dec!(500)).unwrap();

        let ghost = Uuid::new_v4(); // never registered
        let mut tm = TransferManager::new();

        let err = tm
            .transfer(&mut accounts, alice, ghost, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap_err();
        assert!(matches!(err, TransferError::DestinationAccountNotFound(_)));

        // Alice's balance must be fully restored after rollback
        assert_eq!(accounts.get_balance(&alice, "USDT").unwrap().total(), dec!(500));
    }

    #[test]
    fn test_transfer_respects_locked_balance() {
        let mut accounts = AccountManager::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        accounts.create_account(alice);
        accounts.create_account(bob);
        accounts.deposit(alice, "USDT", dec!(1000)).unwrap();

        // Lock 800 of Alice's 1000 USDT (simulates an open order)
        use exchange_common::{Product, ProductType, Symbol};
        use rust_decimal_macros::dec;
        let product = Product {
            symbol: Symbol::new("BTCUSDT"),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            min_notional: dec!(10),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.002),
            max_order_qty: None,
            max_order_notional: None,
            max_leverage: None,
        };
        accounts.add_product(product);
        accounts
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), exchange_common::Side::Buy, dec!(800), dec!(1))
            .unwrap();
        // available = 200 now

        let mut tm = TransferManager::new();

        // Trying to send 500 (more than available 200) should fail
        let err = tm
            .transfer(&mut accounts, alice, bob, "USDT", dec!(500), TransferType::UserToUser)
            .unwrap_err();
        assert!(matches!(err, TransferError::InsufficientBalance { .. }));

        // But 200 should work
        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(200), TransferType::UserToUser)
            .unwrap();
        assert_eq!(accounts.get_balance(&alice, "USDT").unwrap().available(), dec!(0));
    }

    // -----------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_recorded_in_history() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();

        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap();
        tm.transfer(&mut accounts, bob, alice, "USDT", dec!(50), TransferType::UserToUser)
            .unwrap();

        assert_eq!(tm.user_transfers(alice, 0).len(), 2);
        assert_eq!(tm.user_transfers(bob, 0).len(), 2);
    }

    #[test]
    fn test_user_transfers_returns_newest_first() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();

        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap();
        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(200), TransferType::UserToUser)
            .unwrap();

        let history = tm.user_transfers(alice, 0);
        assert_eq!(history[0].amount, dec!(200)); // most recent first
        assert_eq!(history[1].amount, dec!(100));
    }

    #[test]
    fn test_user_transfers_limit() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();
        for _ in 0..5 {
            tm.transfer(&mut accounts, alice, bob, "USDT", dec!(10), TransferType::UserToUser)
                .unwrap();
            // Give bob enough for reverse transfers
        }
        assert_eq!(tm.user_transfers(alice, 3).len(), 3);
        assert_eq!(tm.user_transfers(alice, 0).len(), 5);
    }

    #[test]
    fn test_user_transfers_only_own_records() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let charlie = Uuid::new_v4();
        accounts.create_account(charlie);
        accounts.deposit(charlie, "USDT", dec!(500)).unwrap();

        let mut tm = TransferManager::new();
        tm.transfer(&mut accounts, alice, bob, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap();
        tm.transfer(&mut accounts, charlie, bob, "USDT", dec!(100), TransferType::UserToUser)
            .unwrap();

        // Alice sees only her transfer
        assert_eq!(tm.user_transfers(alice, 0).len(), 1);
        // Bob sees both (he received in both)
        assert_eq!(tm.user_transfers(bob, 0).len(), 2);
    }

    #[test]
    fn test_failed_transfer_not_recorded() {
        let (mut accounts, alice, bob) = setup_two_accounts();
        let mut tm = TransferManager::new();

        // This should fail (too much)
        let _ = tm.transfer(&mut accounts, alice, bob, "USDT", dec!(9999), TransferType::UserToUser);

        assert!(tm.user_transfers(alice, 0).is_empty());
    }
}
