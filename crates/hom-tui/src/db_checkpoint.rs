//! Implements `CheckpointStore` for `HomDb` so the workflow executor can
//! persist checkpoints and step results to SQLite.
//!
//! This lives in `hom-tui` (not `hom-db`) because `CheckpointStore` is
//! defined in `hom-workflow`, and `hom-db` is not allowed to depend on
//! `hom-workflow` per the crate dependency rules.

use async_trait::async_trait;
use std::sync::Arc;

use hom_core::HomResult;
use hom_db::HomDb;
use hom_db::workflow::SaveStepRecord;
use hom_workflow::{CheckpointStore, StepResultRecord};

/// Wrapper around `HomDb` that implements `CheckpointStore`.
pub struct DbCheckpointStore {
    db: Arc<HomDb>,
}

impl DbCheckpointStore {
    pub fn new(db: Arc<HomDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CheckpointStore for DbCheckpointStore {
    async fn save_checkpoint(&self, workflow_id: &str, checkpoint_json: &str) -> HomResult<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT OR REPLACE INTO checkpoints (workflow_id, checkpoint_json, created_at)
             VALUES (?, ?, ?)",
        )
        .bind(workflow_id)
        .bind(checkpoint_json)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(|e| hom_core::HomError::DatabaseError(format!("save checkpoint: {e}")))?;
        Ok(())
    }

    async fn save_step_result(&self, record: StepResultRecord<'_>) -> HomResult<()> {
        let id = uuid::Uuid::new_v4().to_string();
        hom_db::workflow::save_step(
            self.db.pool(),
            SaveStepRecord {
                id: &id,
                workflow_id: record.workflow_id,
                step_name: record.step_id,
                harness: record.harness,
                model: record.model,
                status: record.status,
                prompt: record.prompt,
                output: record.output,
                duration_ms: record.duration_ms,
                attempt: record.attempt,
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use hom_db::HomDb;

    async fn open_temp_db() -> Arc<HomDb> {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("hom.sqlite");
        std::mem::forget(temp);
        Arc::new(HomDb::open(db_path.to_str().unwrap()).await.unwrap())
    }

    #[tokio::test]
    async fn save_checkpoint_persists_latest_json() {
        let db = open_temp_db().await;
        let store = DbCheckpointStore::new(db.clone());

        store
            .save_checkpoint("wf-1", r#"{"step":"plan"}"#)
            .await
            .unwrap();
        store
            .save_checkpoint("wf-1", r#"{"step":"review"}"#)
            .await
            .unwrap();

        let row: (String,) =
            sqlx::query_as("SELECT checkpoint_json FROM checkpoints WHERE workflow_id = ?")
                .bind("wf-1")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(row.0, r#"{"step":"review"}"#);
    }

    #[tokio::test]
    async fn save_step_result_writes_step_row() {
        let db = open_temp_db().await;
        let store = DbCheckpointStore::new(db.clone());

        hom_db::workflow::save_workflow(
            db.pool(),
            "wf-2",
            "demo",
            "/tmp/demo.yaml",
            "running",
            "{}",
        )
        .await
        .unwrap();

        store
            .save_step_result(StepResultRecord {
                workflow_id: "wf-2",
                step_id: "plan",
                harness: "claude",
                model: Some("opus"),
                status: "completed",
                prompt: "make a plan",
                output: "done",
                duration_ms: 123,
                attempt: 2,
            })
            .await
            .unwrap();

        let row: (String, String, String, Option<String>, i64, i32) = sqlx::query_as(
            "SELECT step_name, harness, status, model, duration_ms, attempt FROM steps WHERE workflow_id = ?",
        )
        .bind("wf-2")
        .fetch_one(db.pool())
        .await
        .unwrap();

        assert_eq!(row.0, "plan");
        assert_eq!(row.1, "claude");
        assert_eq!(row.2, "completed");
        assert_eq!(row.3.as_deref(), Some("opus"));
        assert_eq!(row.4, 123);
        assert_eq!(row.5, 2);
    }
}
