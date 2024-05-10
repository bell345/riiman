use anyhow::Context;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::{FieldStore, FieldValue};
use crate::errors::AppError;

use super::field::FieldDefinition;
use super::item::Item;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Vault {
    pub name: String,
    definitions: DashMap<Uuid, FieldDefinition>,
    fields: DashMap<Uuid, FieldValue>,
    items: DashMap<String, Item>,

    #[serde(skip)]
    pub file_path: Option<Box<Path>>,
}

impl Vault {
    pub fn new(name: String) -> Vault {
        Vault {
            name,
            ..Default::default()
        }
        .with_standard_defs()
    }

    pub fn with_file_path(mut self, path: &Path) -> Self {
        self.set_file_path(path);
        self
    }

    pub fn with_standard_defs(self) -> Self {
        for def in crate::fields::defs() {
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

    pub fn set_definition(&self, definition: FieldDefinition) {
        for parent_id in definition.iter_parent_ids() {
            if let Some(mut parent_ref) = self.definitions.get_mut(&parent_id) {
                parent_ref.add_child(definition.id);
            }
        }
        for child_id in definition.iter_child_ids() {
            if let Some(mut child_ref) = self.definitions.get_mut(&child_id) {
                child_ref.add_parent(definition.id);
            }
        }
        self.definitions.insert(definition.id, definition);
    }

    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
    }

    pub fn resolve_rel_path<'a>(&self, path: &'a Path) -> anyhow::Result<&'a str> {
        let rel_path = match (path.is_relative(), self.file_path.as_ref()) {
            (true, Some(_)) => path,
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                path.strip_prefix(root_dir)?
            }
            (_, None) => path,
        };

        rel_path
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("while decoding path: {}", path.display()))
    }

    pub fn resolve_abs_path(&self, path: &Path) -> anyhow::Result<String> {
        let abs_path = match (path.is_absolute(), self.file_path.as_ref()) {
            (true, _) => path.to_owned(),
            (_, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                root_dir.join(path)
            }
            (_, None) => path.to_owned(),
        };

        Ok(abs_path
            .to_str()
            .ok_or(AppError::InvalidUnicode)
            .with_context(|| format!("while decoding path: {}", abs_path.display()))?
            .to_string())
    }

    pub fn get_item<'a>(
        &'a self,
        path: &Path,
    ) -> anyhow::Result<Option<impl Deref<Target = Item> + 'a>> {
        let rel_path = self.resolve_rel_path(path)?;
        Ok(self.items.get(rel_path))
    }

    pub fn ensure_item_mut(&self, path: &Path) -> anyhow::Result<RefMut<String, Item>> {
        let rel_path = self.resolve_rel_path(path)?.to_string();
        Ok(self
            .items
            .entry(rel_path.clone())
            .or_insert_with(|| Item::new(rel_path)))
    }

    pub fn len_items(&self) -> usize {
        self.items.len()
    }

    pub fn iter_items(&self) -> impl Iterator<Item = RefMulti<'_, String, Item>> {
        self.items.iter()
    }
}

impl FieldStore for Vault {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}
