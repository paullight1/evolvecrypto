-- Materialized view for balance summary (refreshed every minute by cron job)
CREATE MATERIALIZED VIEW IF NOT EXISTS balance_summary AS
SELECT asset,
       COUNT(DISTINCT user_id) as user_count,
       SUM(total) as total_balance,
       SUM(locked) as total_locked,
       SUM(total - locked) as total_available
FROM balances
GROUP BY asset;

CREATE UNIQUE INDEX ON balance_summary (asset);
