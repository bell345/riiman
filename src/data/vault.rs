use anyhow::anyhow;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use uuid::Uuid;

use crate::data::field_refs::FieldDefRefOrPlaceholder;
use crate::data::transform::SourceKind;
use crate::data::vault_ui::{VaultCacheModel, VaultViewModel};
use crate::data::{kind, FieldDefinition, FieldStore, FieldValue, Item, ItemId};
use crate::errors::{path_to_str, AppError, AppResult, HierarchyError};
use crate::fields;

#[derive(Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(skip)]
    pub name: String,
    definitions: DashMap<Uuid, FieldDefinition>,
    fields: DashMap<Uuid, FieldValue>,
    items: DashMap<String, Arc<Item>>,

    #[serde(default)]
    vm: Mutex<VaultViewModel>,

    #[serde(skip)]
    cache: Mutex<VaultCacheModel>,

    #[serde(skip)]
    pub file_path: Option<Box<Path>>,
    #[serde(skip)]
    items_by_id: DashMap<ItemId, Weak<Item>>,
    #[serde(skip)]
    prevent_save: AtomicBool,
}

impl Debug for Vault {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

enum HierarchyWalkPosition {
    FromParent { id: Uuid, parent_id: Uuid },
    FromChild { id: Uuid, child_id: Uuid },
}

impl HierarchyWalkPosition {
    fn id(&self) -> &Uuid {
        match self {
            Self::FromParent { id, .. } | Self::FromChild { id, .. } => id,
        }
    }
}

impl Vault {
    #[tracing::instrument]
    pub fn new(name: String) -> Vault {
        Vault {
            name,
            ..Default::default()
        }
        .with_standard_defs()
    }

    pub fn with_file_path(mut self, path: &Path) -> Self {
        if let Some(name) = path.file_stem() {
            if let Some(s) = name.to_str() {
                self.name = s.to_string();
            }
        }
        self.set_file_path(path);
        self
    }

    pub fn with_id_lookup(self) -> Self {
        if self.file_path.is_none() {
            return self;
        }

        for item in &self.items {
            self.items_by_id.insert(
                ItemId::from_item(&self, &item),
                Arc::downgrade(item.value()),
            );
        }

        self
    }

    pub fn add_parent_refs(self: &Arc<Vault>) {
        for item in &self.items {
            item.with_vault(&self);
        }
    }

    pub fn with_standard_defs(self) -> Self {
        for def in fields::defs() {
            self.set_definition((*def).clone());
        }
        self
    }

    pub fn root_dir(&self) -> Result<PathBuf, AppError> {
        Ok(self
            .file_path
            .as_ref()
            .ok_or(AppError::VaultNoPath)?
            .parent()
            .ok_or(AppError::VaultNoParent)?
            .into())
    }

    pub fn get_definition(&self, def_id: &Uuid) -> Option<Ref<Uuid, FieldDefinition>> {
        self.definitions.get(def_id)
    }

    pub fn get_definition_or_placeholder(
        &self,
        def_id: &Uuid,
    ) -> FieldDefRefOrPlaceholder<Ref<Uuid, FieldDefinition>> {
        self.get_definition(def_id).into()
    }

    pub fn has_definition(&self, def_id: &Uuid) -> bool {
        self.get_definition(def_id).is_some()
    }

    pub fn set_definition(&self, definition: FieldDefinition) {
        for parent_id in definition.iter_parent_ids() {
            if let Some(parent_ref) = self.definitions.get_mut(&parent_id) {
                parent_ref.add_child(definition.id);
            }
        }
        for child_id in definition.iter_child_ids() {
            if let Some(child_ref) = self.definitions.get_mut(&child_id) {
                child_ref.add_parent(definition.id);
            }
        }
        self.definitions.insert(definition.id, definition);
        self.set_last_updated();
    }

    pub fn remove_definition(&self, id: &Uuid) {
        if self.definitions.remove(id).is_some() {
            for item in self.find_items_by_field(id) {
                item.remove_field(id);
            }

            let desc_ids: Vec<_> = self
                .iter_descendants(id)
                .into_iter()
                .map(|def| def.id)
                .collect();
            for desc_id in desc_ids {
                self.remove_definition(&desc_id);
            }

            self.set_last_updated();
        }
    }

    #[tracing::instrument]
    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
    }

