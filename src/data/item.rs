use std::collections::hash_map::Iter;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;

use anyhow::Context;
use dashmap::DashMap;
use eframe::egui;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::field::KnownField;
use crate::data::field_store::FieldStore;
use crate::data::{kind, FieldDefinition, FieldValue, Utf32CachedString, Vault};
use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    path: Utf32CachedString,
    fields: DashMap<Uuid, FieldValue>,
}

impl Item {
    pub fn new(path: String) -> Item {
        Item {
            path: path.into(),
            fields: Default::default(),
        }
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn path_string(&self) -> &Utf32CachedString {
        &self.path
    }
}

impl FieldStore for Item {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::data::field_store::FieldStore;
    use std::path::Path;

    #[test]
    fn test_has_tag() {
        let vault = Vault::new("test".to_string());
        let path = Path::new("path");
        //let mut item = vault.ensure_item_mut(Path::new("path")).unwrap();

        let item = vault.get_item_or_init(path).unwrap();
        item.set_known_field_value(fields::image::WIDTH, 200);

        let item = vault.get_item_opt(path).unwrap().unwrap();

        assert!(item.has_tag(&vault, &fields::image::NAMESPACE.id).unwrap());
    }
}
