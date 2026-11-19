use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::AccountError;

/// Balance for a single asset within an account.
///
/// Invariants (MUST hold at all times):
///   1. total >= 0
///   2. locked >= 0
///   3. available = total - locked >= 0
///
/// These are enforced in every mutation method.
/// In production, PostgreSQL CHECK constraints provide a second safety net.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    total: Decimal,
    locked: Decimal,
}

impl Balance {
    pub fn zero() -> Self {
        Self {
            total: Decimal::ZERO,
            locked: Decimal::ZERO,
        }
    }

    pub fn new(total: Decimal, locked: Decimal) -> Result<Self, AccountError> {
        if total < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(total));
        }
        if locked < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(locked));
        }
        if locked > total {
            return Err(AccountError::InsufficientBalance {
                needed: locked,
                available: total,
            });
        }
        Ok(Self { total, locked })
    }

    // --- Accessors ---

    pub fn total(&self) -> Decimal {
        self.total
    }

    pub fn locked(&self) -> Decimal {
        self.locked
    }

    pub fn available(&self) -> Decimal {
        self.total - self.locked
    }

    // --- Mutations ---

    /// Deposit funds (e.g., crypto deposit confirmed on-chain).
    /// Increases total and available.
    pub fn deposit(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        self.total += amount;
        self.assert_invariants();
        Ok(())
    }

    /// Withdraw funds. Only available (unlocked) balance can be withdrawn.
    pub fn withdraw(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        if amount > self.available() {
            return Err(AccountError::InsufficientBalance {
                needed: amount,
                available: self.available(),
            });
        }
        self.total -= amount;
        self.assert_invariants();
        Ok(())
    }

    /// Lock funds for an order placement.
    /// Moves `amount` from available → locked.
    ///
    /// Called by Order Gateway BEFORE sending command to matching engine.
    /// - Buy order: lock quote asset (price * qty)
    /// - Sell order: lock base asset (qty)
    pub fn try_lock(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        if amount > self.available() {
            return Err(AccountError::InsufficientBalance {
                needed: amount,
                available: self.available(),
            });
        }
        self.locked += amount;
        self.assert_invariants();
        Ok(())
    }

    /// Unlock funds when an order is cancelled or rejected.
    /// Moves `amount` from locked → available.
    pub fn unlock(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        if amount > self.locked {
            return Err(AccountError::InsufficientLocked {
                needed: amount,
                locked: self.locked,
            });
        }
        self.locked -= amount;
        self.assert_invariants();
        Ok(())
    }

    /// Deduct locked funds after a fill.
    /// Reduces both total and locked by `amount`.
    ///
    /// Used for the "outgoing" side of a trade:
    /// - Buyer: deduct quote (USDT spent)
    /// - Seller: deduct base (BTC sold)
    pub fn deduct_locked(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        if amount > self.locked {
            return Err(AccountError::InsufficientLocked {
                needed: amount,
                locked: self.locked,
            });
        }
        self.locked -= amount;
        self.total -= amount;
        self.assert_invariants();
        Ok(())
    }

    /// Credit funds after a fill (incoming asset).
    /// Increases total (available goes up automatically).
    ///
    /// Used for the "incoming" side of a trade:
    /// - Buyer: credit base (BTC received)
    /// - Seller: credit quote (USDT received)
    pub fn credit(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if amount < Decimal::ZERO {
            return Err(AccountError::NegativeAmount(amount));
        }
        self.total += amount;
        self.assert_invariants();
        Ok(())
    }

    /// Panic if invariants are violated. This is a programming error.
    fn assert_invariants(&self) {
        debug_assert!(
            self.total >= Decimal::ZERO,
            "INVARIANT VIOLATION: total < 0: {}",
            self.total
        );
        debug_assert!(
            self.locked >= Decimal::ZERO,
            "INVARIANT VIOLATION: locked < 0: {}",
            self.locked
        );
        debug_assert!(
            self.available() >= Decimal::ZERO,
            "INVARIANT VIOLATION: available < 0: {} (total={}, locked={})",
            self.available(),
            self.total,
            self.locked
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_zero_balance() {
        let b = Balance::zero();
        assert_eq!(b.total(), dec!(0));
        assert_eq!(b.locked(), dec!(0));
        assert_eq!(b.available(), dec!(0));
    }

    #[test]
    fn test_deposit() {
        let mut b = Balance::zero();
        b.deposit(dec!(1000)).unwrap();
        assert_eq!(b.total(), dec!(1000));
        assert_eq!(b.available(), dec!(1000));
        assert_eq!(b.locked(), dec!(0));
    }

    #[test]
    fn test_deposit_negative_fails() {
        let mut b = Balance::zero();
        assert!(b.deposit(dec!(-1)).is_err());
    }

    #[test]
    fn test_withdraw() {
        let mut b = Balance::zero();
        b.deposit(dec!(1000)).unwrap();
        b.withdraw(dec!(400)).unwrap();
        assert_eq!(b.total(), dec!(600));
        assert_eq!(b.available(), dec!(600));
    }

    #[test]
    fn test_withdraw_insufficient() {
        let mut b = Balance::zero();
        b.deposit(dec!(100)).unwrap();
        let err = b.withdraw(dec!(200)).unwrap_err();
        assert!(matches!(err, AccountError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_withdraw_respects_locked() {
        let mut b = Balance::zero();
        b.deposit(dec!(1000)).unwrap();
        b.try_lock(dec!(800)).unwrap();
        // available = 200, can't withdraw 500
        let err = b.withdraw(dec!(500)).unwrap_err();
        assert!(matches!(err, AccountError::InsufficientBalance { .. }));
        // but can withdraw 200
        b.withdraw(dec!(200)).unwrap();
        assert_eq!(b.total(), dec!(800));
        assert_eq!(b.available(), dec!(0));
    }

    #[test]
    fn test_lock_unlock() {
        let mut b = Balance::zero();
        b.deposit(dec!(1000)).unwrap();

        b.try_lock(dec!(600)).unwrap();
        assert_eq!(b.total(), dec!(1000));
        assert_eq!(b.locked(), dec!(600));
        assert_eq!(b.available(), dec!(400));

        b.unlock(dec!(200)).unwrap();
        assert_eq!(b.locked(), dec!(400));
        assert_eq!(b.available(), dec!(600));
    }

    #[test]
    fn test_lock_insufficient() {
        let mut b = Balance::zero();
        b.deposit(dec!(100)).unwrap();
        let err = b.try_lock(dec!(200)).unwrap_err();
        assert!(matches!(err, AccountError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_unlock_more_than_locked() {
        let mut b = Balance::zero();
        b.deposit(dec!(100)).unwrap();
        b.try_lock(dec!(50)).unwrap();
        let err = b.unlock(dec!(100)).unwrap_err();
        assert!(matches!(err, AccountError::InsufficientLocked { .. }));
    }

    #[test]
    fn test_deduct_locked() {
        let mut b = Balance::zero();
        b.deposit(dec!(1000)).unwrap();
        b.try_lock(dec!(500)).unwrap();

        // Simulate: seller's BTC gets deducted after fill
        b.deduct_locked(dec!(300)).unwrap();
        assert_eq!(b.total(), dec!(700));
        assert_eq!(b.locked(), dec!(200));
        assert_eq!(b.available(), dec!(500));
    }

    #[test]
    fn test_credit() {
        let mut b = Balance::zero();
        // Buyer receives BTC after fill
        b.credit(dec!(1.5)).unwrap();
        assert_eq!(b.total(), dec!(1.5));
        assert_eq!(b.available(), dec!(1.5));
    }

    #[test]
    fn test_full_trade_flow() {
        // Scenario: Alice buys 0.1 BTC at 50000 USDT from Bob

        // Alice: has 10000 USDT
        let mut alice_usdt = Balance::zero();
        alice_usdt.deposit(dec!(10000)).unwrap();
        let mut alice_btc = Balance::zero();

        // Bob: has 1 BTC
        let mut bob_btc = Balance::zero();
        bob_btc.deposit(dec!(1)).unwrap();
        let mut bob_usdt = Balance::zero();

        // Step 1: Lock funds when orders placed
        // Alice (buyer): lock 5000 USDT (0.1 BTC * 50000)
        alice_usdt.try_lock(dec!(5000)).unwrap();
        // Bob (seller): lock 0.1 BTC
        bob_btc.try_lock(dec!(0.1)).unwrap();

        // Step 2: Fill — settle the trade
        let fill_qty = dec!(0.1);
        let fill_price = dec!(50000);
        let fill_cost = fill_qty * fill_price; // 5000 USDT

        // Alice: deduct locked USDT, credit BTC
        alice_usdt.deduct_locked(fill_cost).unwrap();
        alice_btc.credit(fill_qty).unwrap();

        // Bob: deduct locked BTC, credit USDT
        bob_btc.deduct_locked(fill_qty).unwrap();
        bob_usdt.credit(fill_cost).unwrap();

        // Verify final state
        assert_eq!(alice_usdt.total(), dec!(5000));
        assert_eq!(alice_usdt.available(), dec!(5000));
        assert_eq!(alice_usdt.locked(), dec!(0));
        assert_eq!(alice_btc.total(), dec!(0.1));

        assert_eq!(bob_btc.total(), dec!(0.9));
        assert_eq!(bob_btc.available(), dec!(0.9));
        assert_eq!(bob_btc.locked(), dec!(0));
        assert_eq!(bob_usdt.total(), dec!(5000));
    }

    #[test]
    fn test_partial_fill_then_cancel() {
        // Alice places buy 1 BTC at 50000, gets 0.3 filled, cancels rest

        let mut alice_usdt = Balance::zero();
        alice_usdt.deposit(dec!(100000)).unwrap();

        // Lock for full order: 1 BTC * 50000 = 50000 USDT
        alice_usdt.try_lock(dec!(50000)).unwrap();
        assert_eq!(alice_usdt.available(), dec!(50000));

        // Partial fill: 0.3 BTC * 50000 = 15000 USDT
        alice_usdt.deduct_locked(dec!(15000)).unwrap();
        assert_eq!(alice_usdt.total(), dec!(85000));
        assert_eq!(alice_usdt.locked(), dec!(35000));
        assert_eq!(alice_usdt.available(), dec!(50000));

        // Cancel remainder: unlock remaining 35000 USDT
        alice_usdt.unlock(dec!(35000)).unwrap();
        assert_eq!(alice_usdt.total(), dec!(85000));
        assert_eq!(alice_usdt.locked(), dec!(0));
        assert_eq!(alice_usdt.available(), dec!(85000));
    }
}
