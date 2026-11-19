use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Order row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub price: Option<Decimal>,
    pub qty: Decimal,
    pub filled_qty: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct OrderRepo;

impl OrderRepo {
    /// Insert a new order.
    pub async fn create(
        pool: &PgPool,
        id: Uuid,
        user_id: Uuid,
        symbol: &str,
        side: &str,
        order_type: &str,
        time_in_force: &str,
        price: Option<Decimal>,
        qty: Decimal,
    ) -> Result<OrderRow, sqlx::Error> {
        let row = sqlx::query_as::<_, OrderRow>(
            "INSERT INTO orders (id, user_id, symbol, side, order_type, time_in_force, price, qty, filled_qty, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, 'Pending')
             RETURNING *"
        )
        .bind(id)
        .bind(user_id)
        .bind(symbol)
        .bind(side)
        .bind(order_type)
        .bind(time_in_force)
        .bind(price)
        .bind(qty)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Update order status and filled_qty.
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        filled_qty: Decimal,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE orders SET status = $2, filled_qty = $3, updated_at = NOW() WHERE id = $1"
        )
        .bind(id)
        .bind(status)
        .bind(filled_qty)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get order by ID.
    pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<OrderRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Get user's recent orders.
    pub async fn user_orders(
        pool: &PgPool,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OrderRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2"
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get user's orders with pagination and optional symbol filter.
    pub async fn user_orders_paginated(
        pool: &PgPool,
        user_id: Uuid,
        symbol: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OrderRow>, sqlx::Error> {
        if let Some(sym) = symbol {
            sqlx::query_as::<_, OrderRow>(
                "SELECT * FROM orders WHERE user_id = $1 AND symbol = $2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            )
            .bind(user_id)
            .bind(sym)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as::<_, OrderRow>(
                "SELECT * FROM orders WHERE user_id = $1
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(user_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }

    /// Get open orders for a user on a symbol.
    pub async fn open_orders(
        pool: &PgPool,
        user_id: Uuid,
        symbol: &str,
    ) -> Result<Vec<OrderRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE user_id = $1 AND symbol = $2 AND status IN ('Open', 'PartiallyFilled')
             ORDER BY created_at DESC"
        )
        .bind(user_id)
        .bind(symbol)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get all open orders across all users.
    pub async fn list_open(pool: &PgPool) -> Result<Vec<OrderRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE status IN ('Open', 'Pending', 'PartiallyFilled') ORDER BY created_at"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get IDs of all terminal orders (Filled, Cancelled, Rejected).
    ///
    /// Used during WAL replay to skip commands for already-settled orders.
    pub async fn terminal_order_ids(pool: &PgPool) -> Result<std::collections::HashSet<Uuid>, sqlx::Error> {
        let rows = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM orders WHERE status IN ('Filled', 'Cancelled', 'Rejected')"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().collect())
    }

    /// Get all open orders for a user across all symbols.
    ///
    /// "Open" means status is `Open` or `PartiallyFilled`. Ordered by creation
    /// time descending (most recent first).
    pub async fn open_orders_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<OrderRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders
             WHERE user_id = $1 AND status IN ('Open', 'PartiallyFilled')
             ORDER BY created_at DESC",
        )
        .bind(user_id)
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

    #[tokio::test]
    #[ignore]
    async fn test_order_create_update_open_filter() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();

        // Create user + product (foreign key constraints)
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("Failed to create user");
        sqlx::query(
            "INSERT INTO products (symbol, base_asset, quote_asset, tick_size, lot_size, min_notional, maker_fee, taker_fee)
             VALUES ('TSTTST', 'TST', 'TST', 0.01, 0.01, 1, 0.001, 0.001)
             ON CONFLICT DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("Failed to create product");

        let order_id = Uuid::new_v4();

        // Create order
        let row = OrderRepo::create(
            &pool,
            order_id,
            user_id,
            "TSTTST",
            "Buy",
            "Limit",
            "Gtc",
            Some(dec!(100.00)),
            dec!(1.0),
        )
        .await
        .expect("create failed");
        assert_eq!(row.id, order_id);
        assert_eq!(row.status, "Pending");
        assert_eq!(row.filled_qty, dec!(0));

        // update_status sets updated_at and filled_qty
        OrderRepo::update_status(&pool, order_id, "Open", dec!(0))
            .await
            .expect("update_status failed");

        let fetched = OrderRepo::get(&pool, order_id)
            .await
            .expect("get failed")
            .expect("row should exist");
        assert_eq!(fetched.status, "Open");

        // open_orders_for_user should include this order
        let open = OrderRepo::open_orders_for_user(&pool, user_id)
            .await
            .expect("open_orders_for_user failed");
        assert!(open.iter().any(|o| o.id == order_id));

        // After filling, should not appear in open orders
        OrderRepo::update_status(&pool, order_id, "Filled", dec!(1.0))
            .await
            .expect("fill update failed");

        let open_after = OrderRepo::open_orders_for_user(&pool, user_id)
            .await
            .expect("open_orders_for_user after fill failed");
        assert!(!open_after.iter().any(|o| o.id == order_id));
    }
}
