use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{CustodyProvider, ProviderTxStatus};

/// Withdrawal status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WithdrawalStatus {
    /// Submitted by user, awaiting 2FA confirmation.
    PendingConfirmation,
    /// 2FA confirmed, awaiting approval (auto or manual).
    PendingApproval,
    /// M22-3: AML screening in progress.
    AmlScreening,
    /// M22-3: Sanctions check in progress.
    SanctionsCheck,
    /// M22-3: Awaiting admin approval (for amounts above auto-approve threshold).
    ApprovalQueue,
    /// Approved, submitted to custody provider.
    Processing,
    /// On-chain, waiting for confirmations.
    OnChain { tx_hash: String },
    /// Successfully completed.
    Completed { tx_hash: String },
    /// Rejected by compliance or admin.
    Rejected { reason: String },
    /// Failed at provider level.
    Failed { reason: String },
}

/// Withdrawal record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub asset: String,
    pub network: String,
    pub address: String,
    pub amount: Decimal,
    pub fee: Decimal,
    pub status: WithdrawalStatus,
    pub provider_tx_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WithdrawalError {
    #[error("Withdrawal not found: {0}")]
    NotFound(Uuid),

    #[error("Invalid status transition: cannot {action} a {status} withdrawal")]
    InvalidTransition { status: String, action: String },

    #[error("Amount too small: {amount} (minimum: {minimum})")]
    AmountTooSmall { amount: Decimal, minimum: Decimal },
}

/// User tier controlling the daily withdrawal limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTier {
    /// Unverified user — $1,000 / 24h.
    Default,
    /// KYC-verified user — $10,000 / 24h.
    KycVerified,
    /// VIP user — $100,000 / 24h.
    Vip,
}

/// Configuration for per-tier 24-hour withdrawal limits.
#[derive(Debug, Clone)]
pub struct DailyLimitConfig {
    /// Default (unverified) user limit in USD equivalent.
    pub default_limit_usd: Decimal,
    /// KYC-verified user limit in USD equivalent.
    pub kyc_verified_limit_usd: Decimal,
    /// VIP user limit in USD equivalent.
    pub vip_limit_usd: Decimal,
}

impl Default for DailyLimitConfig {
    fn default() -> Self {
        Self {
            default_limit_usd: Decimal::new(1_000, 0),
            kyc_verified_limit_usd: Decimal::new(10_000, 0),
            vip_limit_usd: Decimal::new(100_000, 0),
        }
    }
}

impl DailyLimitConfig {
    /// Return the daily limit for the given tier.
    pub fn limit_for(&self, tier: UserTier) -> Decimal {
        match tier {
            UserTier::Default => self.default_limit_usd,
            UserTier::KycVerified => self.kyc_verified_limit_usd,
            UserTier::Vip => self.vip_limit_usd,
        }
    }
}

/// Error returned when a withdrawal would exceed the user's daily limit.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Daily withdrawal limit exceeded: used {daily_used}, limit {daily_limit}, remaining {remaining_limit}")]
pub struct WithdrawalLimitError {
    /// Amount already withdrawn in the past 24 hours.
    pub daily_used: Decimal,
    /// The user's daily limit for their tier.
    pub daily_limit: Decimal,
    /// How much the user can still withdraw today.
    pub remaining_limit: Decimal,
}

/// Manages withdrawal requests and approval flow.
///
/// Flow:
/// 1. User submits withdrawal → PendingApproval
/// 2. Auto-approve if amount < threshold, else manual review
/// 3. Approved → submit to CustodyProvider → Processing
/// 4. Provider confirms → Completed
///
/// Phase 1: manual approval via admin API.
/// Phase 2: auto-approve with AML checks + 2FA confirmation.
pub struct WithdrawalManager {
    withdrawals: Vec<WithdrawalRecord>,
    /// Auto-approve threshold in USD equivalent.
    auto_approve_threshold: Decimal,
    /// Withdrawal fees per asset.
    fees: HashMap<String, Decimal>,
    /// Minimum withdrawal per asset.
    minimums: HashMap<String, Decimal>,
    /// Per-tier 24-hour withdrawal limits.
    daily_limit_config: DailyLimitConfig,
    /// Per-user custom overrides set by admin (overrides tier-based limits).
    user_overrides: HashMap<Uuid, Decimal>,
}

