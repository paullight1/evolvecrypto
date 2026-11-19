mod config;

use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, Utc};
use exchange_api_gateway::exchange::{
    Announcement, BlockTrade, Competition, CompetitionEntry, DualInvestDirection,
    DualInvestProduct, DualInvestStatus, Exchange, P2pAd,
};
use exchange_api_gateway::rest_engagement::BonusCampaign;
use exchange_api_gateway::rest_products::{
    LaunchpadProject, LaunchpoolPool, LaunchpoolStatus, LiquidityPool,
};
use exchange_common::*;
use exchange_market_data::MarketDataProvider;
use exchange_fix_gateway::tcp_server::{ExchangeAdapter, FixTcpServer, FixTcpServerConfig};
use exchange_settlement::InsuranceFund;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

// ── FIX gateway adapter ──────────────────────────────────────────────────────

/// Wraps `Arc<Exchange>` and implements the FIX gateway's `ExchangeAdapter`
/// trait, delegating directly to the exchange's core order-placement methods.
struct FixExchangeAdapter(Arc<Exchange>);

impl ExchangeAdapter for FixExchangeAdapter {
    fn place_order(&self, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
        self.0.place_order(cmd)
    }

    fn cancel_order(&self, cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
        self.0.cancel_order(cmd)
    }

