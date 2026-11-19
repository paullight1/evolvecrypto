use chrono::{DateTime, Utc};
use exchange_common::{OrderType, Side, Symbol, UserId};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{BotOrder, BotStatus};

/// Configuration for a Dollar-Cost Averaging bot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DcaBotConfig {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Quote-asset amount to spend per order.
    pub amount_per_order: Decimal,
    /// Minimum seconds between consecutive orders.
    pub interval_secs: u64,
    /// Maximum number of orders to place. `None` = run forever.
    pub total_orders: Option<u32>,
    /// If set, only buy when price has dipped at least this % below
    /// the last fill price.
    pub price_deviation_pct: Option<Decimal>,
}

/// A running DCA bot instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DcaBot {
    pub id: Uuid,
    pub config: DcaBotConfig,
    pub orders_placed: u32,
    /// Total quote-asset spent across all fills.
    pub total_invested: Decimal,
    /// Total base-asset received across all fills.
    pub total_received: Decimal,
    /// Volume-weighted average fill price.
    pub avg_price: Decimal,
    pub status: BotStatus,
    pub last_order_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Price at the time of the last fill; used for deviation check.
    last_fill_price: Option<Decimal>,
}

impl DcaBot {
    pub fn new(config: DcaBotConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            config,
            orders_placed: 0,
            total_invested: Decimal::ZERO,
            total_received: Decimal::ZERO,
            avg_price: Decimal::ZERO,
            status: BotStatus::Active,
            last_order_at: None,
            created_at: Utc::now(),
            last_fill_price: None,
        }
    }

    /// Whether the bot should fire an order right now.
    ///
    /// Returns `false` when:
    /// - The bot is not active.
    /// - All configured orders have been placed.
    /// - The interval since the last order has not elapsed.
    /// - A price-deviation filter is configured and the current price
    ///   has not dipped enough from the last fill price.
    pub fn should_execute(&self, now: DateTime<Utc>, current_price: Decimal) -> bool {
        if self.status != BotStatus::Active {
            return false;
        }

        if self.is_complete() {
            return false;
        }

        // Interval gate.
        if let Some(last_at) = self.last_order_at {
            let elapsed_secs = (now - last_at).num_seconds();
            if elapsed_secs < self.config.interval_secs as i64 {
                return false;
            }
        }

        // Price deviation gate.
        if let (Some(deviation_pct), Some(last_price)) =
            (self.config.price_deviation_pct, self.last_fill_price)
        {
            let dip_pct = (last_price - current_price) / last_price * dec_100();
            if dip_pct < deviation_pct {
                return false;
            }
        }

        true
    }

    /// Build a market buy order for this DCA step.
    ///
    /// The qty is derived from `amount_per_order / current_price` so
    /// we spend approximately the configured USDT amount.
    pub fn generate_order(&mut self, current_price: Decimal) -> BotOrder {
        let qty = self.config.amount_per_order / current_price;
        self.orders_placed += 1;
        self.last_order_at = Some(Utc::now());

        BotOrder {
            order_id: Uuid::new_v4(),
            user_id: self.config.user_id,
            symbol: self.config.symbol.clone(),
            side: Side::Buy,
            price: current_price,
            qty,
            order_type: OrderType::Market,
        }
    }

    /// Update bot state when a fill arrives.
    ///
    /// Recalculates `avg_price` using the weighted-average formula:
    /// `avg = (total_invested + price * qty) / (total_received + qty)`
    pub fn on_fill(&mut self, price: Decimal, qty: Decimal) {
        self.total_invested += price * qty;
        self.total_received += qty;
        self.last_fill_price = Some(price);

        if self.total_received > Decimal::ZERO {
            self.avg_price = self.total_invested / self.total_received;
        }
    }

    /// `true` when all configured orders have been placed.
    pub fn is_complete(&self) -> bool {
        match self.config.total_orders {
            Some(limit) => self.orders_placed >= limit,
            None => false,
        }
    }

    /// Return on investment as a percentage.
    ///
    /// `roi = (current_value - total_invested) / total_invested * 100`
    ///
    /// Requires the current market price to compute current portfolio value.
    pub fn roi(&self, current_price: Decimal) -> Decimal {
        if self.total_invested == Decimal::ZERO {
            return Decimal::ZERO;
        }
        let current_value = self.total_received * current_price;
        (current_value - self.total_invested) / self.total_invested * dec_100()
    }
}