impl WithdrawalManager {
    pub fn new() -> Self {
        let mut fees = HashMap::new();
        fees.insert("BTC".to_string(), Decimal::new(5, 5));    // 0.00005 BTC
        fees.insert("ETH".to_string(), Decimal::new(1, 3));    // 0.001 ETH
        fees.insert("USDT".to_string(), Decimal::new(1, 0));   // 1 USDT
        fees.insert("SOL".to_string(), Decimal::new(1, 2));    // 0.01 SOL

        let mut minimums = HashMap::new();
        minimums.insert("BTC".to_string(), Decimal::new(1, 4));   // 0.0001 BTC
        minimums.insert("ETH".to_string(), Decimal::new(1, 3));   // 0.001 ETH
        minimums.insert("USDT".to_string(), Decimal::new(10, 0)); // 10 USDT
        minimums.insert("SOL".to_string(), Decimal::new(1, 1));   // 0.1 SOL

        Self {
            withdrawals: Vec::new(),
            auto_approve_threshold: Decimal::new(1000, 0), // $1000
            fees,
            minimums,
            daily_limit_config: DailyLimitConfig::default(),
            user_overrides: HashMap::new(),
        }
    }

    /// Submit a withdrawal request.
    pub fn submit(
        &mut self,
        user_id: Uuid,
        asset: &str,
        network: &str,
        address: &str,
        amount: Decimal,
    ) -> Result<WithdrawalRecord, WithdrawalError> {
        // Check minimum
        if let Some(&min) = self.minimums.get(asset) {
            if amount < min {
                return Err(WithdrawalError::AmountTooSmall {
                    amount,
                    minimum: min,
                });
            }
        }

        let fee = self.fees.get(asset).copied().unwrap_or(Decimal::ZERO);

        let record = WithdrawalRecord {
            id: Uuid::new_v4(),
            user_id,
            asset: asset.to_string(),
            network: network.to_string(),
            address: address.to_string(),
            amount,
            fee,
            status: WithdrawalStatus::PendingConfirmation,
            provider_tx_id: None,
            created_at: Utc::now(),
            completed_at: None,
        };

        self.withdrawals.push(record.clone());
        tracing::info!(
            "Withdrawal submitted: {} {} to {} for user {}",
            amount, asset, address, user_id
        );

        Ok(record)
    }

    /// Confirm a withdrawal with 2FA.
    ///
    /// Transitions: PendingConfirmation → PendingApproval.
    /// The `totp_valid` parameter should be pre-validated by the caller
    /// (e.g., via `TotpManager::verify()`). This method only checks the
    /// status transition — it trusts the caller's authentication.
    pub fn confirm(
        &mut self,
        withdrawal_id: &Uuid,
        totp_valid: bool,
    ) -> Result<(), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        if record.status != WithdrawalStatus::PendingConfirmation {
            return Err(WithdrawalError::InvalidTransition {
                status: format!("{:?}", record.status),
                action: "confirm".to_string(),
            });
        }

        if !totp_valid {
            return Err(WithdrawalError::InvalidTransition {
                status: "PendingConfirmation".to_string(),
                action: "confirm (invalid 2FA code)".to_string(),
            });
        }

        record.status = WithdrawalStatus::PendingApproval;
        tracing::info!(
            withdrawal_id = %withdrawal_id,
            user_id = %record.user_id,
            "Withdrawal 2FA confirmed — moved to PendingApproval"
        );

