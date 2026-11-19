use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Pool configuration
// ---------------------------------------------------------------------------

/// Connection pool configuration advice based on workload.
///
/// These values are derived from operational experience with PostgreSQL under
/// exchange-style workloads (high concurrency, short-lived transactions).
pub struct PoolConfig {
    /// Maximum simultaneous open connections to PostgreSQL.
    pub max_connections: u32,
    /// Connections kept alive and ready when idle.
    pub min_connections: u32,
    /// How long to wait for an available connection before returning an error.
    pub acquire_timeout_secs: u64,
    /// Connections idle longer than this are closed.
    pub idle_timeout_secs: u64,
    /// Hard upper bound on a connection's total lifetime (forces periodic
    /// reconnects so the pool never accumulates stale or leaked server state).
    pub max_lifetime_secs: u64,
}

impl PoolConfig {
    /// Recommended configuration for an exchange workload.
    ///
    /// Rules of thumb:
    /// - `max_connections = 2 × CPU cores` — balances parallelism against
    ///   PostgreSQL lock contention and context-switching overhead.
    /// - `min_connections = CPU cores` — keeps warm connections ready so the
    ///   first burst of requests after an idle period does not pay cold-start
    ///   latency.
    /// - `acquire_timeout = 3 s` — fail fast under overload rather than
    ///   queueing indefinitely and amplifying cascading failures.
    pub fn recommended() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);

        Self {
            max_connections: cpus * 2,
            min_connections: cpus,
            acquire_timeout_secs: 3,
            idle_timeout_secs: 300,
            max_lifetime_secs: 1800,
        }
    }

    /// Apply these settings to a [`PgPoolOptions`] builder.
    ///
    /// # Example
    /// ```text
    /// let pool = PoolConfig::recommended()
    ///     .apply(PgPoolOptions::new())
    ///     .connect(url)
    ///     .await?;
    /// ```
    pub fn apply(&self, pool_opts: PgPoolOptions) -> PgPoolOptions {
        pool_opts
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(self.acquire_timeout_secs))
            .idle_timeout(std::time::Duration::from_secs(self.idle_timeout_secs))
            .max_lifetime(std::time::Duration::from_secs(self.max_lifetime_secs))
    }
}

// ---------------------------------------------------------------------------
// Batch settler
// ---------------------------------------------------------------------------

/// A single fill-settlement operation representing one trade execution.
///
/// After a trade executes in the matching engine the buyer gains base asset
/// and the seller gains quote asset.  Both sides also have their locked funds
/// reduced by the amounts that were committed when their orders were placed.
#[derive(Debug, Clone)]
pub struct SettlementOp {
    pub buyer_id: Uuid,
    pub seller_id: Uuid,
    pub base_asset: String,
    pub quote_asset: String,
    pub base_qty: Decimal,
    pub quote_cost: Decimal,
    /// Base credited to the buyer (base_qty minus buyer fee).
    pub buyer_credit: Decimal,
    /// Quote credited to the seller (quote_cost minus seller fee).
    pub seller_credit: Decimal,
}

/// Batch settlement buffer.
///
/// Instead of one database round-trip per fill, `BatchSettler` accumulates
/// fills until either `max_batch` entries are queued or the caller decides to
/// flush based on elapsed time.  The resulting batch can then be written in a
/// single transaction, reducing DB round-trips at the cost of up to
/// `max_delay_ms` of additional settlement latency.
///
/// # Usage pattern
/// ```text
/// let mut settler = BatchSettler::new(50, 10);
///
/// // On each fill event:
/// let full = settler.push(op);
/// if full {
///     let batch = settler.flush();
///     db::settle_batch(&pool, batch).await?;
/// }
///
/// // On timer tick (every max_delay_ms):
/// if !settler.is_empty() {
///     let batch = settler.flush();
///     db::settle_batch(&pool, batch).await?;
/// }
/// ```
pub struct BatchSettler {
    pending: Vec<SettlementOp>,
    max_batch: usize,
    /// Stored for reference by callers driving the timer flush.
    pub max_delay_ms: u64,
}

