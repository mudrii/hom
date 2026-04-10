//! # hom-db
//!
//! SQLite storage layer for HOM. Handles workflow state persistence,
//! session save/restore, and cost tracking.

pub mod cost;
pub mod session;
pub mod workflow;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

use hom_core::{HomError, HomResult};

/// The central database handle.
pub struct HomDb {
    pool: SqlitePool,
}

impl HomDb {
    /// Open (or create) the database at the given path.
    pub async fn open(path: &str) -> HomResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| HomError::DatabaseError(format!("create db dir: {e}")))?;
        }

        let url = format!("sqlite:{path}?mode=rwc");
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .map_err(|e| HomError::DatabaseError(format!("connect: {e}")))?;

        let db = Self { pool };
        db.run_migrations().await?;

        info!(path, "database opened");
        Ok(db)
    }

    /// Run pending migrations.
    async fn run_migrations(&self) -> HomResult<()> {
        let migration_sql = include_str!("migrations/001_initial.sql");
        sqlx::raw_sql(migration_sql)
            .execute(&self.pool)
            .await
            .map_err(|e| HomError::DatabaseError(format!("migration: {e}")))?;
        Ok(())
    }

    /// Get a reference to the connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
