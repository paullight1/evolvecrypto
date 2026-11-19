use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Trade row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TradeRow {
    pub id: i64,
    pub symbol: String,
    pub price: Decimal,
    pub qty: Decimal,
    pub maker_order_id: Uuid,
    pub taker_order_id: Uuid,
    pub maker_user_id: Uuid,
    pub taker_user_id: Uuid,
    pub taker_side: String,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub created_at: DateTime<Utc>,
}

pub struct TradeRepo;

impl TradeRepo {
    /// Insert a new trade.
    pub async fn create(
        pool: &PgPool,
        symbol: &str,
        price: Decimal,
        qty: Decimal,
        maker_order_id: Uuid,
        taker_order_id: Uuid,
        maker_user_id: Uuid,
        taker_user_id: Uuid,
        taker_side: &str,
        maker_fee: Decimal,
        taker_fee: Decimal,
    ) -> Result<TradeRow, sqlx::Error> {
        let row = sqlx::query_as::<_, TradeRow>(
            "INSERT INTO trades (symbol, price, qty, maker_order_id, taker_order_id,
                                 maker_user_id, taker_user_id, taker_side, maker_fee, taker_fee)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING *"
        )
        .bind(symbol)
        .bind(price)
        .bind(qty)
        .bind(maker_order_id)
        .bind(taker_order_id)
        .bind(maker_user_id)
        .bind(taker_user_id)
        .bind(taker_side)
        .bind(maker_fee)
        .bind(taker_fee)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Get recent trades for a symbol.
    pub async fn recent(pool: &PgPool, symbol: &str, limit: i64) -> Result<Vec<TradeRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TradeRow>(
            "SELECT * FROM trades WHERE symbol = $1 ORDER BY created_at DESC LIMIT $2"
        )
        .bind(symbol)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get trades for a user (as maker or taker).
    pub async fn user_trades(pool: &PgPool, user_id: Uuid, limit: i64) -> Result<Vec<TradeRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TradeRow>(
            "SELECT * FROM trades
             WHERE maker_user_id = $1 OR taker_user_id = $1
             ORDER BY created_at DESC LIMIT $2"
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get total trade volume for a symbol in last N hours.
    pub async fn volume_since(
        pool: &PgPool,
        symbol: &str,
        hours: i32,
    ) -> Result<Decimal, sqlx::Error> {
        let vol = sqlx::query_scalar::<_, Option<Decimal>>(
            "SELECT SUM(price * qty) FROM trades
             WHERE symbol = $1 AND created_at > NOW() - make_interval(hours => $2)"
        )
        .bind(symbol)
        .bind(hours)
        .fetch_one(pool)
        .await?;

        Ok(vol.unwrap_or(Decimal::ZERO))
    }

    /// Get user's trades with pagination and optional symbol filter.
    pub async fn user_trades_paginated(
        pool: &PgPool,
        user_id: Uuid,
        symbol: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TradeRow>, sqlx::Error> {
        if let Some(sym) = symbol {
            sqlx::query_as::<_, TradeRow>(
                "SELECT * FROM trades
                 WHERE (maker_user_id = $1 OR taker_user_id = $1) AND symbol = $2
                 ORDER BY created_at DESC
                 LIMIT $3 OFFSET $4",
            )
            .bind(user_id)
            .bind(sym)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as::<_, TradeRow>(
                "SELECT * FROM trades
                 WHERE maker_user_id = $1 OR taker_user_id = $1
                 ORDER BY created_at DESC
                 LIMIT $2 OFFSET $3",
            )
            .bind(user_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }

    /// Get recent public trades for a symbol.
    ///
    /// Returns up to `limit` trades ordered by creation time descending (newest
    /// first). Intended for the public trade feed endpoint — caller should strip
    /// any user-identifying fields before exposing to unauthenticated clients.
    pub async fn recent_public(
        pool: &PgPool,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<TradeRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, TradeRow>(
            "SELECT * FROM trades WHERE symbol = $1
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(symbol)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    async fn test_pool() -> PgPool {
        PgPool::connect(
            &std::env::var("TEST_DATABASE_URL")
                .expect("TEST_DATABASE_URL must be set for integration tests"),
        )
        .await
        .expect("Failed to connect to test database")
    }

    /// Helper: insert the minimum fixtures required for a trade row (users, product, orders).
    async fn insert_trade_fixtures(
        pool: &PgPool,
    ) -> (Uuid, Uuid, Uuid, Uuid) {
        let maker_user = Uuid::new_v4();
        let taker_user = Uuid::new_v4();

        for uid in [maker_user, taker_user] {
            sqlx::query("INSERT INTO users (id) VALUES ($1)")
                .bind(uid)
                .execute(pool)
                .await
                .expect("Failed to create user");
        }

        sqlx::query(
            "INSERT INTO products (symbol, base_asset, quote_asset, tick_size, lot_size, min_notional, maker_fee, taker_fee)
             VALUES ('TRDSYM', 'TRD', 'SYM', 0.01, 0.01, 1, 0.001, 0.001)
             ON CONFLICT DO NOTHING",
        )
        .execute(pool)
        .await
        .expect("Failed to create product");

        let maker_order = Uuid::new_v4();
        let taker_order = Uuid::new_v4();

        for (oid, uid, side) in [
            (maker_order, maker_user, "Sell"),
            (taker_order, taker_user, "Buy"),
        ] {
            sqlx::query(
                "INSERT INTO orders (id, user_id, symbol, side, order_type, time_in_force, price, qty, filled_qty, status)
                 VALUES ($1, $2, 'TRDSYM', $3, 'Limit', 'Gtc', 100, 1, 0, 'Filled')",
            )
            .bind(oid)
            .bind(uid)
            .bind(side)
            .execute(pool)
            .await
            .expect("Failed to create order");
        }

        (maker_user, taker_user, maker_order, taker_order)
    }

    #[tokio::test]
    #[ignore]
    async fn test_trade_create_and_recent_public() {
        let pool = test_pool().await;
        let (maker_user, taker_user, maker_order, taker_order) =
            insert_trade_fixtures(&pool).await;

        // Create a trade
        let trade = TradeRepo::create(
            &pool,
            "TRDSYM",
            dec!(100.00),
            dec!(1.0),
            maker_order,
            taker_order,
            maker_user,
            taker_user,
            "Buy",
            dec!(0.001),
            dec!(0.002),
        )
        .await
        .expect("create trade failed");

        assert_eq!(trade.symbol, "TRDSYM");
        assert_eq!(trade.maker_fee, dec!(0.001));
        assert_eq!(trade.taker_fee, dec!(0.002));
        assert_eq!(trade.taker_side, "Buy");

        // recent_public should return the trade
        let public = TradeRepo::recent_public(&pool, "TRDSYM", 10)
            .await
            .expect("recent_public failed");
        assert!(public.iter().any(|t| t.id == trade.id));

        // Limit is respected
        let limited = TradeRepo::recent_public(&pool, "TRDSYM", 1)
            .await
            .expect("recent_public with limit failed");
        assert_eq!(limited.len(), 1);
    }
}
