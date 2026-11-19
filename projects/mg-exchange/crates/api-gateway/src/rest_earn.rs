//! Advanced financial products REST handlers.
//!
//! Three product areas:
//!
//! 1. **Dual Investment** (`/earn/dual-investment/*`) — structured product where users
//!    deposit a currency and either receive the underlying asset at strike price or get
//!    their principal back with a premium yield, depending on the market outcome at expiry.
//!
//! 2. **ETH/SOL Staking** (`/earn/staking/*`) — native staking with unbonding periods,
//!    hourly reward accrual (~4 % APY for ETH, ~7 % for SOL), and APY history.
//!
//! 3. **Crypto Loans** (`/loan/*`) — collateral-backed loans with auto-liquidation
//!    when LTV exceeds 85 %.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::exchange::{
    CryptoLoan, DualInvestPosition, DualInvestProduct, DualInvestStatus, LoanStatus,
    StakingPosition, UnbondingEntry, STAKING_APY, STAKING_UNBONDING_DAYS,
};
use crate::rest::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn ok(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(body))
}

fn created(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::CREATED, Json(body))
}

// =============================================================================
// Dual Investment
// =============================================================================

/// Request body for POST /earn/dual-investment/subscribe.
#[derive(Debug, Deserialize)]
pub struct DualInvestSubscribeRequest {
    /// Underlying asset symbol, e.g. "BTC" or "ETH".
    pub underlying: String,
    /// "buy_low" or "sell_high".
    pub direction: String,
    pub strike_price: Decimal,
    /// Amount to deposit.
    /// - `buy_low`  : USDT amount.
    /// - `sell_high`: underlying coin amount.
    pub amount: Decimal,
    /// Product tenor in days (1, 3, 7, 14, or 30).
    pub expiry_days: u32,
}

/// POST /api/v1/earn/dual-investment/subscribe
///
/// Subscribe to a dual investment product. The exchange finds or creates a
/// matching product template and locks the user's deposit.
///
/// Deposit currency:
/// - `buy_low`  → USDT (deducted from trading account)
/// - `sell_high` → underlying coin (deducted from trading account)
pub async fn dual_invest_subscribe(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<DualInvestSubscribeRequest>,
) -> impl IntoResponse {
    use crate::exchange::DualInvestDirection;

    let user_id = auth.0;

    // Validate direction.
    let direction = match req.direction.as_str() {
        "buy_low" => DualInvestDirection::BuyLow,
        "sell_high" => DualInvestDirection::SellHigh,
        _ => return bad_request("direction must be 'buy_low' or 'sell_high'"),
    };

    if req.underlying.trim().is_empty() {
        return bad_request("underlying is required");
    }
    if req.strike_price <= Decimal::ZERO {
        return bad_request("strike_price must be positive");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if !matches!(req.expiry_days, 1 | 3 | 7 | 14 | 30) {
        return bad_request("expiry_days must be one of: 1, 3, 7, 14, 30");
    }

    // Determine deposit currency and APY.
    let deposit_coin = match direction {
        DualInvestDirection::BuyLow => "USDT".to_string(),
        DualInvestDirection::SellHigh => req.underlying.clone(),
    };

    // APY scales with tenor: longer tenors earn more.
    let base_apy = Decimal::new(12, 2); // 12 % annualised base
    let tenor_multiplier = Decimal::from(req.expiry_days) / Decimal::from(30u32);
    let apy = (base_apy * (Decimal::ONE + tenor_multiplier))
        .round_dp(4);

    // Lock the user's deposit upfront.
    if let Err(e) = exchange.accounts.withdraw(user_id, &deposit_coin, req.amount) {
        return bad_request(format!("insufficient balance: {e}"));
    }

    let now = Utc::now();
    let expiry = now + chrono::Duration::days(req.expiry_days as i64);

    // Find or create the matching product template.
    let product_id = {
        let mut products = exchange
            .dual_invest_products
            .lock().unwrap_or_else(|e| e.into_inner());

        // Reuse an existing active product that matches the parameters.
        let existing = products.iter().find(|p| {
            p.underlying == req.underlying
                && p.direction == direction
                && p.strike_price == req.strike_price
                && p.status == DualInvestStatus::Active
        });

        if let Some(p) = existing {
            p.id
        } else {
            let product = DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: req.underlying.clone(),
                direction,
                strike_price: req.strike_price,
                expiry,
                apy,
                status: DualInvestStatus::Active,
                created_at: now,
            };
            let id = product.id;
            products.push(product);
            id
        }
    };

    let position = DualInvestPosition {
        id: Uuid::new_v4(),
        user_id,
        product_id,
        amount: req.amount,
        settled: false,
        payout: None,
        payout_coin: None,
        subscribed_at: now,
    };
    let position_id = position.id;

    exchange
        .dual_invest_positions
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(position);

    tracing::info!(
        user_id = %user_id,
        position_id = %position_id,
        product_id = %product_id,
        underlying = %req.underlying,
        direction = %req.direction,
        strike_price = %req.strike_price,
        amount = %req.amount,
        deposit_coin = %deposit_coin,
        expiry_days = req.expiry_days,
        "dual_invest: position subscribed",
    );

    created(serde_json::json!({
        "position_id": position_id,
        "product_id": product_id,
        "underlying": req.underlying,
        "direction": req.direction,
        "strike_price": req.strike_price,
        "amount": req.amount,
        "deposit_coin": deposit_coin,
        "expiry": expiry,
        "apy": apy,
    }))
}

