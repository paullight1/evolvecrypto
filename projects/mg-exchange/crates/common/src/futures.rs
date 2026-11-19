//! Types for perpetual futures trading.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::types::{Price, Qty, Symbol, UserId};

/// Contract denomination type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ContractType {
    /// USDT/USDC-margined (linear). PnL in quote asset.
    #[default]
    Linear,
    /// Coin-margined (inverse). PnL in base asset.
    Inverse,
}

/// Position mode for a user/symbol pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PositionMode {
    /// Single position per symbol; opposite-side orders flip the position.
    #[default]
    OneWay,
    /// Simultaneous Long and Short allowed; each has its own margin.
    Hedge,
}

/// Position index within a user/symbol pair.
/// - `0` = one-way mode (default)
/// - `1` = buy/long side (hedge mode)
/// - `2` = sell/short side (hedge mode)
pub type PositionIdx = u8;

/// Position index constants.
pub const POSITION_IDX_ONE_WAY: PositionIdx = 0;
pub const POSITION_IDX_LONG: PositionIdx = 1;
pub const POSITION_IDX_SHORT: PositionIdx = 2;

/// A single tier in a tiered leverage / maintenance-margin schedule.
///
/// Exchanges (Bybit, Binance) reduce max leverage and raise maintenance margin
/// rates as position size grows, to limit systemic risk from large positions.
///
/// Tiers are ordered by `min_notional` and must be contiguous:
/// the `max_notional` of tier N equals the `min_notional` of tier N+1.
/// The last tier should use `Decimal::MAX` as `max_notional`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeverageTier {
    /// Lower bound of position notional this tier applies to (inclusive).
    pub min_notional: Decimal,
    /// Upper bound of position notional this tier applies to (exclusive).
    /// Use `Decimal::MAX` for the final (unbounded) tier.
    pub max_notional: Decimal,
    /// Maximum leverage permitted at this notional level.
    pub max_leverage: Decimal,
    /// Maintenance margin rate (e.g., `0.005` = 0.5%) below which liquidation triggers.
    pub maintenance_margin_rate: Decimal,
}

/// Margin mode for a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarginMode {
    /// Cross margin — all available balance is shared across positions.
    Cross,
    /// Isolated margin — each position has its own margin.
    Isolated,
}

/// Position side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionSide {
    Long,
    Short,
}

impl PositionSide {
    pub fn opposite(&self) -> Self {
        match self {
            PositionSide::Long => PositionSide::Short,
            PositionSide::Short => PositionSide::Long,
        }
    }

    /// PnL sign multiplier: Long = +1, Short = -1.
    pub fn sign(&self) -> Decimal {
        match self {
            PositionSide::Long => Decimal::ONE,
            PositionSide::Short => Decimal::NEGATIVE_ONE,
        }
    }
}

/// A perpetual futures position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: PositionSide,
    /// Position size in base asset (e.g., 0.5 BTC).
    pub size: Qty,
    /// Average entry price.
    pub entry_price: Price,
    /// Leverage (e.g., 10 = 10x).
    pub leverage: Decimal,
    /// Margin mode.
    pub margin_mode: MarginMode,
    /// Margin allocated to this position (in quote asset).
    /// For isolated: explicit. For cross: computed from equity.
    pub margin: Decimal,
    /// Accumulated realized PnL (from partial closes).
    pub realized_pnl: Decimal,
    /// Accumulated funding payments (positive = received, negative = paid).
    pub funding_accumulated: Decimal,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Position index: 0 = one-way, 1 = hedge long, 2 = hedge short.
    #[serde(default)]
    pub position_idx: PositionIdx,
    /// Take-profit trigger price (None = not set).
    #[serde(default)]
    pub tp_price: Option<Price>,
    /// Stop-loss trigger price (None = not set).
    #[serde(default)]
    pub sl_price: Option<Price>,
    /// Trailing stop distance as percentage (e.g., 1.5 = 1.5%).
    #[serde(default)]
    pub trailing_stop: Option<Decimal>,
    /// Trigger order ID backing the TP (for cancellation).
    #[serde(default)]
    pub tp_trigger_id: Option<uuid::Uuid>,
    /// Trigger order ID backing the SL (for cancellation).
    #[serde(default)]
    pub sl_trigger_id: Option<uuid::Uuid>,
    /// Partial TP size (None = full position).
    #[serde(default)]
    pub tp_size: Option<Qty>,
    /// Partial SL size (None = full position).
    #[serde(default)]
    pub sl_size: Option<Qty>,
    /// Whether to automatically add margin from available balance when margin
    /// ratio approaches the liquidation threshold (isolated margin only).
    #[serde(default)]
    pub auto_add_margin: bool,
    /// Accumulated unpaid funding debt (in quote asset, e.g. USDT).
    ///
    /// When a funding payment debit fails because the user's balance is
    /// insufficient, the owed amount is tracked here instead of being silently
    /// forgiven. The debt reduces effective margin so the position gets
    /// liquidated sooner — correct behaviour for an under-margined position.
    #[serde(default)]
    pub unpaid_funding_debt: Decimal,
}

