//! Cost tracking — record and query token usage and costs.

use sqlx::SqlitePool;

use hom_core::{HomError, HomResult};

/// Log a cost entry.
pub async fn log_cost(
    pool: &SqlitePool,
    pane_id: u32,
    harness: &str,
    model: Option<&str>,
    tokens_input: i64,
    tokens_output: i64,
    cost_usd: f64,
) -> HomResult<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO cost_log (pane_id, harness, model, tokens_input, tokens_output, cost_usd, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(pane_id)
    .bind(harness)
    .bind(model)
    .bind(tokens_input)
    .bind(tokens_output)
    .bind(cost_usd)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("log cost: {e}")))?;

    Ok(())
}

/// Get total cost for the current session.
pub async fn total_cost(pool: &SqlitePool) -> HomResult<f64> {
    let row: (f64,) = sqlx::query_as("SELECT COALESCE(SUM(cost_usd), 0.0) FROM cost_log")
        .fetch_one(pool)
        .await
        .map_err(|e| HomError::DatabaseError(format!("total cost: {e}")))?;

    Ok(row.0)
}

/// Get cost breakdown by harness.
pub async fn cost_by_harness(pool: &SqlitePool) -> HomResult<Vec<(String, f64)>> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT harness, COALESCE(SUM(cost_usd), 0.0) FROM cost_log GROUP BY harness ORDER BY 2 DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("cost by harness: {e}")))?;

    Ok(rows)
}
