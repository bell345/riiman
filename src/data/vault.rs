use anyhow::{anyhow, Context};
use chrono::{DateTime, TimeZone, Utc};
use std::collections::{HashSet, VecDeque};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use eframe::egui;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::data::field_refs::FieldDefRefOrPlaceholder;
use crate::data::{kind, FieldDefinition, FieldStore, FieldValue, Item};
use crate::errors::{AppError, HierarchyError};
use crate::fields;
use crate::state::AppStateRef;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Vault {
    pub name: String,
    definitions: DashMap<Uuid, FieldDefinition>,
    fields: DashMap<Uuid, FieldValue>,
    items: DashMap<String, Arc<Item>>,
    last_updated: Mutex<Option<DateTime<Utc>>>,

    #[serde(skip)]
    pub file_path: Option<Box<Path>>,
    #[serde(skip)]
    items_by_id: DashMap<egui::Id, Weak<Item>>,
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
    pub fn new(name: String) -> Vault {
        Vault {
            name,
            last_updated: Some(Utc::now()).into(),
            ..Default::default()
        }
        .with_standard_defs()
    }

    pub fn with_file_path(mut self, path: &Path) -> Self {
        self.set_file_path(path);
        self
    }

    pub fn with_id_lookup(self) -> Self {
        if self.file_path.is_none() {
            return self;
        }

        for item in &self.items {
            let Ok(abs_path) = self.resolve_abs_path(Path::new(item.path())) else {
                break;
            };
            self.items_by_id
                .insert(egui::Id::new(abs_path), Arc::downgrade(item.value()));
        }

        self
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

    pub fn last_updated(&self) -> DateTime<Utc> {
        self.last_updated
            .lock()
            .unwrap()
            .unwrap_or(Utc.timestamp_nanos(0))
    }

    pub fn set_last_updated(&self) {
        *self.last_updated.lock().unwrap() = Some(Utc::now());
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

    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
        self.set_last_updated();
    }

    pub fn resolve_rel_path<'a>(&self, path: &'a Path) -> anyhow::Result<&'a str> {
        let rel_path = match (path.is_relative(), self.file_path.as_ref()) {
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                path.strip_prefix(root_dir)?
            }
            _ => path,
        };

        rel_path
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("while decoding path: {}", path.display()))
    }

    pub fn resolve_abs_path(&self, path: &Path) -> anyhow::Result<String> {
        let abs_path = match (path.is_absolute(), self.file_path.as_ref()) {
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                root_dir.join(path)
            }
            _ => path.to_owned(),
        };

        Ok(abs_path
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("while decoding path: {}", abs_path.display()))?
            .to_string())
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

    pub fn get_item_opt_by_id(&self, id: egui::Id) -> Option<Arc<Item>> {
        self.items_by_id.get(&id).and_then(|r| r.upgrade())
    }

    pub fn get_item_by_id(&self, id: egui::Id) -> anyhow::Result<Arc<Item>> {
        self.get_item_opt_by_id(id)
            .ok_or(anyhow!(AppError::MissingItemId { id }))
    }

    pub fn get_item_or_init(&self, path: &Path) -> anyhow::Result<Arc<Item>> {
        let rel_path = self.resolve_rel_path(path)?;
        let abs_path = self.resolve_abs_path(path)?;
        Ok(self.items.get(rel_path).map_or_else(
            || {
                let item = Arc::new(Item::new(rel_path.to_owned()));
                self.items_by_id
                    .insert(egui::Id::new(abs_path), Arc::downgrade(&item));
                self.set_last_updated();
                item
            },
            |i| Arc::clone(i.value()),
        ))
    }

    pub fn remove_item(&self, path: &Path) -> anyhow::Result<()> {
        let rel_path = self.resolve_rel_path(path)?;
        self.items.remove(rel_path);

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
}

impl FieldStore for Vault {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}
