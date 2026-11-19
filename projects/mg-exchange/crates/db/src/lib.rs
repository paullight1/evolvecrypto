pub mod account_status_repo;
pub mod aml_repo;
pub mod api_key_repo;
pub mod audit_repo;
pub mod balance_repo;
pub mod fee_repo;
pub mod funding_repo;
pub mod kyc_document_repo;
pub mod kyc_repo;
pub mod ledger;
pub mod optimization;
pub mod order_repo;
pub mod pg_account_manager;
pub mod position_repo;
pub mod trade_repo;
pub mod user_repo;

pub use optimization::{BatchSettler, PoolConfig, SettlementOp};

use sqlx::PgPool;

/// Database connection pool wrapper.
#[derive(Clone)]
pub struct Database {
    pub pool: PgPool,
}

impl Database {
    /// Connect to PostgreSQL.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        tracing::info!("Connected to PostgreSQL");
        Ok(Self { pool })
    }

    /// Connect with custom pool options.
    pub async fn connect_with(url: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        tracing::info!("Connected to PostgreSQL (max_connections: {})", max_connections);
        Ok(Self { pool })
    }

    /// Run migrations from the migrations directory.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        tracing::info!("Database migrations applied");
        Ok(())
    }
}
