use std::collections::HashMap;
use std::path::Path;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::FieldValue;

use super::field::FieldDefinition;
use super::item::Item;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Vault {
    pub name: String,
    definitions: HashMap<Uuid, FieldDefinition>,
    fields: HashMap<Uuid, FieldValue>,
    items: HashMap<String, Item>,

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

    pub fn with_standard_defs(mut self) -> Self {
        for def in crate::fields::defs() {
            let _ = self.add_definition((*def).clone());
        }
        self
    }

    pub fn add_definition(&mut self, definition: FieldDefinition) -> anyhow::Result<()> {
        let FieldDefinition { id, name, .. } = definition.clone();
        if self.definitions.insert(id, definition).is_some() {
            return Err(anyhow!(
                "Duplicate definition found for ID={}, name={}",
                id,
                name
            ));
        }
        Ok(())
    }

    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
    }
}
