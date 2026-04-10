//! Session save/restore — persist pane layout and configurations.

use sqlx::SqlitePool;

use hom_core::{HomError, HomResult};

/// Save a session.
pub async fn save_session(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    layout_json: &str,
    panes_json: &str,
) -> HomResult<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT OR REPLACE INTO sessions (id, name, layout, panes, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(layout_json)
    .bind(panes_json)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("save session: {e}")))?;

    Ok(())
}

/// Load a session by name.
pub async fn load_session(pool: &SqlitePool, name: &str) -> HomResult<Option<(String, String)>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT layout, panes FROM sessions WHERE name = ? ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("load session: {e}")))?;

    Ok(row)
}

/// List all saved session names.
pub async fn list_sessions(pool: &SqlitePool) -> HomResult<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT name FROM sessions ORDER BY updated_at DESC")
            .fetch_all(pool)
            .await
            .map_err(|e| HomError::DatabaseError(format!("list sessions: {e}")))?;

    Ok(rows.into_iter().map(|(name,)| name).collect())
}
