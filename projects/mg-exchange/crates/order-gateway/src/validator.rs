use exchange_common::*;
use rust_decimal::Decimal;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Order limit configuration (KG-2, Sprint 8)
// ---------------------------------------------------------------------------

/// Per-user open-order caps.
///
/// These are the primary DoS mitigation for the T1.5 STP filter which runs in
/// O(orders_at_level) per FOK check. `max_orders_per_user_per_price_level`
/// bounds PER-USER scan cost in the STP filter at
/// `crates/matching-engine/src/orderbook.rs` to 50 iterations. Note: this is
/// a per-user bound, NOT a per-level total. Per-level total worst case is
/// `num_users_at_level × 50`.
///
/// # References
/// - Bybit open order limits: "Maximum number of open orders: 500 per symbol
///   (linear/inverse)" — https://bybit-exchange.github.io/docs/v5/order/create-order
/// - Binance MAX_NUM_ORDERS filter: per-symbol cap (default 200 open orders) —
///   https://developers.binance.com/docs/binance-spot-api-docs/filters#max-num-orders
/// - OKX: tiered by account level — we use a flat cap matching Bybit.
///
/// Caps are configurable so tests can use small values (e.g. 3 / 2) to exercise
/// the rejection paths without placing hundreds of orders.
#[derive(Debug, Clone)]
pub struct OrderLimitConfig {
    /// Max open orders per user per symbol. Default 500 (matches Bybit).
    ///
    /// Scope: per-symbol (not global across all symbols).  Two users on the
    /// same symbol have independent quotas.  One user on two different symbols
    /// has independent quotas per symbol.
    ///
    /// Source of truth: `OrderBook::user_open_order_count()` in
    /// `crates/matching-engine/src/orderbook.rs` — the per-user index
    /// maintained by `insert_order` / `cancel_order` / `match_order`.
    pub max_open_orders_per_user_per_symbol: usize,

    /// Maximum orders a single user can place at one specific price level on
    /// one symbol. Default: 50.
    ///
    /// **Security role:** this is the per-user bound on the STP filter cost
    /// in `can_fill_fully`. Per-level total is bounded only by
    /// (num_users_at_level × this_cap).
    ///
    /// Source of truth: `OrderBook::user_orders_at_price()` in
    /// `crates/matching-engine/src/orderbook.rs`.
    pub max_orders_per_user_per_price_level: usize,

    /// Maximum TOTAL open orders per user across ALL symbols. Default: 5000.
    ///
    /// **Security role:** bounds per-user memory footprint for order state.
    /// Without this cap, a user with high per-symbol limits can accumulate
    /// `max_open_orders_per_user_per_symbol × num_symbols` open orders
    /// (e.g. 500 × 20 = 10,000 orders ≈ 3 MB per user). At scale this is a
    /// memory exhaustion vector. KG-7 closed in Sprint 10.
    ///
    /// **Implementation note (Option A — sum at query time):** the check is
    /// implemented by summing per-symbol counts from the engine pool.  Cost is
    /// O(N_symbols) with one engine round-trip each (~50 µs for 20 symbols).
    /// This is acceptable because validation does NOT run on the matching hot
    /// path.  If profiling later shows N_symbols > 100 and this becomes a
    /// bottleneck, migrate to Option B (central `AtomicUsize` per user).
    ///
    /// The check runs AFTER the per-symbol cap (Step 8a) so that a cheaper
    /// per-symbol rejection avoids the cross-symbol sum entirely.
    pub max_open_orders_per_user_total: usize,
}

