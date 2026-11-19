//! Fiat on/off ramp framework.
//!
//! Defines the [`PaymentProvider`] trait and stub implementations for the three
//! initial providers: bank transfer, credit/debit card, and P2P.
//!
//! The public REST surface is thin — handlers live in `rest.rs`. This module
//! owns the types and logic so they can be tested independently.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Supported fiat currencies ────────────────────────────────────────────────

/// A supported fiat currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FiatCurrency {
    USD,
    EUR,
    UAH,
    GBP,
}

impl FiatCurrency {
    /// Parse from a string slice (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "USD" => Some(Self::USD),
            "EUR" => Some(Self::EUR),
            "UAH" => Some(Self::UAH),
            "GBP" => Some(Self::GBP),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::USD => "USD",
            Self::EUR => "EUR",
            Self::UAH => "UAH",
            Self::GBP => "GBP",
        }
    }
}

impl std::fmt::Display for FiatCurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── KYC tier deposit limits ──────────────────────────────────────────────────

/// Daily deposit/withdrawal limits (in USD equivalent) per KYC verification tier.
#[derive(Debug, Clone, Copy)]
pub struct FiatTierLimit {
    /// Maximum USD equivalent per day. `None` means no limit (tier 3).
    pub daily_usd: Option<Decimal>,
}

impl FiatTierLimit {
    /// Tier 1 (basic KYC): $1,000 / day.
    pub fn tier1() -> Self {
        Self { daily_usd: Some(Decimal::from(1_000)) }
    }

    /// Tier 2 (full KYC): $50,000 / day.
    pub fn tier2() -> Self {
        Self { daily_usd: Some(Decimal::from(50_000)) }
    }

    /// Tier 3 (enhanced KYC + institutional): unlimited.
    pub fn tier3() -> Self {
        Self { daily_usd: None }
    }

    /// Look up the limit for a numeric tier value (0–3).
    pub fn for_tier(tier: u8) -> Self {
        match tier {
            1 => Self::tier1(),
            2 => Self::tier2(),
            _ if tier >= 3 => Self::tier3(),
            _ => Self { daily_usd: Some(Decimal::ZERO) }, // tier 0 = unverified, no fiat
        }
    }
}

// ── Payment provider trait ───────────────────────────────────────────────────

/// The outcome of creating a payment intent.
#[derive(Debug, Clone, Serialize)]
pub struct PaymentIntent {
    /// Internal reference — also the primary key in [`PaymentRecord`].
    pub payment_id: Uuid,
    /// Provider-specific reference (e.g. bank wire reference, card session ID).
    pub provider_reference: String,
    /// URL the user must visit to complete the payment (card, P2P).
    pub redirect_url: Option<String>,
    pub status: PaymentStatus,
}

/// Status of a fiat payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Refunded,
    Expired,
}

impl PaymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Refunded => "refunded",
            Self::Expired => "expired",
        }
    }
}

/// A payout request created for fiat withdrawals.
#[derive(Debug, Clone, Serialize)]
pub struct PayoutRequest {
    pub payout_id: Uuid,
    pub provider_reference: String,
    pub status: PaymentStatus,
    /// Estimated processing time as a human-readable string.
    pub estimated_completion: String,
}

/// A persistent record of a completed or pending fiat payment/payout.
#[derive(Debug, Clone, Serialize)]
pub struct PaymentRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    /// "deposit" or "withdrawal"
    pub direction: String,
    pub provider: String,
    pub currency: String,
    pub amount: Decimal,
    /// Fee charged in the same currency.
    pub fee: Decimal,
    pub status: PaymentStatus,
    pub provider_reference: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub details: String,
}

/// A request to create a fiat deposit.
#[derive(Debug, Clone)]
pub struct DepositRequest {
    pub user_id: Uuid,
    pub currency: FiatCurrency,
    pub amount: Decimal,
}

/// A request to create a fiat withdrawal / payout.
#[derive(Debug, Clone)]
pub struct WithdrawalRequest {
    pub user_id: Uuid,
    pub currency: FiatCurrency,
    pub amount: Decimal,
    /// Bank account IBAN / card number / P2P identifier.
    pub destination: String,
}