    fn authenticate_fix(&self, api_key: &str, _api_secret: &str) -> anyhow::Result<UserId> {
        // Look up the API key in the exchange's key store.
        let key_store = self.0.key_store.as_ref()
            .ok_or_else(|| anyhow::anyhow!("API key store not configured"))?;
        let key_record = key_store.get(api_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown API key"))?;

        if !key_record.is_active {
            return Err(anyhow::anyhow!("API key is deactivated"));
        }

        if !key_record.permissions.can_trade {
            return Err(anyhow::anyhow!("API key does not have trading permission"));
        }

        Ok(key_record.user_id)
    }

    fn fix_auth_required(&self) -> bool {
        // S19 D.1: cached field replaces env::var syscall per FIX connection.
        self.0.is_production
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Production safety checks ─────────────────────────────────────────────
    // These run before the tracing subscriber so failures appear on stderr even
    // if logging is not yet configured.
    let is_production = std::env::var("PRODUCTION").unwrap_or_default() == "true";
    let dev_mode = std::env::var("DEV_MODE").unwrap_or_default() == "true";

    if is_production && dev_mode {
        panic!("FATAL: Cannot run DEV_MODE in production. Set PRODUCTION=false or remove DEV_MODE.");
    }
    if is_production && std::env::var("DATABASE_URL").is_err() {
        panic!("FATAL: DATABASE_URL required in production mode.");
    }
    // H3: admin endpoints must be protected in production.
    // The ApiKeyStore is created later, but the env var check can happen now.
    if is_production && std::env::var("API_SECRET_ENCRYPTION_KEY").is_err() {
        panic!(
            "FATAL: API_SECRET_ENCRYPTION_KEY required in production mode. \
             Without it, the ApiKeyStore cannot be created and admin endpoints \
             will be unprotected."
        );
    }
    if is_production && std::env::var("JWT_SECRET").is_err() {
        panic!("FATAL: JWT_SECRET required in production mode.");
    }
    if is_production && std::env::var("WEBHOOK_SECRET").is_err() {
        panic!("FATAL: WEBHOOK_SECRET required in production mode.");
    }
    if is_production && std::env::var("API_SECRET_ENCRYPTION_KEY").is_err() {
        panic!("FATAL: API_SECRET_ENCRYPTION_KEY required in production mode.");
    }
    // P2-4: Sumsub webhook secret — required if Sumsub KYC is configured.
    if is_production && std::env::var("SUMSUB_APP_TOKEN").is_ok()
        && std::env::var("SUMSUB_WEBHOOK_SECRET").is_err()
    {
        panic!("FATAL: SUMSUB_WEBHOOK_SECRET required when Sumsub is configured in production.");
    }
    // ─────────────────────────────────────────────────────────────────────────

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,exchange=debug".parse().unwrap());
    let use_json = std::env::var("LOG_FORMAT").as_deref() == Ok("json")
        || std::env::var("PRODUCTION").as_deref() == Ok("true");

    // P3-O1: OpenTelemetry OTLP exporter — enabled when OTEL_EXPORTER_OTLP_ENDPOINT is set.
    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    if let Some(ref endpoint) = otel_endpoint {
        use opentelemetry::trace::TracerProvider as _;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        use opentelemetry_otlp::WithExportConfig;

        let exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(endpoint);

        let resource = opentelemetry_sdk::Resource::new(vec![
            opentelemetry::KeyValue::new("service.name", "exchange"),
        ]);

        let provider = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(exporter)
            .with_trace_config(
                opentelemetry_sdk::trace::Config::default()
                    .with_resource(resource),
            )
            .install_batch(opentelemetry_sdk::runtime::Tokio)
            .expect("OTLP pipeline install failed");

        let tracer = provider.tracer("exchange");
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(otel_layer);

        if use_json {
            subscriber
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            subscriber
                .with(tracing_subscriber::fmt::layer())
                .init();
        }

        tracing::info!(endpoint = %endpoint, "OpenTelemetry OTLP exporter enabled");
    } else if use_json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    tracing::info!(
        "Starting exchange server in {} mode",
        if dev_mode { "DEVELOPMENT" } else { "PRODUCTION" }
    );

    // P3-5: Warn about recommended secrets that are not configured.
    // Required secrets are already enforced above (panic on missing).
    // This surfaces optional-but-important gaps as early as possible.
    if is_production {
        warn_missing_recommended_secrets();
    }

    // When running without a database (in-memory only) enable dev mode so that
    // the nil-user shortcut in `require_permission` stays active.  Production
    // deployments always set DATABASE_URL and should remove DEV_MODE explicitly.
    // The production guard above already panics if PRODUCTION=true and DATABASE_URL
    // is absent, so this branch is only reachable in dev/test environments.
    if !is_production && std::env::var("DATABASE_URL").is_err() && std::env::var("DEV_MODE").is_err() {
        // Safety: single-threaded at startup; no other threads read this env var yet.
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("DEV_MODE", "1") };
        tracing::warn!("DATABASE_URL not set — enabling DEV_MODE (nil-user auth bypass active)");
    }

    // Connect to PostgreSQL if DATABASE_URL is set
    let db = if let Ok(url) = std::env::var("DATABASE_URL") {
        tracing::info!("Connecting to PostgreSQL...");
        // P1-9: Configurable pool size (default 40, sqlx default was 10).
        let pool_max: u32 = std::env::var("DB_POOL_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40);
        let db = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            exchange_db::Database::connect_with(&url, pool_max),
        )
        .await
        .map_err(|_| anyhow::anyhow!(
            "FATAL: PostgreSQL connection timed out after 10s. \
             Check DATABASE_URL and ensure Postgres is reachable."
        ))??;
        tracing::info!(pool_max, "PostgreSQL pool configured");
        db.migrate().await?;
        Some(db)
    } else {
        tracing::warn!("DATABASE_URL not set — running in-memory only (data lost on restart)");
        None
    };

    // WAL for crash recovery
    let data_dir = std::env::var("EXCHANGE_DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    std::fs::create_dir_all(&data_dir)?;
    let wal_path = format!("{data_dir}/commands.wal");

    let exchange = Exchange::with_journal(&wal_path, None)?;
    tracing::info!("WAL: {}", wal_path);

    // Compliance startup check: warn loudly if the sanctions screener has no
    // entries loaded.  The `with_defaults()` constructor seeds a minimal list;
    // a real deployment should supplement it with the full live OFAC feed via
    // POST /admin/sanctions/load.
    {
        let sanctions = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
        if sanctions.is_empty() {
            tracing::error!(
                "COMPLIANCE: SanctionsScreener has no entries loaded. \
                All users will pass sanctions screening. \
                Load a sanctions list via POST /admin/sanctions/load before going live."
            );
        } else {
            tracing::info!("COMPLIANCE: SanctionsScreener initialized with default entries.");
        }
    }

    // Index price startup check: in production, warn if no index sources are
    // healthy. The per-order health gate will reject perpetual orders until
    // feeds come online, but ops should be alerted immediately.
    if is_production {
        let has_any_healthy = {
            let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
            agg.symbols().iter().any(|sym| agg.calculate_index(sym).is_ok())
        };
        if !has_any_healthy {
            tracing::error!(
                "INDEX PRICE: No healthy index price sources detected at startup. \
                Perpetual trading will be suspended until external price feeds connect. \
                Ensure the live-feeds feature is enabled and feed WebSockets can reach \
                Binance, Coinbase, and Kraken."
            );
        }
    }

    // Register trading pairs
    let products = vec![
        // === PERPETUAL FUTURES (USDT-margined) ===
        Product {
            symbol: Symbol::new("BTCUSDT"),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.10),
            lot_size: dec!(0.001),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("ETHUSDT"),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("SOLUSDT"),
            base_asset: "SOL".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.001),
            lot_size: dec!(0.1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("BNBUSDT"),
            base_asset: "BNB".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("XRPUSDT"),
            base_asset: "XRP".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.0001),
            lot_size: dec!(1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("DOGEUSDT"),
            base_asset: "DOGE".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.00001),
            lot_size: dec!(10),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("AVAXUSDT"),
            base_asset: "AVAX".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("ADAUSDT"),
            base_asset: "ADA".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.0001),
            lot_size: dec!(1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("LINKUSDT"),
            base_asset: "LINK".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.001),
            lot_size: dec!(0.1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("DOTUSDT"),
            base_asset: "DOT".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.001),
            lot_size: dec!(0.1),
            min_notional: dec!(5),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        // === INVERSE (COIN-MARGINED) PERPETUALS ===
        // BTCUSD: 1 contract = 1 USD of BTC, margined in BTC.
        Product {
            symbol: Symbol::new("BTCUSD"),
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.50),
            lot_size: dec!(1),
            min_notional: dec!(1),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        // ETHUSD: 1 contract = 1 USD of ETH, margined in ETH.
        Product {
            symbol: Symbol::new("ETHUSD"),
            base_asset: "ETH".to_string(),
            quote_asset: "USD".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.05),
            lot_size: dec!(1),
            min_notional: dec!(1),
            maker_fee: dec!(0.0002),
            taker_fee: dec!(0.0005),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        // === SPOT MARKETS ===
        Product {
            symbol: Symbol::new("BTC-USDT"),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            min_notional: dec!(10),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("ETH-USDT"),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.01),
            lot_size: dec!(0.0001),
            min_notional: dec!(10),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("SOL-USDT"),
            base_asset: "SOL".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            min_notional: dec!(5),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("BNB-USDT"),
            base_asset: "BNB".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            min_notional: dec!(5),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("XRP-USDT"),
            base_asset: "XRP".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.0001),
            lot_size: dec!(1),
            min_notional: dec!(5),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("DOGE-USDT"),
            base_asset: "DOGE".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.00001),
            lot_size: dec!(10),
            min_notional: dec!(5),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("ETH-BTC"),
            base_asset: "ETH".to_string(),
            quote_asset: "BTC".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.000001),
            lot_size: dec!(0.001),
            min_notional: dec!(0.0001),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
        Product {
            symbol: Symbol::new("SOL-BTC"),
            base_asset: "SOL".to_string(),
            quote_asset: "BTC".to_string(),
            product_type: ProductType::Spot,
            tick_size: dec!(0.00000001),
            lot_size: dec!(0.01),
            min_notional: dec!(0.0001),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.001),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
        },
    ];

    for product in products {
        tracing::info!("Registered {} ({:?})", product.symbol, product.product_type);
        exchange.add_product(product);
    }

    // Create test account with funds (in-memory always, DB if connected)
    let test_user = Uuid::nil();
    exchange.create_account(test_user);
    exchange.deposit(test_user, "USDT", dec!(1_000_000))?;
    exchange.deposit(test_user, "BTC", dec!(100))?;
    exchange.deposit(test_user, "ETH", dec!(1000))?;
    exchange.deposit(test_user, "SOL", dec!(10000))?;

    if let Some(ref db) = db {
        // Enable write-behind ledger: all financial ops now persist to PostgreSQL.
        exchange.set_db_pool(db.pool.clone());

        // P0-3: Continue audit log hash chain from the last persisted entry.
        {
            let last_hash = exchange_db::audit_repo::AuditRepo::last_entry_hash(&db.pool)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "Failed to read last audit hash from DB — starting chain from genesis");
                    "genesis".to_string()
                });
            let next_id = exchange_db::audit_repo::AuditRepo::count(&db.pool)
                .await
                .map(|n| (n as u64) + 1)
                .unwrap_or(1);
            let new_log = exchange_admin::AuditLog::continuing_from(last_hash, next_id);
            *exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner()) = new_log;
            tracing::info!(next_id, "Audit log chain resumed from DB state");
        }

        // Seed DB with test user + balances
        let pool = &db.pool;
        let _ = exchange_db::user_repo::UserRepo::create(pool, test_user, Some("test@exchange.ua")).await;
        let _ = exchange_db::balance_repo::BalanceRepo::deposit(pool, test_user, "USDT", dec!(1_000_000)).await;
        let _ = exchange_db::balance_repo::BalanceRepo::deposit(pool, test_user, "BTC", dec!(100)).await;
        let _ = exchange_db::balance_repo::BalanceRepo::deposit(pool, test_user, "ETH", dec!(1000)).await;
        let _ = exchange_db::balance_repo::BalanceRepo::deposit(pool, test_user, "SOL", dec!(10000)).await;
        tracing::info!("Test account seeded in PostgreSQL");

        // Load existing state from DB
        exchange.load_from_db(&db.pool).await?;

        // P0-4: Hydrate in-memory KYC manager from Postgres.
        {
            let rows = exchange_db::kyc_repo::KycRepo::list_all(&db.pool)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "Failed to load KYC records from DB — starting with empty KYC state");
                    Vec::new()
                });
            if !rows.is_empty() {
                let records: Vec<exchange_compliance::kyc::KycRecord> = rows
                    .into_iter()
                    .filter_map(|row| kyc_row_to_record(row))
                    .collect();
                let count = records.len();
                let mut svc = exchange.verification_service.lock().await;
                svc.restore_records(records);
                // Also populate the sync-accessible kyc_approved DashSet.
                for r in svc.all_records() {
                    if r.status == exchange_compliance::kyc::KycStatus::Verified {
                        exchange.kyc_approved.insert(r.user_id);
                    }
                }
                tracing::info!(count, "KYC records loaded from DB");
            }
        }
    }

    tracing::info!("Created test account with funds");

    // WAL replay: rebuild order book state from the write-ahead log.
    // Terminal orders (filled/cancelled in DB) are skipped.
    {
        let terminal_ids: std::collections::HashSet<uuid::Uuid> = if let Some(ref db) = db {
            exchange_db::order_repo::OrderRepo::terminal_order_ids(&db.pool)
                .await
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };

        // P1-D3: Load engine snapshot if available (cuts replay from O(all) to O(recent)).
        let snapshot_path = std::path::PathBuf::from(&data_dir)
            .join("snapshots")
            .join("engine_snapshot.json");
        let engine_snap = exchange_api_gateway::engine_snapshot::load_snapshot(&snapshot_path)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to load engine snapshot — full WAL replay");
                None
            });
        let replay_after_seq = if let Some(ref snap) = engine_snap {
            // Restore order books from the snapshot before WAL replay.
            // Note: the snapshot was taken from engine threads via the command
            // channel. We can't restore directly into the running engine threads
            // (they already have empty books). Instead, we skip the snapshot-
            // covered WAL entries and only replay the delta.
            tracing::info!(
                symbols = snap.books.len(),
                wal_sequence = snap.wal_sequence,
                created_at = %snap.created_at,
                "Engine snapshot loaded — replaying WAL from sequence {}",
                snap.wal_sequence,
            );
            snap.wal_sequence
        } else {
            tracing::info!("No engine snapshot found — full WAL replay");
            0
        };

        // Replay from the segmented WAL directory (Exchange::with_journal
        // creates data/wal/ from the legacy data/commands.wal path).
        let wal_dir = {
            let p = std::path::Path::new(&wal_path);
            if p.is_dir() { p.to_path_buf() } else {
                p.parent().unwrap_or(p).join("wal")
            }
        };
        let commands = if wal_dir.exists() {
            let seg_wal = exchange_journal::SegmentedWal::open(&wal_dir)
                .map_err(|e| anyhow::anyhow!("SegmentedWal open for replay failed: {e}"))?;
            seg_wal.replay_after(replay_after_seq)
                .map_err(|e| anyhow::anyhow!("SegmentedWal replay failed — refusing to start: {e}"))?
        } else {
            // Fallback: try legacy single-file WAL for backward compatibility.
            exchange_journal::WriteAheadLog::replay_from(&wal_path, replay_after_seq)
                .map_err(|e| anyhow::anyhow!("WAL replay_from failed — refusing to start: {e}"))?
        };

        if !commands.is_empty() {
            // Extract position snapshot BEFORE consuming commands (replay takes ownership).
            let snapshot_positions = exchange_journal::extract_latest_position_snapshot(&commands);

            let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
            let result = exchange_journal::replay_into_pool(
                commands,
                &pool,
                |order_id| terminal_ids.contains(order_id),
            );
            tracing::info!(
                replayed = result.commands_replayed,
                skipped = result.commands_skipped,
                errors = result.errors.len(),
                "WAL replay complete"
            );

            if !result.errors.is_empty() {
                let allow = std::env::var("ALLOW_WAL_REPLAY_ERRORS").is_ok();
                for (seq, desc) in &result.errors {
                    tracing::error!(seq, error = %desc, "WAL replay error");
                }
                if !allow {
                    anyhow::bail!(
                        "WAL replay had {} error(s) — refusing to start. \
                         Set ALLOW_WAL_REPLAY_ERRORS=1 to override (data may be inconsistent).",
                        result.errors.len(),
                    );
                }
                tracing::warn!(
                    "ALLOW_WAL_REPLAY_ERRORS set — proceeding despite {} replay error(s)",
                    result.errors.len(),
                );
            }

            // Restore futures positions from the latest WAL snapshot.
            if !snapshot_positions.is_empty() {
                let mut pm = exchange.positions.lock();
                for pos in snapshot_positions {
                    pm.restore_position(pos);
                }
                let count = pm.all_positions().len();
                tracing::info!(
                    count = count,
                    "Restored futures positions from WAL snapshot"
                );
            }
        }
    }

    // Demo mode: seed realistic orderbook, products, and start background trader
    if std::env::var("DEMO_MODE").is_ok() {
        exchange.is_demo.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("Demo mode enabled — seeding market data and products");
        seed_demo_products(&exchange);
        // Reset rate limiter before heavy orderbook seeding (200 orders per symbol × 13 symbols).
        exchange.rate_limiter.reset();
        seed_demo_orderbook(&exchange);
        exchange.rate_limiter.reset();
        seed_demo_candles(&exchange);
        spawn_demo_trader(Arc::clone(&exchange));
        spawn_demo_price_feed(Arc::clone(&exchange));
        tracing::info!("Demo mode fully initialized");
    } else {
        // Production / non-demo: start live WebSocket price feeds (Binance, Coinbase, Kraken).
        // These feed into the index price aggregator which drives mark price, funding rate,
        // and liquidation calculations for perpetual contracts.
        // P1-2: Live index feed bridge.
        // Feeds run in async tasks with tokio channels. A bridge task reads
        // PriceUpdate from the merged channel and writes to the Exchange's
        // std::sync::Mutex<IndexPriceAggregator>.
        {
            let ex = Arc::clone(&exchange);
            // P1-3: Derive feed symbols from registered perpetual products.
            // S17: perp_configs migrated from Mutex<HashMap> to ArcSwap<HashMap>
            // — lock-free atomic read via load_full().
            let feed_symbols: Vec<String> = {
                let configs = exchange.perp_configs.load_full();
                let mut syms: Vec<String> = configs.keys().map(|s| s.to_string()).collect();
                if syms.is_empty() {
                    syms = vec!["BTCUSDT".into(), "ETHUSDT".into(), "SOLUSDT".into()];
                }
                syms
            };
            tracing::info!(symbols = ?feed_symbols, "Subscribing index feeds to product symbols");
            let (feed_handles, mut price_rx) =
                exchange_index_price::feeds::spawner::spawn_all_feeds_channel(feed_symbols);

            // Bridge task: channel → std::sync::Mutex aggregator.
            tokio::spawn(async move {
                while let Some(update) = price_rx.recv().await {
                    let mut agg = ex.index_aggregator
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    agg.update_source_price(
                        &update.symbol,
                        &update.source,
                        update.price,
                        update.timestamp,
                    );
                }
                tracing::warn!("Live price feed bridge exiting — all feeds disconnected");
            });

            // Keep feed handles alive (they auto-reconnect internally).
            tokio::spawn(async move {
                for h in feed_handles {
                    let _ = h.await;
                }
            });
        }
        tracing::info!("Live index price feeds started (Binance, Coinbase, Kraken)");
    }

    // Start FIX 4.4 TCP gateway (port 8081) in a background task.
    // The FIX server is non-blocking: it runs independently and does not
    // prevent the HTTP server from starting even if the FIX bind fails.
    {
        let fix_exchange = Arc::clone(&exchange);
        let fix_port = std::env::var("FIX_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8081);
        let fix_cfg = FixTcpServerConfig {
            bind_addr: format!("0.0.0.0:{fix_port}"),
            heartbeat_interval: 30,
            target_comp_id: "EXCHANGE".to_string(),
        };
        // P3-2: Check TLS posture before starting. If FIX_REQUIRE_TLS=true this
        // returns an error and we skip the gateway entirely.
        match FixTcpServer::check_tls_requirement() {
            Err(e) => {
                tracing::error!(error = %e, "FIX TCP server not started due to TLS policy");
            }
            Ok(()) => match FixTcpServer::new(fix_cfg) {
                Ok(fix_server) => {
                    let adapter = Arc::new(FixExchangeAdapter(fix_exchange));
                    tokio::spawn(async move {
                        if let Err(e) = fix_server.start(adapter).await {
                            tracing::error!(error = %e, "FIX TCP server terminated");
                        }
                    });
                    tracing::info!(port = fix_port, "FIX 4.4 TCP gateway started");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to create FIX TCP server — FIX gateway disabled");
                }
            },
        }
    }

    // Start HTTP + WebSocket server
    let host = "0.0.0.0";
    let port = 8080;

    exchange_api_gateway::server::run_with_tasks(host, port, exchange).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// P3-5: Startup secrets validation
// ---------------------------------------------------------------------------

/// Warn about recommended secrets that are absent in production.
///
/// Required secrets (`JWT_SECRET`, `WEBHOOK_SECRET`, `DATABASE_URL`,
/// `API_SECRET_ENCRYPTION_KEY`) are enforced with `panic!` earlier in `main`.
/// This function covers recommended-but-not-required vars whose absence means
/// certain features will be degraded or unavailable.
/// Convert a DB row to an in-memory KycRecord. Returns None if the
/// level/status strings don't parse (e.g. corrupted data).
fn kyc_row_to_record(row: exchange_db::kyc_repo::KycRow) -> Option<exchange_compliance::kyc::KycRecord> {
    use exchange_compliance::kyc::{KycLevel, KycStatus};

    let level = match row.level.as_str() {
        "None" => KycLevel::None,
        "Basic" => KycLevel::Basic,
        "Full" => KycLevel::Full,
        other => {
            tracing::warn!(user_id = %row.user_id, level = other, "Unknown KYC level in DB — skipping");
            return None;
        }
    };
    let status = match row.status.as_str() {
        "Unverified" => KycStatus::Unverified,
        "Pending" => KycStatus::Pending,
        "Verified" => KycStatus::Verified,
        "Rejected" => KycStatus::Rejected,
        "Suspended" => KycStatus::Suspended,
        other => {
            tracing::warn!(user_id = %row.user_id, status = other, "Unknown KYC status in DB — skipping");
            return None;
        }
    };

    Some(exchange_compliance::kyc::KycRecord {
        user_id: row.user_id,
        level,
        status,
        provider_ref: row.provider_ref,
        reason: row.reason,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn warn_missing_recommended_secrets() {
    let recommended = [
        ("CUSTODY_BASE_URL",        "custody deposits/withdrawals will be unavailable"),
        ("CUSTODY_HMAC_SECRET",     "custody webhook verification will be skipped"),
        ("SMTP_HOST",               "email notifications will be disabled"),
        ("GRAFANA_ADMIN_PASSWORD",  "Grafana monitoring dashboard will use default credentials"),
    ];
    for (var, reason) in &recommended {
        if std::env::var(var).is_err() {
            tracing::warn!("{var} is not set — {reason}");
        }
    }
}

// --- Demo mode ---

/// Fixed UUIDs for the two demo accounts.
/// These are stable, well-known identifiers for demo maker/taker so they
/// can be identified in logs and are never confused with real user accounts.
/// (Nil UUID + 1 and + 2, with a distinctive prefix 0xDE_00 to mark them as demo.)
const DEMO_MAKER_ID: u128 = 0x0000_DE00_0000_0001_0000_0000_0000_0001;
const DEMO_USER_3: u128 = 0x0000_DE00_0000_0003_0000_0000_0000_0003;
const DEMO_USER_4: u128 = 0x0000_DE00_0000_0004_0000_0000_0000_0004;
const DEMO_USER_5: u128 = 0x0000_DE00_0000_0005_0000_0000_0000_0005;
const DEMO_USER_6: u128 = 0x0000_DE00_0000_0006_0000_0000_0000_0006;
#[allow(dead_code)]
const DEMO_USER_7: u128 = 0x0000_DE00_0000_0007_0000_0000_0000_0007;
const DEMO_TAKER_ID: u128 = 0x0000_DE00_0000_0002_0000_0000_0000_0002;

/// Seed 200 historical 1-minute candles per symbol into the MarketDataService
/// so the chart has data immediately on load, before any real trades occur.
///
/// Prices walk deterministically using integer arithmetic — no f64 involved.
/// The walk oscillates around the mid price with ±0.1% amplitude.
fn seed_demo_candles(exchange: &Arc<Exchange>) {
    // (symbol, mid_price_cents, price_dp)
    // cents here means price × 10^price_dp, matching format_price conventions.
    let symbols: &[(&str, u64, usize)] = &[
        ("BTCUSDT",  6_750_000, 2), // $67,500.00
        ("ETHUSDT",    345_000, 2), // $ 3,450.00
        ("SOLUSDT",    145_000, 3), // $   145.000
        ("BNBUSDT",     58_000, 2), // $   580.00
        ("XRPUSDT",      1_450, 4), // $     1.4500
        ("DOGEUSDT",       850, 5), // $     0.00850
        ("AVAXUSDT",     3_500, 2), // $    35.00
        ("ADAUSDT",        450, 4), // $     0.4500
        ("LINKUSDT",     1_450, 3), // $    14.500
        ("DOTUSDT",        750, 3), // $     7.500
        ("BTC-USDT", 6_750_000, 2), // $67,500.00
        ("ETH-USDT",   345_000, 2), // $ 3,450.00
        ("SOL-USDT",   145_000, 3), // $   145.000
    ];

    let now = chrono::Utc::now();

    for &(sym, mid_cents, price_dp) in symbols.iter() {
        let symbol = Symbol::new(sym);
        // Choose a plausible trade quantity based on price magnitude.
        // High-price assets (BTC) trade in small fractions; low-price assets in larger lots.
        let qty = if mid_cents >= 1_000_000 {
            dec!(0.001)   // BTC-range
        } else if mid_cents >= 100_000 {
            dec!(0.01)    // ETH-range
        } else if mid_cents >= 10_000 {
            dec!(0.1)     // SOL / BNB-range
        } else {
            dec!(1.0)     // XRP / DOGE / ADA-range
        };

        // Generate 200 minutes of history plus a trade in the current minute.
        // i=199 is the oldest minute, i=0 is the just-closed minute.
        for i in (0u64..=200).rev() {
            let ts = now - chrono::Duration::minutes(i as i64);

            // Deterministic price walk: oscillate ±0.1% around mid using a simple
            // sine-approximation built from integer arithmetic.
            // offset cycles through [0, 100) with step 7, then maps to [-50, +50].
            let phase = (i.wrapping_mul(7)) % 100;
            // amplitude: 0.1% of mid = mid_cents / 1000
            let amplitude = mid_cents / 1000;
            let offset_signed: i64 = (phase as i64 - 50) * amplitude as i64 / 50;
            let price_cents = (mid_cents as i64 + offset_signed) as u64;

            // Ensure price never hits zero (shouldn't happen with 0.1% amplitude).
            if price_cents == 0 {
                continue;
            }

            // Generate 5 trades per candle to create real OHLC shape:
            // open → high → low → close (with slight variations)
            let high_offset = amplitude / 3;  // ~0.03% above base
            let low_offset = amplitude / 4;   // ~0.025% below base
            let close_jitter: i64 = ((i.wrapping_mul(13) % 100) as i64 - 50) * amplitude as i64 / 150;

            let open_cents = price_cents;
            let high_cents = price_cents + high_offset;
            let low_cents = if price_cents > low_offset { price_cents - low_offset } else { price_cents };
            let close_cents = (price_cents as i64 + close_jitter) as u64;

            let prices = [open_cents, high_cents, low_cents, close_cents, close_cents];
            for (j, &pc) in prices.iter().enumerate() {
                if pc == 0 { continue; }
                let trade_ts = ts + chrono::Duration::seconds(j as i64 * 10);
                let trade_price = format_price(pc, price_dp);
                // Alternate taker side so delta data is realistic from the start.
                let taker_side = if j % 2 == 0 { Side::Buy } else { Side::Sell };
                exchange.market_data.on_trade(&symbol, trade_price, qty, taker_side, trade_ts);
            }
        }

        tracing::debug!("Seeded 200 historical candles for {sym}");
    }

    tracing::info!("Demo candle history seeded for {} symbols", symbols.len());
}

/// Seed the order book with 50 bid + 50 ask levels for each demo symbol.
///
/// Prices are calculated as plain integer arithmetic in "cents" (or the
/// appropriate sub-unit) and formatted to the correct tick-size precision,
/// then parsed into `Decimal`. This avoids any f64 involvement in price
/// construction.
fn seed_demo_orderbook(exchange: &Arc<Exchange>) {
    // Use TWO separate accounts for bids/asks to avoid self-trade prevention (STP).
    let bid_maker = Uuid::from_u128(DEMO_MAKER_ID);
    let ask_maker = Uuid::from_u128(0x0000_DE00_0000_0001_0000_0000_0000_0002);
    exchange.create_account(bid_maker);
    exchange.create_account(ask_maker);

    // Bid maker needs USDT (for both spot buys and perp margin).
    exchange.deposit(bid_maker, "USDT", dec!(500_000_000)).ok();

    // Ask maker needs USDT (for perp margin) and base assets (for spot sells).
    exchange.deposit(ask_maker, "USDT", dec!(500_000_000)).ok();
    for asset in ["BTC", "ETH", "SOL", "BNB", "XRP", "DOGE", "AVAX", "ADA", "LINK", "DOT"] {
        exchange.deposit(ask_maker, asset, dec!(1_000_000_000)).ok();
    }

    // Temporarily disable rate limiting for heavy orderbook seeding.
    // We'll reset the limiter between each symbol batch.
    // Alternative: the seed loop calls place_order directly which hits the rate limiter.

    // (symbol, mid_price_cents, level_spacing_cents, tick_decimals, base_qty_base, qty_step_base)
    // Prices are stored as integer cents so we can use pure integer arithmetic.
    // tick_decimals: number of decimal places in the formatted price string.
    let symbols: &[(&str, u64, u64, usize, &str, &str)] = &[
        // BTC: mid $67500.00, $0.50 per level, 2 dp
        ("BTCUSDT",  6_750_000, 50, 2, "0.01000", "0.00005"),
        // ETH: mid $3450.00, $0.05 per level, 2 dp
        ("ETHUSDT",    345_000,  5, 2, "0.1000",  "0.0005"),
        // SOL: mid $145.000, $0.005 per level, 3 dp
        ("SOLUSDT",    145_000,  5, 3, "1.00",    "0.01"),
        // BNB: mid $580.00, $0.05 per level, 2 dp
        ("BNBUSDT",     58_000,  5, 2, "0.50",    "0.005"),
        // XRP: mid $1.4500, $0.0001 per level, 4 dp
        ("XRPUSDT",      1_450,  1, 4, "100.0",   "1.0"),
        // DOGE: mid $0.08500, $0.00001 per level, 5 dp
        ("DOGEUSDT",       850,  1, 5, "1000.0",  "10.0"),
        // AVAX: mid $35.00, $0.01 per level, 2 dp
        ("AVAXUSDT",     3_500,  1, 2, "5.0",     "0.05"),
        // ADA: mid $0.4500, $0.0001 per level, 4 dp
        ("ADAUSDT",        450,  1, 4, "500.0",   "5.0"),
        // LINK: mid $14.500, $0.001 per level, 3 dp
        ("LINKUSDT",     1_450,  1, 3, "10.0",    "0.1"),
        // DOT: mid $7.500, $0.001 per level, 3 dp
        ("DOTUSDT",        750,  1, 3, "20.0",    "0.2"),
        // Spot pairs
        ("BTC-USDT", 6_750_000, 50, 2, "0.01000", "0.00005"),
        ("ETH-USDT",   345_000,  5, 2, "0.1000",  "0.0005"),
        ("SOL-USDT",   145_000,  5, 3, "1.00",    "0.01"),
    ];

    for &(sym, mid_cents, spacing_cents, price_dp, qty_base, qty_step) in symbols {
        // Reset rate limiter for each symbol to avoid hitting the 50/sec limit
        // (each symbol places 200 orders: 100 bids + 100 asks).
        exchange.rate_limiter.reset();

        let qty_base = Decimal::from_str(qty_base).expect("valid qty_base");
        let qty_step = Decimal::from_str(qty_step).expect("valid qty_step");

        for level in 1u64..=100 {
            // Quantity: more near mid, less far out. Never goes negative.
            let qty = (qty_base - qty_step * Decimal::from(level - 1)).max(Decimal::new(1, 4));

            let bid_cents = mid_cents - level * spacing_cents;
            let ask_cents = mid_cents + level * spacing_cents;

            let bid_price = format_price(bid_cents, price_dp);
            let ask_price = format_price(ask_cents, price_dp);

            // Bid
            exchange.place_order(PlaceOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: bid_maker,
                symbol: Symbol::new(sym),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: bid_price,
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
            }).ok();

            // Ask
            let ask_result = exchange.place_order(PlaceOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: ask_maker,
                symbol: Symbol::new(sym),
                side: Side::Sell,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: ask_price,
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
            });
            if level == 1 {
                if let Err(e) = &ask_result {
                    eprintln!("SEED ASK FAIL {sym}: {e}");
                }
            }
            drop(ask_result);
        }

        tracing::info!("Seeded 100 orderbook levels for {sym}");
    }
}

/// Format a price stored as integer "micro-units" (cents × scale) into a
/// `Decimal` with the correct number of decimal places.
///
/// `price_cents` here is the price multiplied by 10^dp, so that integer
/// arithmetic gives exact results.
///
/// Examples:
/// - BTC bid_cents = 6_749_950, dp = 2 → "67499.50" → Decimal
/// - SOL ask_cents = 145_005, dp = 3 → "145.005" → Decimal
fn format_price(price_cents: u64, dp: usize) -> Decimal {
    let scale = 10u64.pow(dp as u32);
    let integer_part = price_cents / scale;
    let frac_part = price_cents % scale;
    let s = format!("{integer_part}.{frac_part:0>dp$}");
    Decimal::from_str(&s).expect("format_price produced a valid decimal string")
}

/// Spawn a background task that continuously places small market orders to
/// simulate real taker flow and keep the terminal looking alive.
///
/// Uses deterministic counter-based "randomness" — no external crates needed.
fn spawn_demo_trader(exchange: Arc<Exchange>) {
    tokio::spawn(async move {
        let demo_taker = Uuid::from_u128(DEMO_TAKER_ID);
        let demo_maker = Uuid::from_u128(DEMO_MAKER_ID);

        exchange.create_account(demo_taker);
        exchange.deposit(demo_taker, "USDT", dec!(500_000_000)).ok();

        // (symbol, mid_cents, spacing_cents, price_dp, base_qty_str, qty_step_str)
        // Mirrors the seeding table so replenishment offsets make sense.
        let symbols: &[(&str, u64, u64, usize, &str, &str)] = &[
            ("BTCUSDT", 6_750_000, 50, 2, "0.00010", "0.000005"),
            ("ETHUSDT",   345_000,  5, 2, "0.00100", "0.000050"),
            ("SOLUSDT",   145_000,  5, 3, "0.01000", "0.000500"),
        ];

        let mut counter: u64 = 0;

        loop {
            // Deterministic delay: 500ms + up to 2500ms based on counter.
            // Pattern: 500, 637, 774, ... cycles every 2500ms range.
            let delay_ms = 500 + (counter.wrapping_mul(137) % 2500);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;

            let sym_idx = (counter % 3) as usize;
            let &(sym, mid_cents, spacing_cents, price_dp, base_qty_str, qty_step_str) = &symbols[sym_idx];

            let side = if counter % 2 == 0 { Side::Buy } else { Side::Sell };

            // Small taker quantity: varies across 20 steps then repeats.
            let step = counter % 20;
            let base_qty = Decimal::from_str(base_qty_str).unwrap_or(dec!(0.01));
            let qty_step = Decimal::from_str(qty_step_str).unwrap_or(dec!(0.001));
            let qty = (base_qty + qty_step * Decimal::from(step)).max(Decimal::new(1, 5));

            let cmd = PlaceOrderCommand {
                order_id: Uuid::new_v4(),
                user_id: demo_taker,
                symbol: Symbol::new(sym),
                side,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Ioc,
                price: Decimal::ZERO,
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
            };

            if let Ok(events) = exchange.place_order(cmd) {
                // Replenish each consumed level so the book never runs dry.
                for event in &events {
                    if let MatchEvent::Fill(fill) = event {
                        // The taker consumed a maker level — put a fresh limit
                        // order back on the maker's side one tick inside the
                        // previous level so the spread stays tight.
                        let replenish_side = fill.taker_side.opposite();

                        // Offset by one spacing unit back toward mid.
                        let price_offset_cents = spacing_cents;
                        let replenish_price_cents = if replenish_side == Side::Buy {
                            // Consumed an ask → replenish a bid just below fill price
                            let fill_cents = decimal_to_cents(fill.price, price_dp);
                            fill_cents.saturating_sub(price_offset_cents)
                        } else {
                            // Consumed a bid → replenish an ask just above fill price
                            let fill_cents = decimal_to_cents(fill.price, price_dp);
                            fill_cents + price_offset_cents
                        };

                        // Skip degenerate prices (shouldn't happen, but be safe).
                        if replenish_price_cents == 0 {
                            continue;
                        }

                        // Guard: replenish price must not cross mid by more than the
                        // outer boundary (100 × spacing). This prevents runaway drift.
                        let max_drift = mid_cents + 100 * spacing_cents;
                        let min_drift = mid_cents.saturating_sub(100 * spacing_cents);
                        if replenish_price_cents > max_drift || replenish_price_cents < min_drift {
                            continue;
                        }

                        let replenish_price = format_price(replenish_price_cents, price_dp);

                        exchange.place_order(PlaceOrderCommand {
                            order_id: Uuid::new_v4(),
                            user_id: demo_maker,
                            symbol: fill.symbol.clone(),
                            side: replenish_side,
                            order_type: OrderType::Limit,
                            time_in_force: TimeInForce::Gtc,
                            price: replenish_price,
                            qty: fill.qty,
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
                        }).ok();

                        // Also replenish 3 buffer levels further from mid so the
                        // book stays deep even under sustained taker pressure.
                        for offset in 1u64..=3 {
                            let buffer_cents = if replenish_side == Side::Buy {
                                replenish_price_cents.saturating_sub(offset * spacing_cents)
                            } else {
                                replenish_price_cents + offset * spacing_cents
                            };
                            if buffer_cents == 0
                                || buffer_cents > max_drift
                                || buffer_cents < min_drift
                            {
                                continue;
                            }
                            let buffer_price = format_price(buffer_cents, price_dp);
                            exchange.place_order(PlaceOrderCommand {
                                order_id: Uuid::new_v4(),
                                user_id: demo_maker,
                                symbol: fill.symbol.clone(),
                                side: replenish_side,
                                order_type: OrderType::Limit,
                                time_in_force: TimeInForce::Gtc,
                                price: buffer_price,
                                qty: fill.qty,
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
                            }).ok();
                        }
                    }
                }
            }

            // Periodic book refill: every ~30 seconds (counter increments every
            // ~0.5-3s so 30 iterations ≈ 30s) check if either side has fewer
            // than 20 levels and seed more liquidity if needed.
            if counter % 30 == 0 {
                let &(sym, mid_cents, spacing_cents, price_dp, base_qty_str, _qty_step_str) =
                    &symbols[sym_idx];
                let base_qty = Decimal::from_str(base_qty_str).unwrap_or(dec!(0.01));
                let symbol = Symbol::new(sym);

                let (bid_count, ask_count) = {
                    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                    match pool.snapshot(&symbol, 200) {
                        Ok(snap) => (snap.bids.len(), snap.asks.len()),
                        Err(_) => (0, 0),
                    }
                };

                if bid_count < 20 || ask_count < 20 {
                    tracing::debug!(
                        "Book refill triggered for {sym}: bids={bid_count} asks={ask_count}"
                    );
                    // Add fresh levels on the thin side(s).
                    for side in [Side::Buy, Side::Sell] {
                        let count = if side == Side::Buy { bid_count } else { ask_count };
                        if count >= 20 {
                            continue;
                        }
                        let levels_needed = (40 - count) as u64;
                        for lvl in 1..=levels_needed {
                            let price_cents = if side == Side::Buy {
                                mid_cents.saturating_sub(lvl * spacing_cents)
                            } else {
                                mid_cents + lvl * spacing_cents
                            };
                            if price_cents == 0 {
                                continue;
                            }
                            let price = format_price(price_cents, price_dp);
                            exchange.place_order(PlaceOrderCommand {
                                order_id: Uuid::new_v4(),
                                user_id: demo_maker,
                                symbol: symbol.clone(),
                                side,
                                order_type: OrderType::Limit,
                                time_in_force: TimeInForce::Gtc,
                                price,
                                qty: base_qty,
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
                            }).ok();
                        }
                    }
                }
            }

            counter = counter.wrapping_add(1);
        }
    });
}

/// Convert a `Decimal` price back to its integer cent representation so we can
/// do integer arithmetic for replenishment offsets.
///
/// `dp` must match the number of decimal places used when the price was formed.
fn decimal_to_cents(price: Decimal, dp: usize) -> u64 {
    let scale = Decimal::from(10u64.pow(dp as u32));
    let cents = (price * scale).round();
    u64::try_from(cents.mantissa().unsigned_abs()).unwrap_or(0)
}

/// Spawn a background task that feeds simulated spot prices into the index
/// aggregator and updates the market-data ticker with mark/index/funding data.
///
/// Runs only in DEMO_MODE. Prices walk ±0.05 % per second using a
/// deterministic integer phase so the aggregator always has ≥ 2 fresh sources
/// (the minimum required by `IndexPriceAggregator::with_defaults`).
fn spawn_demo_price_feed(exchange: Arc<Exchange>) {
    tokio::spawn(async move {
        // (symbol_str, base_price_cents, price_dp)
        // base_price_cents = price × 10^price_dp  (pure integer, no f64 in price math)
        let symbols_and_prices: &[(&str, u64, u32)] = &[
            ("BTCUSDT",  6_750_000_u64, 2),  // $67 500.00
            ("ETHUSDT",    345_000_u64, 2),  // $ 3 450.00
            ("SOLUSDT",    145_000_u64, 3),  // $   145.000
            ("BNBUSDT",     58_000_u64, 2),  // $   580.00
            ("XRPUSDT",      1_450_u64, 4),  // $     1.4500
            ("DOGEUSDT",       850_u64, 5),  // $     0.08500
            ("AVAXUSDT",     3_500_u64, 2),  // $    35.00
            ("ADAUSDT",        450_u64, 4),  // $     0.4500
            ("LINKUSDT",     1_450_u64, 3),  // $    14.500
            ("DOTUSDT",        750_u64, 3),  // $     7.500
        ];

        // Four sources matching the names expected by `IndexPriceAggregator::with_defaults`.
        // Small integer offsets give each source a slightly different price,
        // satisfying the weighted-median requirement for multiple sources.
        //
        // Source offsets in units of 10^-dp (i.e. the smallest price increment):
        // Binance: -2, Coinbase: -1, Kraken: +1, OKX: +2
        let source_offsets: &[(&str, i64)] = &[
            ("Binance",  -2),
            ("Coinbase", -1),
            ("Kraken",    1),
            ("OKX",       2),
        ];

        let mut counter: u64 = 0;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(1_000)).await;

            let now = chrono::Utc::now();

            // --- Feed index aggregator ---
            {
                let mut agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());

                for &(sym, base_cents, dp) in symbols_and_prices {
                    let symbol = Symbol::new(sym);
                    let scale = 10_u64.pow(dp);

                    // Sinusoidal price walk: period ≈ 200 ticks, amplitude 0.05 %.
                    // amplitude_cents = base_cents * 0.0005 (0.05 %) — integer division is fine
                    // since base_cents for BTC is 6_750_000 → amplitude = 3_375.
                    let amplitude_cents = (base_cents / 2000).max(1);
                    let phase = counter.wrapping_mul(7) % 200;
                    // Maps [0, 200) → [-amplitude, +amplitude]
                    let offset_cents = (phase as i64 - 100) * amplitude_cents as i64 / 100;

                    for &(source, source_offset) in source_offsets {
                        let price_cents = (base_cents as i64 + offset_cents + source_offset)
                            .max(1) as u64;
                        let integer_part = price_cents / scale;
                        let frac_part = price_cents % scale;
                        let price_str = format!(
                            "{}.{:0>width$}",
                            integer_part,
                            frac_part,
                            width = dp as usize
                        );
                        if let Ok(price_dec) = Decimal::from_str(&price_str) {
                            agg.update_source_price(&symbol, source, price_dec, now);
                        }
                    }
                }
            }

            // --- Update market-data ticker with mark / index / funding data ---
            {
                let next_funding_ms = exchange
                    .funding_scheduler
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .next_funding_time(now)
                    .ok()
                    .map(|t| t.timestamp_millis())
                    .unwrap_or(0);

                for &(sym, _, _) in symbols_and_prices {
                    let symbol = Symbol::new(sym);

                    let index_price = {
                        let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                        match agg.calculate_index(&symbol) {
                            Ok(ip) => ip.price,
                            Err(_) => continue,
                        }
                    };

                    // Simplified: mark price ≈ index price in demo mode.
                    let mark_price = index_price;

                    let funding_rate = exchange
                        .funding_history
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&symbol)
                        .and_then(|h| h.latest_rate())
                        .map(|r| r.rate)
                        .unwrap_or(rust_decimal::Decimal::ZERO);

                    // Open interest: sum of all open position sizes for this symbol.
                    let open_interest = {
                        let pm = exchange.positions.lock();
                        pm.all_positions()
                            .iter()
                            .filter(|p| p.symbol == symbol)
                            .map(|p| p.size)
                            .sum::<rust_decimal::Decimal>()
                    };

                    // Open interest value in USDT (notional).
                    let open_interest_value = open_interest * index_price;

                    exchange.market_data.update_futures_data(
                        &symbol,
                        mark_price,
                        index_price,
                        open_interest,
                        open_interest_value,
                        funding_rate,
                        next_funding_ms,
                    );
                }
            }

            counter = counter.wrapping_add(1);
        }
    });
}