impl Default for OrderLimitConfig {
    fn default() -> Self {
        Self {
            max_open_orders_per_user_per_symbol: 500,
            max_orders_per_user_per_price_level: 50,
            max_open_orders_per_user_total: 5000,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait for querying open-order counts (KG-2, Sprint 8)
// ---------------------------------------------------------------------------

/// Abstraction over the order book state needed for limit checks.
///
/// Implemented by the `EnginePool` in production. A mock implementation is
/// used in unit tests to avoid requiring a running engine thread.
pub trait OrderCountSource {
    /// Return the number of open orders for `user_id` on `symbol`.
    /// Returns 0 if the symbol has no engine or the user has no orders.
    fn user_open_order_count(&self, symbol: &Symbol, user_id: UserId) -> usize;

    /// Return the number of open orders for `user_id` at `price` on `side` for `symbol`.
    /// Returns 0 if the symbol/price/user has no orders.
    fn user_orders_at_price(
        &self,
        symbol: &Symbol,
        user_id: UserId,
        side: Side,
        price: Price,
    ) -> usize;

    /// Return the TOTAL number of open orders for `user_id` across ALL symbols
    /// registered in the system.
    ///
    /// **Option A implementation:** implementations MAY sum per-symbol counts
    /// (O(N_symbols) round-trips, ~50 µs for 20 symbols).  This is acceptable
    /// for the validation path which does not sit on the matching hot path.
    /// If O(N_symbols) becomes a bottleneck at 100+ symbols, migrate callers to
    /// Option B (central atomic counter updated on every insert/cancel/fill).
    ///
    /// Returns 0 if no engine pool is available or the user has no orders.
    fn user_total_open_order_count(&self, user_id: UserId) -> usize;
}

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Symbol not registered: {0}")]
    UnknownSymbol(String),

    #[error("Price must be positive for limit orders, got {0}")]
    InvalidPrice(Decimal),

    #[error("Quantity must be positive, got {0}")]
    InvalidQuantity(Decimal),

    #[error("Price {price} not aligned to tick size {tick_size}")]
    PriceNotOnTick { price: Decimal, tick_size: Decimal },

    #[error("Quantity {qty} not aligned to lot size {lot_size}")]
    QtyNotOnLot { qty: Decimal, lot_size: Decimal },

    #[error("Quantity {qty} exceeds maximum {max}")]
    ExceedsMaxQty { qty: Decimal, max: Decimal },

    #[error("Notional {notional} exceeds maximum {max}")]
    ExceedsMaxNotional { notional: Decimal, max: Decimal },

    #[error("Order notional {notional} below minimum {min_notional}")]
    BelowMinNotional { notional: Decimal, min_notional: Decimal },

    #[error("Market order cannot be PostOnly")]
    MarketPostOnly,

    #[error("Market order cannot be FOK")]
    MarketFok,

    #[error("Self-trade would occur")]
    SelfTrade,

    /// Reduce-only order rejected because no opposite-side position exists.
    #[error("Reduce-only order rejected: no open position to reduce")]
    ReduceOnlyNoPosition,

    /// Reduce-only order rejected because `qty` exceeds the open position size.
    #[error("Reduce-only order qty {qty} exceeds open position size {position_size}")]
    ReduceOnlyExceedsPosition { qty: Decimal, position_size: Decimal },

    /// Per-symbol open order cap exceeded (HTTP 429).
    ///
    /// Raised when the user already has `limit` open orders on `symbol` and
    /// attempts to place one more.  Callers map this to HTTP 429.
    #[error(
        "Max open orders per symbol reached: {current}/{limit} for symbol {symbol}"
    )]
    MaxOpenOrdersPerSymbol {
        symbol: Symbol,
        current: usize,
        limit: usize,
    },

    /// Per-price-level open order cap exceeded (HTTP 429).
    ///
    /// Raised when the user already has `limit` open orders at `price` on `symbol`
    /// and attempts to place one more at the same price.  Callers map this to HTTP 429.
    #[error(
        "Max orders per price level reached: {current}/{limit} for symbol {symbol} at price {price}"
    )]
    MaxOrdersPerPriceLevel {
        symbol: Symbol,
        price: Price,
        current: usize,
        limit: usize,
    },

    /// Cross-symbol total open order cap exceeded (HTTP 429).
    ///
    /// Raised when the user's total open orders across ALL symbols reaches
    /// `limit`. Callers map this to HTTP 429.  KG-7 closed in Sprint 10.
    #[error("Max total open orders per user reached: {current}/{limit}")]
    MaxOpenOrdersPerUserTotal { current: usize, limit: usize },
}

