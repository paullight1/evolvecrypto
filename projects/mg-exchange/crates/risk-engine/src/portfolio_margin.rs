//! Portfolio margin: scenario-based stress testing and delta netting.
//!
//! # Overview
//!
//! Portfolio margin allows hedged positions to use less margin by netting risk
//! across correlated exposures.  Instead of computing margin per-position in
//! isolation, we ask: *what is the worst portfolio loss across a grid of price
//! shocks?*  That worst-case loss (multiplied by a safety factor) becomes the
//! required margin.
//!
//! # Scenario grid
//!
//! We apply six price shocks: ±5 %, ±10 %, and ±15 % from the current mark
//! price.  For each shock we compute the *aggregate* unrealised PnL across all
//! of the user's positions for that symbol.  This naturally captures delta
//! netting: a long BTCUSDT position and a short BTCUSD (inverse) will partially
//! cancel at each shock level.
//!
//! ```text
//! required_margin = abs(min(scenario_pnls)) * SAFETY_FACTOR
//! ```
//!
//! # Delta netting
//!
//! Delta is the sensitivity of the portfolio to a 1-unit move in the underlying.
//! For a linear perp: Δ = side_sign × size.
//! For an inverse perp: Δ_usd ≈ side_sign × size × contract_size / mark_price².
//! We report net delta per *base asset* (e.g., "BTC").

use std::collections::HashMap;

use rust_decimal::Decimal;
use tracing::debug;

use exchange_common::futures::{ContractType, PerpetualConfig, Position, PositionSide};
use exchange_common::types::Symbol;

/// Safety multiplier applied to the worst-case scenario loss to arrive at the
/// required portfolio margin.
///
/// 1.2 = 20 % buffer on top of the stress-tested loss.
pub const PORTFOLIO_MARGIN_SAFETY_FACTOR: Decimal = Decimal::from_parts(12, 0, 0, false, 1); // 1.2

/// Shock percentages applied symmetrically around the mark price.
///
/// Expressed as fractions (not percentages): 0.05 = 5 %.
pub const SCENARIO_SHOCKS: &[Decimal] = &[
    Decimal::from_parts(5, 0, 0, false, 2),   // +5%
    Decimal::from_parts(5, 0, 0, true, 2),    // -5%
    Decimal::from_parts(10, 0, 0, false, 2),  // +10%
    Decimal::from_parts(10, 0, 0, true, 2),   // -10%
    Decimal::from_parts(15, 0, 0, false, 2),  // +15%
    Decimal::from_parts(15, 0, 0, true, 2),   // -15%
];

/// PnL of the entire portfolio under a single price shock scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Shock fraction applied to all mark prices (e.g., 0.05 = +5 %, -0.10 = -10 %).
    pub shock_pct: Decimal,
    /// Signed aggregate portfolio PnL under this scenario (USDT-equivalent).
    pub portfolio_pnl: Decimal,
}

/// Full portfolio-margin analysis for a single user.
#[derive(Debug, Clone)]
pub struct PortfolioMarginAnalysis {
    /// Per-scenario stress test results (6 entries for ±5 %, ±10 %, ±15 %).
    pub scenarios: Vec<ScenarioResult>,
    /// Net delta per base asset (e.g., "BTC" → net BTC delta across all positions).
    pub net_delta: HashMap<String, Decimal>,
    /// Required portfolio margin = abs(worst_pnl) × safety_factor.
    pub required_margin: Decimal,
    /// The worst-case PnL scenario (most negative value).
    pub worst_pnl: Decimal,
}