// =============================================================================
// Demo product seeding — populates all products so every page has data
// =============================================================================

fn seed_demo_products(exchange: &Arc<Exchange>) {
    let now = Utc::now();

    // --- Competitions ---
    {
        let comps = vec![
            Competition {
                id: Uuid::from_u128(0xDE00_C001_0000_0000_0000_0000_0000_0001),
                name: "BTC Trading Championship".into(),
                start_time: now - Duration::days(7),
                end_time: now + Duration::days(23),
                prize_pool: dec!(50_000),
                metric: "pnl".into(),
                symbols: vec!["BTCUSDT".into()],
                created_at: now - Duration::days(8),
            },
            Competition {
                id: Uuid::from_u128(0xDE00_C002_0000_0000_0000_0000_0000_0002),
                name: "Volume Kings".into(),
                start_time: now - Duration::days(3),
                end_time: now + Duration::days(27),
                prize_pool: dec!(25_000),
                metric: "volume".into(),
                symbols: vec![],
                created_at: now - Duration::days(4),
            },
            Competition {
                id: Uuid::from_u128(0xDE00_C003_0000_0000_0000_0000_0000_0003),
                name: "ROI Masters".into(),
                start_time: now - Duration::days(14),
                end_time: now + Duration::days(16),
                prize_pool: dec!(10_000),
                metric: "roi".into(),
                symbols: vec!["ETHUSDT".into(), "SOLUSDT".into()],
                created_at: now - Duration::days(15),
            },
        ];

        let mut comp_lock = exchange.competitions.lock().unwrap_or_else(|e| e.into_inner());
        let mut part_lock = exchange
            .competition_participants
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        for comp in &comps {
            let count: u32 = match comp.metric.as_str() {
                "pnl" => 15,
                "volume" => 10,
                _ => 8,
            };
            let mut entries = Vec::new();
            for i in 0..count {
                entries.push(CompetitionEntry {
                    user_id: Uuid::from_u128(0xDE00_E000_0000_0000_0000_0000_0000_0000 + i as u128),
                    joined_at: comp.start_time + Duration::hours(i as i64),
                    snapshot_balance: Decimal::new(5000 + (i as i64 * 3500), 0),
                });
            }
            part_lock.insert(comp.id, entries);
        }
        *comp_lock = comps;
    }

    // --- P2P Ads ---
    {
        let demo_users = [
            Uuid::from_u128(DEMO_USER_3),
            Uuid::from_u128(DEMO_USER_4),
            Uuid::from_u128(DEMO_USER_5),
            Uuid::from_u128(DEMO_USER_6),
        ];
        for &uid in &demo_users {
            exchange.create_account(uid);
            let _ = exchange.deposit(uid, "USDT", dec!(100_000));
            let _ = exchange.deposit(uid, "BTC", dec!(5));
        }

        let ads = vec![
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[0],
                side: "sell".into(), asset: "USDT".into(), fiat_currency: "USD".into(),
                price: dec!(0.998), min_amount: dec!(50), max_amount: dec!(5000),
                payment_methods: vec!["Bank Transfer".into(), "Wise".into()],
                is_active: true, created_at: now - Duration::hours(12),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[1],
                side: "sell".into(), asset: "USDT".into(), fiat_currency: "USD".into(),
                price: dec!(1.001), min_amount: dec!(100), max_amount: dec!(10000),
                payment_methods: vec!["Zelle".into(), "Bank Transfer".into()],
                is_active: true, created_at: now - Duration::hours(8),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[2],
                side: "sell".into(), asset: "USDT".into(), fiat_currency: "EUR".into(),
                price: dec!(0.92), min_amount: dec!(50), max_amount: dec!(3000),
                payment_methods: vec!["SEPA".into(), "Revolut".into()],
                is_active: true, created_at: now - Duration::hours(6),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[3],
                side: "sell".into(), asset: "USDT".into(), fiat_currency: "EUR".into(),
                price: dec!(0.93), min_amount: dec!(100), max_amount: dec!(5000),
                payment_methods: vec!["SEPA".into()],
                is_active: true, created_at: now - Duration::hours(4),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[0],
                side: "buy".into(), asset: "USDT".into(), fiat_currency: "USD".into(),
                price: dec!(0.995), min_amount: dec!(100), max_amount: dec!(10000),
                payment_methods: vec!["Bank Transfer".into()],
                is_active: true, created_at: now - Duration::hours(3),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[1],
                side: "buy".into(), asset: "USDT".into(), fiat_currency: "USD".into(),
                price: dec!(0.999), min_amount: dec!(50), max_amount: dec!(5000),
                payment_methods: vec!["Wise".into(), "Zelle".into()],
                is_active: true, created_at: now - Duration::hours(2),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[2],
                side: "sell".into(), asset: "BTC".into(), fiat_currency: "USD".into(),
                price: dec!(67_500), min_amount: dec!(100), max_amount: dec!(50000),
                payment_methods: vec!["Wire Transfer".into()],
                is_active: true, created_at: now - Duration::hours(1),
            },
            P2pAd {
                id: Uuid::new_v4(), user_id: demo_users[3],
                side: "sell".into(), asset: "ETH".into(), fiat_currency: "EUR".into(),
                price: dec!(3200), min_amount: dec!(50), max_amount: dec!(10000),
                payment_methods: vec!["SEPA".into(), "Revolut".into()],
                is_active: true, created_at: now,
            },
        ];
        *exchange.p2p_ads.lock().unwrap_or_else(|e| e.into_inner()) = ads;
    }

    // --- Copy Trading Masters ---
    {
        let masters_data: &[(u128, &str, i64, u64, i64, u32, i64)] = &[
            (0xDE00_A001, "CryptoAlpha",     24500000, 1847,  68, 342, 10),
            (0xDE00_A002, "BTC_Whale",        51200000, 523,   72, 891, 15),
            (0xDE00_A003, "ScalpMaster",       8900000, 12456, 61, 156,  8),
            (0xDE00_A004, "SwingTrader_Pro",  17800000, 234,   74, 445, 12),
            (0xDE00_A005, "ETH_Degen",         6700000, 3210,  58,  89, 10),
        ];

        let mut reg = exchange.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());
        for &(suffix, name, pnl_cents, trades, wr, followers, ps) in masters_data {
            let uid = Uuid::from_u128(suffix);
            exchange.create_account(uid);
            let _ = reg.register(uid, name, Decimal::new(ps, 2));
            if let Some(m) = reg.get_mut(uid) {
                m.total_pnl = Decimal::new(pnl_cents, 2);
                m.total_trades = trades;
                m.win_rate = Decimal::new(wr, 2);
                m.followers_count = followers;
            }
        }
    }

    // --- Launchpad Projects ---
    {
        use exchange_api_gateway::rest_products::LaunchpadStatus;
        let projects = vec![
            LaunchpadProject {
                id: Uuid::from_u128(0xDE00_D001_0000_0000_0000_0000_0000_0001),
                token_name: "Nexus Protocol".into(),
                token_symbol: "NXS".into(),
                total_supply: dec!(10_000_000),
                price_per_token: dec!(0.05),
                start_time: now - Duration::days(2),
                end_time: now + Duration::days(5),
                min_allocation: dec!(100),
                max_allocation: dec!(5000),
                required_staking: Decimal::ZERO,
                status: LaunchpadStatus::Active,
                created_at: now - Duration::days(10),
            },
            LaunchpadProject {
                id: Uuid::from_u128(0xDE00_D002_0000_0000_0000_0000_0000_0002),
                token_name: "MetaVerse AI".into(),
                token_symbol: "MVAI".into(),
                total_supply: dec!(50_000_000),
                price_per_token: dec!(0.02),
                start_time: now + Duration::days(3),
                end_time: now + Duration::days(10),
                min_allocation: dec!(50),
                max_allocation: dec!(2000),
                required_staking: Decimal::ZERO,
                status: LaunchpadStatus::Upcoming,
                created_at: now - Duration::days(5),
            },
            LaunchpadProject {
                id: Uuid::from_u128(0xDE00_D003_0000_0000_0000_0000_0000_0003),
                token_name: "DeFi Shield".into(),
                token_symbol: "DFSH".into(),
                total_supply: dec!(20_000_000),
                price_per_token: dec!(0.10),
                start_time: now - Duration::days(14),
                end_time: now - Duration::days(7),
                min_allocation: dec!(200),
                max_allocation: dec!(10000),
                required_staking: Decimal::ZERO,
                status: LaunchpadStatus::Ended,
                created_at: now - Duration::days(21),
            },
        ];
        *exchange.launchpad_projects.lock().unwrap_or_else(|e| e.into_inner()) = projects;
    }

    // --- Launchpool Pools ---
    {
        let pools = vec![
            LaunchpoolPool {
                id: Uuid::from_u128(0xDE00_F001_0000_0000_0000_0000_0000_0001),
                reward_token: "NXS".into(),
                staking_token: "USDT".into(),
                total_rewards: dec!(1_000_000),
                duration_days: 30,
                start_time: now - Duration::days(5),
                end_time: now + Duration::days(25),
                status: LaunchpoolStatus::Active,
                total_staked: dec!(2_500_000),
                created_at: now - Duration::days(10),
            },
            LaunchpoolPool {
                id: Uuid::from_u128(0xDE00_F002_0000_0000_0000_0000_0000_0002),
                reward_token: "MVAI".into(),
                staking_token: "USDT".into(),
                total_rewards: dec!(5_000_000),
                duration_days: 14,
                start_time: now + Duration::days(3),
                end_time: now + Duration::days(17),
                status: LaunchpoolStatus::Upcoming,
                total_staked: Decimal::ZERO,
                created_at: now - Duration::days(3),
            },
        ];
        *exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner()) = pools;
    }

    // --- Liquidity Pools ---
    {
        let pools = vec![
            LiquidityPool {
                id: Uuid::from_u128(0xDE00_B001_0000_0000_0000_0000_0000_0001),
                pair: "BTC/USDT".into(),
                reserve_a: dec!(50),
                reserve_b: dec!(3_375_000),
                total_fees_earned: dec!(45_000),
                apr: dec!(12),
                total_lp_shares: dec!(1_000_000),
                created_at: now - Duration::days(30),
            },
            LiquidityPool {
                id: Uuid::from_u128(0xDE00_B002_0000_0000_0000_0000_0000_0002),
                pair: "ETH/USDT".into(),
                reserve_a: dec!(1000),
                reserve_b: dec!(3_450_000),
                total_fees_earned: dec!(28_000),
                apr: dec!(18),
                total_lp_shares: dec!(500_000),
                created_at: now - Duration::days(21),
            },
            LiquidityPool {
                id: Uuid::from_u128(0xDE00_B003_0000_0000_0000_0000_0000_0003),
                pair: "SOL/USDT".into(),
                reserve_a: dec!(10_000),
                reserve_b: dec!(1_450_000),
                total_fees_earned: dec!(15_000),
                apr: dec!(25),
                total_lp_shares: dec!(200_000),
                created_at: now - Duration::days(14),
            },
        ];
        *exchange.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner()) = pools;
    }

    // --- Announcements ---
    {
        let anns = vec![
            Announcement {
                id: Uuid::new_v4(),
                title: "SOL-USDT Spot Market Now Live".into(),
                content: "Trade SOL/USDT with maker fee 0.01% and taker fee 0.06%.".into(),
                announcement_type: "listing".into(),
                publish_at: now - Duration::days(7),
                created_at: now - Duration::days(7),
            },
            Announcement {
                id: Uuid::new_v4(),
                title: "New Testnet Features Available".into(),
                content: "Copy Trading, Trading Bots, and Launchpad are now available on testnet.".into(),
                announcement_type: "general".into(),
                publish_at: now - Duration::days(3),
                created_at: now - Duration::days(3),
            },
            Announcement {
                id: Uuid::new_v4(),
                title: "Scheduled System Upgrade Complete".into(),
                content: "The matching engine has been upgraded. Latency improved by 30%.".into(),
                announcement_type: "maintenance".into(),
                publish_at: now - Duration::days(14),
                created_at: now - Duration::days(14),
            },
            Announcement {
                id: Uuid::new_v4(),
                title: "AVAX Perpetual Futures Launch".into(),
                content: "AVAXUSDT perpetual is now available with up to 50x leverage.".into(),
                announcement_type: "listing".into(),
                publish_at: now - Duration::days(5),
                created_at: now - Duration::days(5),
            },
            Announcement {
                id: Uuid::new_v4(),
                title: "Reduced Trading Fees for VIP Tiers".into(),
                content: "VIP 3+ users now enjoy 0.005% maker rebate and 0.03% taker fee.".into(),
                announcement_type: "general".into(),
                publish_at: now - Duration::days(1),
                created_at: now - Duration::days(1),
            },
        ];
        *exchange.announcements.lock().unwrap_or_else(|e| e.into_inner()) = anns;
    }

    // --- Insurance Funds (set realistic balances) ---
    {
        let balances: &[(&str, i64)] = &[
            ("BTCUSDT", 500_000), ("ETHUSDT", 200_000), ("SOLUSDT", 100_000),
            ("BNBUSDT", 50_000), ("XRPUSDT", 50_000), ("DOGEUSDT", 50_000),
            ("AVAXUSDT", 50_000), ("ADAUSDT", 50_000), ("LINKUSDT", 50_000),
            ("DOTUSDT", 50_000), ("BTCUSD", 30_000), ("ETHUSD", 30_000),
        ];
        let mut funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
        for &(sym, bal) in balances {
            let symbol = Symbol::new(sym);
            funds.insert(symbol.clone(), InsuranceFund::new(symbol, Decimal::new(bal, 0)));
        }
    }

    // --- Dual Investment Products ---
    {
        let products = vec![
            DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: "BTC".into(),
                direction: DualInvestDirection::BuyLow,
                strike_price: dec!(60_000),
                expiry: now + Duration::days(7),
                apy: dec!(35),
                status: DualInvestStatus::Active,
                created_at: now - Duration::days(1),
            },
            DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: "BTC".into(),
                direction: DualInvestDirection::SellHigh,
                strike_price: dec!(75_000),
                expiry: now + Duration::days(7),
                apy: dec!(28),
                status: DualInvestStatus::Active,
                created_at: now - Duration::days(1),
            },
            DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: "ETH".into(),
                direction: DualInvestDirection::BuyLow,
                strike_price: dec!(3_000),
                expiry: now + Duration::days(3),
                apy: dec!(42),
                status: DualInvestStatus::Active,
                created_at: now,
            },
            DualInvestProduct {
                id: Uuid::new_v4(),
                underlying: "ETH".into(),
                direction: DualInvestDirection::SellHigh,
                strike_price: dec!(4_000),
                expiry: now + Duration::days(14),
                apy: dec!(22),
                status: DualInvestStatus::Active,
                created_at: now,
            },
        ];
        *exchange.dual_invest_products.lock().unwrap_or_else(|e| e.into_inner()) = products;
    }

    // --- Block Trades ---
    {
        let maker = Uuid::from_u128(DEMO_MAKER_ID);
        let taker = Uuid::from_u128(DEMO_USER_3);
        let trades = vec![
            BlockTrade {
                id: Uuid::new_v4(), rfq_id: Uuid::new_v4(),
                maker_id: maker, taker_id: taker,
                symbol: "BTCUSDT".into(), price: dec!(67_450), qty: dec!(10),
                taker_side: "Buy".into(), timestamp: now - Duration::days(6),
            },
            BlockTrade {
                id: Uuid::new_v4(), rfq_id: Uuid::new_v4(),
                maker_id: maker, taker_id: taker,
                symbol: "BTCUSDT".into(), price: dec!(67_520), qty: dec!(8),
                taker_side: "Sell".into(), timestamp: now - Duration::days(4),
            },
            BlockTrade {
                id: Uuid::new_v4(), rfq_id: Uuid::new_v4(),
                maker_id: maker, taker_id: taker,
                symbol: "BTCUSDT".into(), price: dec!(67_480), qty: dec!(5),
                taker_side: "Buy".into(), timestamp: now - Duration::days(2),
            },
            BlockTrade {
                id: Uuid::new_v4(), rfq_id: Uuid::new_v4(),
                maker_id: maker, taker_id: taker,
                symbol: "ETHUSDT".into(), price: dec!(3_448), qty: dec!(100),
                taker_side: "Buy".into(), timestamp: now - Duration::days(3),
            },
            BlockTrade {
                id: Uuid::new_v4(), rfq_id: Uuid::new_v4(),
                maker_id: maker, taker_id: taker,
                symbol: "ETHUSDT".into(), price: dec!(3_452), qty: dec!(50),
                taker_side: "Sell".into(), timestamp: now - Duration::days(1),
            },
        ];
        *exchange.block_trades.lock().unwrap_or_else(|e| e.into_inner()) = trades;
    }

    // --- Leveraged Tokens (fix zero supply) ---
    {
        let updates: &[(&str, i64, i64)] = &[
            ("BTC3L", 50_000, 67_500),
            ("BTC3S", 25_000, 67_500),
            ("ETH3L", 30_000, 3_450),
            ("ETH3S", 15_000, 3_450),
        ];
        let mut lts = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
        for &(sym, supply, rebal_price) in updates {
            if let Some(lt) = lts.get_mut(&sym.to_string()) {
                lt.total_supply = Decimal::new(supply, 0);
                lt.last_rebalance_price = Some(Decimal::new(rebal_price, 0));
            }
        }
    }

    // --- Bonus Campaigns ---
    {
        let campaigns = vec![
            BonusCampaign {
                id: Uuid::new_v4(),
                name: "Welcome Bonus".into(),
                bonus_type: "signup".into(),
                amount: dec!(50),
                min_deposit: Decimal::ZERO,
                expiry: now + Duration::days(30),
                max_uses: 1000,
                uses_remaining: 987,
                coupon_code: None,
                is_active: true,
                created_at: now - Duration::days(7),
            },
            BonusCampaign {
                id: Uuid::new_v4(),
                name: "Deposit Bonus".into(),
                bonus_type: "deposit".into(),
                amount: dec!(100),
                min_deposit: dec!(500),
                expiry: now + Duration::days(14),
                max_uses: 500,
                uses_remaining: 478,
                coupon_code: Some("DEMO2024".into()),
                is_active: true,
                created_at: now - Duration::days(3),
            },
        ];
        *exchange.bonus_campaigns.lock().unwrap_or_else(|e| e.into_inner()) = campaigns;
    }

    tracing::info!("Demo products seeded: 3 competitions, 8 P2P ads, 5 copy-trading masters, \
        3 launchpad projects, 2 launchpool pools, 3 liquidity pools, 5 announcements, \
        12 insurance funds, 4 dual-invest products, 5 block trades, 4 leveraged tokens, \
        2 bonus campaigns");
}

