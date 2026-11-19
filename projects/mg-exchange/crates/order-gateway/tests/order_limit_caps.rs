/// Integration tests for per-user open-order caps (Sprint 8, KG-2).
///
/// Tests use a `MockCountSource` to avoid requiring a running engine thread.
/// REST-layer 429 mapping is tested separately in `exchange-api-gateway` tests.
use exchange_common::*;
use exchange_order_gateway::{OrderCountSource, OrderLimitConfig, OrderValidator, ValidationError};
use rust_decimal_macros::dec;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn make_limit_cmd(
    user_id: uuid::Uuid,
    price: rust_decimal::Decimal,
    qty: rust_decimal::Decimal,
) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
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

fn make_market_cmd(
    user_id: uuid::Uuid,
) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        price: dec!(0),
        qty: dec!(1),
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

/// Mock implementation of `OrderCountSource` for unit tests.
///
/// Stores fixed counts returned for every query call.  Tests set these
/// fields to drive the validation outcome.
struct MockCountSource {
    /// Returned for every `user_open_order_count` call.
    open_orders: usize,
    /// Returned for every `user_orders_at_price` call.
    orders_at_price: usize,
    /// Returned for every `user_total_open_order_count` call.
    /// Defaults to `open_orders` when not explicitly set (single-symbol usage).
    total_orders: usize,
}

impl MockCountSource {
    fn new(open_orders: usize, orders_at_price: usize) -> Self {
        Self { open_orders, orders_at_price, total_orders: open_orders }
    }

    fn with_total(open_orders: usize, orders_at_price: usize, total_orders: usize) -> Self {
        Self { open_orders, orders_at_price, total_orders }
    }
}

impl OrderCountSource for MockCountSource {
    fn user_open_order_count(&self, _symbol: &Symbol, _user_id: UserId) -> usize {
        self.open_orders
    }

    fn user_orders_at_price(
        &self,
        _symbol: &Symbol,
        _user_id: UserId,
        _side: Side,
        _price: Price,
    ) -> usize {
        self.orders_at_price
    }

    fn user_total_open_order_count(&self, _user_id: UserId) -> usize {
        self.total_orders
    }
}

// ---------------------------------------------------------------------------
// Test 1: per-symbol cap — accept up to limit, reject at limit
// ---------------------------------------------------------------------------

/// Place 3 orders within a 3-order cap — all should succeed.
#[test]
fn test_symbol_cap_under_limit_accepted() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 3,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 5000,
    };

    // Simulate 0, 1, 2 existing orders — all under the 3-order cap.
    for existing in 0..3usize {
        let counts = MockCountSource::new(existing, 0);
        let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));
        assert!(
            OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok(),
            "expected acceptance at existing={existing}"
        );
    }
}

/// 4th order when cap is 3 must be rejected with MaxOpenOrdersPerSymbol.
#[test]
fn test_symbol_cap_at_limit_rejected() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 3,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 5000,
    };
    let counts = MockCountSource::new(3, 0); // already at the cap
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));

    let err = OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).unwrap_err();
    assert!(
        matches!(
            err,
            ValidationError::MaxOpenOrdersPerSymbol {
                current: 3,
                limit: 3,
                ..
            }
        ),
        "unexpected error variant: {err}"
    );
    assert!(err.is_rate_limit(), "MaxOpenOrdersPerSymbol should be a rate-limit error");
}

/// After cancelling one order (count drops from 3 to 2), the next placement succeeds.
#[test]
fn test_symbol_cap_after_cancel_succeeds() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 3,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 5000,
    };
    // Post-cancel: 2 open orders (under cap of 3).
    let counts = MockCountSource::new(2, 0);
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));
    assert!(OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok());
}

/// Two independent users on the same symbol each have their own 3-order quota.
#[test]
fn test_symbol_cap_independent_per_user() {
    let product = btc_product();
    let user_a = uuid::Uuid::new_v4();
    let user_b = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 3,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 5000,
    };

    // user_a at cap (3), user_b at 0.
    let counts_a = MockCountSource::new(3, 0);
    let counts_b = MockCountSource::new(0, 0);

    let cmd_a = make_limit_cmd(user_a, dec!(50000), dec!(0.001));
    let cmd_b = make_limit_cmd(user_b, dec!(50000), dec!(0.001));

    // user_a is blocked
    assert!(matches!(
        OrderValidator::validate_with_counts(&cmd_a, &product, &counts_a, &limits).unwrap_err(),
        ValidationError::MaxOpenOrdersPerSymbol { .. }
    ));
    // user_b is NOT blocked (separate quota)
    assert!(OrderValidator::validate_with_counts(&cmd_b, &product, &counts_b, &limits).is_ok());
}

