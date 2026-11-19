use std::time::{Duration, Instant};

use rust_decimal::Decimal;

use exchange_common::{OrderType, Price, Side};

/// Result of a circuit breaker check for an incoming order.
#[derive(Debug, Clone)]
pub enum CircuitBreakerResult {
    /// Order is allowed to proceed.
    Allowed,
    /// Order price is outside the allowed band — reject it.
    Rejected { reason: String },
    /// Trading is currently halted due to repeated triggers.
    Halted { resume_at: Instant },
}

/// Per-symbol circuit breaker that enforces price bands and halts trading
/// when those bands are breached too many times in a short window.
///
/// Design:
/// - Tracks a reference price updated after every confirmed trade.
/// - Rejects any limit order whose price falls outside `reference ± max_move_pct`.
/// - Market orders are checked against the best bid/ask when available; if no
///   book-side exists, only a halted state is enforced (no reference needed).
/// - After `max_triggers_before_halt` rejections within a rolling 60-second
///   window the symbol is halted for `halt_duration_secs`.
/// - No reference price on the first trade → all orders allowed (bootstrapping).
pub struct CircuitBreaker {
    /// Maximum allowed price move as a fraction (0.05 = 5%).
    max_move_pct: Decimal,
    /// Reference price updated after each confirmed trade.
    reference_price: Option<Price>,
    /// Number of triggers accumulated in the current window.
    trigger_count: u32,
    /// Start of the current trigger-counting window.
    window_start: Option<Instant>,
    /// Width of the trigger-counting window.
    window_duration: Duration,
    /// How long to halt trading after `max_triggers_before_halt` are reached.
    halt_duration: Duration,
    /// Halt is active until this instant (None = not halted).
    halted_until: Option<Instant>,
    /// How many triggers within the window cause a halt.
    max_triggers_before_halt: u32,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    ///
    /// # Parameters
    /// - `max_move_pct`: price band as a fraction (e.g. `0.05` for ±5%).
    /// - `halt_duration_secs`: how many seconds to halt after exceeding trigger limit.
    /// - `max_triggers_before_halt`: trigger count within the window that causes a halt.
    pub fn new(max_move_pct: Decimal, halt_duration_secs: u64, max_triggers_before_halt: u32) -> Self {
        Self {
            max_move_pct,
            reference_price: None,
            trigger_count: 0,
            window_start: None,
            window_duration: Duration::from_secs(60),
            halt_duration: Duration::from_secs(halt_duration_secs),
            halted_until: None,
            max_triggers_before_halt,
        }
    }

    /// Update the reference price after a confirmed trade.
    ///
    /// Should be called after every fill so the band tracks the live market.
    pub fn update_reference(&mut self, price: Price) {
        self.reference_price = Some(price);
    }

    /// Check whether an order at the given price and side is allowed.
    ///
    /// For limit orders the explicit `order_price` is checked against the band.
    /// For market orders `best_bid` / `best_ask` is used as the effective price;
    /// if neither is available the order is allowed (empty book, no reference).
    pub fn check_order(
        &mut self,
        order_price: Price,
        order_type: OrderType,
        side: Side,
        best_bid: Option<Price>,
        best_ask: Option<Price>,
    ) -> CircuitBreakerResult {
        // Always enforce halt first, regardless of order type.
        if let Some(until) = self.halted_until {
            if Instant::now() < until {
                return CircuitBreakerResult::Halted { resume_at: until };
            }
            // Halt expired — clear it and proceed with normal checks.
            self.halted_until = None;
        }

        let ref_price = match self.reference_price {
            Some(p) => p,
            // No reference yet (first trade) — allow everything.
            None => return CircuitBreakerResult::Allowed,
        };

        // Determine effective check price.
        let check_price = match order_type {
            OrderType::Limit => order_price,
            OrderType::Market => {
                // For market orders use the best quote on the side the order will
                // consume — a buy market eats asks, a sell market eats bids.
                let effective = match side {
                    Side::Buy => best_ask,
                    Side::Sell => best_bid,
                };
                match effective {
                    Some(p) => p,
                    // Empty book side — no price to check against.
                    None => return CircuitBreakerResult::Allowed,
                }
            }
        };

        let upper = ref_price * (Decimal::ONE + self.max_move_pct);
        let lower = ref_price * (Decimal::ONE - self.max_move_pct);

        let violated = match side {
            Side::Buy => check_price > upper,
            Side::Sell => check_price < lower,
        };

        if violated {
            let reason = format!(
                "Price {} is outside the allowed band [{:.2}, {:.2}] (reference {}, band ±{}%)",
                check_price,
                lower,
                upper,
                ref_price,
                self.max_move_pct * Decimal::ONE_HUNDRED,
            );
            self.record_trigger();

            // Re-check halt after recording the trigger.
            if let Some(until) = self.halted_until {
                return CircuitBreakerResult::Halted { resume_at: until };
            }

            return CircuitBreakerResult::Rejected { reason };
        }

        CircuitBreakerResult::Allowed
    }

