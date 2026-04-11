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
        sqlx::query_as("SELECT name FROM sessions GROUP BY name ORDER BY MAX(updated_at) DESC")
            .fetch_all(pool)
            .await
            .map_err(|e| HomError::DatabaseError(format!("list sessions: {e}")))?;

    Ok(rows.into_iter().map(|(name,)| name).collect())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::HomDb;

    async fn open_temp_db() -> HomDb {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("hom.sqlite");
        std::mem::forget(temp);
        HomDb::open(db_path.to_str().unwrap()).await.unwrap()
    }

    #[tokio::test]
    async fn save_and_load_session_round_trip() {
        let db = open_temp_db().await;

        save_session(
            db.pool(),
            "s1",
            "demo",
            "{\"layout\":\"grid\"}",
            "[{\"id\":1}]",
        )
        .await
        .unwrap();

        let loaded = load_session(db.pool(), "demo").await.unwrap();
        assert_eq!(
            loaded,
            Some((
                "{\"layout\":\"grid\"}".to_string(),
                "[{\"id\":1}]".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn load_session_returns_latest_row_for_name_and_list_is_distinct() {
        let db = open_temp_db().await;

        save_session(db.pool(), "old", "shared", "\"hsplit\"", "[{\"id\":1}]")
            .await
            .unwrap();
        save_session(db.pool(), "new", "shared", "\"grid\"", "[{\"id\":2}]")
            .await
            .unwrap();
        save_session(db.pool(), "other", "other", "\"single\"", "[{\"id\":3}]")
            .await
            .unwrap();

        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(10_i64)
            .bind("old")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(20_i64)
            .bind("new")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(15_i64)
            .bind("other")
            .execute(db.pool())
            .await
            .unwrap();

        let loaded = load_session(db.pool(), "shared").await.unwrap();
        assert_eq!(
            loaded,
            Some(("\"grid\"".to_string(), "[{\"id\":2}]".to_string()))
        );

        let listed = list_sessions(db.pool()).await.unwrap();
        assert_eq!(listed, vec!["shared".to_string(), "other".to_string()]);
    }
}
