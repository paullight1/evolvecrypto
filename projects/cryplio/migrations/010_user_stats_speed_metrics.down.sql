BEGIN;

ALTER TABLE user_stats 
DROP COLUMN avg_payment_time,
DROP COLUMN avg_release_time;

COMMIT;