impl Position {
    /// Unrealized PnL at a given mark price.
    pub fn unrealized_pnl(&self, mark_price: Price) -> Decimal {
        self.side.sign() * self.size * (mark_price - self.entry_price)
    }

    /// Unrealized PnL for inverse (coin-margined) contracts.
    ///
    /// Formula: `side * size * (1/entry_price - 1/mark_price)`
    ///
    /// Result is denominated in the base asset (e.g., BTC for BTCUSD).
    pub fn unrealized_pnl_inverse(&self, mark_price: Price) -> Decimal {
        if mark_price.is_zero() || self.entry_price.is_zero() {
            return Decimal::ZERO;
        }
        self.side.sign() * self.size * (Decimal::ONE / self.entry_price - Decimal::ONE / mark_price)
    }

    /// Position notional value at mark price.
    ///
    /// For linear: `size * mark_price` (in quote asset, e.g. USDT).
    /// For inverse use `notional_inverse`.
    pub fn notional(&self, mark_price: Price) -> Decimal {
        self.size * mark_price
    }

    /// Notional value for inverse (coin-margined) contracts denominated in USD.
    ///
    /// For inverse perps `size` is the number of contracts (each worth `contract_size` USD):
    ///   notional_usd = size * contract_size
    ///
    /// The mark_price is included for API symmetry but the USD value is fixed per contract.
    pub fn notional_inverse(&self, contract_size: Decimal) -> Decimal {
        self.size * contract_size
    }

    /// Effective margin (margin + unrealized PnL - unpaid funding debt).
    ///
    /// For linear positions the result is in the quote asset (e.g. USDT).
    /// Unpaid funding debt reduces effective margin so positions that owe
    /// accumulated funding are liquidated sooner — matching real exchange
    /// behaviour where funding is deducted before margin health is assessed.
    pub fn effective_margin(&self, mark_price: Price) -> Decimal {
        self.margin + self.unrealized_pnl(mark_price) - self.unpaid_funding_debt
    }

    /// Effective margin for inverse (coin-margined) positions.
    ///
    /// Result is in the base asset (e.g. BTC).
    pub fn effective_margin_inverse(&self, mark_price: Price) -> Decimal {
        self.margin + self.unrealized_pnl_inverse(mark_price)
    }

    /// Margin ratio = effective_margin / notional. Lower = more risk.
    pub fn margin_ratio(&self, mark_price: Price) -> Decimal {
        let notional = self.notional(mark_price);
        if notional.is_zero() {
            return Decimal::MAX;
        }
        self.effective_margin(mark_price) / notional
    }

    /// Liquidation price (isolated margin).
    ///
    /// Long: liq_price = entry_price - margin / size
    /// Short: liq_price = entry_price + margin / size
    pub fn liquidation_price(&self) -> Option<Price> {
        if self.size.is_zero() {
            return None;
        }
        let margin_per_unit = self.margin / self.size;
        match self.side {
            PositionSide::Long => {
                let liq = self.entry_price - margin_per_unit;
                if liq > Decimal::ZERO { Some(liq) } else { None }
            }
            PositionSide::Short => {
                Some(self.entry_price + margin_per_unit)
            }
        }
    }

