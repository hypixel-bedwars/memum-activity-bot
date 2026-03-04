/// Database initialization and module re-exports.
///
/// Call `init_db` at startup to create the connection pool and run pending
/// migrations automatically.
pub mod models;
pub mod queries;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use tracing::info;

/// Initialize the SQLite database.
///
/// 1. Parses the connection string from `database_url`.
/// 2. Enables WAL journal mode and `create_if_missing` so the file is created
///    automatically on first run.
/// 3. Runs all pending SQLx migrations embedded at compile time.
/// 4. Returns the connection pool.
pub async fn init_db(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database migrations complete.");

    Ok(pool)
}