        Ok(())
    }

    /// Prepare a withdrawal for approval: validate status and return info needed to call the provider.
    ///
    /// Returns `(withdrawal_id, asset, network, address, net_amount)`.
    /// Caller must submit to the custody provider and then call `complete_approve()`.
    pub fn prepare_approve(
        &self,
        withdrawal_id: &Uuid,
    ) -> Result<(Uuid, String, String, String, Decimal), Box<dyn std::error::Error + Send + Sync>> {
        let record = self
            .withdrawals
            .iter()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        if record.status != WithdrawalStatus::PendingApproval {
            return Err(Box::new(WithdrawalError::InvalidTransition {
                status: format!("{:?}", record.status),
                action: "approve".to_string(),
            }));
        }

        let net_amount = record.amount - record.fee;
        Ok((record.id, record.asset.clone(), record.network.clone(), record.address.clone(), net_amount))
    }

    /// Complete the approval after custody provider returns a transaction ID.
    pub fn complete_approve(
        &mut self,
        withdrawal_id: &Uuid,
        provider_tx_id: String,
    ) -> Result<(), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        record.provider_tx_id = Some(provider_tx_id);
        record.status = WithdrawalStatus::Processing;
        Ok(())
    }

    /// Approve a withdrawal and submit to custody provider (convenience method for tests/internal use).
    pub async fn approve(
        &mut self,
        withdrawal_id: &Uuid,
        provider: &dyn CustodyProvider,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (id, asset, network, address, net_amount) = self.prepare_approve(withdrawal_id)?;

        let tx_id = provider.submit_withdrawal(
            id, &asset, &network, &address, net_amount,
        ).await?;

        self.complete_approve(withdrawal_id, tx_id)?;
        Ok(())
    }

    /// Reject a withdrawal.
    pub fn reject(
        &mut self,
        withdrawal_id: &Uuid,
        reason: String,
    ) -> Result<(), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        if record.status != WithdrawalStatus::PendingApproval {
            return Err(WithdrawalError::InvalidTransition {
                status: format!("{:?}", record.status),
                action: "reject".to_string(),
            });
        }

        record.status = WithdrawalStatus::Rejected { reason };
        Ok(())
    }

    /// Poll custody provider for status updates on processing withdrawals.
    pub async fn poll_status(&mut self, provider: &dyn CustodyProvider) {
        for record in &mut self.withdrawals {
            if record.status != WithdrawalStatus::Processing {
                continue;
            }

            if let Some(ref tx_id) = record.provider_tx_id {
                match provider.check_withdrawal_status(tx_id).await {
                    Ok(ProviderTxStatus::Completed { tx_hash }) => {
                        record.status = WithdrawalStatus::Completed {
                            tx_hash: tx_hash.clone(),
                        };
                        record.completed_at = Some(Utc::now());
                        tracing::info!(
                            "Withdrawal completed: {} {} tx={}",
                            record.amount, record.asset, tx_hash
                        );
                    }
                    Ok(ProviderTxStatus::Failed { reason }) => {
                        record.status = WithdrawalStatus::Failed { reason };
                    }
                    Ok(ProviderTxStatus::Confirming { .. }) | Ok(ProviderTxStatus::Pending) => {
                        // Still processing
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check withdrawal status: {}", e);
                    }
                }
            }
        }
    }

    // --- Webhook-driven lifecycle transitions ---

    /// Transition a withdrawal to OnChain after custody broadcasts the transaction.
    ///
    /// Called by the custody webhook handler when `WithdrawalBroadcast` is received.
    pub fn on_broadcast(
        &mut self,
        withdrawal_id: &Uuid,
        tx_hash: String,
    ) -> Result<(), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        if record.status != WithdrawalStatus::Processing {
            return Err(WithdrawalError::InvalidTransition {
                status: format!("{:?}", record.status),
                action: "on_broadcast".to_string(),
            });
        }

        record.status = WithdrawalStatus::OnChain { tx_hash: tx_hash.clone() };
        tracing::info!(
            withdrawal_id = %withdrawal_id,
            tx_hash = %tx_hash,
            "Withdrawal broadcast on-chain"
        );
        Ok(())
    }

    /// Transition a withdrawal to Completed after custody confirms finality.
    ///
    /// Called by the custody webhook handler when `WithdrawalConfirmed` is received.
    /// Accepts from either Processing or OnChain states.
    pub fn on_confirmed(
        &mut self,
        withdrawal_id: &Uuid,
        tx_hash: String,
    ) -> Result<(), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        match &record.status {
            WithdrawalStatus::Processing | WithdrawalStatus::OnChain { .. } => {}
            other => {
                return Err(WithdrawalError::InvalidTransition {
                    status: format!("{:?}", other),
                    action: "on_confirmed".to_string(),
                });
            }
        }

        record.status = WithdrawalStatus::Completed { tx_hash: tx_hash.clone() };
        record.completed_at = Some(Utc::now());
        tracing::info!(
            withdrawal_id = %withdrawal_id,
            tx_hash = %tx_hash,
            "Withdrawal confirmed and completed"
        );
        Ok(())
    }

    /// Transition a withdrawal to Failed and return amount + fee for refund.
    ///
    /// Called by the custody webhook handler when `WithdrawalFailed` is received.
    /// Returns `(user_id, asset, amount, fee)` so the caller can refund the locked balance.
    pub fn on_failed(
        &mut self,
        withdrawal_id: &Uuid,
        reason: String,
    ) -> Result<(Uuid, String, Decimal, Decimal), WithdrawalError> {
        let record = self
            .withdrawals
            .iter_mut()
            .find(|w| w.id == *withdrawal_id)
            .ok_or(WithdrawalError::NotFound(*withdrawal_id))?;

        match &record.status {
            WithdrawalStatus::Processing | WithdrawalStatus::OnChain { .. } => {}
            other => {
                return Err(WithdrawalError::InvalidTransition {
                    status: format!("{:?}", other),
                    action: "on_failed".to_string(),
                });
            }
        }

        let refund_info = (
            record.user_id,
            record.asset.clone(),
            record.amount,
            record.fee,
        );

        record.status = WithdrawalStatus::Failed { reason: reason.clone() };
        tracing::warn!(
            withdrawal_id = %withdrawal_id,
            reason = %reason,
            "Withdrawal failed — refund required"
        );
        Ok(refund_info)
    }

    /// Look up a withdrawal by its ID.
    /// All withdrawal records (all statuses).
    pub fn all_withdrawals(&self) -> &[WithdrawalRecord] {
        &self.withdrawals
    }

    pub fn get(&self, withdrawal_id: &Uuid) -> Option<&WithdrawalRecord> {
        self.withdrawals.iter().find(|w| w.id == *withdrawal_id)
    }

    /// Get all withdrawals currently in Processing state with their provider tx IDs.
    ///
    /// Used by the polling task to check status without holding a lock across async calls.
    pub fn processing_withdrawals(&self) -> Vec<(Uuid, String)> {
        self.withdrawals
            .iter()
            .filter(|w| w.status == WithdrawalStatus::Processing)
            .filter_map(|w| w.provider_tx_id.as_ref().map(|tx| (w.id, tx.clone())))
            .collect()
    }

    /// Get pending approvals.
    pub fn pending_approvals(&self) -> Vec<&WithdrawalRecord> {
        self.withdrawals
            .iter()
            .filter(|w| w.status == WithdrawalStatus::PendingApproval)
            .collect()
    }

    /// Get withdrawals for a user.
    pub fn user_withdrawals(&self, user_id: &Uuid) -> Vec<&WithdrawalRecord> {
        self.withdrawals.iter().filter(|w| w.user_id == *user_id).collect()
    }

    /// Get withdrawal fee for an asset.
    pub fn fee(&self, asset: &str) -> Decimal {
        self.fees.get(asset).copied().unwrap_or(Decimal::ZERO)
    }

    /// Get minimum withdrawal for an asset.
    pub fn minimum(&self, asset: &str) -> Decimal {
        self.minimums.get(asset).copied().unwrap_or(Decimal::ZERO)
    }

    /// Sum of all non-rejected withdrawals submitted in the past 24 hours for a user.
    ///
    /// Phase 1: amount is treated as a USD proxy (no currency conversion).
    fn daily_total(&self, user_id: &Uuid) -> Decimal {
        let cutoff = Utc::now() - Duration::hours(24);
        self.withdrawals
            .iter()
            .filter(|w| {
                w.user_id == *user_id
                    && w.created_at > cutoff
                    && !matches!(w.status, WithdrawalStatus::Rejected { .. })
            })
            .map(|w| w.amount)
            .sum()
    }

    /// Check whether `amount` would push the user over their daily limit.
    ///
    /// Returns `Ok(())` if the withdrawal is within the limit, or
    /// `Err(WithdrawalLimitError)` with details about remaining capacity.
    pub fn check_daily_limit(
        &self,
        user_id: &Uuid,
        amount: Decimal,
        tier: UserTier,
    ) -> Result<(), WithdrawalLimitError> {
        let daily_limit = self
            .user_overrides
            .get(user_id)
            .copied()
            .unwrap_or_else(|| self.daily_limit_config.limit_for(tier));

        let daily_used = self.daily_total(user_id);
        let remaining_limit = (daily_limit - daily_used).max(Decimal::ZERO);

        if daily_used + amount > daily_limit {
            return Err(WithdrawalLimitError {
                daily_used,
                daily_limit,
                remaining_limit,
            });
        }

        Ok(())
    }

    /// Set a custom daily limit override for a specific user (admin operation).
    ///
    /// The override takes precedence over the tier-based limit until removed.
    pub fn set_override(&mut self, user_id: Uuid, custom_limit: Decimal) {
        self.user_overrides.insert(user_id, custom_limit);
    }
}

