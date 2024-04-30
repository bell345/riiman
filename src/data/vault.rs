use std::path::Path;

use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::FieldValue;
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

    pub fn set_definition(&self, definition: FieldDefinition) {
        self.definitions.insert(definition.id, definition);
    }

    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
    }

    fn resolve_rel_path<'a>(&self, path: &'a Path) -> anyhow::Result<&'a str> {
        let rel_path = match (path.is_relative(), self.file_path.as_ref()) {
            (true, Some(_)) => path,
            (false, Some(vault_path)) => {
                let root_dir = vault_path.parent().ok_or(AppError::VaultNoParent)?;
                path.strip_prefix(root_dir)?
            }
            (_, None) => path,
        };

        Ok(rel_path.to_str().ok_or(AppError::InvalidUnicode)?)
    }

    pub fn get_item(&self, path: &Path) -> anyhow::Result<Option<Ref<String, Item>>> {
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

    pub fn iter_items(&self) -> dashmap::iter::Iter<String, Item> {
        self.items.iter()
    }
}
