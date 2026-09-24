//! Durable identities and incremental change anchors for native file providers.
use super::*;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Bound,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    #[serde(default)]
    pub source_id: String,
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
    #[serde(skip)]
    pub(super) paths: HashMap<String, String>,
    #[serde(skip)]
    pub(super) children: HashMap<String, BTreeSet<(String, String)>>,
    #[serde(skip)]
    pub(super) working_set: BTreeSet<(String, String)>,
    #[serde(skip)]
    pub(super) children_by_date: HashMap<String, BTreeSet<(Reverse<i64>, String)>>,
    #[serde(skip)]
    pub(super) working_set_by_date: BTreeSet<(Reverse<i64>, String)>,
    #[serde(default)]
    pub(super) source_ids: HashMap<String, String>,
}

impl Index {
    pub fn page(
        &mut self,
        container: &str,
        page: Option<&str>,
        limit: usize,
        sort_by_date: bool,
    ) -> Result<(Vec<Item>, Option<String>), Error> {
        let (anchor, order, after) = match page {
            Some(page) => {
                let (anchor, rest) = page.split_once('|').ok_or_else(|| Error {
                    code: "pageExpired".into(),
                    message: "file listing changed; start again".into(),
                })?;
                let (order, after) = rest.split_once('|').ok_or_else(|| Error {
                    code: "pageExpired".into(),
                    message: "file listing changed; start again".into(),
                })?;
                (anchor.parse::<u64>().ok(), order, Some(after))
            }
            None => (
                Some(self.sequence),
                if sort_by_date { "d" } else { "n" },
                None,
            ),
        };
        if anchor != Some(self.sequence) || !matches!(order, "d" | "n") {
            return Err(Error {
                code: "pageExpired".into(),
                message: "file listing changed; start again".into(),
            });
        }
        self.ensure_indexes();
        let after_item = after
            .map(|id| {
                self.items.get(id).ok_or_else(|| Error {
                    code: "pageExpired".into(),
                    message: "file listing changed; start again".into(),
                })
            })
            .transpose()?;
        let mut items = if order == "d" {
            let after_key = after_item.map(|item| (Reverse(item.modified_at_ms), item.id.clone()));
            let ordered = if container == "workingSet" {
                Some(&self.working_set_by_date)
            } else {
                self.children_by_date.get(container)
            };
            ordered
                .into_iter()
                .flat_map(|ordered| {
                    ordered.range((
                        after_key.as_ref().map_or(Bound::Unbounded, Bound::Excluded),
                        Bound::Unbounded,
                    ))
                })
                .take(limit + 1)
                .filter_map(|(_, id)| self.items.get(id).cloned())
                .collect::<Vec<_>>()
        } else {
            let after_key = after_item.map(|item| (item.name.clone(), item.id.clone()));
            let ordered = if container == "workingSet" {
                Some(&self.working_set)
            } else {
                self.children.get(container)
            };
            ordered
                .into_iter()
                .flat_map(|ordered| {
                    ordered.range((
                        after_key.as_ref().map_or(Bound::Unbounded, Bound::Excluded),
                        Bound::Unbounded,
                    ))
                })
                .take(limit + 1)
                .filter_map(|(_, id)| self.items.get(id).cloned())
                .collect::<Vec<_>>()
        };
        let next = if items.len() > limit {
            items.truncate(limit);
            items
                .last()
                .map(|item| format!("{}|{}|{}", self.sequence, order, item.id))
        } else {
            None
        };
        Ok((items, next))
    }

    pub fn changes_page(
        &self,
        anchor: u64,
        limit: usize,
    ) -> Result<(Vec<Change>, u64, bool), Error> {
        self.since(anchor)?;
        let changes: Vec<_> = self
            .changes
            .iter()
            .filter(|change| change.sequence > anchor)
            .take(limit)
            .cloned()
            .collect();
        let next = changes.last().map_or(anchor, |change| change.sequence);
        Ok((changes, next, next < self.sequence))
    }