/// Returns `true` if this `ValidationError` should be reported as HTTP 429
/// (Too Many Requests) rather than HTTP 400 (Bad Request).
///
/// Binance uses 429 for order-count limit violations.
/// Reference: https://developers.binance.com/docs/binance-spot-api-docs/filters#max-num-orders
impl ValidationError {
    pub fn is_rate_limit(&self) -> bool {
        matches!(
            self,
            ValidationError::MaxOpenOrdersPerSymbol { .. }
                | ValidationError::MaxOrdersPerPriceLevel { .. }
                | ValidationError::MaxOpenOrdersPerUserTotal { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Validator
// ---------------------------------------------------------------------------

/// Pre-trade validation for incoming orders.
///
/// Validates against product specification BEFORE balance lock.
/// This is the first line of defense — reject obviously invalid orders
/// without touching balance or matching engine.
pub struct OrderValidator;

impl OrderValidator {
    /// Validate an order command against a product's rules (product-level checks only).
    ///
    /// This overload does NOT check open-order counts — it is called in contexts
    /// where the engine pool is not accessible (e.g. batch validation before lock).
    /// Use `validate_with_counts` when you have access to an `OrderCountSource`.
    pub fn validate(cmd: &PlaceOrderCommand, product: &Product) -> Result<(), ValidationError> {
        // 1. Quantity must be positive
        if cmd.qty <= Decimal::ZERO {
            return Err(ValidationError::InvalidQuantity(cmd.qty));
        }

        // 2. Lot size alignment
        if !is_aligned(cmd.qty, product.lot_size) {
            return Err(ValidationError::QtyNotOnLot {
                qty: cmd.qty,
                lot_size: product.lot_size,
            });
        }

        match cmd.order_type {
            OrderType::Limit => {
                // 3. Price must be positive for limit orders
                if cmd.price <= Decimal::ZERO {
                    return Err(ValidationError::InvalidPrice(cmd.price));
                }

                // 4. Tick size alignment
                if !is_aligned(cmd.price, product.tick_size) {
                    return Err(ValidationError::PriceNotOnTick {
                        price: cmd.price,
                        tick_size: product.tick_size,
                    });
                }

                // 5. Minimum notional (price * qty)
                let notional = cmd.price * cmd.qty;
                if notional < product.min_notional {
                    return Err(ValidationError::BelowMinNotional {
                        notional,
                        min_notional: product.min_notional,
                    });
                }
            }
            OrderType::Market => {
                // Market orders: no price check, but some TIF combos are invalid
                if cmd.time_in_force == TimeInForce::PostOnly {
                    return Err(ValidationError::MarketPostOnly);
                }
                if cmd.time_in_force == TimeInForce::Fok {
                    return Err(ValidationError::MarketFok);
                }
            }
        }

        // 6. Max order quantity
        if let Some(max_qty) = product.max_order_qty {
            if cmd.qty > max_qty {
                return Err(ValidationError::ExceedsMaxQty { qty: cmd.qty, max: max_qty });
            }
        }

        // 7. Max order notional
        if let Some(max_notional) = product.max_order_notional {
            let notional = cmd.price * cmd.qty;
            if notional > max_notional {
                return Err(ValidationError::ExceedsMaxNotional { notional, max: max_notional });
            }
        }

        Ok(())
    }

    /// Validate an order against product rules AND per-user open-order caps.
    ///
    /// Runs all checks from `validate` first (product-level), then queries
    /// `counts` for per-user counts.  Callers must pass a live `OrderCountSource`
    /// (the `EnginePool` in production) and an `OrderLimitConfig`.
    ///
    /// Order of checks (fail-fast):
    ///   1–7: product rules (qty, tick, lot, notional, TIF combos)
    ///   8: per-symbol open order cap
    ///   9: per-price-level open order cap (limit orders only)
    ///
    /// Checks 8–9 use the source of truth in the matching engine book, NOT an
    /// approximate counter.  This means the check is consistent with the actual
    /// book state at the moment of validation.
    ///
    /// Note: there is a narrow TOCTOU window between the count read and the
    /// matching engine accepting the order (concurrent placements on different
    /// REST connections).  This is acceptable per `systems-qa` review because:
    ///  (a) the engine is single-threaded per symbol — the count read goes
    ///      through the engine's command channel and reflects the exact book
    ///      state at that moment;
    ///  (b) REST handlers call `spawn_blocking` which serialises with the
    ///      engine thread per-symbol, so the window only exists when two users
    ///      both approach the limit simultaneously — not a DoS vector.
    pub fn validate_with_counts(
        cmd: &PlaceOrderCommand,
        product: &Product,
        counts: &dyn OrderCountSource,
        limits: &OrderLimitConfig,
    ) -> Result<(), ValidationError> {
        // Steps 1–7: product-level validation.
        Self::validate(cmd, product)?;

        // Step 8: per-symbol open order cap.
        let current_symbol =
            counts.user_open_order_count(&cmd.symbol, cmd.user_id);
        if current_symbol >= limits.max_open_orders_per_user_per_symbol {
            return Err(ValidationError::MaxOpenOrdersPerSymbol {
                symbol: cmd.symbol.clone(),
                current: current_symbol,
                limit: limits.max_open_orders_per_user_per_symbol,
            });
        }

        // Step 8a: cross-symbol total open order cap (KG-7, Sprint 10).
        //
        // Runs AFTER the per-symbol check so a cheap O(1) rejection avoids
        // triggering the O(N_symbols) sum.  Uses Option A (sum across pool at
        // query time): for 20 symbols this is ~20 engine round-trips (~50 µs),
        // acceptable for the validation path which is outside the matching hot
        // path.  See `OrderLimitConfig::max_open_orders_per_user_total` docs for
        // the Option A vs B trade-off discussion.
        let current_total = counts.user_total_open_order_count(cmd.user_id);
        if current_total >= limits.max_open_orders_per_user_total {
            return Err(ValidationError::MaxOpenOrdersPerUserTotal {
                current: current_total,
                limit: limits.max_open_orders_per_user_total,
            });
        }

        // Step 9: per-price-level cap (only relevant for limit orders that will
        // rest on the book; market orders never rest).
        if cmd.order_type == OrderType::Limit {
            let current_level =
                counts.user_orders_at_price(&cmd.symbol, cmd.user_id, cmd.side, cmd.price);
            if current_level >= limits.max_orders_per_user_per_price_level {
                return Err(ValidationError::MaxOrdersPerPriceLevel {
                    symbol: cmd.symbol.clone(),
                    price: cmd.price,
                    current: current_level,
                    limit: limits.max_orders_per_user_per_price_level,
                });
            }
        }

        Ok(())
    }
}

/// Validate a reduce-only constraint against the caller-supplied position size.
///
/// This is a separate function because the validator itself has no access to live
/// position state — the caller (order gateway handler) must fetch the current
/// position size and pass it in.
///
/// # Arguments
/// * `cmd` — the incoming place-order command (must have `reduce_only == true`).
/// * `position_size` — current size of the open position on the **opposite** side to
///   `cmd.side`, or `None` if no such position exists.
///
/// # Errors
/// - [`ValidationError::ReduceOnlyNoPosition`] — `position_size` is `None` or zero.
/// - [`ValidationError::ReduceOnlyExceedsPosition`] — `cmd.qty > position_size`.
pub fn validate_reduce_only(
    cmd: &PlaceOrderCommand,
    position_size: Option<types::Qty>,
) -> Result<(), ValidationError> {
    match position_size {
        None => Err(ValidationError::ReduceOnlyNoPosition),
        Some(size) if size.is_zero() => Err(ValidationError::ReduceOnlyNoPosition),
        Some(size) if cmd.qty > size => Err(ValidationError::ReduceOnlyExceedsPosition {
            qty: cmd.qty,
            position_size: size,
        }),
        Some(_) => Ok(()),
    }
}

/// Check if value is aligned to step (value % step == 0).
fn is_aligned(value: Decimal, step: Decimal) -> bool {
    if step.is_zero() {
        return true;
    }
    (value % step).is_zero()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn btc_product() -> Product {
        Product {
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
        }
    }

    fn make_cmd(
        side: Side,
        order_type: OrderType,
        tif: TimeInForce,
        price: Decimal,
        qty: Decimal,
    ) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type,
            time_in_force: tif,
            price,
            qty,
            timestamp: chrono::Utc::now(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        }
    }

    #[test]
    fn test_valid_limit_order() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000.00), dec!(0.001));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());
    }

    #[test]
    fn test_valid_market_order() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Market, TimeInForce::Gtc, dec!(0), dec!(1));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());
    }

    #[test]
    fn test_zero_qty_rejected() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(0));
        assert_eq!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::InvalidQuantity(dec!(0))
        );
    }

    #[test]
    fn test_negative_qty_rejected() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(-1));
        assert_eq!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::InvalidQuantity(dec!(-1))
        );
    }

    #[test]
    fn test_zero_price_limit_rejected() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(0), dec!(1));
        assert_eq!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::InvalidPrice(dec!(0))
        );
    }

    #[test]
    fn test_price_not_on_tick() {
        let product = btc_product(); // tick_size = 0.01
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000.005), dec!(1));
        assert!(matches!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::PriceNotOnTick { .. }
        ));
    }

    #[test]
    fn test_qty_not_on_lot() {
        let product = btc_product(); // lot_size = 0.00001
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(0.000001));
        assert!(matches!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::QtyNotOnLot { .. }
        ));
    }

    #[test]
    fn test_below_min_notional() {
        let product = btc_product(); // min_notional = 10
        // 50000 * 0.00001 = 0.5 USDT < 10
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(0.00001));
        assert!(matches!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::BelowMinNotional { .. }
        ));
    }

    #[test]
    fn test_market_post_only_rejected() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Market, TimeInForce::PostOnly, dec!(0), dec!(1));
        assert_eq!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::MarketPostOnly
        );
    }

    #[test]
    fn test_market_fok_rejected() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Market, TimeInForce::Fok, dec!(0), dec!(1));
        assert_eq!(
            OrderValidator::validate(&cmd, &product).unwrap_err(),
            ValidationError::MarketFok
        );
    }

    #[test]
    fn test_market_ioc_allowed() {
        let product = btc_product();
        let cmd = make_cmd(Side::Buy, OrderType::Market, TimeInForce::Ioc, dec!(0), dec!(1));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());
    }

    #[test]
    fn test_exact_min_notional_passes() {
        let product = btc_product(); // min_notional = 10
        // 50000 * 0.0002 = 10.0 exactly
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(0.0002));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());
    }

    #[test]
    fn test_tick_alignment_edge_cases() {
        let product = btc_product(); // tick_size = 0.01
        // Exactly on tick
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000.01), dec!(1));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());

        // Whole number — always on tick
        let cmd = make_cmd(Side::Buy, OrderType::Limit, TimeInForce::Gtc, dec!(50000), dec!(1));
        assert!(OrderValidator::validate(&cmd, &product).is_ok());
    }

    // --- validate_reduce_only ---

    fn reduce_only_cmd(qty: Decimal) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Sell, // closing a long position
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Gtc,
            price: Decimal::ZERO,
            qty,
            timestamp: chrono::Utc::now(),
            reduce_only: true,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        }
    }

    #[test]
    fn test_reduce_only_valid_partial_reduce() {
        // Selling 0.5 BTC against a 1 BTC long position — valid.
        let cmd = reduce_only_cmd(dec!(0.5));
        assert!(validate_reduce_only(&cmd, Some(dec!(1))).is_ok());
    }

    #[test]
    fn test_reduce_only_valid_full_reduce() {
        // Selling exactly the full position size — valid (full close).
        let cmd = reduce_only_cmd(dec!(1));
        assert!(validate_reduce_only(&cmd, Some(dec!(1))).is_ok());
    }

    #[test]
    fn test_reduce_only_no_position_none() {
        // No position at all — rejected.
        let cmd = reduce_only_cmd(dec!(1));
        assert_eq!(
            validate_reduce_only(&cmd, None).unwrap_err(),
            ValidationError::ReduceOnlyNoPosition
        );
    }

    #[test]
    fn test_reduce_only_no_position_zero_size() {
        // Position exists but is already closed (size = 0) — rejected.
        let cmd = reduce_only_cmd(dec!(1));
        assert_eq!(
            validate_reduce_only(&cmd, Some(dec!(0))).unwrap_err(),
            ValidationError::ReduceOnlyNoPosition
        );
    }

    #[test]
    fn test_reduce_only_exceeds_position() {
        // Trying to reduce 2 BTC but only 1 BTC is open — rejected.
        let cmd = reduce_only_cmd(dec!(2));
        assert_eq!(
            validate_reduce_only(&cmd, Some(dec!(1))).unwrap_err(),
            ValidationError::ReduceOnlyExceedsPosition {
                qty: dec!(2),
                position_size: dec!(1),
            }
        );
    }

    #[test]
    fn test_reduce_only_exceeds_position_fractional() {
        // 1.0001 BTC against a 1 BTC position — fractionally exceeds, rejected.
        let cmd = reduce_only_cmd(dec!(1.0001));
        assert!(matches!(
            validate_reduce_only(&cmd, Some(dec!(1))).unwrap_err(),
            ValidationError::ReduceOnlyExceedsPosition { .. }
        ));
    }

    #[test]
    fn test_reduce_only_error_messages() {
        // Verify error messages are human-readable.
        let no_pos = ValidationError::ReduceOnlyNoPosition;
        assert!(no_pos.to_string().contains("no open position"));

        let exceeds = ValidationError::ReduceOnlyExceedsPosition {
            qty: dec!(5),
            position_size: dec!(2),
        };
        let msg = exceeds.to_string();
        assert!(msg.contains("5") && msg.contains("2"));
    }
}
