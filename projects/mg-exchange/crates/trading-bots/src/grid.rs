use chrono::{DateTime, Utc};
use exchange_common::{OrderType, Side, Symbol, UserId};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::BotOrder;

/// Which direction the grid bot operates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GridSide {
    /// Place both buy and sell orders throughout the range.
    Neutral,
    /// Only place buy orders; flip to sell on fills.
    Long,
    /// Only place sell orders; flip to buy on fills.
    Short,
}

/// Configuration for a grid trading bot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridBotConfig {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Top of the price range.
    pub upper_price: Decimal,
    /// Bottom of the price range.
    pub lower_price: Decimal,
    /// Number of grid levels (intervals = grid_count - 1).
    pub grid_count: u32,
    /// Total capital deployed across all grid orders.
    pub total_investment: Decimal,
    pub side: GridSide,
}

/// Current status of any bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BotStatus {
    Active,
    Paused,
    Stopped,
}

/// A single price level in the grid.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridLine {
    pub price: Decimal,
    pub qty: Decimal,
    pub buy_order_id: Option<Uuid>,
    pub sell_order_id: Option<Uuid>,
    /// Which side was most recently filled at this level.
    pub filled_side: Option<Side>,
}

/// A running grid bot instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridBot {
    pub id: Uuid,
    pub config: GridBotConfig,
    pub grid_lines: Vec<GridLine>,
    pub status: BotStatus,
    pub realized_pnl: Decimal,
    pub total_trades: u64,
    pub created_at: DateTime<Utc>,
}

impl GridBot {
    /// Create a new grid bot and compute its evenly-spaced grid lines.
    ///
    /// Grid count must be >= 2. The qty per level is
    /// `total_investment / grid_count / mid_price` approximated as
    /// `total_investment / grid_count / upper_price` to keep arithmetic
    /// purely in-range without requiring a live price.
    pub fn new(config: GridBotConfig) -> Self {
        assert!(
            config.grid_count >= 2,
            "grid_count must be at least 2"
        );
        assert!(
            config.upper_price > config.lower_price,
            "upper_price must be greater than lower_price"
        );

        let spacing = Self::compute_spacing(&config);
        let qty_per_level = config.total_investment
            / Decimal::from(config.grid_count)
            / config.upper_price;

        let grid_lines = (0..config.grid_count)
            .map(|i| {
                let price = config.lower_price + spacing * Decimal::from(i);
                GridLine {
                    price,
                    qty: qty_per_level,
                    buy_order_id: None,
                    sell_order_id: None,
                    filled_side: None,
                }
            })
            .collect();

        Self {
            id: Uuid::new_v4(),
            config,
            grid_lines,
            status: BotStatus::Active,
            realized_pnl: Decimal::ZERO,
            total_trades: 0,
            created_at: Utc::now(),
        }
    }

    /// Spacing between adjacent grid levels.
    pub fn grid_spacing(&self) -> Decimal {
        Self::compute_spacing(&self.config)
    }

    fn compute_spacing(config: &GridBotConfig) -> Decimal {
        (config.upper_price - config.lower_price) / Decimal::from(config.grid_count - 1)
    }

    /// Generate the initial set of resting orders around `current_price`.
    ///
    /// - Buy orders are placed at all grid lines **below** current_price.
    /// - Sell orders are placed at all grid lines **above** current_price.
    /// - The `GridSide` filter suppresses the irrelevant side for Long/Short bots.
    pub fn generate_initial_orders(&mut self, current_price: Decimal) -> Vec<BotOrder> {
        let mut orders = Vec::new();

        for line in &mut self.grid_lines {
            let want_buy = line.price < current_price;
            let want_sell = line.price > current_price;

            let place_buy = want_buy
                && matches!(self.config.side, GridSide::Neutral | GridSide::Long);
            let place_sell = want_sell
                && matches!(self.config.side, GridSide::Neutral | GridSide::Short);

            if place_buy {
                let id = Uuid::new_v4();
                line.buy_order_id = Some(id);
                orders.push(BotOrder {
                    order_id: id,
                    user_id: self.config.user_id,
                    symbol: self.config.symbol.clone(),
                    side: Side::Buy,
                    price: line.price,
                    qty: line.qty,
                    order_type: OrderType::Limit,
                });
            }

            if place_sell {
                let id = Uuid::new_v4();
                line.sell_order_id = Some(id);
                orders.push(BotOrder {
                    order_id: id,
                    user_id: self.config.user_id,
                    symbol: self.config.symbol.clone(),
                    side: Side::Sell,
                    price: line.price,
                    qty: line.qty,
                    order_type: OrderType::Limit,
                });
            }
        }

        orders
    }