    /// Liquidation price for inverse (coin-margined) isolated-margin positions.
    ///
    /// For inverse perps, margin is held in base asset (BTC) and the payoff is
    /// non-linear.  The liquidation price is the mark price at which the effective
    /// margin in base-asset terms reaches zero (bankruptcy price).
    ///
    /// Derivation (Long, N contracts, each worth `cs` USD, margin `m` BTC):
    ///   unrealised_pnl_btc = N*cs * (1/entry - 1/mark)
    ///   effective = m + N*cs*(1/entry - 1/bk) = 0 at liquidation
    ///   → N*cs/bk = N*cs/entry + m
    ///   → bk = entry*N*cs / (N*cs + m*entry)
    ///   Note: bk < entry (long liquidated when price falls)
    ///
    /// Derivation (Short, N contracts, margin `m` BTC):
    ///   unrealised_pnl_btc = N*cs * (1/mark - 1/entry)
    ///   effective = m + N*cs*(1/bk - 1/entry) = 0 at liquidation
    ///   → N*cs/bk = N*cs/entry - m
    ///   → bk = entry*N*cs / (N*cs - m*entry)
    ///   Note: bk > entry (short liquidated when price rises)
    pub fn liquidation_price_inverse(&self, contract_size: Decimal) -> Option<Price> {
        if self.size.is_zero() || self.entry_price.is_zero() || contract_size.is_zero() {
            return None;
        }
        let n_cs = self.size * contract_size; // total USD notional
        match self.side {
            PositionSide::Long => {
                // bk = entry * N*cs / (N*cs + m*entry)  — always positive, always < entry
                let denom = n_cs + self.margin * self.entry_price;
                if denom.is_zero() {
                    return None;
                }
                let liq = self.entry_price * n_cs / denom;
                if liq > Decimal::ZERO { Some(liq) } else { None }
            }
            PositionSide::Short => {
                // bk = entry * N*cs / (N*cs - m*entry)  — > entry when m*entry < N*cs
                let denom = n_cs - self.margin * self.entry_price;
                if denom <= Decimal::ZERO {
                    // Margin so large that position cannot be liquidated at any finite price.
                    return None;
                }
                let liq = self.entry_price * n_cs / denom;
                if liq > Decimal::ZERO { Some(liq) } else { None }
            }
        }
    }

    /// Is the position empty (fully closed)?
    pub fn is_closed(&self) -> bool {
        self.size.is_zero()
    }
}

/// Perpetual contract configuration (extends Product).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerpetualConfig {
    pub symbol: Symbol,
    /// Maximum leverage allowed at the smallest tier (tier-0 default).
    /// Mirrors `tiers[0].max_leverage` for backward-compatible access.
    pub max_leverage: Decimal,
    /// Initial margin rate at the smallest tier = 1 / max_leverage.
    pub initial_margin_rate: Decimal,
    /// Maintenance margin rate at the smallest tier (tier-0 default).
    /// Mirrors `tiers[0].maintenance_margin_rate` for backward-compatible access.
    pub maintenance_margin_rate: Decimal,
    /// Funding rate interval in hours (typically 8).
    pub funding_interval_hours: u32,
    /// Maximum funding rate per interval (e.g., 0.01 = 1%).
    pub max_funding_rate: Decimal,
    /// Insurance fund balance for this symbol.
    pub insurance_fund: Decimal,
    /// Tiered leverage and maintenance-margin schedule, ordered ascending by `min_notional`.
    /// Must be non-empty; `tiers[0]` corresponds to the smallest positions.
    pub tiers: Vec<LeverageTier>,
    /// Contract type: Linear (USDT-margined) or Inverse (coin-margined).
    #[serde(default)]
    pub contract_type: ContractType,
    /// Settlement asset for this contract.
    ///
    /// For linear contracts this is the quote asset (e.g. "USDT").
    /// For inverse contracts this is the base asset (e.g. "BTC" for BTCUSD).
    #[serde(default = "default_settle_coin")]
    pub settle_coin: String,
    /// Face value of one contract in the quote asset.
    ///
    /// For linear perpetuals this is typically 1 (one unit of base asset per contract).
    /// For inverse perpetuals this is the USD face value per contract (e.g. 1 USD for BTCUSD).
    #[serde(default = "default_contract_size")]
    pub contract_size: Decimal,
}

fn default_settle_coin() -> String {
    "USDT".to_string()
}

fn default_contract_size() -> Decimal {
    Decimal::ONE
}

