//! Durable identities and incremental change anchors for native file providers.
use super::*;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub path: String,
    pub folder: bool,
    pub size: u64,
    pub modified_at_ms: i64,
    pub revision: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub sequence: u64,
    pub item: Item,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    pub sequence: u64,
    pub items: BTreeMap<String, Item>,
    pub changes: Vec<Change>,
    pub enumerated: HashSet<String>,
}

impl Index {
    pub fn root(name: &str, writable: bool) -> Self {
        let mut index = Self::default();
        index.items.insert(
            "root".into(),
            Item {
                id: "root".into(),
                parent_id: "root".into(),
                name: name.into(),
                path: String::new(),
                folder: true,
                size: 0,
                modified_at_ms: 0,
                revision: String::new(),
                writable,
            },
        );
        index
    }

    pub fn item(&self, id: &str) -> Result<Item, Error> {
        self.items
            .get(id)
            .cloned()
            .ok_or_else(|| Error::missing("file no longer exists"))
    }

    pub fn upsert(&mut self, mut item: Item) -> Item {
        if let Some(previous) = self.items.values().find(|old| old.path == item.path) {
            item.id = previous.id.clone();
        }
        if self.items.get(&item.id) != Some(&item) {
            self.items.insert(item.id.clone(), item.clone());
            self.record(item.clone(), false);
        }
        item
    }

    fn record(&mut self, item: Item, deleted: bool) {
        self.sequence += 1;
        self.changes.push(Change {
            sequence: self.sequence,
            item,
            deleted,
        });
        if self.changes.len() > 10_000 {
            self.changes.drain(..5_000);
        }
    }

    pub fn remove(&mut self, id: &str) {
        let Some(item) = self.items.get(id).cloned() else {
            return;
        };
        let prefix = format!("{}/", item.path);
        let ids: Vec<_> = self
            .items
            .values()
            .filter(|child| child.id == id || child.path.starts_with(&prefix))
            .map(|child| child.id.clone())
            .collect();
        for id in ids {
            if let Some(item) = self.items.remove(&id) {
                self.record(item, true);
            }
            self.enumerated.remove(&id);
        }
    }

    pub fn moved(&mut self, id: &str, parent: &Item, name: &str) -> Result<(), Error> {
        let mut item = self.item(id)?;
        if item.parent_id != parent.id {
            // Container enumerators need a deletion from the old parent. The
            // working set coalesces this with the subsequent update by ID.
            self.record(item.clone(), true);
        }
        let old = item.path.clone();
        let new = join(&parent.path, name);
        item.path = new.clone();
        item.parent_id = parent.id.clone();
        item.name = name.into();
        self.items.insert(id.into(), item.clone());
        self.record(item, false);
        let descendants: Vec<_> = self
            .items
            .values()
            .filter(|child| child.path.starts_with(&format!("{old}/")))
            .cloned()
            .collect();
        for mut child in descendants {
            child.path = format!("{new}{}", &child.path[old.len()..]);
            self.items.insert(child.id.clone(), child.clone());
            self.record(child, false);
        }
        Ok(())
    }

    pub fn since(&self, anchor: u64) -> Result<Vec<Change>, Error> {
        if anchor > self.sequence
            || self
                .changes
                .first()
                .is_some_and(|c| anchor.saturating_add(1) < c.sequence)
        {
            return Err(Error {
                code: "anchorExpired".into(),
                message: "file index must be refreshed".into(),
            });
        }
        Ok(self
            .changes
            .iter()
            .filter(|change| change.sequence > anchor)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(id: &str, path: &str, parent: &str) -> Item {
        Item {
            id: id.into(),
            path: path.into(),
            parent_id: parent.into(),
            name: path.rsplit('/').next().unwrap().into(),
            folder: false,
            size: 1,
            modified_at_ms: 1,
            revision: "v1".into(),
            writable: true,
        }
    }
    #[test]
    fn identities_survive_move_and_reload_and_deletions_are_recorded() {
        let mut index = Index::root("Share", true);
        let mut folder = file("folder", "old", "root");
        folder.folder = true;
        index.upsert(folder);
        index.upsert(file("file", "old/a", "folder"));
        index
            .moved("folder", &index.item("root").unwrap(), "new")
            .unwrap();
        assert_eq!(index.item("file").unwrap().path, "new/a");
        let mut restored: Index =
            serde_json::from_slice(&serde_json::to_vec(&index).unwrap()).unwrap();
        let anchor = restored.sequence;
        restored.remove("folder");
        assert_eq!(restored.since(anchor).unwrap().len(), 2);
        assert_eq!(restored.items.len(), 1);
    }
    #[test]
    fn enumeration_reuses_identity_and_unchanged_state_does_not_advance_anchor() {
        let mut index = Index::root("Share", true);
        index.upsert(file("one", "a", "root"));
        let sequence = index.sequence;
        assert_eq!(index.upsert(file("two", "a", "root")).id, "one");
        assert_eq!(index.sequence, sequence);
        assert!(index.since(sequence + 1).is_err());
    }
    #[test]
    fn trimmed_journal_requires_a_full_rescan_instead_of_losing_changes() {
        let mut index = Index::root("Share", true);
        for n in 0..10_002 {
            let mut item = file("one", "a", "root");
            item.size = n;
            index.upsert(item);
        }
        assert!(index.since(0).is_err());
        assert_eq!(index.since(index.sequence - 1).unwrap().len(), 1);
        let restored: Index = serde_json::from_slice(&serde_json::to_vec(&index).unwrap()).unwrap();
        assert!(restored.since(0).is_err());
    }
}
