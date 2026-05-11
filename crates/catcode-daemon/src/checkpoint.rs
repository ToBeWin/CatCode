use crate::session::Session;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info};

/// Metadata about a checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub id: String,
    pub session_id: String,
    pub created_at: chrono::DateTime<Utc>,
    pub turn_count: u64,
}

/// A checkpoint contains a full snapshot of a session's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub meta: CheckpointMeta,
    pub session: Session,
    /// Serialized conversation messages (JSON).
    pub messages_json: String,
}

/// Manages session checkpoints on disk.
///
/// Checkpoints are saved as JSON files in the `.catcode/checkpoints/` directory.
/// Each checkpoint captures the full session state at a point in time, allowing
/// recovery after daemon restarts.
#[derive(Debug)]
pub struct CheckpointManager {
    base_dir: PathBuf,
}

impl CheckpointManager {
    /// Create a new checkpoint manager storing checkpoints in the given directory.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Ensure the checkpoints directory exists.
    pub fn init(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        Ok(())
    }

    /// Save a checkpoint for the given session.
    ///
    /// The checkpoint file is named `<session_id>_<turn>.json`.
    pub fn save(&self, session: &Session, messages_json: &str) -> anyhow::Result<CheckpointMeta> {
        self.init()?;

        let id = format!("{}_{}", session.id, session.turn_count);
        let meta = CheckpointMeta {
            id: id.clone(),
            session_id: session.id.clone(),
            created_at: Utc::now(),
            turn_count: session.turn_count,
        };

        let checkpoint = Checkpoint {
            meta: meta.clone(),
            session: session.clone(),
            messages_json: messages_json.to_string(),
        };

        let path = self.base_dir.join(format!("{}.json", id));
        let json = serde_json::to_string_pretty(&checkpoint)?;
        std::fs::write(&path, json)?;
        info!(checkpoint_id = %id, path = %path.display(), "Saved checkpoint");
        Ok(meta)
    }

    /// Load the latest checkpoint for a session.
    pub fn load_latest(&self, session_id: &str) -> anyhow::Result<Option<Checkpoint>> {
        let checkpoints = self.list(session_id)?;
        if checkpoints.is_empty() {
            return Ok(None);
        }

        // Sort by turn count descending to get the latest
        let latest = checkpoints.iter().max_by_key(|c| c.turn_count).unwrap();

        self.load(&latest.id)
    }

    /// Load a specific checkpoint by ID.
    pub fn load(&self, checkpoint_id: &str) -> anyhow::Result<Option<Checkpoint>> {
        let path = self.base_dir.join(format!("{}.json", checkpoint_id));
        if !path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&path)?;
        let checkpoint: Checkpoint = serde_json::from_str(&json)?;
        Ok(Some(checkpoint))
    }

    /// List all checkpoints for a session.
    pub fn list(&self, session_id: &str) -> anyhow::Result<Vec<CheckpointMeta>> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut checkpoints = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();

            if !name.ends_with(".json") || !name.starts_with(session_id) {
                continue;
            }

            let json = std::fs::read_to_string(entry.path())?;
            if let Ok(checkpoint) = serde_json::from_str::<Checkpoint>(&json) {
                checkpoints.push(checkpoint.meta);
            }
        }

        checkpoints.sort_by_key(|c| c.turn_count);
        Ok(checkpoints)
    }

    /// Delete a specific checkpoint.
    pub fn delete(&self, checkpoint_id: &str) -> anyhow::Result<bool> {
        let path = self.base_dir.join(format!("{}.json", checkpoint_id));
        if path.exists() {
            std::fs::remove_file(&path)?;
            debug!(checkpoint_id, "Deleted checkpoint");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete all checkpoints for a session.
    pub fn delete_all(&self, session_id: &str) -> anyhow::Result<usize> {
        let checkpoints = self.list(session_id)?;
        let count = checkpoints.len();
        for cp in &checkpoints {
            self.delete(&cp.id)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_manager() -> (CheckpointManager, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = CheckpointManager::new(tmp.path().to_path_buf());
        (mgr, tmp)
    }

    fn make_session() -> Session {
        Session::new("test", PathBuf::from("/tmp/project"), "model", "provider")
    }

    #[test]
    fn test_save_and_load() {
        let (mgr, _tmp) = make_manager();
        let mut session = make_session();
        session.turn_count = 5;

        let meta = mgr
            .save(&session, r#"[{"role":"user","content":"hello"}]"#)
            .unwrap();
        assert_eq!(meta.turn_count, 5);

        let loaded = mgr.load(&meta.id).unwrap().unwrap();
        assert_eq!(loaded.session.id, session.id);
        assert!(loaded.messages_json.contains("hello"));
    }

    #[test]
    fn test_load_latest() {
        let (mgr, _tmp) = make_manager();
        let mut session = make_session();

        session.turn_count = 1;
        mgr.save(&session, "msg1").unwrap();
        session.turn_count = 5;
        mgr.save(&session, "msg5").unwrap();
        session.turn_count = 3;
        mgr.save(&session, "msg3").unwrap();

        let latest = mgr.load_latest(&session.id).unwrap().unwrap();
        assert_eq!(latest.meta.turn_count, 5);
        assert_eq!(latest.messages_json, "msg5");
    }

    #[test]
    fn test_load_nonexistent() {
        let (mgr, _tmp) = make_manager();
        let result = mgr.load("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_checkpoints() {
        let (mgr, _tmp) = make_manager();
        let mut session = make_session();

        session.turn_count = 1;
        mgr.save(&session, "a").unwrap();
        session.turn_count = 2;
        mgr.save(&session, "b").unwrap();

        let list = mgr.list(&session.id).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].turn_count, 1);
        assert_eq!(list[1].turn_count, 2);
    }

    #[test]
    fn test_delete_checkpoint() {
        let (mgr, _tmp) = make_manager();
        let session = make_session();
        let meta = mgr.save(&session, "data").unwrap();

        assert!(mgr.delete(&meta.id).unwrap());
        assert!(mgr.load(&meta.id).unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let (mgr, _tmp) = make_manager();
        assert!(!mgr.delete("nonexistent").unwrap());
    }

    #[test]
    fn test_delete_all() {
        let (mgr, _tmp) = make_manager();
        let mut session = make_session();

        session.turn_count = 1;
        mgr.save(&session, "a").unwrap();
        session.turn_count = 2;
        mgr.save(&session, "b").unwrap();
        session.turn_count = 3;
        mgr.save(&session, "c").unwrap();

        let deleted = mgr.delete_all(&session.id).unwrap();
        assert_eq!(deleted, 3);
        assert!(mgr.list(&session.id).unwrap().is_empty());
    }
}