impl PerpetualConfig {
    /// Return the maintenance margin rate for a position of the given notional.
    ///
    /// Finds the first tier whose `[min_notional, max_notional)` range contains
    /// `notional`. If `notional` exceeds all tier bounds, the last tier is used.
    pub fn effective_mmr(&self, notional: Decimal) -> Decimal {
        self.find_tier(notional).maintenance_margin_rate
    }

    /// Return the maximum leverage allowed for a position of the given notional.
    pub fn max_leverage_for_notional(&self, notional: Decimal) -> Decimal {
        self.find_tier(notional).max_leverage
    }

    /// Find the tier that covers `notional`. Falls back to the last tier if none match.
    fn find_tier(&self, notional: Decimal) -> &LeverageTier {
        // Tiers are ordered by min_notional ascending. Walk forward and return the
        // first tier whose range contains notional. The last tier is always returned
        // as a fallback (handles Decimal::MAX upper bound).
        for tier in &self.tiers {
            if notional >= tier.min_notional && notional < tier.max_notional {
                return tier;
            }
        }
        // Safety fallback: return the last tier (should always match via Decimal::MAX bound).
        self.tiers.last().expect("tiers must be non-empty")
    }

    pub fn btc_default() -> Self {
        // Bybit BTCUSDT tiered leverage schedule (approximate).
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(2_000_000, 0),
                max_leverage: Decimal::new(100, 0),
                maintenance_margin_rate: Decimal::new(5, 3), // 0.5%
            },
            LeverageTier {
                min_notional: Decimal::new(2_000_000, 0),
                max_notional: Decimal::new(6_000_000, 0),
                max_leverage: Decimal::new(50, 0),
                maintenance_margin_rate: Decimal::new(1, 2), // 1.0%
            },
            LeverageTier {
                min_notional: Decimal::new(6_000_000, 0),
                max_notional: Decimal::new(12_000_000, 0),
                max_leverage: Decimal::new(25, 0),
                maintenance_margin_rate: Decimal::new(2, 2), // 2.0%
            },
            LeverageTier {
                min_notional: Decimal::new(12_000_000, 0),
                max_notional: Decimal::new(25_000_000, 0),
                max_leverage: Decimal::new(15, 0),
                maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            },
            LeverageTier {
                min_notional: Decimal::new(25_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
        ];
        Self {
            symbol: Symbol::new("BTCUSDT"),
            max_leverage: Decimal::new(100, 0),
            initial_margin_rate: Decimal::new(1, 2),     // 1% = 100x
            maintenance_margin_rate: Decimal::new(5, 3), // 0.5%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),         // 1%
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Linear,
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
        }
    }

    pub fn eth_default() -> Self {
        // ETH: 50x/25x/15x/10x at similar notional brackets.
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(1_000_000, 0),
                max_leverage: Decimal::new(50, 0),
                maintenance_margin_rate: Decimal::new(1, 2), // 1.0%
            },
            LeverageTier {
                min_notional: Decimal::new(1_000_000, 0),
                max_notional: Decimal::new(4_000_000, 0),
                max_leverage: Decimal::new(25, 0),
                maintenance_margin_rate: Decimal::new(2, 2), // 2.0%
            },
            LeverageTier {
                min_notional: Decimal::new(4_000_000, 0),
                max_notional: Decimal::new(10_000_000, 0),
                max_leverage: Decimal::new(15, 0),
                maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            },
            LeverageTier {
                min_notional: Decimal::new(10_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
        ];
        Self {
            symbol: Symbol::new("ETHUSDT"),
            max_leverage: Decimal::new(50, 0),
            initial_margin_rate: Decimal::new(2, 2),     // 2% = 50x
            maintenance_margin_rate: Decimal::new(1, 2), // 1%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Linear,
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
        }
    }

    pub fn sol_default() -> Self {
        // SOL: 20x/10x/5x tiers.
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(500_000, 0),
                max_leverage: Decimal::new(20, 0),
                maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            },
            LeverageTier {
                min_notional: Decimal::new(500_000, 0),
                max_notional: Decimal::new(2_000_000, 0),
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
            LeverageTier {
                min_notional: Decimal::new(2_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(5, 0),
                maintenance_margin_rate: Decimal::new(1, 1), // 10.0%
            },
        ];
        Self {
            symbol: Symbol::new("SOLUSDT"),
            max_leverage: Decimal::new(20, 0),
            initial_margin_rate: Decimal::new(5, 2),      // 5% = 20x
            maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Linear,
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
        }
    }