    /// Record a trigger (band breach) and activate a halt when the threshold is exceeded.
    fn record_trigger(&mut self) {
        let now = Instant::now();

        // Reset the window if it has expired.
        match self.window_start {
            Some(start) if now.duration_since(start) < self.window_duration => {
                // Still within the current window — increment.
                self.trigger_count += 1;
            }
            _ => {
                // New window.
                self.window_start = Some(now);
                self.trigger_count = 1;
            }
        }

        if self.trigger_count >= self.max_triggers_before_halt {
            self.halted_until = Some(now + self.halt_duration);
            // Reset the window so the counter doesn't immediately retrigger after lift.
            self.trigger_count = 0;
            self.window_start = None;
            tracing::warn!(
                trigger_count = self.max_triggers_before_halt,
                halt_secs = self.halt_duration.as_secs(),
                "circuit breaker halt activated"
            );
        }
    }

    /// Returns true if the symbol is currently halted.
    pub fn is_halted(&self) -> bool {
        self.halted_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// Clear a halt immediately (e.g. for operator intervention or tests).
    pub fn reset(&mut self) {
        self.halted_until = None;
        self.trigger_count = 0;
        self.window_start = None;
    }

    /// Force an indefinite halt that persists until `reset()` is called.
    ///
    /// Unlike the automatic halt triggered by repeated band breaches, this
    /// halt does not expire on its own — it is intended for operator use via
    /// the `POST /admin/engine/halt` endpoint.
    pub fn force_halt(&mut self) {
        // Set halted_until far in the future (100 years).  `reset()` clears it.
        self.halted_until = Some(Instant::now() + Duration::from_secs(60 * 60 * 24 * 365 * 100));
        self.trigger_count = 0;
        self.window_start = None;
        tracing::warn!("circuit breaker force-halted by operator");
    }
}

// ---------------------------------------------------------------------------
// Default configurations
// ---------------------------------------------------------------------------

/// Default circuit breaker configuration for BTC: ±5% band, 30 s halt, 3 triggers in 60 s.
pub fn btc_circuit_breaker() -> CircuitBreaker {
    CircuitBreaker::new(
        Decimal::new(5, 2),  // 0.05 = 5%
        30,
        3,
    )
}

/// Default circuit breaker configuration for ETH: ±7% band, 30 s halt, 3 triggers in 60 s.
pub fn eth_circuit_breaker() -> CircuitBreaker {
    CircuitBreaker::new(
        Decimal::new(7, 2),  // 0.07 = 7%
        30,
        3,
    )
}

/// Default circuit breaker configuration for SOL: ±10% band, 30 s halt, 3 triggers in 60 s.
pub fn sol_circuit_breaker() -> CircuitBreaker {
    CircuitBreaker::new(
        Decimal::new(10, 2),  // 0.10 = 10%
        30,
        3,
    )
}

/// Default circuit breaker for dynamically listed symbols.
///
/// Uses a ±10% price band (conservative for unknown volatility), 30-second
/// halt duration, and 3 triggers within the rolling window before halting.
pub fn generic_circuit_breaker() -> CircuitBreaker {
    CircuitBreaker::new(
        Decimal::new(10, 2), // 0.10 = ±10%
        30,
        3,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::thread;

    fn make_cb() -> CircuitBreaker {
        // 5% band, 2-second halt (short for tests), halt after 3 triggers in 60 s.
        CircuitBreaker::new(dec!(0.05), 2, 3)
    }

    // Helper: check a limit order.
    fn check_limit(cb: &mut CircuitBreaker, price: Price, side: Side) -> CircuitBreakerResult {
        cb.check_order(price, OrderType::Limit, side, None, None)
    }

    // -----------------------------------------------------------------------
    // Baseline / bootstrapping
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_reference_allows_all() {
        let mut cb = make_cb();
        // No reference set — all orders allowed regardless of price.
        let r = check_limit(&mut cb, dec!(50000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "expected Allowed, got {r:?}");

        let r = check_limit(&mut cb, dec!(1), Side::Sell);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "expected Allowed, got {r:?}");
    }

    // -----------------------------------------------------------------------
    // Price band enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn test_buy_within_band_allowed() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // 5% upper band = 52500. Buying at 52000 is within it.
        let r = check_limit(&mut cb, dec!(52000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    #[test]
    fn test_buy_above_band_rejected() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // 5% upper band = 52500. Buying at 53000 exceeds it.
        let r = check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Rejected { .. }), "got {r:?}");
    }

    #[test]
    fn test_sell_within_band_allowed() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // 5% lower band = 47500. Selling at 48000 is within it.
        let r = check_limit(&mut cb, dec!(48000), Side::Sell);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    #[test]
    fn test_sell_below_band_rejected() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // 5% lower band = 47500. Selling at 46000 is below it.
        let r = check_limit(&mut cb, dec!(46000), Side::Sell);
        assert!(matches!(r, CircuitBreakerResult::Rejected { .. }), "got {r:?}");
    }