/// GET /api/v1/earn/dual-investment/products
///
/// Returns all active dual investment product templates with their current APY.
pub async fn dual_invest_products(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let products = exchange
        .dual_invest_products
        .lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|p| p.status == DualInvestStatus::Active)
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "underlying": p.underlying,
                "direction": p.direction,
                "strike_price": p.strike_price,
                "expiry": p.expiry,
                "apy": p.apy,
                "status": p.status,
            })
        })
        .collect::<Vec<_>>();

    ok(serde_json::json!({ "products": products }))
}

/// GET /api/v1/earn/dual-investment/positions
///
/// Returns the authenticated user's active and settled dual investment positions.
pub async fn dual_invest_positions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let positions = exchange
        .dual_invest_positions
        .lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|p| p.user_id == user_id)
        .map(|p| serde_json::to_value(p).unwrap_or(serde_json::Value::Null))
        .collect::<Vec<_>>();

    ok(serde_json::json!({ "positions": positions }))
}

// =============================================================================
// ETH/SOL Staking
// =============================================================================

/// Request body for POST /earn/staking/stake.
#[derive(Debug, Deserialize)]
pub struct StakeRequest {
    /// Coin to stake — "ETH" or "SOL".
    pub coin: String,
    pub amount: Decimal,
}

/// Request body for POST /earn/staking/unstake.
#[derive(Debug, Deserialize)]
pub struct UnstakeRequest {
    pub coin: String,
    pub amount: Decimal,
}