fn dec_100() -> Decimal {
    Decimal::from(100u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    fn sample_config() -> DcaBotConfig {
        DcaBotConfig {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("ETHUSDT"),
            amount_per_order: dec!(100),
            interval_secs: 3600,
            total_orders: Some(5),
            price_deviation_pct: None,
        }
    }

    fn make_bot() -> DcaBot {
        DcaBot::new(sample_config())
    }

    #[test]
    fn new_bot_starts_with_zero_stats() {
        let bot = make_bot();
        assert_eq!(bot.orders_placed, 0);
        assert_eq!(bot.total_invested, dec!(0));
        assert_eq!(bot.total_received, dec!(0));
        assert_eq!(bot.avg_price, dec!(0));
        assert_eq!(bot.status, BotStatus::Active);
    }

    #[test]
    fn should_execute_true_for_fresh_bot() {
        let bot = make_bot();
        let now = Utc::now();
        assert!(bot.should_execute(now, dec!(2000)));
    }

    #[test]
    fn should_execute_false_before_interval_elapses() {
        let mut bot = make_bot();
        let now = Utc::now();
        bot.generate_order(dec!(2000));

        // Only 10 seconds have passed, interval is 3600.
        assert!(!bot.should_execute(now + Duration::seconds(10), dec!(2000)));
    }

    #[test]
    fn should_execute_true_after_interval_elapses() {
        let mut bot = make_bot();
        let now = Utc::now();
        bot.generate_order(dec!(2000));

        assert!(bot.should_execute(now + Duration::seconds(3601), dec!(2000)));
    }

    #[test]
    fn should_execute_false_when_complete() {
        let mut bot = make_bot();
        let now = Utc::now();

        for i in 0..5 {
            bot.generate_order(dec!(2000));
            // Advance time past interval for each order.
            bot.last_order_at = Some(now - Duration::seconds(3601 * (5 - i)));
        }

        assert!(!bot.should_execute(now + Duration::seconds(3601), dec!(2000)));
    }

    #[test]
    fn should_execute_false_when_paused() {
        let mut bot = make_bot();
        bot.status = BotStatus::Paused;
        assert!(!bot.should_execute(Utc::now(), dec!(2000)));
    }

    #[test]
    fn price_deviation_blocks_when_price_has_not_dipped_enough() {
        let mut config = sample_config();
        config.price_deviation_pct = Some(dec!(5)); // need 5% dip
        let mut bot = DcaBot::new(config);

        // First fill at 2000.
        bot.on_fill(dec!(2000), dec!(0.05));
        bot.last_order_at = Some(Utc::now() - chrono::Duration::seconds(3601));

        // Price only 1% below → should NOT execute.
        let slightly_lower = dec!(1980);
        assert!(!bot.should_execute(Utc::now(), slightly_lower));
    }

    #[test]
    fn price_deviation_allows_when_price_has_dipped_enough() {
        let mut config = sample_config();
        config.price_deviation_pct = Some(dec!(5));
        let mut bot = DcaBot::new(config);

        bot.on_fill(dec!(2000), dec!(0.05));
        bot.last_order_at = Some(Utc::now() - chrono::Duration::seconds(3601));

        // Price 6% below → should execute.
        let low_price = dec!(1880);
        assert!(bot.should_execute(Utc::now(), low_price));
    }

    #[test]
    fn generate_order_is_market_buy() {
        let mut bot = make_bot();
        let order = bot.generate_order(dec!(2000));
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.order_type, OrderType::Market);
    }

    #[test]
    fn generate_order_qty_equals_amount_divided_by_price() {
        let mut bot = make_bot();
        let order = bot.generate_order(dec!(2000));
        // 100 USDT / 2000 = 0.05 ETH
        assert_eq!(order.qty, dec!(0.05));
    }

    #[test]
    fn generate_order_increments_orders_placed() {
        let mut bot = make_bot();
        assert_eq!(bot.orders_placed, 0);
        bot.generate_order(dec!(2000));
        assert_eq!(bot.orders_placed, 1);
    }

    #[test]
    fn on_fill_updates_totals_and_avg_price() {
        let mut bot = make_bot();
        bot.on_fill(dec!(2000), dec!(0.05));

        assert_eq!(bot.total_invested, dec!(100)); // 2000 * 0.05
        assert_eq!(bot.total_received, dec!(0.05));
        assert_eq!(bot.avg_price, dec!(2000));
    }

    #[test]
    fn avg_price_is_volume_weighted_across_multiple_fills() {
        let mut bot = make_bot();
        bot.on_fill(dec!(2000), dec!(0.05)); // 100 invested
        bot.on_fill(dec!(1000), dec!(0.10)); // 100 invested

        // avg = (100 + 100) / (0.05 + 0.10) = 200 / 0.15 ≈ 1333.33...
        let expected = dec!(200) / dec!(0.15);
        let diff = (bot.avg_price - expected).abs();
        assert!(diff < dec!(0.01), "avg_price mismatch: {}", bot.avg_price);
    }

    #[test]
    fn is_complete_true_when_all_orders_placed() {
        let mut bot = make_bot(); // total_orders = Some(5)
        assert!(!bot.is_complete());
        for _ in 0..5 {
            bot.generate_order(dec!(2000));
        }
        assert!(bot.is_complete());
    }

    #[test]
    fn is_complete_false_for_infinite_bot() {
        let mut config = sample_config();
        config.total_orders = None;
        let mut bot = DcaBot::new(config);
        for _ in 0..100 {
            bot.generate_order(dec!(2000));
        }
        assert!(!bot.is_complete());
    }

    #[test]
    fn roi_zero_when_nothing_invested() {
        let bot = make_bot();
        assert_eq!(bot.roi(dec!(2000)), dec!(0));
    }

    #[test]
    fn roi_positive_when_price_above_avg() {
        let mut bot = make_bot();
        bot.on_fill(dec!(1000), dec!(0.1)); // 100 USDT at 1000
        let roi = bot.roi(dec!(2000)); // now worth 200
        assert_eq!(roi, dec!(100)); // +100%
    }

    #[test]
    fn roi_negative_when_price_below_avg() {
        let mut bot = make_bot();
        bot.on_fill(dec!(2000), dec!(0.05)); // 100 USDT at 2000
        let roi = bot.roi(dec!(1000)); // now worth 50
        assert_eq!(roi, dec!(-50)); // -50%
    }
}