// ── Live price feeds ────────────────────────────────────────────────────────

/// Spawn live WebSocket price feeds from Binance, Coinbase, and Kraken.
///
/// Each feed runs in its own task, sending price updates through a channel.
/// A single writer task reads all updates and feeds them into the exchange's
/// `IndexPriceAggregator` (sync Mutex). A separate ticker-update loop
/// propagates index prices into the market-data ticker every second.
#[allow(dead_code)]
fn spawn_live_price_feeds(exchange: Arc<Exchange>) {
    use exchange_index_price::feeds::{PriceUpdate, binance::BinanceFeed, coinbase::CoinbaseFeed, kraken::KrakenFeed};

    let (tx, mut rx) = tokio::sync::mpsc::channel::<PriceUpdate>(512);

    // Spawn WebSocket feed tasks.
    let binance = BinanceFeed::new(vec![
        "btcusdt".into(), "ethusdt".into(), "solusdt".into(),
        "bnbusdt".into(), "xrpusdt".into(), "dogeusdt".into(),
        "avaxusdt".into(), "adausdt".into(), "linkusdt".into(), "dotusdt".into(),
    ]);
    let coinbase = CoinbaseFeed::new(vec![
        "BTC-USD".into(), "ETH-USD".into(), "SOL-USD".into(),
    ]);
    let kraken = KrakenFeed::new(vec![
        "XBT/USD".into(), "ETH/USD".into(), "SOL/USD".into(),
    ]);

    // Each feed's connect() blocks forever (reconnecting on failure).
    let tx_b = tx.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = binance.connect(tx_b.clone()).await {
                tracing::error!(error = %e, "Binance feed error — reconnecting in 5s");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    let tx_c = tx.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = coinbase.connect(tx_c.clone()).await {
                tracing::error!(error = %e, "Coinbase feed error — reconnecting in 5s");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    tokio::spawn(async move {
        loop {
            if let Err(e) = kraken.connect(tx.clone()).await {
                tracing::error!(error = %e, "Kraken feed error — reconnecting in 5s");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    // Writer task: receives price updates → feeds into exchange's aggregator.
    let ex_writer = Arc::clone(&exchange);
    tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            let mut agg = ex_writer.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
            agg.update_source_price(&update.symbol, &update.source, update.price, update.timestamp);
        }
    });

    // Ticker update loop: every second, propagate index prices to market-data.
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let symbols = [
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT",
            "DOGEUSDT", "AVAXUSDT", "ADAUSDT", "LINKUSDT", "DOTUSDT",
        ];

        loop {
            ticker.tick().await;
            let now = chrono::Utc::now();

            let next_funding_ms = exchange
                .funding_scheduler
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .next_funding_time(now)
                .ok()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);

            for &sym in &symbols {
                let symbol = Symbol::new(sym);

                let index_price = {
                    let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                    match agg.calculate_index(&symbol) {
                        Ok(ip) => ip.price,
                        Err(_) => continue,
                    }
                };

                let mark_price = index_price;

                let funding_rate = exchange
                    .funding_history
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&symbol)
                    .and_then(|h| h.latest_rate())
                    .map(|r| r.rate)
                    .unwrap_or(Decimal::ZERO);

                let open_interest = {
                    let pm = exchange.positions.lock();
                    pm.all_positions()
                        .iter()
                        .filter(|p| p.symbol == symbol)
                        .map(|p| p.size)
                        .sum::<Decimal>()
                };
                let open_interest_value = open_interest * index_price;

                exchange.market_data.update_futures_data(
                    &symbol, mark_price, index_price, open_interest,
                    open_interest_value, funding_rate, next_funding_ms,
                );
            }
        }
    });
}