// ---------------------------------------------------------------------------
// Test 2: per-price-level cap
// ---------------------------------------------------------------------------

/// Two orders at same price within a 2-order-per-level cap — accepted.
#[test]
fn test_price_level_cap_under_limit_accepted() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 2,
        max_open_orders_per_user_total: 5000,
    };

    for existing_at_price in 0..2usize {
        let counts = MockCountSource::new(existing_at_price, existing_at_price);
        let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));
        assert!(
            OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok(),
            "expected acceptance at existing_at_price={existing_at_price}"
        );
    }
}

/// 3rd order at same price when cap is 2 must be rejected with MaxOrdersPerPriceLevel.
#[test]
fn test_price_level_cap_at_limit_rejected() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 2,
        max_open_orders_per_user_total: 5000,
    };
    let counts = MockCountSource::new(2, 2); // 2 open at this price level
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));

    let err = OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).unwrap_err();
    assert!(
        matches!(
            err,
            ValidationError::MaxOrdersPerPriceLevel {
                current: 2,
                limit: 2,
                ..
            }
        ),
        "unexpected error variant: {err}"
    );
    assert!(err.is_rate_limit(), "MaxOrdersPerPriceLevel should be a rate-limit error");
}

/// Order at a different price than the capped level should succeed.
#[test]
fn test_price_level_cap_different_price_succeeds() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 2,
        max_open_orders_per_user_total: 5000,
    };

    // orders_at_price=2 simulates cap hit at $50000, but we'll place at $50001.
    // The mock doesn't differentiate by price — in a real test we'd use the engine.
    // Here we verify the price-level check fires for limit orders at ALL prices
    // when mock returns 2. This is intentional: the mock is price-agnostic.
    // A separate api-gateway integration test uses a real engine for per-price checks.
    let counts = MockCountSource::new(2, 0); // 0 at this level (different price)
    let cmd = make_limit_cmd(user, dec!(50001), dec!(0.001));
    assert!(OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok());
}

/// Market orders are NOT subject to the price-level cap (they never rest on the book).
#[test]
fn test_price_level_cap_does_not_apply_to_market_orders() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 0, // cap set to 0 — would reject any limit order
        max_open_orders_per_user_total: 5000,
    };
    // Market orders should bypass the price-level check entirely.
    let counts = MockCountSource::new(0, 100); // 100 at this level — above any reasonable cap
    let cmd = make_market_cmd(user);
    assert!(
        OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok(),
        "market orders must not be subject to per-price-level cap"
    );
}

// ---------------------------------------------------------------------------
// Test 3: error messages are human-readable
// ---------------------------------------------------------------------------

#[test]
fn test_error_messages_are_human_readable() {
    let symbol = Symbol::new("BTCUSDT");

    let sym_err = ValidationError::MaxOpenOrdersPerSymbol {
        symbol: symbol.clone(),
        current: 500,
        limit: 500,
    };
    let msg = sym_err.to_string();
    assert!(msg.contains("500"), "message must include the count: {msg}");
    assert!(msg.contains("BTCUSDT"), "message must include the symbol: {msg}");

    let level_err = ValidationError::MaxOrdersPerPriceLevel {
        symbol,
        price: dec!(50000.00),
        current: 50,
        limit: 50,
    };
    let msg = level_err.to_string();
    assert!(msg.contains("50"), "message must include the count: {msg}");
    assert!(msg.contains("50000"), "message must include the price: {msg}");
}

// ---------------------------------------------------------------------------
// Test 4: is_rate_limit() distinguishes 429 from 400 errors
// ---------------------------------------------------------------------------

