//! Transactional metadata storage. Routine updates write only changed rows.
use super::{
    index::{Change, Index, Item},
    Error,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{path::Path, sync::Mutex};

pub(super) struct Store {
    pool: SqlitePool,
    persisted: Mutex<(u64, usize)>,
}

fn database_error(error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("file index database: {error}"))
}

impl Store {
    pub async fn open(path: &Path, fallback: Index) -> Result<(Self, Index), Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(database_error)?;
        for schema in [
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL)",
            "CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, json BLOB NOT NULL)",
            "CREATE TABLE IF NOT EXISTS changes (sequence INTEGER PRIMARY KEY, json BLOB NOT NULL)",
            "CREATE TABLE IF NOT EXISTS enumerated (id TEXT PRIMARY KEY)",
            "CREATE TABLE IF NOT EXISTS source_ids (source_id TEXT PRIMARY KEY, id TEXT NOT NULL)",
        ] {
            sqlx::query(schema)
                .execute(&pool)
                .await
                .map_err(database_error)?;
        }
        let initialized =
            sqlx::query_scalar::<_, i64>("SELECT value FROM meta WHERE key='sequence'")
                .fetch_optional(&pool)
                .await
                .map_err(database_error)?;
        let index = if let Some(sequence) = initialized {
            let item_rows = sqlx::query("SELECT json FROM items")
                .fetch_all(&pool)
                .await
                .map_err(database_error)?;
            let mut index = Index {
                sequence: sequence as u64,
                ..Index::default()
            };
            for row in item_rows {
                let item: Item =
                    serde_json::from_slice(&row.get::<Vec<u8>, _>(0)).map_err(database_error)?;
                index.items.insert(item.id.clone(), item);
            }
            if !index.items.contains_key("root") {
                return Err(Error::unavailable("file index database has no root"));
            }
            for row in sqlx::query("SELECT json FROM changes ORDER BY sequence")
                .fetch_all(&pool)
                .await
                .map_err(database_error)?
            {
                index.changes.push(
                    serde_json::from_slice(&row.get::<Vec<u8>, _>(0)).map_err(database_error)?,
                );
            }
            for row in sqlx::query("SELECT id FROM enumerated")
                .fetch_all(&pool)
                .await
                .map_err(database_error)?
            {
                index.enumerated.insert(row.get::<String, _>(0));
            }
            for row in sqlx::query("SELECT source_id,id FROM source_ids")
                .fetch_all(&pool)
                .await
                .map_err(database_error)?
            {
                index.source_ids.insert(row.get(0), row.get(1));
            }
            index
        } else {
            let mut tx = pool.begin().await.map_err(database_error)?;
            for item in fallback.items.values() {
                sqlx::query("INSERT INTO items (id,json) VALUES (?,?)")
                    .bind(&item.id)
                    .bind(serde_json::to_vec(item).map_err(database_error)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            for (source_id, id) in &fallback.source_ids {
                sqlx::query("INSERT INTO source_ids (source_id,id) VALUES (?,?)")
                    .bind(source_id)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            for change in &fallback.changes {
                sqlx::query("INSERT INTO changes (sequence,json) VALUES (?,?)")
                    .bind(change.sequence as i64)
                    .bind(serde_json::to_vec(change).map_err(database_error)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            for id in &fallback.enumerated {
                sqlx::query("INSERT INTO enumerated (id) VALUES (?)")
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            sqlx::query("INSERT INTO meta (key,value) VALUES ('sequence',?)")
                .bind(fallback.sequence as i64)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
            tx.commit().await.map_err(database_error)?;
            fallback
        };
        let persisted = Mutex::new((index.sequence, index.enumerated.len()));
        Ok((Self { pool, persisted }, index))
    }

    pub fn delta(
        &self,
        index: &Index,
    ) -> Option<(u64, Vec<Change>, Vec<String>, Option<Vec<Item>>)> {
        let (sequence, count) = *self.persisted.lock().unwrap();
        if sequence == index.sequence && count == index.enumerated.len() {
            return None;
        }
        let changes: Vec<_> = index
            .changes
            .iter()
            .filter(|change| change.sequence > sequence)
            .cloned()
            .collect();
        let gap = sequence < index.sequence
            && changes
                .first()
                .is_none_or(|change| change.sequence != sequence + 1);
        Some((
            index.sequence,
            changes,
            index.enumerated.iter().cloned().collect(),
            gap.then(|| index.items.values().cloned().collect()),
        ))
    }

    pub async fn save(
        &self,
        sequence: u64,
        changes: Vec<Change>,
        enumerated: Vec<String>,
        snapshot: Option<Vec<Item>>,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        if let Some(items) = snapshot {
            sqlx::query("DELETE FROM items")
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
            sqlx::query("DELETE FROM changes")
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
            for item in items {
                sqlx::query("INSERT INTO items (id,json) VALUES (?,?)")
                    .bind(&item.id)
                    .bind(serde_json::to_vec(&item).map_err(database_error)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
        }
        for change in changes {
            if !change.item.source_id.is_empty() {
                sqlx::query("INSERT OR REPLACE INTO source_ids (source_id,id) VALUES (?,?)")
                    .bind(&change.item.source_id)
                    .bind(&change.item.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            // The snapshot, when present, already contains the final item state.
            // Reapplying these changes is harmless and keeps this path simple.
            if change.deleted {
                sqlx::query("DELETE FROM items WHERE id=?")
                    .bind(&change.item.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            } else {
                sqlx::query("INSERT OR REPLACE INTO items (id,json) VALUES (?,?)")
                    .bind(&change.item.id)
                    .bind(serde_json::to_vec(&change.item).map_err(database_error)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(database_error)?;
            }
            sqlx::query("INSERT OR REPLACE INTO changes (sequence,json) VALUES (?,?)")
                .bind(change.sequence as i64)
                .bind(serde_json::to_vec(&change).map_err(database_error)?)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
        }
        sqlx::query("DELETE FROM changes WHERE sequence <= ?")
            .bind(sequence.saturating_sub(10_000) as i64)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        for id in &enumerated {
            sqlx::query("INSERT OR IGNORE INTO enumerated (id) VALUES (?)")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
        }
        sqlx::query("DELETE FROM enumerated WHERE id NOT IN (SELECT id FROM items)")
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("UPDATE meta SET value=? WHERE key='sequence'")
            .bind(sequence as i64)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        tx.commit().await.map_err(database_error)?;
        *self.persisted.lock().unwrap() = (sequence, enumerated.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn incremental_index_survives_restart_and_repairs_a_trimmed_gap() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("index.sqlite3");
        let (store, mut index) = Store::open(&path, Index::root("Shared", true))
            .await
            .unwrap();
        index.upsert(Item {
            id: "file".into(),
            source_id: "source-file".into(),
            parent_id: "root".into(),
            name: "one".into(),
            path: "one".into(),
            folder: false,
            size: 1,
            modified_at_ms: 1,
            revision: "v1".into(),
            writable: true,
        });
        let (sequence, changes, enumerated, snapshot) = store.delta(&index).unwrap();
        assert!(snapshot.is_none());
        store
            .save(sequence, changes, enumerated, snapshot)
            .await
            .unwrap();
        drop(store);
        let (store, mut restored) = Store::open(&path, Index::default()).await.unwrap();
        assert_eq!(restored.item("file").unwrap().name, "one");
        assert_eq!(
            restored.source_ids.get("source-file").map(String::as_str),
            Some("file")
        );
        for size in 2..10_010 {
            let mut file = restored.item("file").unwrap();
            file.size = size;
            restored.upsert(file);
        }
        let (sequence, changes, enumerated, snapshot) = store.delta(&restored).unwrap();
        assert!(snapshot.is_some());
        store
            .save(sequence, changes, enumerated, snapshot)
            .await
            .unwrap();
        drop(store);
        let (store, mut repaired) = Store::open(&path, Index::default()).await.unwrap();
        assert_eq!(repaired.item("file").unwrap().size, 10_009);
        let mut replacement = repaired.item("file").unwrap();
        replacement.source_id = "replacement-source".into();
        repaired.upsert(replacement.clone());
        let (sequence, changes, enumerated, snapshot) = store.delta(&repaired).unwrap();
        store
            .save(sequence, changes, enumerated, snapshot)
            .await
            .unwrap();
        repaired.remove("file");
        let (sequence, changes, enumerated, snapshot) = store.delta(&repaired).unwrap();
        store
            .save(sequence, changes, enumerated, snapshot)
            .await
            .unwrap();
        drop(store);
        let (_, mut reopened) = Store::open(&path, Index::default()).await.unwrap();
        replacement.id = "replacement-source".into();
        replacement.path = "renamed".into();
        replacement.name = "renamed".into();
        assert_eq!(reopened.upsert(replacement).id, "file");
    }
}
