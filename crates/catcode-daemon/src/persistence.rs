use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::info;

use crate::session::SessionState;

/// SQLite-based persistence layer.
///
/// Replaces the JSON checkpoint system with a proper relational database.
/// Stores sessions, messages, token usage, and audit logs.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database at the given path and run migrations.
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Create an in-memory database (for testing).
    pub async fn new_in_memory() -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Run all database migrations.
    async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        sqlx::query(SCHEMA_SQL).execute(&self.pool).await?;
        info!("Database migrations completed");
        Ok(())
    }

    // === Sessions ===

    /// Insert or update a session.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_session(
        &self,
        id: &str,
        name: &str,
        state: &str,
        project_dir: &str,
        model_id: &str,
        provider_id: &str,
        turn_count: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO sessions (id, name, state, project_dir, model_id, provider_id, turn_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                state = excluded.state,
                model_id = excluded.model_id,
                provider_id = excluded.provider_id,
                turn_count = excluded.turn_count,
                updated_at = datetime('now')"
        )
        .bind(id)
        .bind(name)
        .bind(state)
        .bind(project_dir)
        .bind(model_id)
        .bind(provider_id)
        .bind(turn_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a session by ID.
    pub async fn get_session(&self, id: &str) -> Result<Option<SessionRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, SessionRow>("SELECT * FROM sessions WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Result<Vec<SessionRow>, sqlx::Error> {
        let rows =
            sqlx::query_as::<_, SessionRow>("SELECT * FROM sessions ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    /// Delete a session.
    pub async fn delete_session(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // === Messages ===

    /// Insert a message.
    pub async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        token_count: Option<i64>,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO messages (session_id, role, content, token_count, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(token_count)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Get messages for a session.
    pub async fn get_messages(&self, session_id: &str) -> Result<Vec<MessageRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT * FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get recent messages for a session (with limit).
    pub async fn get_recent_messages(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT * FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // === Token Usage ===

    /// Record token usage.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_usage(
        &self,
        session_id: &str,
        provider_id: &str,
        model_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cost_usd: f64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO token_usage (session_id, provider_id, model_id, input_tokens, output_tokens, cache_read_tokens, cost_usd, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))"
        )
        .bind(session_id)
        .bind(provider_id)
        .bind(model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cache_read_tokens)
        .bind(cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get total token usage for a session.
    pub async fn get_session_usage(
        &self,
        session_id: &str,
    ) -> Result<Option<UsageRow>, sqlx::Error> {
        // First check if any usage exists
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM token_usage WHERE session_id = ?1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;

        if count.0 == 0 {
            return Ok(None);
        }

        let row = sqlx::query_as::<_, UsageRow>(
            "SELECT
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as cache_read_tokens,
                COALESCE(SUM(cost_usd), 0.0) as cost_usd
             FROM token_usage WHERE session_id = ?1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Some(row))
    }

    // === Audit Log ===

    /// Insert an audit log entry.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_audit_log(
        &self,
        session_id: &str,
        operation: &str,
        tool: Option<&str>,
        args: Option<&str>,
        level: &str,
        approved_by: Option<&str>,
        result: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO audit_log (session_id, operation, tool, args, level, approved_by, result, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))"
        )
        .bind(session_id)
        .bind(operation)
        .bind(tool)
        .bind(args)
        .bind(level)
        .bind(approved_by)
        .bind(result)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get audit log for a session.
    pub async fn get_audit_log(
        &self,
        session_id: &str,
    ) -> Result<Vec<AuditRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT * FROM audit_log WHERE session_id = ?1 ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

// === Schema ===

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    project_dir TEXT NOT NULL,
    model_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    turn_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS token_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    recorded_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    tool TEXT,
    args TEXT,
    level TEXT NOT NULL,
    approved_by TEXT,
    result TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_token_usage_session ON token_usage(session_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_session ON audit_log(session_id);
";

// === Row types ===

#[derive(Debug, Clone, sqlx::FromRow)]
/// A session row from the database.
pub struct SessionRow {
    pub id: String,
    pub name: String,
    pub state: String,
    pub project_dir: String,
    pub model_id: String,
    pub provider_id: String,
    pub turn_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionRow {
/// Parse the stored state string into a `SessionState`.
    pub fn parse_state(&self) -> SessionState {
        match self.state.as_str() {
            "running" => SessionState::Running,
            "paused" => SessionState::Paused,
            "completed" => SessionState::Completed,
            other if other.starts_with("failed:") => {
                SessionState::Failed(other.strip_prefix("failed:").unwrap_or("").to_string())
            }
            _ => SessionState::Failed("unknown state".to_string()),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
/// A message row from the database.
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub token_count: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
/// Aggregated token usage row from the database.
pub struct UsageRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
/// An audit log entry from the database.
pub struct AuditRow {
    pub id: i64,
    pub session_id: String,
    pub operation: String,
    pub tool: Option<String>,
    pub args: Option<String>,
    pub level: String,
    pub approved_by: Option<String>,
    pub result: String,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> Database {
        Database::new_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_create_database() {
        let db = test_db().await;
        // Should not panic
        let sessions = db.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_upsert_and_get_session() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "model", "provider", 0)
            .await
            .unwrap();

        let session = db.get_session("s1").await.unwrap().unwrap();
        assert_eq!(session.name, "test");
        assert_eq!(session.state, "running");
    }

    #[tokio::test]
    async fn test_upsert_updates_existing() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();
        db.upsert_session("s1", "test", "completed", "/tmp", "m", "p", 5)
            .await
            .unwrap();

        let session = db.get_session("s1").await.unwrap().unwrap();
        assert_eq!(session.state, "completed");
        assert_eq!(session.turn_count, 5);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let db = test_db().await;
        db.upsert_session("s1", "a", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();
        db.upsert_session("s2", "b", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();

        let sessions = db.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();
        db.delete_session("s1").await.unwrap();

        assert!(db.get_session("s1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_messages() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();

        let id1 = db.insert_message("s1", "user", "hello", Some(5)).await.unwrap();
        let id2 = db.insert_message("s1", "assistant", "hi there", Some(10)).await.unwrap();
        assert!(id1 < id2);

        let messages = db.get_messages("s1").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");

        let recent = db.get_recent_messages("s1", 1).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hi there");
    }

    #[tokio::test]
    async fn test_token_usage() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();

        db.record_token_usage("s1", "p", "m", 1000, 500, 800, 0.05)
            .await
            .unwrap();
        db.record_token_usage("s1", "p", "m", 2000, 1000, 1600, 0.10)
            .await
            .unwrap();

        let usage = db.get_session_usage("s1").await.unwrap().unwrap();
        assert_eq!(usage.input_tokens, 3000);
        assert_eq!(usage.output_tokens, 1500);
        assert_eq!(usage.cache_read_tokens, 2400);
        assert!((usage.cost_usd - 0.15).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_audit_log() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();

        db.insert_audit_log(
            "s1",
            "tool_call",
            Some("bash"),
            Some(r#"{"command":"ls"}"#),
            "dangerous",
            Some("auto"),
            "success",
        )
        .await
        .unwrap();

        let log = db.get_audit_log("s1").await.unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].operation, "tool_call");
        assert_eq!(log[0].tool.as_deref(), Some("bash"));
    }

    #[tokio::test]
    async fn test_session_state_parsing() {
        let row = SessionRow {
            id: "s1".to_string(),
            name: "test".to_string(),
            state: "failed:timeout".to_string(),
            project_dir: "/tmp".to_string(),
            model_id: "m".to_string(),
            provider_id: "p".to_string(),
            turn_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(row.parse_state(), SessionState::Failed("timeout".to_string()));
    }

    #[tokio::test]
    async fn test_cascade_delete() {
        let db = test_db().await;
        db.upsert_session("s1", "test", "running", "/tmp", "m", "p", 0)
            .await
            .unwrap();
        db.insert_message("s1", "user", "hello", None).await.unwrap();
        db.record_token_usage("s1", "p", "m", 100, 50, 0, 0.01)
            .await
            .unwrap();
        db.insert_audit_log("s1", "test", None, None, "safe", None, "ok")
            .await
            .unwrap();

        db.delete_session("s1").await.unwrap();

        assert!(db.get_messages("s1").await.unwrap().is_empty());
        assert!(db.get_session_usage("s1").await.unwrap().is_none());
        assert!(db.get_audit_log("s1").await.unwrap().is_empty());
    }
}