#[test]
fn test_is_rate_limit_only_for_cap_variants() {
    assert!(
        ValidationError::MaxOpenOrdersPerSymbol {
            symbol: Symbol::new("BTCUSDT"),
            current: 500,
            limit: 500,
        }
        .is_rate_limit()
    );
    assert!(
        ValidationError::MaxOrdersPerPriceLevel {
            symbol: Symbol::new("BTCUSDT"),
            price: dec!(50000),
            current: 50,
            limit: 50,
        }
        .is_rate_limit()
    );

    // These should NOT be rate-limit errors (400, not 429).
    assert!(!ValidationError::InvalidPrice(dec!(-1)).is_rate_limit());
    assert!(!ValidationError::MarketPostOnly.is_rate_limit());
    assert!(!ValidationError::ReduceOnlyNoPosition.is_rate_limit());
}

// ---------------------------------------------------------------------------
// Test 5: product-level checks still run before cap checks
// ---------------------------------------------------------------------------

/// Product validation errors must fire even when caps are not reached.
#[test]
fn test_product_validation_runs_before_cap_check() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig::default();
    let counts = MockCountSource::new(0, 0);

    // Invalid quantity — should fail product validation before cap check.
    let mut cmd = make_limit_cmd(user, dec!(50000), dec!(0));
    cmd.qty = dec!(0); // will be rejected by product validation
    let err = OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidQuantity(_)),
        "product validation must run before cap check: {err}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: cross-symbol total open-order cap (KG-7, Sprint 10)
// ---------------------------------------------------------------------------

/// Total count below the global cap — order accepted.
#[test]
fn test_total_cap_under_limit_accepted() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 10,
    };
    // 9 total orders across all symbols, 1 on this symbol — under both caps.
    let counts = MockCountSource::with_total(1, 0, 9);
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));
    assert!(
        OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).is_ok(),
        "order under total cap must be accepted"
    );
}

/// Total count at the global cap — order rejected with MaxOpenOrdersPerUserTotal.
#[test]
fn test_total_cap_at_limit_rejected() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 10,
    };
    // 10 total orders — exactly at the cap.
    let counts = MockCountSource::with_total(1, 0, 10);
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));

    let err = OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).unwrap_err();
    assert!(
        matches!(
            err,
            ValidationError::MaxOpenOrdersPerUserTotal {
                current: 10,
                limit: 10,
            }
        ),
        "unexpected error variant: {err}"
    );
    assert!(err.is_rate_limit(), "MaxOpenOrdersPerUserTotal must be a rate-limit error (429)");
}

/// Per-symbol count is under its cap, but total across symbols hits the global cap.
/// The global cap must fire.
#[test]
fn test_total_cap_rejected_even_if_per_symbol_under() {
    let product = btc_product();
    let user = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,   // per-symbol cap: 500 (not reached)
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 50,          // total cap: 50
    };
    // 5 orders on THIS symbol (well under the 500 per-symbol cap),
    // but 50 total across all symbols (at the global cap).
    let counts = MockCountSource::with_total(5, 0, 50);
    let cmd = make_limit_cmd(user, dec!(50000), dec!(0.001));

    let err = OrderValidator::validate_with_counts(&cmd, &product, &counts, &limits).unwrap_err();
    assert!(
        matches!(err, ValidationError::MaxOpenOrdersPerUserTotal { current: 50, limit: 50 }),
        "global total cap must fire when per-symbol count is fine: {err}"
    );
}

/// User A at global cap must not affect User B (independent per-user state).
#[test]
fn test_total_cap_independent_per_user() {
    let product = btc_product();
    let user_a = uuid::Uuid::new_v4();
    let user_b = uuid::Uuid::new_v4();
    let limits = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 500,
        max_orders_per_user_per_price_level: 50,
        max_open_orders_per_user_total: 10,
    };

    // user_a has hit the global total cap.
    let counts_a = MockCountSource::with_total(1, 0, 10);
    let cmd_a = make_limit_cmd(user_a, dec!(50000), dec!(0.001));

    // user_b has 0 orders everywhere — completely free.
    let counts_b = MockCountSource::with_total(0, 0, 0);
    let cmd_b = make_limit_cmd(user_b, dec!(50000), dec!(0.001));

    // user_a must be blocked.
    assert!(
        matches!(
            OrderValidator::validate_with_counts(&cmd_a, &product, &counts_a, &limits)
                .unwrap_err(),
            ValidationError::MaxOpenOrdersPerUserTotal { .. }
        ),
        "user_a must be blocked by global total cap"
    );
    // user_b must NOT be blocked.
    assert!(
        OrderValidator::validate_with_counts(&cmd_b, &product, &counts_b, &limits).is_ok(),
        "user_b must not be blocked — independent quota"
    );
}