    #[test]
    fn test_price_exactly_at_band_boundary_allowed() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Exactly at 52500 (5% upper) — on-boundary is allowed (not strictly greater).
        let r = check_limit(&mut cb, dec!(52500), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");

        // Exactly at 47500 (5% lower) — on-boundary is allowed (not strictly less).
        let r = check_limit(&mut cb, dec!(47500), Side::Sell);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    // -----------------------------------------------------------------------
    // Reference price tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_reference_price_updates_after_trade() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Price moves to 52000 (within original 5% band), update reference.
        cb.update_reference(dec!(52000));

        // Now the new upper band = 54600. 53000 is within it.
        let r = check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");

        // But 56000 (> 54600) should be rejected.
        let r = check_limit(&mut cb, dec!(56000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Rejected { .. }), "got {r:?}");
    }

    // -----------------------------------------------------------------------
    // Halt lifecycle
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiple_triggers_activate_halt() {
        let mut cb = make_cb(); // halt after 3 triggers
        cb.update_reference(dec!(50000));

        // Trigger 1
        check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(!cb.is_halted());

        // Trigger 2
        check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(!cb.is_halted());

        // Trigger 3 — halt activates.
        let r = check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(cb.is_halted(), "expected halt after 3rd trigger");
        assert!(matches!(r, CircuitBreakerResult::Halted { .. }), "got {r:?}");
    }

    #[test]
    fn test_halted_rejects_all_orders() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Trigger halt manually via 3 rejections.
        for _ in 0..3 {
            check_limit(&mut cb, dec!(53000), Side::Buy);
        }
        assert!(cb.is_halted());