impl BatchSettler {
    /// Create a new `BatchSettler`.
    ///
    /// - `max_batch`: flush when this many operations accumulate.
    /// - `max_delay_ms`: caller's responsibility to flush after this interval
    ///   (the settler itself does not own a timer).
    pub fn new(max_batch: usize, max_delay_ms: u64) -> Self {
        Self {
            pending: Vec::with_capacity(max_batch),
            max_batch,
            max_delay_ms,
        }
    }

    /// Push a settlement operation into the buffer.
    ///
    /// Returns `true` when the buffer has reached `max_batch` capacity,
    /// signalling the caller that it should call [`flush`](Self::flush) and
    /// commit the batch to the database immediately.
    pub fn push(&mut self, op: SettlementOp) -> bool {
        self.pending.push(op);
        self.pending.len() >= self.max_batch
    }

    /// Drain the buffer and return all accumulated operations.
    ///
    /// After this call `is_empty()` returns `true`.
    pub fn flush(&mut self) -> Vec<SettlementOp> {
        std::mem::take(&mut self.pending)
    }

    /// `true` if there are no operations waiting to be flushed.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Number of operations currently buffered.
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_op() -> SettlementOp {
        SettlementOp {
            buyer_id: Uuid::new_v4(),
            seller_id: Uuid::new_v4(),
            base_asset: "BTC".to_owned(),
            quote_asset: "USDT".to_owned(),
            base_qty: dec!(1),
            quote_cost: dec!(50000),
            buyer_credit: dec!(0.999),
            seller_credit: dec!(49975),
        }
    }

    // Test 6: PoolConfig::recommended returns sensible values.
    #[test]
    fn test_pool_config_recommended_sensible() {
        let cfg = PoolConfig::recommended();

        // Must have at least 1 connection.
        assert!(cfg.max_connections >= 1, "max_connections must be >= 1");
        assert!(cfg.min_connections >= 1, "min_connections must be >= 1");

        // min <= max (sanity check — a pool where min > max would deadlock).
        assert!(
            cfg.min_connections <= cfg.max_connections,
            "min_connections ({}) must be <= max_connections ({})",
            cfg.min_connections,
            cfg.max_connections,
        );

        // Acquire timeout must be non-zero so we can fail fast under overload.
        assert!(cfg.acquire_timeout_secs > 0, "acquire_timeout must be > 0");

        // Connections should not live forever (forces periodic reconnects).
        assert!(cfg.max_lifetime_secs > 0, "max_lifetime must be > 0");
        assert!(
            cfg.idle_timeout_secs > 0,
            "idle_timeout must be > 0"
        );

        // max = 2 × cpus, min = cpus (both are at least 2 because available_parallelism >= 1).
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        assert_eq!(cfg.max_connections, cpus * 2);
        assert_eq!(cfg.min_connections, cpus);
    }

    // Test 7: BatchSettler push + flush returns the correct operations.
    #[test]
    fn test_batch_settler_push_and_flush() {
        let mut settler = BatchSettler::new(10, 50);

        assert!(settler.is_empty());
        assert_eq!(settler.len(), 0);

        let op1 = make_op();
        let op2 = make_op();

        settler.push(op1.clone());
        settler.push(op2.clone());

        assert_eq!(settler.len(), 2);
        assert!(!settler.is_empty());

        let flushed = settler.flush();
        assert_eq!(flushed.len(), 2);

        // Buffer is empty after flush.
        assert!(settler.is_empty());
        assert_eq!(settler.len(), 0);

        // Verify the correct buyers came back.
        assert_eq!(flushed[0].buyer_id, op1.buyer_id);
        assert_eq!(flushed[1].buyer_id, op2.buyer_id);
    }

    // Test 8: push returns true (batch full) exactly when max_batch is reached.
    #[test]
    fn test_batch_settler_full_detection() {
        let max = 3;
        let mut settler = BatchSettler::new(max, 10);

        assert!(!settler.push(make_op()), "1 of 3: not full yet");
        assert!(!settler.push(make_op()), "2 of 3: not full yet");
        assert!(settler.push(make_op()), "3 of 3: batch is now full");

        // After flush, pushing again starts a fresh batch.
        settler.flush();
        assert!(!settler.push(make_op()), "1 of 3 in new batch: not full yet");
    }
}
