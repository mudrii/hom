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
            &id,
            record.workflow_id,
            record.step_id,
            record.harness,
            record.model,
            record.status,
            record.prompt,
            record.output,
            record.duration_ms,
            record.attempt,
        )
        .await
    }
}