    pub fn root(name: &str, writable: bool) -> Self {
        let mut index = Self::default();
        index.items.insert(
            "root".into(),
            Item {
                id: "root".into(),
                source_id: String::new(),
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
        index.rebuild_indexes();
        index
    }

    fn rebuild_indexes(&mut self) {
        self.paths.clear();
        self.children.clear();
        self.working_set.clear();
        self.children_by_date.clear();
        self.working_set_by_date.clear();
        for item in self.items.values() {
            self.paths.insert(item.path.clone(), item.id.clone());
            if !item.source_id.is_empty() {
                self.source_ids
                    .insert(item.source_id.clone(), item.id.clone());
            }
            if item.id != "root" {
                let key = (item.name.clone(), item.id.clone());
                self.children
                    .entry(item.parent_id.clone())
                    .or_default()
                    .insert(key.clone());
                self.working_set.insert(key);
                let date_key = (Reverse(item.modified_at_ms), item.id.clone());
                self.children_by_date
                    .entry(item.parent_id.clone())
                    .or_default()
                    .insert(date_key.clone());
                self.working_set_by_date.insert(date_key);
            }
        }
    }

    fn ensure_indexes(&mut self) {
        if self.paths.len() != self.items.len()
            || self.working_set.len() + 1 != self.items.len()
            || self.working_set_by_date.len() + 1 != self.items.len()
        {
            self.rebuild_indexes();
        }
    }

    pub fn item(&self, id: &str) -> Result<Item, Error> {
        self.items
            .get(id)
            .cloned()
            .ok_or_else(|| Error::missing("file no longer exists"))
    }

    pub fn id_for_path(&mut self, path: &str) -> Option<String> {
        self.ensure_indexes();
        self.paths.get(path).cloned()
    }

    pub fn upsert(&mut self, mut item: Item) -> Item {
        self.ensure_indexes();
        if !item.source_id.is_empty() {
            if let Some(id) = self.paths.get(&item.path) {
                item.id = id.clone();
            } else if let Some(id) = self.source_ids.get(&item.source_id) {
                item.id = id.clone();
            }
            self.source_ids
                .insert(item.source_id.clone(), item.id.clone());
        }
        if let Some(previous) = self.items.get(&item.id).cloned() {
            if previous.parent_id != item.parent_id {
                self.record(previous.clone(), true);
            }
            if previous.folder && previous.path != item.path {
                let prefix = format!("{}/", previous.path);
                let descendants: Vec<_> = self
                    .items
                    .values()
                    .filter(|child| child.path.starts_with(&prefix))
                    .cloned()
                    .collect();
                for mut child in descendants {
                    self.paths.remove(&child.path);
                    child.path = format!("{}{}", item.path, &child.path[previous.path.len()..]);
                    self.paths.insert(child.path.clone(), child.id.clone());
                    self.items.insert(child.id.clone(), child.clone());
                    self.record(child, false);
                }
            }
        }
        if let Some(previous_id) = self.paths.get(&item.path).cloned() {
            if previous_id != item.id {
                self.remove(&previous_id);
            }
        }
        if self.items.get(&item.id) != Some(&item) {
            if let Some(previous) = self.items.get(&item.id) {
                self.paths.remove(&previous.path);
                self.children
                    .entry(previous.parent_id.clone())
                    .or_default()
                    .remove(&(previous.name.clone(), previous.id.clone()));
                self.working_set
                    .remove(&(previous.name.clone(), previous.id.clone()));
                self.children_by_date
                    .entry(previous.parent_id.clone())
                    .or_default()
                    .remove(&(Reverse(previous.modified_at_ms), previous.id.clone()));
                self.working_set_by_date
                    .remove(&(Reverse(previous.modified_at_ms), previous.id.clone()));
            }
            self.paths.insert(item.path.clone(), item.id.clone());
            if item.id != "root" {
                let key = (item.name.clone(), item.id.clone());
                self.children
                    .entry(item.parent_id.clone())
                    .or_default()
                    .insert(key.clone());
                self.working_set.insert(key);
                let date_key = (Reverse(item.modified_at_ms), item.id.clone());
                self.children_by_date
                    .entry(item.parent_id.clone())
                    .or_default()
                    .insert(date_key.clone());
                self.working_set_by_date.insert(date_key);
            }
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
        self.ensure_indexes();
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
                self.paths.remove(&item.path);
                self.children
                    .entry(item.parent_id.clone())
                    .or_default()
                    .remove(&(item.name.clone(), item.id.clone()));
                self.working_set
                    .remove(&(item.name.clone(), item.id.clone()));
                self.children_by_date
                    .entry(item.parent_id.clone())
                    .or_default()
                    .remove(&(Reverse(item.modified_at_ms), item.id.clone()));
                self.working_set_by_date
                    .remove(&(Reverse(item.modified_at_ms), item.id.clone()));
                self.record(item, true);
            }
            self.enumerated.remove(&id);
        }
    }

    pub fn moved(&mut self, id: &str, parent: &Item, name: &str) -> Result<(), Error> {
        self.ensure_indexes();
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
        self.paths.remove(&old);
        self.paths.insert(new.clone(), id.into());
        self.record(item, false);
        let descendants: Vec<_> = self
            .items
            .values()
            .filter(|child| child.path.starts_with(&format!("{old}/")))
            .cloned()
            .collect();
        for mut child in descendants {
            self.paths.remove(&child.path);
            child.path = format!("{new}{}", &child.path[old.len()..]);
            self.paths.insert(child.path.clone(), child.id.clone());
            self.items.insert(child.id.clone(), child.clone());
            self.record(child, false);
        }
        self.rebuild_indexes();
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
            source_id: id.into(),
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
    fn source_identity_survives_rename_and_path_updates_keep_the_local_identity() {
        let mut index = Index::root("Share", true);
        index.upsert(file("one", "a", "root"));
        let sequence = index.sequence;
        assert_eq!(index.upsert(file("one", "a", "root")).id, "one");
        assert_eq!(index.sequence, sequence);
        assert_eq!(index.upsert(file("two", "a", "root")).id, "one");
        assert_eq!(index.item("one").unwrap().source_id, "two");
        assert_eq!(index.upsert(file("two", "renamed", "root")).id, "one");
        assert!(index.since(index.sequence + 1).is_err());
    }
    #[test]
    fn pages_follow_name_order_and_expire_after_a_change() {
        let mut index = Index::root("Share", true);
        index.upsert(file("id-z", "z", "root"));
        index.upsert(file("id-a", "a", "root"));
        index.upsert(file("id-m", "m", "root"));
        let (first, next) = index.page("root", None, 2, false).unwrap();
        assert_eq!(
            first
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "m"]
        );
        let (last, _) = index.page("root", next.as_deref(), 2, false).unwrap();
        assert_eq!(last[0].name, "z");
        index.upsert(file("id-b", "b", "root"));
        assert!(index.page("root", next.as_deref(), 2, false).is_err());
    }
    #[test]
    fn date_pages_put_recent_items_first() {
        let mut index = Index::root("Share", true);
        for (id, modified_at_ms) in [("old", 1), ("new", 3), ("middle", 2)] {
            let mut item = file(id, id, "root");
            item.modified_at_ms = modified_at_ms;
            index.upsert(item);
        }
        let (first, next) = index.page("root", None, 2, true).unwrap();
        assert_eq!(
            first
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["new", "middle"]
        );
        let (last, _) = index.page("root", next.as_deref(), 2, false).unwrap();
        assert_eq!(last[0].id, "old");
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
