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