    /// React to a fill at `price` on `side`.
    ///
    /// Grid logic:
    /// - Buy filled → place a sell one grid level above.
    /// - Sell filled → place a buy one grid level below.
    ///
    /// Accumulates PnL: each completed buy→sell round-trip earns `spacing * qty`.
    pub fn on_fill(&mut self, price: Decimal, side: Side) -> Vec<BotOrder> {
        if self.status != BotStatus::Active {
            return vec![];
        }

        let spacing = self.grid_spacing();
        let mut orders = Vec::new();

        // Find the grid line closest to this fill price.
        let line_idx = self
            .grid_lines
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| {
                let diff = (l.price - price).abs();
                // Use mantissa as a sortable integer proxy (prices are finite).
                diff.mantissa().unsigned_abs()
            })
            .map(|(i, _)| i);

        let Some(idx) = line_idx else {
            return vec![];
        };

        self.total_trades += 1;
        self.grid_lines[idx].filled_side = Some(side);

        match side {
            Side::Buy => {
                // Clear the buy order that was filled.
                self.grid_lines[idx].buy_order_id = None;

                // Place sell one level up if within range.
                let sell_price = price + spacing;
                if sell_price <= self.config.upper_price {
                    if let Some(target) = self
                        .grid_lines
                        .iter_mut()
                        .find(|l| (l.price - sell_price).abs() < spacing / Decimal::from(2u32))
                    {
                        let id = Uuid::new_v4();
                        target.sell_order_id = Some(id);
                        orders.push(BotOrder {
                            order_id: id,
                            user_id: self.config.user_id,
                            symbol: self.config.symbol.clone(),
                            side: Side::Sell,
                            price: sell_price,
                            qty: target.qty,
                            order_type: OrderType::Limit,
                        });
                    }
                }
            }
            Side::Sell => {
                // Clear the sell order that was filled.
                self.grid_lines[idx].sell_order_id = None;

                // Accumulate PnL: sold at `price`, the buy was at `price - spacing`.
                let qty = self.grid_lines[idx].qty;
                self.realized_pnl += spacing * qty;

                // Place buy one level down if within range.
                let buy_price = price - spacing;
                if buy_price >= self.config.lower_price {
                    if let Some(target) = self
                        .grid_lines
                        .iter_mut()
                        .find(|l| (l.price - buy_price).abs() < spacing / Decimal::from(2u32))
                    {
                        let id = Uuid::new_v4();
                        target.buy_order_id = Some(id);
                        orders.push(BotOrder {
                            order_id: id,
                            user_id: self.config.user_id,
                            symbol: self.config.symbol.clone(),
                            side: Side::Buy,
                            price: buy_price,
                            qty: target.qty,
                            order_type: OrderType::Limit,
                        });
                    }
                }
            }
        }