/// POST /api/v1/earn/staking/stake
///
/// Stake an amount of ETH or SOL. The coins are deducted from the user's
/// trading balance immediately; rewards accrue at the configured hourly rate.
pub async fn staking_stake(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<StakeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if !matches!(req.coin.as_str(), "ETH" | "SOL") {
        return bad_request("coin must be 'ETH' or 'SOL'");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    // Deduct from trading balance.
    if let Err(e) = exchange.accounts.withdraw(user_id, &req.coin, req.amount) {
        return bad_request(format!("insufficient balance: {e}"));
    }

    let now = Utc::now();
    let key = (user_id, req.coin.clone());

    let mut positions = exchange
        .staking_positions
        .lock().unwrap_or_else(|e| e.into_inner());

    let pos = positions.entry(key).or_insert_with(|| StakingPosition {
        user_id,
        coin: req.coin.clone(),
        staked_amount: Decimal::ZERO,
        pending_rewards: Decimal::ZERO,
        staked_at: now,
        last_accrued_at: now,
    });
    pos.staked_amount += req.amount;

    let staked_total = pos.staked_amount;
    drop(positions);

    tracing::info!(
        user_id = %user_id,
        coin = %req.coin,
        amount = %req.amount,
        total_staked = %staked_total,
        "staking: coins staked",
    );

    created(serde_json::json!({
        "coin": req.coin,
        "staked_amount": req.amount,
        "total_staked": staked_total,
    }))
}

/// POST /api/v1/earn/staking/unstake
///
/// Begin the unbonding process for staked coins. The amount is deducted from
/// the staked balance immediately and becomes available after the unbonding period
/// (ETH: 3 days, SOL: 2 days).
pub async fn staking_unstake(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<UnstakeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if !matches!(req.coin.as_str(), "ETH" | "SOL") {
        return bad_request("coin must be 'ETH' or 'SOL'");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let unbonding_days = STAKING_UNBONDING_DAYS
        .iter()
        .find(|(c, _)| *c == req.coin.as_str())
        .map(|(_, d)| *d)
        .unwrap_or(3);

    let now = Utc::now();
    let available_at = now + chrono::Duration::days(unbonding_days);

    {
        let mut positions = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner());

        let key = (user_id, req.coin.clone());
        let pos = match positions.get_mut(&key) {
            Some(p) => p,
            None => return not_found("no staking position found for this coin"),
        };

        if pos.staked_amount < req.amount {
            return bad_request(format!(
                "insufficient staked balance: have {}, requested {}",
                pos.staked_amount, req.amount
            ));
        }
        pos.staked_amount -= req.amount;
    }

    let entry = UnbondingEntry {
        id: Uuid::new_v4(),
        user_id,
        coin: req.coin.clone(),
        amount: req.amount,
        available_at,
        requested_at: now,
    };
    let entry_id = entry.id;

    exchange
        .staking_unbonding
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(entry);

    tracing::info!(
        user_id = %user_id,
        coin = %req.coin,
        amount = %req.amount,
        available_at = %available_at,
        unbonding_id = %entry_id,
        "staking: unstake initiated",
    );

    ok(serde_json::json!({
        "unbonding_id": entry_id,
        "coin": req.coin,
        "amount": req.amount,
        "available_at": available_at,
        "unbonding_days": unbonding_days,
    }))
}

/// GET /api/v1/earn/staking/balance
///
/// Returns the authenticated user's staked balances, pending rewards, and
/// any in-progress unbonding entries.
pub async fn staking_balance(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let positions: Vec<serde_json::Value> = {
        let pos_lock = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner());
        pos_lock
            .iter()
            .filter(|((uid, _), _)| *uid == user_id)
            .map(|((_, coin), pos)| {
                serde_json::json!({
                    "coin": coin,
                    "staked_amount": pos.staked_amount,
                    "pending_rewards": pos.pending_rewards,
                    "staked_at": pos.staked_at,
                    "last_accrued_at": pos.last_accrued_at,
                })
            })
            .collect()
    };

    let unbonding: Vec<serde_json::Value> = {
        let ub_lock = exchange
            .staking_unbonding
            .lock().unwrap_or_else(|e| e.into_inner());
        ub_lock
            .iter()
            .filter(|e| e.user_id == user_id)
            .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
            .collect()
    };

    ok(serde_json::json!({
        "positions": positions,
        "unbonding": unbonding,
    }))
}

/// Request body for POST /earn/staking/claim-rewards.
#[derive(Debug, Deserialize)]
pub struct ClaimRewardsRequest {
    /// Coin whose accrued rewards to claim (e.g. "ETH" or "SOL").
    pub coin: String,
}

/// POST /api/v1/earn/staking/claim-rewards
///
/// Claim all accrued staking rewards for a specific coin.
///
/// The `pending_rewards` balance is transferred to the user's spot trading
/// account and reset to zero.  Returns the amount claimed.
pub async fn staking_claim_rewards(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ClaimRewardsRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if !matches!(req.coin.as_str(), "ETH" | "SOL") {
        return bad_request("coin must be 'ETH' or 'SOL'");
    }

    let key = (user_id, req.coin.clone());

    // Take the pending rewards and reset to zero under lock, then release
    // before calling into accounts (avoids holding two locks simultaneously).
    let rewards = {
        let mut positions = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner());

        match positions.get_mut(&key) {
            Some(pos) if pos.pending_rewards > Decimal::ZERO => {
                let amount = pos.pending_rewards;
                pos.pending_rewards = Decimal::ZERO;
                amount
            }
            Some(_) => {
                return bad_request("no pending rewards to claim");
            }
            None => {
                return bad_request("no staking position found for this coin");
            }
        }
    };

    // Credit rewards to the user's trading balance.
    if let Err(e) = exchange.accounts.deposit(user_id, &req.coin, rewards) {
        // Revert the zeroing on failure to avoid losing the rewards.
        let mut positions = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = positions.get_mut(&key) {
            pos.pending_rewards += rewards;
        }
        return bad_request(format!("failed to credit rewards: {e}"));
    }

    tracing::info!(
        user_id = %user_id,
        coin = %req.coin,
        rewards = %rewards,
        "staking: rewards claimed",
    );

    ok(serde_json::json!({
        "coin": req.coin,
        "claimed": rewards,
    }))
}