/// Trait implemented by each payment provider (bank transfer, card, P2P).
///
/// All methods return `anyhow::Result` so errors carry context through the
/// standard `?` operator chain without provider-specific error types leaking
/// into callers.
pub trait PaymentProvider: Send + Sync {
    /// Human-readable provider name for logging and payment records.
    fn name(&self) -> &str;

    /// Create a deposit intent. The returned [`PaymentIntent`] contains
    /// a provider reference that the user or webhook can be tracked against.
    fn create_deposit(&self, req: &DepositRequest) -> anyhow::Result<PaymentIntent>;

    /// Query the current status of a payment by its provider reference.
    fn check_status(&self, provider_reference: &str) -> anyhow::Result<PaymentStatus>;

    /// Create a withdrawal / payout request.
    fn create_withdrawal(&self, req: &WithdrawalRequest) -> anyhow::Result<PayoutRequest>;
}

// ── Stub implementations ─────────────────────────────────────────────────────

/// Bank transfer provider (SEPA / SWIFT).
///
/// In production this would call the bank's API. Here it stubs out the calls
/// with deterministic in-memory behaviour so the rest of the system compiles
/// and tests run without real credentials.
pub struct BankTransferProvider;

impl PaymentProvider for BankTransferProvider {
    fn name(&self) -> &str {
        "bank_transfer"
    }

    fn create_deposit(&self, req: &DepositRequest) -> anyhow::Result<PaymentIntent> {
        let payment_id = Uuid::new_v4();
        let reference = format!("EXCH-{}", &payment_id.to_string()[..8].to_uppercase());
        tracing::info!(
            user_id = %req.user_id,
            currency = %req.currency,
            amount = %req.amount,
            reference = %reference,
            "BankTransfer: created deposit intent"
        );
        Ok(PaymentIntent {
            payment_id,
            provider_reference: reference,
            redirect_url: None,
            status: PaymentStatus::Pending,
        })
    }

    fn check_status(&self, provider_reference: &str) -> anyhow::Result<PaymentStatus> {
        // Stub: references starting with "EXCH-" are always pending.
        tracing::debug!(reference = %provider_reference, "BankTransfer: status check (stub)");
        Ok(PaymentStatus::Pending)
    }

    fn create_withdrawal(&self, req: &WithdrawalRequest) -> anyhow::Result<PayoutRequest> {
        let payout_id = Uuid::new_v4();
        let reference = format!("PAYOUT-{}", &payout_id.to_string()[..8].to_uppercase());
        tracing::info!(
            user_id = %req.user_id,
            currency = %req.currency,
            amount = %req.amount,
            destination = %req.destination,
            reference = %reference,
            "BankTransfer: created payout request"
        );
        Ok(PayoutRequest {
            payout_id,
            provider_reference: reference,
            status: PaymentStatus::Processing,
            estimated_completion: "1-3 business days".into(),
        })
    }
}

/// Credit / debit card provider.
///
/// In production this would call a PSP (e.g. Stripe, Adyen). The stub always
/// creates a synthetic redirect URL.
pub struct CreditCardProvider;

impl PaymentProvider for CreditCardProvider {
    fn name(&self) -> &str {
        "credit_card"
    }

    fn create_deposit(&self, req: &DepositRequest) -> anyhow::Result<PaymentIntent> {
        let payment_id = Uuid::new_v4();
        let reference = format!("CARD-{payment_id}");
        let redirect_url = format!("https://pay.example.com/card/{payment_id}");
        tracing::info!(
            user_id = %req.user_id,
            currency = %req.currency,
            amount = %req.amount,
            "CreditCard: created deposit intent"
        );
        Ok(PaymentIntent {
            payment_id,
            provider_reference: reference,
            redirect_url: Some(redirect_url),
            status: PaymentStatus::Pending,
        })
    }

    fn check_status(&self, provider_reference: &str) -> anyhow::Result<PaymentStatus> {
        tracing::debug!(reference = %provider_reference, "CreditCard: status check (stub)");
        Ok(PaymentStatus::Pending)
    }