/// Calculate portfolio-margin analysis for a user.
///
/// # Arguments
/// * `positions` — all open positions for this user.
/// * `mark_prices` — current mark price per symbol (used as the shock base).
/// * `configs` — perpetual contract configs (provides contract type and size).
///
/// # Returns
/// [`PortfolioMarginAnalysis`] with scenario results, net delta, and required margin.
///
/// If the user has no open positions or no mark prices are available, returns an
/// analysis with zero required margin.
pub fn analyse(
    positions: &[&Position],
    mark_prices: &HashMap<Symbol, Decimal>,
    configs: &HashMap<Symbol, PerpetualConfig>,
) -> PortfolioMarginAnalysis {
    if positions.is_empty() {
        return PortfolioMarginAnalysis {
            scenarios: build_zero_scenarios(),
            net_delta: HashMap::new(),
            required_margin: Decimal::ZERO,
            worst_pnl: Decimal::ZERO,
        };
    }

    // --- Scenario stress test ---

    let scenarios: Vec<ScenarioResult> = SCENARIO_SHOCKS
        .iter()
        .map(|&shock| {
            let portfolio_pnl = positions.iter().fold(Decimal::ZERO, |acc, pos| {
                let base_price = match mark_prices.get(&pos.symbol) {
                    Some(&p) if p > Decimal::ZERO => p,
                    _ => return acc, // no mark price — skip this position
                };

                let shocked_price = base_price * (Decimal::ONE + shock);
                if shocked_price <= Decimal::ZERO {
                    return acc;
                }

                let config = configs.get(&pos.symbol);
                let contract_type = config
                    .map(|c| c.contract_type)
                    .unwrap_or(ContractType::Linear);

                let pnl = match contract_type {
                    ContractType::Linear => pos.unrealized_pnl(shocked_price),
                    ContractType::Inverse => {
                        // Inverse PnL is in base asset; convert to USDT using shocked price.
                        let pnl_base = pos.unrealized_pnl_inverse(shocked_price);
                        pnl_base * shocked_price
                    }
                };

                acc + pnl
            });

            ScenarioResult { shock_pct: shock, portfolio_pnl }
        })
        .collect();

    // Worst case = most negative portfolio PnL.
    let worst_pnl = scenarios
        .iter()
        .map(|s| s.portfolio_pnl)
        .fold(Decimal::ZERO, |worst, pnl| if pnl < worst { pnl } else { worst });

    let required_margin = if worst_pnl < Decimal::ZERO {
        worst_pnl.abs() * PORTFOLIO_MARGIN_SAFETY_FACTOR
    } else {
        Decimal::ZERO
    };

    debug!(
        worst_pnl = %worst_pnl,
        required_margin = %required_margin,
        "portfolio margin analysis complete"
    );

    // --- Delta netting ---

    let net_delta = compute_net_delta(positions, mark_prices, configs);

    PortfolioMarginAnalysis {
        scenarios,
        net_delta,
        required_margin,
        worst_pnl,
    }
}

/// Calculate the required portfolio margin for a user (scalar convenience wrapper).
///
/// Returns the required margin value directly; useful in hot-path margin checks.
pub fn required_portfolio_margin(
    positions: &[&Position],
    mark_prices: &HashMap<Symbol, Decimal>,
    configs: &HashMap<Symbol, PerpetualConfig>,
) -> Decimal {
    analyse(positions, mark_prices, configs).required_margin
}

/// Compute net delta per base asset across all positions.
///
/// Delta semantics:
/// - **Linear**: Δ = side_sign × size   (in base asset units, e.g. BTC)
/// - **Inverse**: Δ = side_sign × contracts × contract_size / mark_price²
///   This is the USD-denominated price sensitivity expressed per USD of underlying.
///   We convert to base-asset delta by multiplying by 1/mark_price so the result
///   is in base-asset units (matching linear).
///
/// The base asset is extracted from the symbol name: "BTCUSDT" → "BTC",
/// "BTCUSD" → "BTC".  We strip known quote suffixes (USDT, USDC, USD, BUSD).
pub fn compute_net_delta(
    positions: &[&Position],
    mark_prices: &HashMap<Symbol, Decimal>,
    configs: &HashMap<Symbol, PerpetualConfig>,
) -> HashMap<String, Decimal> {
    let mut net_delta: HashMap<String, Decimal> = HashMap::new();

    for pos in positions {
        let base_asset = extract_base_asset(pos.symbol.as_str());

        let config = configs.get(&pos.symbol);
        let contract_type = config
            .map(|c| c.contract_type)
            .unwrap_or(ContractType::Linear);
        let contract_size = config
            .map(|c| c.contract_size)
            .unwrap_or(Decimal::ONE);

        let sign = match pos.side {
            PositionSide::Long => Decimal::ONE,
            PositionSide::Short => Decimal::NEGATIVE_ONE,
        };

        let delta = match contract_type {
            ContractType::Linear => {
                // Linear: size is already in base asset units.
                sign * pos.size
            }
            ContractType::Inverse => {
                // Inverse: size is number of contracts; each worth `contract_size` USD.
                // Delta = dPnL_base / dPrice ≈ -size * contract_size / price²
                // For delta netting purposes we use the simplified DV01-like exposure:
                //   delta_base = sign * contracts * contract_size / mark_price
                // This gives the equivalent number of base-asset units of exposure.
                let mark = mark_prices
                    .get(&pos.symbol)
                    .copied()
                    .filter(|&p| p > Decimal::ZERO)
                    .unwrap_or(Decimal::ONE);
                sign * pos.size * contract_size / mark
            }
        };

        *net_delta.entry(base_asset).or_insert(Decimal::ZERO) += delta;
    }

    net_delta
}

