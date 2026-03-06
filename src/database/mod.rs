/// Database initialization and module re-exports.
///
/// Call `init_db` at startup to create the connection pool and run pending
/// migrations automatically.
pub mod models;
pub mod queries;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

/// Initialize the PostgreSQL database.
pub async fn init_db(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await?;

    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database migrations complete.");

    Ok(pool)
}