    fn create_withdrawal(&self, req: &WithdrawalRequest) -> anyhow::Result<PayoutRequest> {
        // Card refunds are not always supported — return a descriptive error.
        tracing::warn!(
            user_id = %req.user_id,
            "CreditCard: withdrawal not supported for card payments"
        );
        anyhow::bail!("Credit card withdrawals are not supported; use bank transfer instead")
    }
}

/// Peer-to-peer payment provider (e.g. domestic instant payments, Monobank, etc.).
pub struct P2PProvider;

impl PaymentProvider for P2PProvider {
    fn name(&self) -> &str {
        "p2p"
    }

    fn create_deposit(&self, req: &DepositRequest) -> anyhow::Result<PaymentIntent> {
        let payment_id = Uuid::new_v4();
        let reference = format!("P2P-{}", &payment_id.to_string()[..8].to_uppercase());
        let redirect_url = format!("https://p2p.example.com/pay/{payment_id}");
        tracing::info!(
            user_id = %req.user_id,
            currency = %req.currency,
            amount = %req.amount,
            "P2P: created deposit intent"
        );
        Ok(PaymentIntent {
            payment_id,
            provider_reference: reference,
            redirect_url: Some(redirect_url),
            status: PaymentStatus::Pending,
        })
    }

    fn check_status(&self, provider_reference: &str) -> anyhow::Result<PaymentStatus> {
        tracing::debug!(reference = %provider_reference, "P2P: status check (stub)");
        Ok(PaymentStatus::Pending)
    }

    fn create_withdrawal(&self, req: &WithdrawalRequest) -> anyhow::Result<PayoutRequest> {
        let payout_id = Uuid::new_v4();
        let reference = format!("P2P-OUT-{}", &payout_id.to_string()[..8].to_uppercase());
        tracing::info!(
            user_id = %req.user_id,
            currency = %req.currency,
            amount = %req.amount,
            "P2P: created payout request"
        );
        Ok(PayoutRequest {
            payout_id,
            provider_reference: reference,
            status: PaymentStatus::Processing,
            estimated_completion: "Instant to 30 minutes".into(),
        })
    }
}

// ── Exchange rate stubs ──────────────────────────────────────────────────────

/// Hardcoded fiat → USDT exchange rates.
///
/// In production these would be fetched from a live FX feed. The stubs are
/// accurate to within a few percent at authoring time and sufficient for
/// integration testing.
pub fn stub_exchange_rates() -> Vec<ExchangeRateInfo> {
    vec![
        ExchangeRateInfo {
            from: "USD".into(),
            to: "USDT".into(),
            rate: Decimal::new(1, 0),        // 1 USD = 1 USDT
            fee_pct: Decimal::new(0, 0),
        },
        ExchangeRateInfo {
            from: "EUR".into(),
            to: "USDT".into(),
            rate: Decimal::new(108, 2),      // 1 EUR = 1.08 USDT
            fee_pct: Decimal::new(1, 2),     // 0.01% conversion fee
        },
        ExchangeRateInfo {
            from: "UAH".into(),
            to: "USDT".into(),
            rate: Decimal::new(2632, 6),     // 1 UAH ≈ 0.002632 USDT (~38 UAH/USD)
            fee_pct: Decimal::new(15, 3),    // 0.015%
        },
        ExchangeRateInfo {
            from: "GBP".into(),
            to: "USDT".into(),
            rate: Decimal::new(127, 2),      // 1 GBP = 1.27 USDT
            fee_pct: Decimal::new(1, 2),
        },
    ]
}

/// A single fiat ↔ crypto exchange rate.
#[derive(Debug, Clone, Serialize)]
pub struct ExchangeRateInfo {
    pub from: String,
    pub to: String,
    pub rate: Decimal,
    /// Fee percentage expressed as a fraction (e.g. `0.01` = 1%).
    pub fee_pct: Decimal,
}

// ── FiatGateway: manages provider registry and payment records ───────────────

