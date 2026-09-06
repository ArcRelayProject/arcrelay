use super::{
    access::{digest, McpClientView},
    error::ToolFailure,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{path::Path, sync::Arc};
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct McpJournal {
    options: SqliteConnectOptions,
    pool: Arc<OnceCell<SqlitePool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredReply {
    pub value: Option<Value>,
    pub error: Option<ToolFailure>,
}

pub enum Reservation {
    New(String),
    Replay(StoredReply),
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationChange {
    pub id: String,
    pub actor: String,
    pub entity: String,
    pub target_id: String,
    pub status: String,
    pub created_at: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub error: Option<String>,
}

impl McpJournal {
    /// No pool, timer or task is created before the application's long-lived runtime.
    pub fn new(directory: &Path) -> Self {
        Self {
            options: SqliteConnectOptions::new()
                .filename(directory.join("mcp/management.sqlite3"))
                .create_if_missing(true)
                .busy_timeout(std::time::Duration::from_secs(5)),
            pool: Arc::new(OnceCell::new()),
        }
    }
    async fn db(&self) -> Result<&SqlitePool, ToolFailure> {
        self.pool.get_or_try_init(|| async {
            let pool = SqlitePoolOptions::new().max_connections(1).connect_with(self.options.clone()).await?;
            sqlx::raw_sql("CREATE TABLE IF NOT EXISTS mcp_requests (id TEXT PRIMARY KEY, client_id TEXT NOT NULL, request_key TEXT NOT NULL, digest TEXT NOT NULL, reply TEXT, UNIQUE(client_id,request_key));
                CREATE TABLE IF NOT EXISTS configuration_changes (id TEXT PRIMARY KEY, target_id TEXT NOT NULL, created_at TEXT NOT NULL, data TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS configuration_change_target ON configuration_changes(target_id,created_at DESC);")
                .execute(&pool).await?;
            Ok::<_, ToolFailure>(pool)
        }).await
    }

    pub async fn reserve(
        &self,
        client: &McpClientView,
        key: &str,
        operation: &str,
        request: &impl Serialize,
    ) -> Result<Reservation, ToolFailure> {
        if key.trim().is_empty() || key.len() > 128 {
            return Err(ToolFailure::invalid("idempotencyKey", "supply a stable request key of 1–128 bytes; reuse it only for retries of this exact operation"));
        }
        let hash = digest(serde_json::to_vec(&(operation, request))?);
        let id = uuid::Uuid::new_v4().to_string();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO mcp_requests(id,client_id,request_key,digest) VALUES(?,?,?,?)",
        )
        .bind(&id)
        .bind(&client.id)
        .bind(key)
        .bind(&hash)
        .execute(self.db().await?)
        .await?
        .rows_affected();
        if inserted == 1 {
            return Ok(Reservation::New(id));
        }
        let row = sqlx::query(
            "SELECT id,digest,reply FROM mcp_requests WHERE client_id=? AND request_key=?",
        )
        .bind(&client.id)
        .bind(key)
        .fetch_one(self.db().await?)
        .await?;
        if row.get::<String, _>("digest") != hash {
            return Err(ToolFailure::invalid(
                "idempotencyKey",
                "this key was already used with different arguments or another tool",
            ));
        }
        match row.get::<Option<String>, _>("reply") {
            Some(reply) => Ok(Reservation::Replay(serde_json::from_str(&reply)?)),
            None => {
                let mut error = ToolFailure::new("operation.unresolved", "this request is still running or was interrupted; it has not been replayed", "inspect configurations, activities and change history before issuing a new request");
                error.details = Some(Box::new(
                    serde_json::json!({"operationId": row.get::<String, _>("id")}),
                ));
                Err(error)
            }
        }
    }
    pub async fn finish(&self, id: &str, reply: &StoredReply) -> Result<(), ToolFailure> {
        sqlx::query("UPDATE mcp_requests SET reply=? WHERE id=?")
            .bind(serde_json::to_string(reply)?)
            .bind(id)
            .execute(self.db().await?)
            .await?;
        Ok(())
    }

    pub async fn begin_change(
        &self,
        actor: &str,
        entity: &str,
        target_id: &str,
        before: Option<Value>,
        after: Option<Value>,
    ) -> Result<ConfigurationChange, String> {
        let change = ConfigurationChange {
            id: uuid::Uuid::new_v4().to_string(),
            actor: actor.into(),
            entity: entity.into(),
            target_id: target_id.into(),
            status: "pending".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            before,
            after,
            error: None,
        };
        self.put_change(&change).await.map_err(|e| e.message)?;
        Ok(change)
    }
    async fn put_change(&self, change: &ConfigurationChange) -> Result<(), ToolFailure> {
        sqlx::query("INSERT INTO configuration_changes(id,target_id,created_at,data) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data")
            .bind(&change.id).bind(&change.target_id).bind(&change.created_at).bind(serde_json::to_string(change)?)
            .execute(self.db().await?).await?;
        Ok(())
    }
    pub async fn complete_change(
        &self,
        mut change: ConfigurationChange,
        after: Option<Value>,
        error: Option<String>,
    ) -> Result<(), String> {
        change.status = if error.is_some() { "failed" } else { "applied" }.into();
        change.after = after;
        change.error = error;
        self.put_change(&change)
            .await
            .map_err(|e| format!("configuration outcome could not be recorded: {}", e.message))
    }
    pub async fn changes(
        &self,
        target: Option<&str>,
        limit: u32,
        include_definitions: bool,
    ) -> Result<Vec<ConfigurationChange>, ToolFailure> {
        let rows = sqlx::query_scalar::<_, String>("SELECT data FROM configuration_changes WHERE (? IS NULL OR target_id=?) ORDER BY created_at DESC,rowid DESC LIMIT ?")
            .bind(target).bind(target).bind(limit.clamp(1,100)).fetch_all(self.db().await?).await?;
        rows.into_iter()
            .map(|row| {
                let mut change: ConfigurationChange = serde_json::from_str(&row)?;
                if !include_definitions {
                    change.before = None;
                    change.after = None;
                }
                Ok(change)
            })
            .collect()
    }
    pub async fn change(&self, id: &str) -> Result<ConfigurationChange, ToolFailure> {
        let row =
            sqlx::query_scalar::<_, String>("SELECT data FROM configuration_changes WHERE id=?")
                .bind(id)
                .fetch_optional(self.db().await?)
                .await?
                .ok_or_else(|| {
                    ToolFailure::new(
                        "configuration.not_found",
                        "change not found",
                        "list configuration changes",
                    )
                })?;
        Ok(serde_json::from_str(&row)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructs_without_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let journal = McpJournal::new(directory.path());
        assert!(journal.pool.get().is_none());
        assert!(!directory.path().join("mcp/management.sqlite3").exists());
    }
    #[tokio::test]
    async fn retries_do_not_duplicate_or_change_operations_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let access = super::super::access::McpAccess::new(directory.path(), "legacy").unwrap();
        let (client, _) = access.create("Test".into(), Default::default()).unwrap();
        let journal = McpJournal::new(directory.path());
        let request = serde_json::json!({"name":"Morning"});
        let Reservation::New(id) = journal
            .reserve(&client, "one", "create", &request)
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            journal
                .reserve(&client, "one", "create", &request)
                .await
                .err()
                .unwrap()
                .code,
            "operation.unresolved"
        );
        journal
            .finish(
                &id,
                &StoredReply {
                    value: Some(serde_json::json!({"id":"saved"})),
                    error: None,
                },
            )
            .await
            .unwrap();
        let restarted = McpJournal::new(directory.path());
        assert!(matches!(
            restarted
                .reserve(&client, "one", "create", &request)
                .await
                .unwrap(),
            Reservation::Replay(_)
        ));
        assert!(restarted
            .reserve(&client, "one", "run", &request)
            .await
            .is_err());
        let (other, _) = access.create("Other".into(), Default::default()).unwrap();
        assert!(matches!(
            restarted
                .reserve(&other, "one", "create", &request)
                .await
                .unwrap(),
            Reservation::New(_)
        ));
    }
    #[tokio::test]
    async fn change_history_preserves_intent_and_redacts_definitions_from_lists() {
        let directory = tempfile::tempdir().unwrap();
        super::super::access::McpAccess::new(directory.path(), "legacy").unwrap();
        let journal = McpJournal::new(directory.path());
        let before = serde_json::json!({"id":"action", "revision":1, "script":"private script"});
        let after = serde_json::json!({"id":"action", "revision":2, "script":"updated script"});
        let pending = journal
            .begin_change(
                "mcp:client:Test",
                "quickAction",
                "action",
                Some(before.clone()),
                Some(after.clone()),
            )
            .await
            .unwrap();
        assert_eq!(journal.change(&pending.id).await.unwrap().status, "pending");
        journal
            .complete_change(pending.clone(), Some(after.clone()), None)
            .await
            .unwrap();
        let restarted = McpJournal::new(directory.path());
        let summary = restarted.changes(Some("action"), 20, false).await.unwrap();
        assert_eq!(summary.len(), 1);
        assert!(summary[0].before.is_none() && summary[0].after.is_none());
        assert_eq!(summary[0].actor, "mcp:client:Test");
        let saved = restarted.change(&pending.id).await.unwrap();
        assert_eq!(saved.status, "applied");
        assert_eq!(saved.before, Some(before));
        assert_eq!(saved.after, Some(after));
        assert!(restarted
            .changes(Some("other"), 20, false)
            .await
            .unwrap()
            .is_empty());
    }
}
