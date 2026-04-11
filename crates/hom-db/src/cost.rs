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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::HomDb;

    async fn open_temp_db() -> (tempfile::TempDir, HomDb) {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("hom.sqlite");
        let db = HomDb::open(db_path.to_str().unwrap()).await.unwrap();
        (temp, db)
    }

    #[tokio::test]
    async fn total_cost_is_zero_for_empty_log() {
        let (_temp, db) = open_temp_db().await;

        assert_eq!(total_cost(db.pool()).await.unwrap(), 0.0);
        assert!(cost_by_harness(db.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn log_cost_aggregates_totals_and_groups_by_harness() {
        let (_temp, db) = open_temp_db().await;

        log_cost(db.pool(), 1, "claude", Some("opus"), 10, 20, 1.25)
            .await
            .unwrap();
        log_cost(db.pool(), 2, "codex", Some("5.4"), 30, 40, 2.50)
            .await
            .unwrap();
        log_cost(db.pool(), 3, "claude", None, 5, 6, 0.75)
            .await
            .unwrap();

        assert!((total_cost(db.pool()).await.unwrap() - 4.5).abs() < f64::EPSILON);

        let grouped = cost_by_harness(db.pool()).await.unwrap();
        assert_eq!(
            grouped,
            vec![("codex".to_string(), 2.5), ("claude".to_string(), 2.0),]
        );
    }
}