/// Manages the set of registered payment providers and persists payment records.
pub struct FiatGateway {
    providers: HashMap<String, Box<dyn PaymentProvider>>,
    records: Vec<PaymentRecord>,
}

impl FiatGateway {
    /// Create with default stub providers.
    pub fn with_defaults() -> Self {
        let mut providers: HashMap<String, Box<dyn PaymentProvider>> = HashMap::new();
        providers.insert("bank_transfer".into(), Box::new(BankTransferProvider));
        providers.insert("credit_card".into(), Box::new(CreditCardProvider));
        providers.insert("p2p".into(), Box::new(P2PProvider));
        Self { providers, records: Vec::new() }
    }

    /// Return the provider for `name`, or `None` if not registered.
    pub fn provider(&self, name: &str) -> Option<&dyn PaymentProvider> {
        self.providers.get(name).map(|b| b.as_ref())
    }

    /// List all registered provider names.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    /// Store a payment record (called after a successful provider call).
    pub fn record_payment(&mut self, record: PaymentRecord) {
        self.records.push(record);
    }

    /// Return payment history for a user.
    pub fn history_for_user(&self, user_id: Uuid) -> Vec<&PaymentRecord> {
        self.records.iter().filter(|r| r.user_id == user_id).collect()
    }

    /// Return all records (for admin / reconciliation).
    pub fn all_records(&self) -> &[PaymentRecord] {
        &self.records
    }

    /// Update the status of an existing record by payment id.
    pub fn update_status(&mut self, payment_id: Uuid, status: PaymentStatus) {
        if let Some(record) = self.records.iter_mut().find(|r| r.id == payment_id) {
            record.status = status;
            record.updated_at = Utc::now();
        }
    }
}