        orders
    }

    /// Stop the bot and return every live order ID that must be cancelled.
    pub fn stop(&mut self) -> Vec<Uuid> {
        self.status = BotStatus::Stopped;

        self.grid_lines
            .iter_mut()
            .flat_map(|line| {
                let buy = line.buy_order_id.take();
                let sell = line.sell_order_id.take();
                [buy, sell]
            })
            .flatten()
            .collect()
    }

    /// Realized PnL accumulated from completed round-trips.
    pub fn pnl(&self) -> Decimal {
        self.realized_pnl
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_config() -> GridBotConfig {
        GridBotConfig {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            upper_price: dec!(110),
            lower_price: dec!(100),
            grid_count: 11, // 10 intervals → spacing = 1
            total_investment: dec!(1000),
            side: GridSide::Neutral,
        }
    }

    #[test]
    fn new_creates_correct_number_of_grid_lines() {
        let bot = GridBot::new(sample_config());
        assert_eq!(bot.grid_lines.len(), 11);
    }

    #[test]
    fn grid_spacing_is_correct() {
        let bot = GridBot::new(sample_config());
        assert_eq!(bot.grid_spacing(), dec!(1));
    }

    #[test]
    fn first_and_last_line_prices_match_bounds() {
        let bot = GridBot::new(sample_config());
        assert_eq!(bot.grid_lines.first().unwrap().price, dec!(100));
        assert_eq!(bot.grid_lines.last().unwrap().price, dec!(110));
    }

    #[test]
    fn initial_orders_below_current_price_are_buys() {
        let mut bot = GridBot::new(sample_config());
        let orders = bot.generate_initial_orders(dec!(105));

        let buys: Vec<_> = orders.iter().filter(|o| o.side == Side::Buy).collect();
        for buy in &buys {
            assert!(buy.price < dec!(105), "buy price {} should be < 105", buy.price);
        }
        // 5 lines below 105 (100, 101, 102, 103, 104)
        assert_eq!(buys.len(), 5);
    }

    #[test]
    fn initial_orders_above_current_price_are_sells() {
        let mut bot = GridBot::new(sample_config());
        let orders = bot.generate_initial_orders(dec!(105));

        let sells: Vec<_> = orders.iter().filter(|o| o.side == Side::Sell).collect();
        for sell in &sells {
            assert!(sell.price > dec!(105), "sell price {} should be > 105", sell.price);
        }
        // 5 lines above 105 (106, 107, 108, 109, 110)
        assert_eq!(sells.len(), 5);
    }

    #[test]
    fn long_bot_only_places_buy_orders() {
        let mut config = sample_config();
        config.side = GridSide::Long;
        let mut bot = GridBot::new(config);
        let orders = bot.generate_initial_orders(dec!(105));

        assert!(orders.iter().all(|o| o.side == Side::Buy));
    }

    #[test]
    fn short_bot_only_places_sell_orders() {
        let mut config = sample_config();
        config.side = GridSide::Short;
        let mut bot = GridBot::new(config);
        let orders = bot.generate_initial_orders(dec!(105));

        assert!(orders.iter().all(|o| o.side == Side::Sell));
    }

    #[test]
    fn fill_buy_generates_sell_one_level_up() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));

        let follow_ups = bot.on_fill(dec!(104), Side::Buy);

        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].side, Side::Sell);
        assert_eq!(follow_ups[0].price, dec!(105));
    }

    #[test]
    fn fill_sell_generates_buy_one_level_down() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));

        let follow_ups = bot.on_fill(dec!(106), Side::Sell);

        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].side, Side::Buy);
        assert_eq!(follow_ups[0].price, dec!(105));
    }

    #[test]
    fn fill_sell_accumulates_pnl() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));

        assert_eq!(bot.pnl(), dec!(0));
        bot.on_fill(dec!(106), Side::Sell);
        assert!(bot.pnl() > dec!(0), "PnL should be positive after a sell fill");
    }

    #[test]
    fn stop_returns_all_live_order_ids_and_clears_them() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));

        let live_before: usize = bot
            .grid_lines
            .iter()
            .map(|l| l.buy_order_id.is_some() as usize + l.sell_order_id.is_some() as usize)
            .sum();

        let cancelled = bot.stop();
        assert_eq!(cancelled.len(), live_before);
        assert_eq!(bot.status, BotStatus::Stopped);

        let live_after: usize = bot
            .grid_lines
            .iter()
            .map(|l| l.buy_order_id.is_some() as usize + l.sell_order_id.is_some() as usize)
            .sum();
        assert_eq!(live_after, 0);
    }

    #[test]
    fn stopped_bot_ignores_fills() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));
        bot.stop();

        let orders = bot.on_fill(dec!(104), Side::Buy);
        assert!(orders.is_empty());
    }

    #[test]
    fn total_trades_increments_on_each_fill() {
        let mut bot = GridBot::new(sample_config());
        bot.generate_initial_orders(dec!(105));

        bot.on_fill(dec!(104), Side::Buy);
        bot.on_fill(dec!(106), Side::Sell);

        assert_eq!(bot.total_trades, 2);
    }
}