/// Extract the base asset from a symbol name.
///
/// Strips common quote suffixes: USDT, USDC, BUSD, USD, BTC, ETH (as quote).
/// Examples:
/// - "BTCUSDT" → "BTC"
/// - "BTCUSD"  → "BTC"
/// - "ETHUSDT" → "ETH"
/// - "SOLUSDT" → "SOL"
fn extract_base_asset(symbol: &str) -> String {
    // Ordered longest-first so "USDT" is tried before "USD".
    const QUOTE_SUFFIXES: &[&str] = &["USDT", "USDC", "BUSD", "USD", "BTC", "ETH"];

    for suffix in QUOTE_SUFFIXES {
        if let Some(base) = symbol.strip_suffix(suffix) {
            if !base.is_empty() {
                return base.to_string();
            }
        }
    }
    // Fallback: return the full symbol.
    symbol.to_string()
}

/// Build a zero-PnL scenario list (used when there are no positions).
fn build_zero_scenarios() -> Vec<ScenarioResult> {
    SCENARIO_SHOCKS
        .iter()
        .map(|&shock| ScenarioResult {
            shock_pct: shock,
            portfolio_pnl: Decimal::ZERO,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Notional bracket helpers
// ---------------------------------------------------------------------------

/// A single notional bracket defining leverage and margin requirements.
///
/// Brackets are the canonical representation returned by
/// `GET /market/risk-limit/{symbol}`.  They are derived from the
/// [`exchange_common::futures::LeverageTier`] schedule stored in
/// [`PerpetualConfig`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotionalBracket {
    /// Tier index, starting at 1 (for API display).
    pub id: u32,
    /// Lower bound of position notional this bracket covers (inclusive, USD).
    pub min_notional: Decimal,
    /// Upper bound of position notional this bracket covers (exclusive, USD).
    /// `None` indicates the final (unbounded) bracket.
    pub max_notional: Option<Decimal>,
    /// Maximum leverage permitted in this bracket.
    pub max_leverage: Decimal,
    /// Maintenance margin rate required in this bracket (e.g., 0.005 = 0.5 %).
    pub maintenance_margin_rate: Decimal,
    /// Initial margin rate = 1 / max_leverage.
    pub initial_margin_rate: Decimal,
}

/// Build the full bracket table for a symbol from its `PerpetualConfig`.
///
/// Converts internal [`LeverageTier`] entries into [`NotionalBracket`] records
/// with human-readable IDs and cleaned-up `max_notional` (capping `Decimal::MAX`
/// to `None`).
pub fn brackets_for_config(config: &PerpetualConfig) -> Vec<NotionalBracket> {
    config
        .tiers
        .iter()
        .enumerate()
        .map(|(i, tier)| {
            let initial_margin_rate = if tier.max_leverage.is_zero() {
                Decimal::ONE
            } else {
                Decimal::ONE / tier.max_leverage
            };

            let max_notional = if tier.max_notional == Decimal::MAX {
                None
            } else {
                Some(tier.max_notional)
            };

            NotionalBracket {
                id: (i + 1) as u32,
                min_notional: tier.min_notional,
                max_notional,
                max_leverage: tier.max_leverage,
                maintenance_margin_rate: tier.maintenance_margin_rate,
                initial_margin_rate,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::futures::{ContractType, LeverageTier, MarginMode, PerpetualConfig, PositionIdx, PositionSide};
    use exchange_common::types::Symbol;
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use chrono::Utc;

    fn make_position(
        symbol: &str,
        side: PositionSide,
        size: Decimal,
        entry_price: Decimal,
        margin: Decimal,
    ) -> Position {
        Position {
            user_id: Uuid::nil(),
            symbol: Symbol::new(symbol),
            side,
            size,
            entry_price,
            leverage: dec!(10),
            margin_mode: MarginMode::Cross,
            margin,
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            position_idx: 0 as PositionIdx,
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

    fn btc_config() -> PerpetualConfig {
        PerpetualConfig::btc_default()
    }

    fn btcusd_inverse_config() -> PerpetualConfig {
        PerpetualConfig::btcusd_inverse_default()
    }

    // --- extract_base_asset ---

    #[test]
    fn test_extract_base_asset_linear() {
        assert_eq!(extract_base_asset("BTCUSDT"), "BTC");
        assert_eq!(extract_base_asset("ETHUSDT"), "ETH");
        assert_eq!(extract_base_asset("SOLUSDT"), "SOL");
    }

    #[test]
    fn test_extract_base_asset_inverse() {
        assert_eq!(extract_base_asset("BTCUSD"), "BTC");
        assert_eq!(extract_base_asset("ETHUSD"), "ETH");
    }

    #[test]
    fn test_extract_base_asset_unknown() {
        // Unknown suffix: returned unchanged.
        assert_eq!(extract_base_asset("XYZABC"), "XYZABC");
    }

    // --- scenario stress test (unhedged long) ---

    #[test]
    fn test_scenario_long_btc_worst_case_is_down_shock() {
        // Long 1 BTC at 50_000, mark = 50_000.
        // At -15% shock: price = 42_500, PnL = -7_500.
        let pos = make_position("BTCUSDT", PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let positions = vec![&pos];

        let mut mark_prices = HashMap::new();
        mark_prices.insert(Symbol::new("BTCUSDT"), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(Symbol::new("BTCUSDT"), btc_config());

        let analysis = analyse(&positions, &mark_prices, &configs);

        // Worst scenario should be -15% (most negative PnL for a long).
        assert_eq!(analysis.worst_pnl, dec!(-7500));

        // Required margin = 7_500 * 1.2 = 9_000.
        assert_eq!(analysis.required_margin, dec!(9000));

        // +15% scenario: PnL = +7_500.
        let best = analysis
            .scenarios
            .iter()
            .find(|s| s.shock_pct == dec!(0.15))
            .expect("+15% scenario missing");
        assert_eq!(best.portfolio_pnl, dec!(7500));
    }

    // --- scenario stress test (unhedged short) ---

    #[test]
    fn test_scenario_short_btc_worst_case_is_up_shock() {
        // Short 1 BTC at 50_000, mark = 50_000.
        // At +15%: price = 57_500, PnL = -(57500 - 50000) = -7_500.
        let pos = make_position("BTCUSDT", PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        let positions = vec![&pos];

        let mut mark_prices = HashMap::new();
        mark_prices.insert(Symbol::new("BTCUSDT"), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(Symbol::new("BTCUSDT"), btc_config());

        let analysis = analyse(&positions, &mark_prices, &configs);

        assert_eq!(analysis.worst_pnl, dec!(-7500));
        assert_eq!(analysis.required_margin, dec!(9000));
    }

    // --- delta netting: hedged linear long + inverse short ---

    #[test]
    fn test_hedged_positions_reduce_required_margin() {
        // Long 1 BTCUSDT (linear) at 50_000.
        // Short 1 BTCUSD (inverse, 1-contract-per-USD) at 50_000.
        //
        // Under any price shock the gains on one side partially offset losses on
        // the other — the required portfolio margin is less than for either alone.
        let long_pos = make_position("BTCUSDT", PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let short_pos = make_position("BTCUSD", PositionSide::Short, dec!(50000), dec!(50000), dec!(1));

        let positions: Vec<&Position> = vec![&long_pos, &short_pos];

        let mut mark_prices = HashMap::new();
        mark_prices.insert(Symbol::new("BTCUSDT"), dec!(50000));
        mark_prices.insert(Symbol::new("BTCUSD"), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(Symbol::new("BTCUSDT"), btc_config());
        configs.insert(Symbol::new("BTCUSD"), btcusd_inverse_config());

        let analysis = analyse(&positions, &mark_prices, &configs);

        // Unhedged long alone → required = 9_000.
        // With the inverse short the required margin must be lower.
        assert!(
            analysis.required_margin < dec!(9000),
            "hedged margin {} should be less than unhedged 9000",
            analysis.required_margin
        );
    }

    // --- portfolio margin vs isolated margin comparison ---

    #[test]
    fn test_portfolio_margin_less_than_isolated_for_long_short_pair() {
        // Two positions on the same underlying, opposite sides.
        let long_pos = make_position("BTCUSDT", PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let short_pos = make_position("BTCUSDT", PositionSide::Short, dec!(1), dec!(50000), dec!(5000));

        let positions: Vec<&Position> = vec![&long_pos, &short_pos];

        let mut mark_prices = HashMap::new();
        mark_prices.insert(Symbol::new("BTCUSDT"), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(Symbol::new("BTCUSDT"), btc_config());

        let analysis = analyse(&positions, &mark_prices, &configs);

        // Long and short cancel exactly → all scenario PnLs = 0 → required margin = 0.
        assert_eq!(analysis.required_margin, Decimal::ZERO);
        assert_eq!(analysis.worst_pnl, Decimal::ZERO);
    }

    // --- net delta ---

    #[test]
    fn test_net_delta_long_only() {
        let pos = make_position("BTCUSDT", PositionSide::Long, dec!(2), dec!(50000), dec!(10000));
        let positions = vec![&pos];

        let mark_prices = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), dec!(50000));
            m
        };
        let configs = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), btc_config());
            m
        };

        let delta = compute_net_delta(&positions, &mark_prices, &configs);
        assert_eq!(delta.get("BTC").copied().unwrap_or_default(), dec!(2));
    }

    #[test]
    fn test_net_delta_long_short_cancel() {
        let long_pos = make_position("BTCUSDT", PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let short_pos = make_position("BTCUSDT", PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        let positions: Vec<&Position> = vec![&long_pos, &short_pos];

        let mark_prices = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), dec!(50000));
            m
        };
        let configs = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), btc_config());
            m
        };

        let delta = compute_net_delta(&positions, &mark_prices, &configs);
        assert_eq!(delta.get("BTC").copied().unwrap_or_default(), Decimal::ZERO);
    }

    #[test]
    fn test_net_delta_multi_asset() {
        let btc_pos = make_position("BTCUSDT", PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let eth_pos = make_position("ETHUSDT", PositionSide::Short, dec!(5), dec!(3000), dec!(1500));
        let positions: Vec<&Position> = vec![&btc_pos, &eth_pos];

        let mark_prices = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), dec!(50000));
            m.insert(Symbol::new("ETHUSDT"), dec!(3000));
            m
        };
        let configs = {
            let mut m = HashMap::new();
            m.insert(Symbol::new("BTCUSDT"), btc_config());
            m.insert(Symbol::new("ETHUSDT"), PerpetualConfig::eth_default());
            m
        };

        let delta = compute_net_delta(&positions, &mark_prices, &configs);
        assert_eq!(delta.get("BTC").copied().unwrap_or_default(), dec!(1));
        assert_eq!(delta.get("ETH").copied().unwrap_or_default(), dec!(-5));
    }

    // --- empty portfolio ---

    #[test]
    fn test_empty_portfolio_zero_margin() {
        let analysis = analyse(&[], &HashMap::new(), &HashMap::new());
        assert_eq!(analysis.required_margin, Decimal::ZERO);
        assert_eq!(analysis.worst_pnl, Decimal::ZERO);
        assert_eq!(analysis.scenarios.len(), SCENARIO_SHOCKS.len());
        assert!(analysis.net_delta.is_empty());
    }

    // --- notional brackets ---

    #[test]
    fn test_brackets_for_config_btc() {
        let config = btc_config();
        let brackets = brackets_for_config(&config);

        // BTC has 5 tiers.
        assert_eq!(brackets.len(), 5);

        // First bracket: min=0, max=2_000_000, max_leverage=100, mmr=0.5%.
        let first = &brackets[0];
        assert_eq!(first.id, 1);
        assert_eq!(first.min_notional, dec!(0));
        assert_eq!(first.max_notional, Some(dec!(2000000)));
        assert_eq!(first.max_leverage, dec!(100));
        assert_eq!(first.maintenance_margin_rate, dec!(0.005));
        assert_eq!(first.initial_margin_rate, dec!(0.01)); // 1/100

        // Last bracket: unbounded → max_notional = None.
        let last = brackets.last().expect("at least one bracket");
        assert!(last.max_notional.is_none());
    }

    #[test]
    fn test_brackets_initial_margin_rate_correct() {
        let config = btc_config();
        let brackets = brackets_for_config(&config);
        for bracket in &brackets {
            let expected_imr = Decimal::ONE / bracket.max_leverage;
            assert_eq!(
                bracket.initial_margin_rate, expected_imr,
                "IMR mismatch for bracket id {}",
                bracket.id
            );
        }
    }
}