impl std::fmt::Debug for FiatGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FiatGateway")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("record_count", &self.records.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FiatCurrency ─────────────────────────────────────────────────────────

    #[test]
    fn fiat_currency_parse_round_trip() {
        for (s, expected) in [
            ("USD", FiatCurrency::USD),
            ("EUR", FiatCurrency::EUR),
            ("UAH", FiatCurrency::UAH),
            ("GBP", FiatCurrency::GBP),
        ] {
            let parsed = FiatCurrency::from_str(s).expect("should parse");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
        }
    }

    #[test]
    fn fiat_currency_unknown_returns_none() {
        assert!(FiatCurrency::from_str("XYZ").is_none());
    }

    // ── FiatTierLimit ────────────────────────────────────────────────────────

    #[test]
    fn tier_limits_match_spec() {
        assert_eq!(FiatTierLimit::tier1().daily_usd, Some(Decimal::from(1_000)));
        assert_eq!(FiatTierLimit::tier2().daily_usd, Some(Decimal::from(50_000)));
        assert!(FiatTierLimit::tier3().daily_usd.is_none());
    }

    #[test]
    fn tier0_has_zero_limit() {
        let limit = FiatTierLimit::for_tier(0);
        assert_eq!(limit.daily_usd, Some(Decimal::ZERO));
    }

    // ── BankTransferProvider ─────────────────────────────────────────────────

    #[test]
    fn bank_transfer_creates_deposit_intent() {
        let provider = BankTransferProvider;
        let req = DepositRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::USD,
            amount: Decimal::from(500),
        };
        let intent = provider.create_deposit(&req).expect("should succeed");
        assert_eq!(intent.status, PaymentStatus::Pending);
        assert!(intent.redirect_url.is_none(), "bank transfers have no redirect");
        assert!(intent.provider_reference.starts_with("EXCH-"));
    }

    #[test]
    fn bank_transfer_creates_payout() {
        let provider = BankTransferProvider;
        let req = WithdrawalRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::EUR,
            amount: Decimal::from(200),
            destination: "DE89370400440532013000".into(),
        };
        let payout = provider.create_withdrawal(&req).expect("should succeed");
        assert_eq!(payout.status, PaymentStatus::Processing);
        assert!(payout.provider_reference.starts_with("PAYOUT-"));
    }

    // ── CreditCardProvider ───────────────────────────────────────────────────

    #[test]
    fn credit_card_provides_redirect_url() {
        let provider = CreditCardProvider;
        let req = DepositRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::USD,
            amount: Decimal::from(100),
        };
        let intent = provider.create_deposit(&req).expect("should succeed");
        assert!(intent.redirect_url.is_some(), "card payments need a redirect URL");
    }

    #[test]
    fn credit_card_withdrawal_rejected() {
        let provider = CreditCardProvider;
        let req = WithdrawalRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::USD,
            amount: Decimal::from(100),
            destination: "4111111111111111".into(),
        };
        assert!(provider.create_withdrawal(&req).is_err());
    }

    // ── P2PProvider ──────────────────────────────────────────────────────────

    #[test]
    fn p2p_deposit_and_withdrawal_succeed() {
        let provider = P2PProvider;

        let dep_req = DepositRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::UAH,
            amount: Decimal::from(1000),
        };
        let intent = provider.create_deposit(&dep_req).expect("deposit should succeed");
        assert!(intent.redirect_url.is_some());

        let wit_req = WithdrawalRequest {
            user_id: Uuid::new_v4(),
            currency: FiatCurrency::UAH,
            amount: Decimal::from(500),
            destination: "+380501234567".into(),
        };
        let payout = provider.create_withdrawal(&wit_req).expect("withdrawal should succeed");
        assert_eq!(payout.status, PaymentStatus::Processing);
    }

    // ── Exchange rates ───────────────────────────────────────────────────────

    #[test]
    fn exchange_rates_cover_all_currencies() {
        let rates = stub_exchange_rates();
        let froms: Vec<&str> = rates.iter().map(|r| r.from.as_str()).collect();
        for currency in ["USD", "EUR", "UAH", "GBP"] {
            assert!(froms.contains(&currency), "missing rate for {currency}");
        }
    }

    #[test]
    fn exchange_rates_use_decimal_not_float() {
        // All rates must be expressible as Decimal (no f64 in the type).
        // This test just ensures the rate field is non-zero for sanity.
        let rates = stub_exchange_rates();
        for r in &rates {
            assert!(r.rate > Decimal::ZERO, "rate for {} must be positive", r.from);
        }
    }

    // ── FiatGateway ──────────────────────────────────────────────────────────

    #[test]
    fn gateway_has_all_default_providers() {
        let gw = FiatGateway::with_defaults();
        for name in ["bank_transfer", "credit_card", "p2p"] {
            assert!(
                gw.provider(name).is_some(),
                "expected provider '{name}' to be registered"
            );
        }
    }

    #[test]
    fn gateway_history_scoped_to_user() {
        let mut gw = FiatGateway::with_defaults();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        for user_id in [user_a, user_b] {
            gw.record_payment(PaymentRecord {
                id: Uuid::new_v4(),
                user_id,
                direction: "deposit".into(),
                provider: "bank_transfer".into(),
                currency: "USD".into(),
                amount: Decimal::from(100),
                fee: Decimal::ZERO,
                status: PaymentStatus::Pending,
                provider_reference: format!("EXCH-{}", user_id),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                details: String::new(),
            });
        }

        let history_a = gw.history_for_user(user_a);
        assert_eq!(history_a.len(), 1);
        assert_eq!(history_a[0].user_id, user_a);
    }

    #[test]
    fn gateway_update_status_changes_record() {
        let mut gw = FiatGateway::with_defaults();
        let payment_id = Uuid::new_v4();
        gw.record_payment(PaymentRecord {
            id: payment_id,
            user_id: Uuid::new_v4(),
            direction: "deposit".into(),
            provider: "bank_transfer".into(),
            currency: "EUR".into(),
            amount: Decimal::from(250),
            fee: Decimal::ZERO,
            status: PaymentStatus::Pending,
            provider_reference: "EXCH-ABCDEF12".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            details: String::new(),
        });

        gw.update_status(payment_id, PaymentStatus::Completed);

        let record = gw.all_records().iter().find(|r| r.id == payment_id).unwrap();
        assert_eq!(record.status, PaymentStatus::Completed);
    }
}