/// GET /api/v1/earn/staking/apy-history
///
/// Public endpoint returning historical APY data for staking products.
pub async fn staking_apy_history(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let history: serde_json::Value = {
        let hist = exchange
            .staking_apy_history
            .lock().unwrap_or_else(|e| e.into_inner());

        let mut out = serde_json::Map::new();
        for (coin, snapshots) in hist.iter() {
            let snaps: Vec<_> = snapshots
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "apy": s.apy,
                        "recorded_at": s.recorded_at,
                    })
                })
                .collect();
            out.insert(coin.clone(), serde_json::Value::Array(snaps));
        }
        serde_json::Value::Object(out)
    };

    // Also include current APY rates even if no history has been recorded yet.
    let current: serde_json::Value = STAKING_APY
        .iter()
        .map(|(coin, apy)| {
            serde_json::json!({
                "coin": coin,
                "apy": apy,
            })
        })
        .collect::<Vec<_>>()
        .into();

    ok(serde_json::json!({
        "current_rates": current,
        "history": history,
    }))
}

// =============================================================================
// Crypto Loans
// =============================================================================

/// Request body for POST /loan/borrow.
#[derive(Debug, Deserialize)]
pub struct BorrowRequest {
    /// Coin posted as collateral (e.g. "BTC").
    pub collateral_coin: String,
    pub collateral_amount: Decimal,
    /// Coin to borrow (e.g. "USDT").
    pub loan_coin: String,
    pub loan_amount: Decimal,
}

/// Request body for POST /loan/repay.
#[derive(Debug, Deserialize)]
pub struct RepayRequest {
    pub loan_id: Uuid,
    /// Amount of `loan_coin` to repay (principal + accrued interest).
    pub amount: Decimal,
}