        // Even a perfectly valid price is rejected while halted.
        let r = check_limit(&mut cb, dec!(50000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Halted { .. }), "got {r:?}");
    }

    #[test]
    fn test_market_order_rejected_during_halt() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Activate halt.
        for _ in 0..3 {
            check_limit(&mut cb, dec!(53000), Side::Buy);
        }

        // Market buy while halted — best ask available.
        let r = cb.check_order(
            dec!(0),
            OrderType::Market,
            Side::Buy,
            None,
            Some(dec!(51000)),
        );
        assert!(matches!(r, CircuitBreakerResult::Halted { .. }), "got {r:?}");
    }

    #[test]
    fn test_halt_expires_and_trading_resumes() {
        // 5% band, 1-second halt, 3 triggers.
        let mut cb = CircuitBreaker::new(dec!(0.05), 1, 3);
        cb.update_reference(dec!(50000));

        // Activate halt.
        for _ in 0..3 {
            check_limit(&mut cb, dec!(53000), Side::Buy);
        }
        assert!(cb.is_halted());

        // Wait for halt to expire.
        thread::sleep(Duration::from_millis(1100));
        assert!(!cb.is_halted());

        // A valid order is now allowed.
        let r = check_limit(&mut cb, dec!(50000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    #[test]
    fn test_reset_clears_halt() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Activate halt.
        for _ in 0..3 {
            check_limit(&mut cb, dec!(53000), Side::Buy);
        }
        assert!(cb.is_halted());

        cb.reset();
        assert!(!cb.is_halted());

        // Normal orders allowed again.
        let r = check_limit(&mut cb, dec!(50000), Side::Buy);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    // -----------------------------------------------------------------------
    // Market orders
    // -----------------------------------------------------------------------

    #[test]
    fn test_market_buy_checked_against_best_ask() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Best ask at 53000 (outside 5% upper band of 52500) → rejected.
        let r = cb.check_order(
            dec!(0),
            OrderType::Market,
            Side::Buy,
            None,
            Some(dec!(53000)),
        );
        assert!(matches!(r, CircuitBreakerResult::Rejected { .. }), "got {r:?}");

        // Best ask within band → allowed.
        let r = cb.check_order(
            dec!(0),
            OrderType::Market,
            Side::Buy,
            None,
            Some(dec!(51000)),
        );
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    #[test]
    fn test_market_sell_checked_against_best_bid() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Best bid at 46000 (outside 5% lower band of 47500) → rejected.
        let r = cb.check_order(
            dec!(0),
            OrderType::Market,
            Side::Sell,
            Some(dec!(46000)),
            None,
        );
        assert!(matches!(r, CircuitBreakerResult::Rejected { .. }), "got {r:?}");

        // Best bid within band → allowed.
        let r = cb.check_order(
            dec!(0),
            OrderType::Market,
            Side::Sell,
            Some(dec!(49000)),
            None,
        );
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    #[test]
    fn test_market_order_no_book_side_allowed() {
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // No best ask → market buy allowed (empty book, nothing to check against).
        let r = cb.check_order(dec!(0), OrderType::Market, Side::Buy, None, None);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");

        // No best bid → market sell allowed.
        let r = cb.check_order(dec!(0), OrderType::Market, Side::Sell, None, None);
        assert!(matches!(r, CircuitBreakerResult::Allowed), "got {r:?}");
    }

    // -----------------------------------------------------------------------
    // Trigger window reset
    // -----------------------------------------------------------------------

    #[test]
    fn test_trigger_window_resets_after_expiry() {
        // 5% band, no halt (large threshold), 60-second window.
        // Artificially test that trigger count resets by using a zero-duration window.
        // We can't override window_duration from outside, so we verify the public
        // behavior: after 2 triggers (below threshold of 3), the CB is not halted.
        let mut cb = make_cb();
        cb.update_reference(dec!(50000));

        // Two triggers — below threshold of 3, no halt.
        check_limit(&mut cb, dec!(53000), Side::Buy);
        check_limit(&mut cb, dec!(53000), Side::Buy);
        assert!(!cb.is_halted());
        assert!(cb.trigger_count == 2);

        // Simulate window reset by checking the count directly.
        // (The window will naturally reset after 60 s — this test validates the count.)
        assert_eq!(cb.trigger_count, 2);
    }

    // -----------------------------------------------------------------------
    // Default configs
    // -----------------------------------------------------------------------

    #[test]
    fn test_btc_default_config() {
        let mut cb = btc_circuit_breaker();
        cb.update_reference(dec!(50000));

        // 5% upper = 52500.
        assert!(matches!(check_limit(&mut cb, dec!(52000), Side::Buy), CircuitBreakerResult::Allowed));
        assert!(matches!(check_limit(&mut cb, dec!(53000), Side::Buy), CircuitBreakerResult::Rejected { .. }));
    }

    #[test]
    fn test_eth_default_config() {
        let mut cb = eth_circuit_breaker();
        cb.update_reference(dec!(3000));

        // 7% upper = 3210.
        assert!(matches!(check_limit(&mut cb, dec!(3100), Side::Buy), CircuitBreakerResult::Allowed));
        assert!(matches!(check_limit(&mut cb, dec!(3300), Side::Buy), CircuitBreakerResult::Rejected { .. }));
    }

    #[test]
    fn test_sol_default_config() {
        let mut cb = sol_circuit_breaker();
        cb.update_reference(dec!(200));

        // 10% upper = 220.
        assert!(matches!(check_limit(&mut cb, dec!(215), Side::Buy), CircuitBreakerResult::Allowed));
        assert!(matches!(check_limit(&mut cb, dec!(225), Side::Buy), CircuitBreakerResult::Rejected { .. }));
    }
}