    /// Default config for BTCUSD inverse perpetual.
    ///
    /// Contract specification:
    /// - Symbol: BTCUSD
    /// - Contract size: 1 USD per contract (i.e. 1 contract represents $1 of BTC)
    /// - Margin/settlement: BTC
    /// - 1 contract = 1 USD, so 10,000 contracts = $10,000 notional
    ///
    /// Margin for N contracts at price P with leverage L:
    ///   margin_btc = (N * contract_size) / (P * L)
    ///
    /// Example: 10,000 contracts at $50,000 with 10x:
    ///   = (10,000 * 1) / (50,000 * 10) = 0.02 BTC
    pub fn btcusd_inverse_default() -> Self {
        // Bybit BTCUSD inverse perp tiered schedule.
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(2_000_000, 0),
                max_leverage: Decimal::new(100, 0),
                maintenance_margin_rate: Decimal::new(5, 3), // 0.5%
            },
            LeverageTier {
                min_notional: Decimal::new(2_000_000, 0),
                max_notional: Decimal::new(6_000_000, 0),
                max_leverage: Decimal::new(50, 0),
                maintenance_margin_rate: Decimal::new(1, 2), // 1.0%
            },
            LeverageTier {
                min_notional: Decimal::new(6_000_000, 0),
                max_notional: Decimal::new(25_000_000, 0),
                max_leverage: Decimal::new(20, 0),
                maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            },
            LeverageTier {
                min_notional: Decimal::new(25_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
        ];
        Self {
            symbol: Symbol::new("BTCUSD"),
            max_leverage: Decimal::new(100, 0),
            initial_margin_rate: Decimal::new(1, 2),     // 1% = 100x
            maintenance_margin_rate: Decimal::new(5, 3), // 0.5%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Inverse,
            settle_coin: "BTC".to_string(),
            contract_size: Decimal::ONE, // 1 USD per contract
        }
    }

    /// Default config for ETHUSD inverse perpetual.
    ///
    /// Contract specification:
    /// - Symbol: ETHUSD
    /// - Contract size: 1 USD per contract
    /// - Margin/settlement: ETH
    pub fn ethusd_inverse_default() -> Self {
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(1_000_000, 0),
                max_leverage: Decimal::new(50, 0),
                maintenance_margin_rate: Decimal::new(1, 2), // 1.0%
            },
            LeverageTier {
                min_notional: Decimal::new(1_000_000, 0),
                max_notional: Decimal::new(5_000_000, 0),
                max_leverage: Decimal::new(20, 0),
                maintenance_margin_rate: Decimal::new(25, 3), // 2.5%
            },
            LeverageTier {
                min_notional: Decimal::new(5_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
        ];
        Self {
            symbol: Symbol::new("ETHUSD"),
            max_leverage: Decimal::new(50, 0),
            initial_margin_rate: Decimal::new(2, 2),     // 2% = 50x
            maintenance_margin_rate: Decimal::new(1, 2), // 1.0%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Inverse,
            settle_coin: "ETH".to_string(),
            contract_size: Decimal::ONE, // 1 USD per contract
        }
    }