/// POST /api/v1/loan/borrow
///
/// Open a collateral-backed crypto loan.
///
/// Validation:
/// - Collateral pair must be supported.
/// - Initial LTV must not exceed 65 %.
/// - Collateral is locked (deducted) from the trading account immediately.
/// - Loan proceeds are credited to the trading account.
pub async fn loan_borrow(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BorrowRequest>,
) -> impl IntoResponse {
    use crate::exchange::default_loan_collateral_info;

    let user_id = auth.0;

    if req.collateral_amount <= Decimal::ZERO {
        return bad_request("collateral_amount must be positive");
    }
    if req.loan_amount <= Decimal::ZERO {
        return bad_request("loan_amount must be positive");
    }

    // Look up supported collateral config.
    let collateral_infos = default_loan_collateral_info();
    let info = match collateral_infos.iter().find(|c| {
        c.collateral_coin == req.collateral_coin && c.loan_coin == req.loan_coin
    }) {
        Some(i) => i,
        None => {
            return bad_request(format!(
                "unsupported collateral pair: {}/{}",
                req.collateral_coin, req.loan_coin
            ))
        }
    };

    // Get the current mark price for the collateral coin to check LTV.
    // We approximate using the index price; if unavailable we use a conservative
    // fallback that requires the user to over-collateralise.
    let collateral_price = {
        let symbol = format!("{}USDT", req.collateral_coin);
        exchange
            .get_mark_prices()
            .get(&exchange_common::Symbol::new(&symbol))
            .map(|mp| mp.price)
            .unwrap_or_else(|| {
                // No mark price available — deny the loan to protect the platform.
                Decimal::ZERO
            })
    };

    if collateral_price <= Decimal::ZERO {
        return bad_request(format!(
            "cannot determine price for {}; loan rejected",
            req.collateral_coin
        ));
    }

    let collateral_value_usdt = req.collateral_amount * collateral_price;
    let ltv = if req.loan_coin == "USDT" {
        req.loan_amount / collateral_value_usdt
    } else {
        // Generic: assume loan_coin is valued at its USDT price.
        req.loan_amount / collateral_value_usdt
    };

    if ltv > info.max_initial_ltv {
        return bad_request(format!(
            "initial LTV {:.2}% exceeds maximum {:.2}%",
            ltv * Decimal::from(100u32),
            info.max_initial_ltv * Decimal::from(100u32),
        ));
    }

    // Lock collateral + credit loan proceeds atomically on the user's shard.
    let loan_result: Result<(), String> = exchange.accounts.with_user(user_id, |am| {
        if let Err(e) = am.withdraw(user_id, &req.collateral_coin, req.collateral_amount) {
            return Err(format!("insufficient collateral balance: {e}"));
        }
        let _ = am.deposit(user_id, &req.loan_coin, req.loan_amount);
        Ok(())
    });
    if let Err(msg) = loan_result {
        return bad_request(msg);
    }

    let loan = CryptoLoan {
        id: Uuid::new_v4(),
        user_id,
        collateral_coin: req.collateral_coin.clone(),
        collateral_amount: req.collateral_amount,
        loan_coin: req.loan_coin.clone(),
        loan_amount: req.loan_amount,
        interest_accrued: Decimal::ZERO,
        interest_rate: info.daily_rate,
        ltv,
        status: LoanStatus::Active,
        opened_at: Utc::now(),
        closed_at: None,
    };
    let loan_id = loan.id;

    exchange
        .crypto_loans
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(loan);

    tracing::info!(
        user_id = %user_id,
        loan_id = %loan_id,
        collateral = %req.collateral_coin,
        collateral_amount = %req.collateral_amount,
        loan_coin = %req.loan_coin,
        loan_amount = %req.loan_amount,
        ltv = %ltv,
        "crypto_loan: loan opened",
    );

    created(serde_json::json!({
        "loan_id": loan_id,
        "collateral_coin": req.collateral_coin,
        "collateral_amount": req.collateral_amount,
        "loan_coin": req.loan_coin,
        "loan_amount": req.loan_amount,
        "interest_rate_daily": info.daily_rate,
        "ltv": ltv,
        "status": "active",
    }))
}

/// POST /api/v1/loan/repay
///
/// Repay part or all of a loan. The repayment amount is applied first to
/// accrued interest, then to principal. When the loan is fully repaid the
/// collateral is returned to the user's trading account.
pub async fn loan_repay(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<RepayRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    // Find and validate the loan.
    let (collateral_coin, collateral_amount, loan_coin, repaid_fully) = {
        let mut loans = exchange
            .crypto_loans
            .lock().unwrap_or_else(|e| e.into_inner());

        let loan = match loans.iter_mut().find(|l| l.id == req.loan_id) {
            Some(l) => l,
            None => return not_found("loan not found"),
        };

        if loan.user_id != user_id {
            return not_found("loan not found");
        }
        if loan.status != LoanStatus::Active {
            return bad_request("loan is not active");
        }

        let total_owed = loan.loan_amount + loan.interest_accrued;
        if req.amount > total_owed {
            return bad_request(format!(
                "repayment amount {} exceeds total owed {}",
                req.amount, total_owed
            ));
        }

        // Apply payment: interest first, then principal.
        let interest_payment = req.amount.min(loan.interest_accrued);
        let principal_payment = req.amount - interest_payment;
        loan.interest_accrued -= interest_payment;
        loan.loan_amount -= principal_payment;

        let fully_repaid = loan.loan_amount <= Decimal::ZERO && loan.interest_accrued <= Decimal::ZERO;
        if fully_repaid {
            loan.status = LoanStatus::Repaid;
            loan.closed_at = Some(Utc::now());
        }

        (
            loan.collateral_coin.clone(),
            loan.collateral_amount,
            loan.loan_coin.clone(),
            fully_repaid,
        )
    };

    // Deduct repayment from user's balance and, if fully repaid, return
    // collateral — all under one shard lock.
    let repay_result: Result<(), String> = exchange.accounts.with_user(user_id, |am| {
        if let Err(e) = am.withdraw(user_id, &loan_coin, req.amount) {
            return Err(format!("insufficient balance for repayment: {e}"));
        }
        if repaid_fully {
            let _ = am.deposit(user_id, &collateral_coin, collateral_amount);
        }
        Ok(())
    });
    if let Err(msg) = repay_result {
        return bad_request(msg);
    }
    if repaid_fully {
        tracing::info!(
            user_id = %user_id,
            loan_id = %req.loan_id,
            collateral_returned = %collateral_amount,
            "crypto_loan: loan fully repaid, collateral returned",
        );
    }

    ok(serde_json::json!({
        "loan_id": req.loan_id,
        "repaid_amount": req.amount,
        "fully_repaid": repaid_fully,
        "collateral_returned": if repaid_fully { collateral_amount } else { Decimal::ZERO },
    }))
}