impl Default for WithdrawalManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockCustodyProvider;
    use rust_decimal_macros::dec;

    #[test]
    fn test_submit_withdrawal() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "BTC", "bitcoin", "bc1q123", dec!(1)).unwrap();

        assert_eq!(record.status, WithdrawalStatus::PendingConfirmation);
        assert_eq!(record.amount, dec!(1));
        assert_eq!(record.fee, dec!(0.00005));
        // Not yet in pending_approvals until 2FA confirmed.
        assert_eq!(mgr.pending_approvals().len(), 0);
    }

    #[test]
    fn test_below_minimum_rejected() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        let err = mgr.submit(user, "USDT", "erc20", "0xabc", dec!(5)).unwrap_err();
        assert!(matches!(err, WithdrawalError::AmountTooSmall { .. }));
    }

    #[tokio::test]
    async fn test_approve_and_complete() {
        let mut mgr = WithdrawalManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "ETH", "ethereum", "0xabc", dec!(1)).unwrap();
        let wid = record.id;

        // Confirm 2FA → moves to PendingApproval
        mgr.confirm(&wid, true).unwrap();

        // Approve → sends to provider
        mgr.approve(&wid, &provider).await.unwrap();

        let w = mgr.user_withdrawals(&user);
        assert_eq!(w[0].status, WithdrawalStatus::Processing);

        // Poll → mock provider auto-completes
        mgr.poll_status(&provider).await;

        let w = mgr.user_withdrawals(&user);
        assert!(matches!(w[0].status, WithdrawalStatus::Completed { .. }));
    }

    #[test]
    fn test_reject_withdrawal() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "BTC", "bitcoin", "bc1q123", dec!(1)).unwrap();
        // Confirm first, then reject
        mgr.confirm(&record.id, true).unwrap();
        mgr.reject(&record.id, "AML flag".to_string()).unwrap();

        let w = mgr.user_withdrawals(&user);
        assert!(matches!(w[0].status, WithdrawalStatus::Rejected { .. }));
        assert_eq!(mgr.pending_approvals().len(), 0);
    }

    #[tokio::test]
    async fn test_cannot_approve_rejected() {
        let mut mgr = WithdrawalManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "BTC", "bitcoin", "bc1q123", dec!(1)).unwrap();
        mgr.confirm(&record.id, true).unwrap();
        mgr.reject(&record.id, "Bad".to_string()).unwrap();

        let result = mgr.approve(&record.id, &provider).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_confirm_2fa_required_before_approval() {
        let mut mgr = WithdrawalManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "BTC", "bitcoin", "bc1q123", dec!(1)).unwrap();
        assert_eq!(record.status, WithdrawalStatus::PendingConfirmation);

        // Try to approve without confirming — should fail.
        let result = mgr.approve(&record.id, &provider).await;
        assert!(result.is_err(), "Cannot approve without 2FA confirmation");
    }

    #[test]
    fn test_confirm_with_invalid_totp_fails() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        let record = mgr.submit(user, "BTC", "bitcoin", "bc1q123", dec!(1)).unwrap();

        // Confirm with invalid TOTP
        let result = mgr.confirm(&record.id, false);
        assert!(result.is_err(), "Invalid 2FA must fail");

        // Status should still be PendingConfirmation.
        let w = mgr.user_withdrawals(&user);
        assert_eq!(w[0].status, WithdrawalStatus::PendingConfirmation);
    }

    #[tokio::test]
    async fn test_full_withdrawal_flow_with_2fa() {
        let mut mgr = WithdrawalManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        // 1. Submit
        let record = mgr.submit(user, "ETH", "ethereum", "0xdef", dec!(2)).unwrap();
        assert_eq!(record.status, WithdrawalStatus::PendingConfirmation);

        // 2. Confirm 2FA
        mgr.confirm(&record.id, true).unwrap();
        let w = mgr.user_withdrawals(&user);
        assert_eq!(w[0].status, WithdrawalStatus::PendingApproval);

        // 3. Approve
        mgr.approve(&record.id, &provider).await.unwrap();
        let w = mgr.user_withdrawals(&user);
        assert_eq!(w[0].status, WithdrawalStatus::Processing);

        // 4. Complete via poll
        mgr.poll_status(&provider).await;
        let w = mgr.user_withdrawals(&user);
        assert!(matches!(w[0].status, WithdrawalStatus::Completed { .. }));
    }

    #[test]
    fn test_fee_schedule() {
        let mgr = WithdrawalManager::new();
        assert_eq!(mgr.fee("BTC"), dec!(0.00005));
        assert_eq!(mgr.fee("USDT"), dec!(1));
        assert_eq!(mgr.fee("UNKNOWN"), dec!(0));
    }

    #[test]
    fn test_minimum_schedule() {
        let mgr = WithdrawalManager::new();
        assert_eq!(mgr.minimum("BTC"), dec!(0.0001));
        assert_eq!(mgr.minimum("USDT"), dec!(10));
    }

    // --- T12.2: Daily limit tests ---

    #[test]
    fn test_withdrawal_within_limit() {
        let mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();
        // Default tier limit is $1,000; $500 is well within that.
        assert!(mgr.check_daily_limit(&user, dec!(500), UserTier::Default).is_ok());
    }

    #[test]
    fn test_withdrawal_exceeds_daily_limit() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        // Submit $900 — still within the $1,000 default limit.
        mgr.submit(user, "USDT", "erc20", "0xabc", dec!(900)).unwrap();

        // Attempting another $200 should be rejected ($900 + $200 > $1,000).
        let err = mgr
            .check_daily_limit(&user, dec!(200), UserTier::Default)
            .unwrap_err();
        assert_eq!(err.daily_used, dec!(900));
        assert_eq!(err.daily_limit, dec!(1000));
        assert_eq!(err.remaining_limit, dec!(100));
    }

    #[test]
    fn test_kyc_verified_higher_limit() {
        let mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();
        // KYC limit is $10,000; $5,000 is fine.
        assert!(mgr.check_daily_limit(&user, dec!(5000), UserTier::KycVerified).is_ok());
        // But $10,001 is not.
        assert!(mgr.check_daily_limit(&user, dec!(10001), UserTier::KycVerified).is_err());
    }

    #[test]
    fn test_vip_higher_limit() {
        let mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();
        // VIP limit is $100,000.
        assert!(mgr.check_daily_limit(&user, dec!(99999), UserTier::Vip).is_ok());
        assert!(mgr.check_daily_limit(&user, dec!(100001), UserTier::Vip).is_err());
    }

    #[test]
    fn test_admin_override_limit() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();
        // Default limit is $1,000; $2,000 would fail.
        assert!(mgr.check_daily_limit(&user, dec!(2000), UserTier::Default).is_err());
        // After override, $2,000 should pass.
        mgr.set_override(user, dec!(5000));
        assert!(mgr.check_daily_limit(&user, dec!(2000), UserTier::Default).is_ok());
    }

    #[test]
    fn test_rejected_withdrawal_not_counted_in_daily_total() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();

        // Submit, confirm 2FA, then reject a $900 withdrawal.
        let record = mgr.submit(user, "USDT", "erc20", "0xabc", dec!(900)).unwrap();
        mgr.confirm(&record.id, true).unwrap();
        mgr.reject(&record.id, "AML flag".to_string()).unwrap();

        // The rejected amount should not count toward the daily total.
        assert_eq!(mgr.daily_total(&user), dec!(0));
        assert!(mgr.check_daily_limit(&user, dec!(1000), UserTier::Default).is_ok());
    }

    // --- Webhook lifecycle tests ---

    /// Helper: create a withdrawal in Processing state.
    async fn processing_withdrawal(mgr: &mut WithdrawalManager) -> (Uuid, Uuid) {
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();
        let record = mgr.submit(user, "ETH", "ethereum", "0xabc", dec!(1)).unwrap();
        let wid = record.id;
        mgr.confirm(&wid, true).unwrap();
        mgr.approve(&wid, &provider).await.unwrap();
        (user, wid)
    }

    #[tokio::test]
    async fn test_on_broadcast() {
        let mut mgr = WithdrawalManager::new();
        let (_user, wid) = processing_withdrawal(&mut mgr).await;

        mgr.on_broadcast(&wid, "0xabc123".to_string()).unwrap();
        let w = mgr.get(&wid).unwrap();
        assert!(matches!(&w.status, WithdrawalStatus::OnChain { tx_hash } if tx_hash == "0xabc123"));
    }

    #[tokio::test]
    async fn test_on_confirmed_from_processing() {
        let mut mgr = WithdrawalManager::new();
        let (_user, wid) = processing_withdrawal(&mut mgr).await;

        mgr.on_confirmed(&wid, "0xfinal".to_string()).unwrap();
        let w = mgr.get(&wid).unwrap();
        assert!(matches!(&w.status, WithdrawalStatus::Completed { tx_hash } if tx_hash == "0xfinal"));
        assert!(w.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_on_confirmed_from_on_chain() {
        let mut mgr = WithdrawalManager::new();
        let (_user, wid) = processing_withdrawal(&mut mgr).await;

        mgr.on_broadcast(&wid, "0xabc".to_string()).unwrap();
        mgr.on_confirmed(&wid, "0xabc".to_string()).unwrap();
        let w = mgr.get(&wid).unwrap();
        assert!(matches!(&w.status, WithdrawalStatus::Completed { .. }));
    }

    #[tokio::test]
    async fn test_on_failed_returns_refund_info() {
        let mut mgr = WithdrawalManager::new();
        let (user, wid) = processing_withdrawal(&mut mgr).await;

        let (uid, asset, amount, fee) = mgr.on_failed(&wid, "insufficient gas".to_string()).unwrap();
        assert_eq!(uid, user);
        assert_eq!(asset, "ETH");
        assert_eq!(amount, dec!(1));
        assert!(fee > Decimal::ZERO);

        let w = mgr.get(&wid).unwrap();
        assert!(matches!(&w.status, WithdrawalStatus::Failed { reason } if reason == "insufficient gas"));
    }

    #[test]
    fn test_on_broadcast_wrong_status() {
        let mut mgr = WithdrawalManager::new();
        let user = Uuid::new_v4();
        let record = mgr.submit(user, "ETH", "ethereum", "0xabc", dec!(1)).unwrap();

        // Still PendingConfirmation — on_broadcast should fail
        let result = mgr.on_broadcast(&record.id, "0x123".to_string());
        assert!(result.is_err());
    }
}