    /// Generic altcoin linear perpetual config.
    ///
    /// Reference: Bybit altcoin leverage tiers (approximate).
    /// Used for BNB, XRP, DOGE, AVAX, ADA, LINK, DOT and similar mid-cap perps.
    /// - Tier 0: 0 – $250K notional → 25x max, 2% MMR
    /// - Tier 1: $250K – $1M → 10x max, 5% MMR
    /// - Tier 2: $1M+ → 5x max, 10% MMR
    fn altcoin_linear(symbol_str: &str) -> Self {
        let tiers = vec![
            LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::new(250_000, 0),
                max_leverage: Decimal::new(25, 0),
                maintenance_margin_rate: Decimal::new(2, 2), // 2.0%
            },
            LeverageTier {
                min_notional: Decimal::new(250_000, 0),
                max_notional: Decimal::new(1_000_000, 0),
                max_leverage: Decimal::new(10, 0),
                maintenance_margin_rate: Decimal::new(5, 2), // 5.0%
            },
            LeverageTier {
                min_notional: Decimal::new(1_000_000, 0),
                max_notional: Decimal::MAX,
                max_leverage: Decimal::new(5, 0),
                maintenance_margin_rate: Decimal::new(1, 1), // 10.0%
            },
        ];
        Self {
            symbol: Symbol::new(symbol_str),
            max_leverage: Decimal::new(25, 0),
            initial_margin_rate: Decimal::new(4, 2),      // 4% = 25x
            maintenance_margin_rate: Decimal::new(2, 2),   // 2.0%
            funding_interval_hours: 8,
            max_funding_rate: Decimal::new(1, 2),
            insurance_fund: Decimal::ZERO,
            tiers,
            contract_type: ContractType::Linear,
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
        }
    }

    pub fn bnb_default() -> Self { Self::altcoin_linear("BNBUSDT") }
    pub fn xrp_default() -> Self { Self::altcoin_linear("XRPUSDT") }
    pub fn doge_default() -> Self { Self::altcoin_linear("DOGEUSDT") }
    pub fn avax_default() -> Self { Self::altcoin_linear("AVAXUSDT") }
    pub fn ada_default() -> Self { Self::altcoin_linear("ADAUSDT") }
    pub fn link_default() -> Self { Self::altcoin_linear("LINKUSDT") }
    pub fn dot_default() -> Self { Self::altcoin_linear("DOTUSDT") }

    /// Returns true when this is a coin-margined inverse contract.
    pub fn is_inverse(&self) -> bool {
        self.contract_type == ContractType::Inverse
    }
}

/// Funding rate snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    pub symbol: Symbol,
    /// Current funding rate for this interval.
    pub rate: Decimal,
    /// Mark price at the time of calculation.
    pub mark_price: Price,
    /// Index price (spot weighted average).
    pub index_price: Price,
    /// Next funding settlement timestamp.
    pub next_funding_time: chrono::DateTime<chrono::Utc>,
    pub calculated_at: chrono::DateTime<chrono::Utc>,
}

/// Reason a futures position was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    UserClosed,
    TakeProfit,
    StopLoss,
    Liquidation,
    Adl,
}

impl std::fmt::Display for CloseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloseReason::UserClosed => write!(f, "user_closed"),
            CloseReason::TakeProfit => write!(f, "take_profit"),
            CloseReason::StopLoss => write!(f, "stop_loss"),
            CloseReason::Liquidation => write!(f, "liquidation"),
            CloseReason::Adl => write!(f, "adl"),
        }
    }
}

/// An immutable record of a fully-closed futures position for PnL history.
///
/// Created once when a position reaches zero size. The record captures a
/// snapshot of the position's financials at the moment of final close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedPositionRecord {
    pub id: uuid::Uuid,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: PositionSide,
    /// Average entry price across all fills that opened the position.
    pub avg_entry_price: Price,
    /// Fill price of the final closing trade.
    pub avg_exit_price: Price,
    /// Total size that was closed (base asset units).
    pub closed_size: Qty,
    /// Realized PnL from this close, net of entry price.
    pub realized_pnl: Decimal,
    /// Total trading fees incurred (Phase 1: zero — fee deduction not yet per-fill).
    pub trading_fee: Decimal,
    /// Net funding payments accumulated while the position was open.
    /// Positive = received by user; negative = paid by user.
    pub funding_fee: Decimal,
    pub leverage: Decimal,
    pub close_reason: CloseReason,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub closed_at: chrono::DateTime<chrono::Utc>,
}

