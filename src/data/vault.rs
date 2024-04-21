use std::path::Path;

use serde::{Deserialize, Serialize};

use super::field::{FieldDefinition, FieldPair};

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Vault {
    pub name: String,
    definitions: Vec<FieldDefinition>,
    fields: Vec<FieldPair>,

    #[serde(skip)]
    pub file_path: Option<Box<Path>>,
}

impl Vault {
    pub fn new(name: String) -> Vault {
        Vault {
            name,
            ..Default::default()
        }
    }

    pub fn with_file_path(mut self, path: &Path) -> Self {
        self.set_file_path(path);
        self
    }

    pub fn set_file_path(&mut self, path: &Path) {
        self.file_path = Some(path.into());
    }
}
