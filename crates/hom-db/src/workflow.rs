//! Workflow state persistence — save/load workflow executions and step results.

use sqlx::SqlitePool;

use hom_core::{HomError, HomResult};

/// Flat record for persisting a workflow step result.
pub struct SaveStepRecord<'a> {
    pub id: &'a str,
    pub workflow_id: &'a str,
    pub step_name: &'a str,
    pub harness: &'a str,
    pub model: Option<&'a str>,
    pub status: &'a str,
    pub prompt: &'a str,
    pub output: &'a str,
    pub duration_ms: i64,
    pub attempt: i32,
}

/// Save a workflow execution record.
pub async fn save_workflow(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    definition_path: &str,
    status: &str,
    variables_json: &str,
) -> HomResult<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO workflows (id, name, definition_path, status, variables, started_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(definition_path)
    .bind(status)
    .bind(variables_json)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("save workflow: {e}")))?;

    Ok(())
}

/// Update a workflow's status.
pub async fn update_workflow_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    error: Option<&str>,
) -> HomResult<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("UPDATE workflows SET status = ?, completed_at = ?, error = ? WHERE id = ?")
        .bind(status)
        .bind(now)
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| HomError::DatabaseError(format!("update workflow: {e}")))?;

    Ok(())
}

/// Save a step result.
pub async fn save_step(pool: &SqlitePool, record: SaveStepRecord<'_>) -> HomResult<()> {
    let SaveStepRecord {
        id,
        workflow_id,
        step_name,
        harness,
        model,
        status,
        prompt,
        output,
        duration_ms,
        attempt,
    } = record;
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO steps (id, workflow_id, step_name, harness, model, status, prompt, output, started_at, completed_at, duration_ms, attempt)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(workflow_id)
    .bind(step_name)
    .bind(harness)
    .bind(model)
    .bind(status)
    .bind(prompt)
    .bind(output)
    .bind(now)
    .bind(now)
    .bind(duration_ms)
    .bind(attempt)
    .execute(pool)
    .await
    .map_err(|e| HomError::DatabaseError(format!("save step: {e}")))?;

    Ok(())
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
    async fn save_workflow_and_update_status_persist_lifecycle() {
        let (_temp, db) = open_temp_db().await;

        save_workflow(
            db.pool(),
            "wf-1",
            "review",
            "/tmp/review.yaml",
            "running",
            "{\"task\":\"demo\"}",
        )
        .await
        .unwrap();
        update_workflow_status(db.pool(), "wf-1", "failed", Some("boom"))
            .await
            .unwrap();

        let row: (String, String, String, Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT name, definition_path, status, error, completed_at FROM workflows WHERE id = ?",
        )
        .bind("wf-1")
        .fetch_one(db.pool())
        .await
        .unwrap();

        assert_eq!(row.0, "review");
        assert_eq!(row.1, "/tmp/review.yaml");
        assert_eq!(row.2, "failed");
        assert_eq!(row.3.as_deref(), Some("boom"));
        assert!(row.4.is_some());
    }

    #[tokio::test]
    async fn save_step_persists_all_fields() {
        let (_temp, db) = open_temp_db().await;
        save_workflow(
            db.pool(),
            "wf-2",
            "implement",
            "/tmp/implement.yaml",
            "running",
            "{}",
        )
        .await
        .unwrap();

        save_step(
            db.pool(),
            SaveStepRecord {
                id: "step-row",
                workflow_id: "wf-2",
                step_name: "plan",
                harness: "claude",
                model: Some("opus"),
                status: "completed",
                prompt: "plan it",
                output: "done",
                duration_ms: 321,
                attempt: 2,
            },
        )
        .await
        .unwrap();

        let row: (String, String, String, Option<String>, String, String, String, i64, i32) =
            sqlx::query_as(
                "SELECT workflow_id, step_name, harness, model, status, prompt, output, duration_ms, attempt FROM steps WHERE id = ?",
            )
            .bind("step-row")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(row.0, "wf-2");
        assert_eq!(row.1, "plan");
        assert_eq!(row.2, "claude");
        assert_eq!(row.3.as_deref(), Some("opus"));
        assert_eq!(row.4, "completed");
        assert_eq!(row.5, "plan it");
        assert_eq!(row.6, "done");
        assert_eq!(row.7, 321);
        assert_eq!(row.8, 2);
    }
}