    pub fn resolve_rel_path<'a>(&self, path: &'a Path) -> AppResult<&'a str> {
        let rel_path = match (path.is_relative(), self.file_path.as_ref()) {
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                path.strip_prefix(root_dir)
                    .map_err(|_| AppError::IncompatibleAbsolutePath {
                        path: path.to_owned(),
                        root_dir: root_dir.to_owned(),
                    })?
            }
            _ => path,
        };

        path_to_str(rel_path)
    }

    pub fn resolve_abs_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        Ok(match (path.is_absolute(), self.file_path.as_ref()) {
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                root_dir.join(path)
            }
            _ => path.to_owned(),
        })
    }

    pub fn get_item_opt(&self, path: &Path) -> anyhow::Result<Option<Arc<Item>>> {
        let rel_path = self.resolve_rel_path(path)?;
        Ok(self.items.get(rel_path).map(|r| Arc::clone(&r)))
    }

    pub fn get_item(&self, path: &Path) -> anyhow::Result<Arc<Item>> {
        self.get_item_opt(path)?
            .ok_or(anyhow!(AppError::MissingItem {
                path: path.to_string_lossy().into_owned()
            }))
    }

    pub fn get_item_opt_by_id(&self, id: ItemId) -> Option<Arc<Item>> {
        self.items_by_id.get(&id).and_then(|r| r.upgrade())
    }

    pub fn get_item_by_id(&self, id: ItemId) -> AppResult<Arc<Item>> {
        self.get_item_opt_by_id(id)
            .ok_or(AppError::MissingItemId { id })
    }

    pub fn get_item_or_init(self: &Arc<Self>, path: &Path) -> AppResult<Arc<Item>> {
        let rel_path = self.resolve_rel_path(path)?;
        Ok(self
            .items
            .entry(rel_path.to_owned())
            .or_insert_with(|| {
                let item = Arc::new(Item::new(rel_path.to_owned()));
                item.with_vault(self);
                self.items_by_id
                    .insert(ItemId::from_item(self, &item), Arc::downgrade(&item));
                self.set_last_updated();
                item
            })
            .clone())
    }

    pub fn itemref_of(&self, item: &Item) -> kind::ItemRef {
        kind::ItemRef((self.name.clone().into(), item.path_string().to_owned()))
    }

    pub fn resolve_items(&self, spec: ItemsSpec, cache: &VaultCacheModel) -> Vec<Arc<Item>> {
        let ids = match spec {
            ItemsSpec::All => return self.items.iter().map(|i| Arc::clone(&i)).collect(),
            ItemsSpec::Selection => cache.selection.as_slice(),
            ItemsSpec::Filtered => cache.item_list.item_ids(),
            ItemsSpec::IdList(ids) => ids,
        };
        ids.iter()
            .filter_map(|id| self.get_item_opt_by_id(*id))
            .collect()
    }

    pub fn resolve_items_len(&self, spec: ItemsSpec) -> usize {
        let cache = self.cache();
        match spec {
            ItemsSpec::All => self.items.len(),
            ItemsSpec::Selection => cache.selection.len(),
            ItemsSpec::Filtered => cache.item_list.item_ids().len(),
            ItemsSpec::IdList(ids) => ids.len(),
        }
    }

    pub fn remove_item(&self, path: &Path) -> anyhow::Result<()> {
        let rel_path = self.resolve_rel_path(path)?;
        self.items.remove(rel_path);
        self.set_last_updated();

        Ok(())
    }

    pub fn len_items(&self) -> usize {
        self.items.len()
    }

    pub fn iter_items(&self) -> impl Iterator<Item = RefMulti<'_, String, Arc<Item>>> {
        self.items.iter()
    }

    pub fn iter_field_defs(&self) -> impl Iterator<Item = RefMulti<'_, Uuid, FieldDefinition>> {
        self.definitions.iter()
    }

    pub fn resolve_field_defs(
        &self,
        ids: impl Iterator<Item = impl Deref<Target = Uuid>>,
    ) -> impl Iterator<Item = impl Deref<Target = FieldDefinition> + '_> {
        ids.filter_map(|id| self.get_definition(&id))
    }

    #[tracing::instrument]
    pub fn iter_field_ancestor_paths(&self, id: &Uuid) -> Vec<VecDeque<Uuid>> {
        let Some(def) = self.get_definition(id) else {
            return vec![];
        };
        let mut paths: Vec<VecDeque<Uuid>> = def
            .iter_parent_ids()
            .flat_map(|parent_id| self.iter_field_ancestor_paths(&parent_id))
            .map(|mut path| {
                path.push_back(*id);
                path
            })
            .collect();
        if paths.is_empty() {
            paths.push(VecDeque::from([*id]));
        }
        paths
    }

    #[tracing::instrument]
    pub fn iter_descendants(&self, id: &Uuid) -> Vec<Ref<'_, Uuid, FieldDefinition>> {
        let mut res = vec![];
        let mut queue = vec![*id];
        while let Some(id) = queue.pop() {
            let Some(def) = self.get_definition(&id) else {
                continue;
            };
            for child in def.iter_child_ids() {
                let Some(child_def) = self.get_definition(&child) else {
                    continue;
                };
                queue.extend(child_def.iter_child_ids().map(|cid| *cid));
                res.push(child_def);
            }
        }

        res
    }

    pub fn iter_linked_vault_names(&self) -> HashSet<String> {
        self.items
            .iter()
            .filter_map(|item| item.links().ok())
            .flatten()
            .map(|kind::ItemRef((n, _))| n.into_string())
            .collect()
    }

    pub fn find_items_by_tag(&self, id: &Uuid) -> Vec<RefMulti<'_, String, Arc<Item>>> {
        self.iter_items()
            .filter(|item| item.has_tag(self, id).is_ok_and(|v| v))
            .collect()
    }

    pub fn find_items_by_field(&self, id: &Uuid) -> Vec<RefMulti<'_, String, Arc<Item>>> {
        self.iter_items()
            .filter(|item| item.has_field(id))
            .collect()
    }

    pub fn find_hierarchy_error(&self, def: &FieldDefinition) -> Result<(), HierarchyError> {
        let mut parents = HashSet::new();
        let mut children = HashSet::new();

        let mut queue = vec![];
        queue.extend(
            def.iter_parent_ids()
                .map(|id| HierarchyWalkPosition::FromChild {
                    id: *id,
                    child_id: def.id,
                }),
        );
        queue.extend(
            def.iter_child_ids()
                .map(|id| HierarchyWalkPosition::FromParent {
                    id: *id,
                    parent_id: def.id,
                }),
        );

        while let Some(pos) = queue.pop() {
            match &pos {
                HierarchyWalkPosition::FromParent { id, .. } => {
                    if parents.contains(id) {
                        return Err(HierarchyError::FieldTreeLoop { field_id: *id });
                    }
                    children.insert(*id);
                }
                HierarchyWalkPosition::FromChild { id, .. } => {
                    if children.contains(id) {
                        return Err(HierarchyError::FieldTreeLoop { field_id: *id });
                    }
                    parents.insert(*id);
                }
            }

            let pos_def = self
                .get_definition(pos.id())
                .ok_or(HierarchyError::MissingFieldDefinition { id: *pos.id() })?;

            queue.extend(match pos {
                HierarchyWalkPosition::FromChild { .. } => pos_def
                    .iter_parent_ids()
                    .map(|pid| HierarchyWalkPosition::FromChild {
                        id: *pid,
                        child_id: *pos.id(),
                    })
                    .collect_vec(),
                HierarchyWalkPosition::FromParent { .. } => pos_def
                    .iter_child_ids()
                    .map(|cid| HierarchyWalkPosition::FromParent {
                        id: *cid,
                        parent_id: *pos.id(),
                    })
                    .collect_vec(),
            });
        }

        Ok(())
    }

    pub fn vm(&self) -> MutexGuard<VaultViewModel> {
        self.vm.lock().unwrap()
    }

    pub fn cache(&self) -> MutexGuard<VaultCacheModel> {
        self.cache.lock().unwrap()
    }

    pub fn save_is_prevented(&self) -> bool {
        self.prevent_save.load(Ordering::Relaxed)
    }

    pub fn prevent_save(&self) -> VaultSaveSuppressor {
        self.prevent_save.store(true, Ordering::Relaxed);
        VaultSaveSuppressor(&self)
    }
}

impl FieldStore for Vault {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}

pub enum ItemsSpec<'a> {
    All,
    Selection,
    Filtered,
    IdList(&'a [ItemId]),
}

impl From<SourceKind> for ItemsSpec<'_> {
    fn from(value: SourceKind) -> Self {
        match value {
            SourceKind::Selection => ItemsSpec::Selection,
            SourceKind::Filtered => ItemsSpec::Filtered,
            SourceKind::All => ItemsSpec::All,
        }
    }
}

pub struct VaultSaveSuppressor<'v>(&'v Vault);

impl<'v> Drop for VaultSaveSuppressor<'v> {
    fn drop(&mut self) {
        self.0.prevent_save.store(false, Ordering::Relaxed);
    }
}