/// Mark price info (fair price for PnL + liquidation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkPrice {
    pub symbol: Symbol,
    /// Mark price = index_price * (1 + basis).
    pub price: Price,
    /// Spot index price (weighted avg from top exchanges).
    pub index_price: Price,
    /// Last traded price on this exchange.
    pub last_price: Price,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Liquidation order info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationOrder {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: PositionSide,
    pub size: Qty,
    pub bankruptcy_price: Price,
    pub mark_price: Price,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Position index from which this liquidation was triggered.
    #[serde(default)]
    pub position_idx: PositionIdx,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_position(side: PositionSide, size: Decimal, entry: Decimal, margin: Decimal) -> Position {
        Position {
            user_id: Uuid::nil(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            size,
            entry_price: entry,
            leverage: dec!(10),
            margin_mode: MarginMode::Isolated,
            margin,
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            position_idx: 0,
            tp_price: None,
            sl_price: None,
            trailing_stop: None,
            tp_trigger_id: None,
            sl_trigger_id: None,
            tp_size: None,
            sl_size: None,
            auto_add_margin: false,
            unpaid_funding_debt: Decimal::ZERO,
        }
    }

    #[test]
    fn test_long_pnl_profit() {
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unrealized_pnl(dec!(51000)), dec!(1000));
    }

    #[test]
    fn test_long_pnl_loss() {
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unrealized_pnl(dec!(49000)), dec!(-1000));
    }

    #[test]
    fn test_short_pnl_profit() {
        let pos = make_position(PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unrealized_pnl(dec!(49000)), dec!(1000));
    }

    #[test]
    fn test_short_pnl_loss() {
        let pos = make_position(PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unrealized_pnl(dec!(51000)), dec!(-1000));
    }

    #[test]
    fn test_long_liquidation_price() {
        // Entry 50000, margin 5000, size 1 → liq = 50000 - 5000 = 45000
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.liquidation_price(), Some(dec!(45000)));
    }

    #[test]
    fn test_short_liquidation_price() {
        // Entry 50000, margin 5000, size 1 → liq = 50000 + 5000 = 55000
        let pos = make_position(PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.liquidation_price(), Some(dec!(55000)));
    }

    #[test]
    fn test_margin_ratio() {
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        // At entry price: margin_ratio = 5000 / 50000 = 0.1 (10%)
        assert_eq!(pos.margin_ratio(dec!(50000)), dec!(0.1));

        // Price up: effective_margin = 5000 + 1000 = 6000, notional = 51000
        // ratio = 6000/51000 ≈ 0.1176
        let ratio = pos.margin_ratio(dec!(51000));
        assert!(ratio > dec!(0.117) && ratio < dec!(0.118));
    }

    #[test]
    fn test_notional() {
        let pos = make_position(PositionSide::Long, dec!(2), dec!(50000), dec!(10000));
        assert_eq!(pos.notional(dec!(51000)), dec!(102000));
    }

    #[test]
    fn test_perp_config_defaults() {
        let btc = PerpetualConfig::btc_default();
        assert_eq!(btc.max_leverage, dec!(100));
        assert_eq!(btc.maintenance_margin_rate, dec!(0.005));

        let sol = PerpetualConfig::sol_default();
        assert_eq!(sol.max_leverage, dec!(20));
        assert_eq!(sol.maintenance_margin_rate, dec!(0.025));
    }

    #[test]
    fn test_closed_position() {
        let pos = make_position(PositionSide::Long, dec!(0), dec!(50000), dec!(0));
        assert!(pos.is_closed());
        assert_eq!(pos.liquidation_price(), None);
    }

    // --- LeverageTier / PerpetualConfig tier lookup ---

    #[test]
    fn test_btc_tier0_small_position() {
        // Under 2M notional → tier 0: 100x leverage, 0.5% MMR
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(100000)), dec!(100));
        assert_eq!(cfg.effective_mmr(dec!(100000)), dec!(0.005));
    }

    #[test]
    fn test_btc_tier_boundary_at_2m() {
        // Exactly 2_000_000 → tier 1 starts: 50x leverage, 1.0% MMR
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(2000000)), dec!(50));
        assert_eq!(cfg.effective_mmr(dec!(2000000)), dec!(0.01));
    }

    #[test]
    fn test_btc_tier2_mid_range() {
        // 6M–12M → tier 2: 25x, 2.0% MMR
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(8000000)), dec!(25));
        assert_eq!(cfg.effective_mmr(dec!(8000000)), dec!(0.02));
    }

    #[test]
    fn test_btc_tier3() {
        // 12M–25M → tier 3: 15x, 2.5% MMR
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(20000000)), dec!(15));
        assert_eq!(cfg.effective_mmr(dec!(20000000)), dec!(0.025));
    }

    #[test]
    fn test_btc_tier4_large_position() {
        // ≥25M → tier 4: 10x, 5.0% MMR
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(50000000)), dec!(10));
        assert_eq!(cfg.effective_mmr(dec!(50000000)), dec!(0.05));
    }

    #[test]
    fn test_max_leverage_decreases_with_size() {
        // Verify monotonically decreasing leverage across BTC tiers
        let cfg = PerpetualConfig::btc_default();
        let small = cfg.max_leverage_for_notional(dec!(100000));
        let medium = cfg.max_leverage_for_notional(dec!(5000000));
        let large = cfg.max_leverage_for_notional(dec!(15000000));
        let huge = cfg.max_leverage_for_notional(dec!(30000000));
        assert!(small > medium);
        assert!(medium > large);
        assert!(large > huge);
    }

    #[test]
    fn test_mmr_increases_with_size() {
        // Verify monotonically increasing MMR across BTC tiers
        let cfg = PerpetualConfig::btc_default();
        let small = cfg.effective_mmr(dec!(100000));
        let medium = cfg.effective_mmr(dec!(5000000));
        let large = cfg.effective_mmr(dec!(15000000));
        let huge = cfg.effective_mmr(dec!(30000000));
        assert!(small < medium);
        assert!(medium < large);
        assert!(large < huge);
    }

    #[test]
    fn test_eth_tier0() {
        let cfg = PerpetualConfig::eth_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(500000)), dec!(50));
        assert_eq!(cfg.effective_mmr(dec!(500000)), dec!(0.01));
    }

    #[test]
    fn test_sol_tier0() {
        let cfg = PerpetualConfig::sol_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(100000)), dec!(20));
        assert_eq!(cfg.effective_mmr(dec!(100000)), dec!(0.025));
    }

    #[test]
    fn test_sol_tier1() {
        // 500k–2M → 10x, 5.0% MMR
        let cfg = PerpetualConfig::sol_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(1000000)), dec!(10));
        assert_eq!(cfg.effective_mmr(dec!(1000000)), dec!(0.05));
    }

    #[test]
    fn test_sol_tier2_huge_position() {
        // ≥2M → 5x, 10.0% MMR
        let cfg = PerpetualConfig::sol_default();
        assert_eq!(cfg.max_leverage_for_notional(dec!(5000000)), dec!(5));
        assert_eq!(cfg.effective_mmr(dec!(5000000)), dec!(0.1));
    }

    #[test]
    fn test_tier_zero_notional_uses_first_tier() {
        // Zero notional must hit the first tier (min_notional = 0, inclusive)
        let cfg = PerpetualConfig::btc_default();
        assert_eq!(cfg.max_leverage_for_notional(Decimal::ZERO), dec!(100));
        assert_eq!(cfg.effective_mmr(Decimal::ZERO), dec!(0.005));
    }

    // ─── P0-4: Funding debt tests ──────────────────────────────────────────

    #[test]
    fn test_unpaid_funding_debt_reduces_effective_margin() {
        // 1 BTC long at 50_000 entry with 5_000 margin, no price change.
        // Effective margin without debt = margin + unrealized_pnl = 5000 + 0 = 5000.
        // With 200 unpaid debt → effective = 4800.
        let mut pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        pos.unpaid_funding_debt = dec!(200);
        assert_eq!(pos.effective_margin(dec!(50000)), dec!(4800));
    }

    #[test]
    fn test_zero_unpaid_funding_debt_leaves_effective_margin_unchanged() {
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unpaid_funding_debt, Decimal::ZERO);
        // Price rises by 1000 → unrealized PnL = 1000.
        assert_eq!(pos.effective_margin(dec!(51000)), dec!(6000));
    }

    #[test]
    fn test_large_debt_can_make_effective_margin_negative() {
        // A position with debt larger than margin + PnL goes negative,
        // which is the signal the liquidation engine needs to act.
        let mut pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(1000));
        pos.unpaid_funding_debt = dec!(2000);
        // effective = 1000 + 0 - 2000 = -1000
        assert_eq!(pos.effective_margin(dec!(50000)), dec!(-1000));
    }

    #[test]
    fn test_unpaid_debt_accumulates_across_multiple_failures() {
        let mut pos = make_position(PositionSide::Short, dec!(2), dec!(50000), dec!(10000));
        pos.unpaid_funding_debt += dec!(100);
        pos.unpaid_funding_debt += dec!(150);
        pos.unpaid_funding_debt += dec!(75);
        assert_eq!(pos.unpaid_funding_debt, dec!(325));
    }

    #[test]
    fn test_funding_debt_new_position_always_starts_at_zero() {
        // Positions created by position_manager always set unpaid_funding_debt
        // to zero. This test verifies the field starts clean.
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(pos.unpaid_funding_debt, Decimal::ZERO,
            "new positions must have zero unpaid funding debt");
    }
}