/// GET /api/v1/loan/positions
///
/// Returns the authenticated user's active loans with current LTV.
pub async fn loan_positions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let loans: Vec<serde_json::Value> = {
        let lock = exchange
            .crypto_loans
            .lock().unwrap_or_else(|e| e.into_inner());
        lock.iter()
            .filter(|l| l.user_id == user_id && l.status == LoanStatus::Active)
            .map(|l| {
                serde_json::json!({
                    "loan_id": l.id,
                    "collateral_coin": l.collateral_coin,
                    "collateral_amount": l.collateral_amount,
                    "loan_coin": l.loan_coin,
                    "loan_amount": l.loan_amount,
                    "interest_accrued": l.interest_accrued,
                    "total_owed": l.loan_amount + l.interest_accrued,
                    "interest_rate_daily": l.interest_rate,
                    "ltv": l.ltv,
                    "status": l.status,
                    "opened_at": l.opened_at,
                })
            })
            .collect()
    };

    ok(serde_json::json!({ "loans": loans }))
}

/// GET /api/v1/loan/collateral-info
///
/// Public endpoint returning supported collateral pairs, LTV limits, and rates.
pub async fn loan_collateral_info(
    State(_exchange): State<AppState>,
) -> impl IntoResponse {
    use crate::exchange::default_loan_collateral_info;

    let info: Vec<serde_json::Value> = default_loan_collateral_info()
        .into_iter()
        .map(|c| serde_json::to_value(c).unwrap_or(serde_json::Value::Null))
        .collect();

    ok(serde_json::json!({ "collateral_pairs": info }))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;
    use rust_decimal_macros::dec;

    // ── Dual Investment ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dual_invest_subscribe_and_settle() {
        let exchange = Exchange::new();
        let user_id = Uuid::new_v4();
        exchange.accounts.create_account(user_id);
        let _ = exchange.accounts.deposit(user_id, "USDT", dec!(10_000));

        // Subscribe to a buy_low product.
        let product_id = {
            let now = Utc::now();
            let product = DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: "BTC".into(),
                direction: crate::exchange::DualInvestDirection::BuyLow,
                strike_price: dec!(60_000),
                expiry: now + chrono::Duration::days(7),
                apy: dec!(0.12),
                status: DualInvestStatus::Active,
                created_at: now,
            };
            let pid = product.id;
            exchange
                .dual_invest_products
                .lock().unwrap_or_else(|e| e.into_inner())
                .push(product);
            pid
        };

        // Lock 1 000 USDT for the position.
        exchange
            .accounts
            .withdraw(user_id, "USDT", dec!(1_000))
            .expect("withdraw should succeed");

        let position = DualInvestPosition {
            id: Uuid::new_v4(),
            user_id,
            product_id,
            amount: dec!(1_000),
            settled: false,
            payout: None,
            payout_coin: None,
            subscribed_at: Utc::now(),
        };
        let pos_id = position.id;
        exchange
            .dual_invest_positions
            .lock().unwrap_or_else(|e| e.into_inner())
            .push(position);

        // Verify position is recorded.
        let positions = exchange.dual_invest_positions.lock().unwrap_or_else(|e| e.into_inner());
        let pos = positions.iter().find(|p| p.id == pos_id).expect("position should exist");
        assert_eq!(pos.user_id, user_id);
        assert!(!pos.settled);
        drop(positions);

        // Simulate settlement (price did NOT hit strike — buy_low, mark < strike → exercise).
        // Mark price = 55 000 < strike 60 000 → user receives BTC at strike.
        let mark_price = dec!(55_000);
        let strike = dec!(60_000);
        {
            let mut positions = exchange.dual_invest_positions.lock().unwrap_or_else(|e| e.into_inner());
            let pos = positions.iter_mut().find(|p| p.id == pos_id).unwrap();
            let pos_amount = pos.amount;

            // BuyLow settlement: mark ≤ strike → convert USDT to BTC at strike.
            let btc_received = pos_amount / strike;
            let premium = pos_amount * dec!(0.12) / dec!(365) * dec!(7);
            let payout = btc_received + premium / mark_price; // premium in BTC terms
            pos.settled = true;
            pos.payout = Some(payout);
            pos.payout_coin = Some("BTC".into());

            // Credit payout.
            exchange
                .accounts
                .deposit(user_id, "BTC", payout)
                .expect("deposit should succeed");
        }

        let positions = exchange.dual_invest_positions.lock().unwrap_or_else(|e| e.into_inner());
        let pos = positions.iter().find(|p| p.id == pos_id).unwrap();
        assert!(pos.settled);
        assert!(pos.payout.is_some());
        assert_eq!(pos.payout_coin.as_deref(), Some("BTC"));
    }

    // ── Staking ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_staking_stake_unstake_rewards() {
        let exchange = Exchange::new();
        let user_id = Uuid::new_v4();
        exchange.accounts.create_account(user_id);
        let _ = exchange.accounts.deposit(user_id, "ETH", dec!(10));

        // Stake 5 ETH.
        exchange
            .accounts
            .withdraw(user_id, "ETH", dec!(5))
            .expect("withdraw should succeed");

        let now = Utc::now();
        exchange.staking_positions.lock().unwrap_or_else(|e| e.into_inner()).insert(
            (user_id, "ETH".into()),
            StakingPosition {
                user_id,
                coin: "ETH".into(),
                staked_amount: dec!(5),
                pending_rewards: Decimal::ZERO,
                staked_at: now,
                last_accrued_at: now,
            },
        );

        // Accrue one hour of rewards at 4 % APY.
        let hourly_rate: Decimal = "0.04".parse::<Decimal>().unwrap() / Decimal::from(8760u32);
        {
            let mut positions = exchange.staking_positions.lock().unwrap_or_else(|e| e.into_inner());
            let pos = positions.get_mut(&(user_id, "ETH".into())).unwrap();
            let reward = pos.staked_amount * hourly_rate;
            pos.pending_rewards += reward;
        }

        let positions = exchange.staking_positions.lock().unwrap_or_else(|e| e.into_inner());
        let pos = positions.get(&(user_id, "ETH".into())).unwrap();
        assert!(pos.pending_rewards > Decimal::ZERO);
        drop(positions);

        // Unstake 2 ETH — should create unbonding entry.
        {
            let mut positions = exchange.staking_positions.lock().unwrap_or_else(|e| e.into_inner());
            let pos = positions.get_mut(&(user_id, "ETH".into())).unwrap();
            assert!(pos.staked_amount >= dec!(2));
            pos.staked_amount -= dec!(2);
        }
        let available_at = Utc::now() + chrono::Duration::days(3);
        exchange.staking_unbonding.lock().unwrap_or_else(|e| e.into_inner()).push(UnbondingEntry {
            id: Uuid::new_v4(),
            user_id,
            coin: "ETH".into(),
            amount: dec!(2),
            available_at,
            requested_at: Utc::now(),
        });

        let positions = exchange.staking_positions.lock().unwrap_or_else(|e| e.into_inner());
        let pos = positions.get(&(user_id, "ETH".into())).unwrap();
        assert_eq!(pos.staked_amount, dec!(3));
        drop(positions);

        let unbonding = exchange.staking_unbonding.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(unbonding.iter().filter(|e| e.user_id == user_id).count(), 1);
    }

    // ── Crypto Loans ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_loan_borrow_repay() {
        let exchange = Exchange::new();
        let user_id = Uuid::new_v4();
        exchange.accounts.create_account(user_id);
        let _ = exchange.accounts.deposit(user_id, "BTC", dec!(1));

        // Simulate opening a loan directly (without the HTTP handler).
        let collateral_amount = dec!(1);
        let loan_amount = dec!(30_000);
        let ltv = dec!(0.50); // 50 % LTV, well under 65 %

        exchange
            .accounts
            .withdraw(user_id, "BTC", collateral_amount)
            .expect("should withdraw collateral");

        // Deposit loan proceeds + a small buffer to cover accrued interest (0.05 %).
        let interest_buffer = loan_amount * dec!(0.001); // 1 day at 0.05 %/day, with headroom
        let _ = exchange
            .accounts
            .deposit(user_id, "USDT", loan_amount + interest_buffer);

        let loan = CryptoLoan {
            id: Uuid::new_v4(),
            user_id,
            collateral_coin: "BTC".into(),
            collateral_amount,
            loan_coin: "USDT".into(),
            loan_amount,
            interest_accrued: Decimal::ZERO,
            interest_rate: dec!(0.0005),
            ltv,
            status: LoanStatus::Active,
            opened_at: Utc::now(),
            closed_at: None,
        };
        let loan_id = loan.id;
        exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner()).push(loan);

        // Verify loan is active.
        {
            let loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
            let l = loans.iter().find(|l| l.id == loan_id).unwrap();
            assert_eq!(l.status, LoanStatus::Active);
            assert_eq!(l.loan_amount, loan_amount);
        }

        // Accrue one day of interest.
        {
            let mut loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
            let l = loans.iter_mut().find(|l| l.id == loan_id).unwrap();
            let daily_interest = l.loan_amount * l.interest_rate;
            l.interest_accrued += daily_interest;
        }

        // Repay full amount (principal + accrued interest).
        let total_owed = {
            let loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
            let l = loans.iter().find(|l| l.id == loan_id).unwrap();
            l.loan_amount + l.interest_accrued
        };

        // Deduct repayment from USDT balance.
        exchange
            .accounts
            .withdraw(user_id, "USDT", total_owed)
            .expect("should have enough USDT to repay");

        {
            let mut loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
            let l = loans.iter_mut().find(|l| l.id == loan_id).unwrap();
            let interest_paid = total_owed.min(l.interest_accrued);
            let principal_paid = total_owed - interest_paid;
            l.interest_accrued -= interest_paid;
            l.loan_amount -= principal_paid;
            l.status = LoanStatus::Repaid;
            l.closed_at = Some(Utc::now());

            // Return collateral.
            let _ = exchange.accounts.deposit(user_id, "BTC", l.collateral_amount);
        }

        {
            let loans = exchange.crypto_loans.lock().unwrap_or_else(|e| e.into_inner());
            let l = loans.iter().find(|l| l.id == loan_id).unwrap();
            assert_eq!(l.status, LoanStatus::Repaid);
        }

        // BTC balance should be restored.
        let btc_balance = exchange
            .accounts
            .get_balance(user_id, "BTC")
            .map(|b| b.total())
            .unwrap_or(Decimal::ZERO);
        assert_eq!(btc_balance, collateral_amount);
    }

    #[tokio::test]
    async fn test_loan_liquidation_threshold() {
        // Verify the LTV liquidation threshold constants.
        use crate::exchange::default_loan_collateral_info;

        let infos = default_loan_collateral_info();
        let btc_info = infos.iter().find(|c| c.collateral_coin == "BTC").unwrap();

        assert_eq!(btc_info.max_initial_ltv, dec!(0.65));
        assert_eq!(btc_info.liquidation_ltv, dec!(0.85));

        // Simulate LTV crossing the liquidation threshold.
        let collateral_value = dec!(60_000); // 1 BTC at $60k
        let loan_amount = dec!(40_000);

        let ltv = loan_amount / collateral_value;
        assert!(ltv < btc_info.liquidation_ltv, "initial LTV should be below liquidation threshold");

        // Price drops: collateral now worth $47 000.
        let new_collateral_value = dec!(47_000);
        let new_ltv = loan_amount / new_collateral_value;
        assert!(
            new_ltv > btc_info.liquidation_ltv,
            "LTV after price drop should exceed liquidation threshold"
        );
    }
}
