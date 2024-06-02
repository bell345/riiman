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
use crate::data::{kind, FieldDefinition, FieldKind, FieldValue, FieldValueKind, Vault};
use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    path: String,
    fields: DashMap<Uuid, FieldValue>,
}

impl Item {
    pub fn new(path: String) -> Item {
        Item {
            path,
            fields: Default::default(),
        }
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn path_string(&self) -> &String {
        &self.path
    }

    pub fn link_ref(&self) -> anyhow::Result<Option<(String, String)>> {
        self.get_known_field_value(fields::general::LINK)
    }

    pub fn blocking_update_link(&self, state: AppStateRef) -> Result<bool, ()> {
        let r = state.blocking_read();
        let vault = r.catch(|| r.current_vault())?;
        let link_ref = r.catch(|| self.link_ref())?;

        let mut link_res = None;
        if let Some((other_vault_name, _)) = link_ref {
            let other_vault = r.catch(|| r.get_vault(&other_vault_name))?;
            link_res = r.catch(|| vault.update_link(Path::new(self.path()), &other_vault))?;
        }
        drop(vault);

        r.save_current_vault();
        if let Some(kind::ItemRef((other_vault_name, _))) = link_res {
            r.save_vault_by_name(other_vault_name);
            Ok(true)
        } else {
            Ok(false)
        }
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

        let item = vault.get_cloned_item_or_default(path).unwrap();
        item.set_known_field_value(fields::image::WIDTH, 200);
        vault.update_item(path, item).unwrap();

        let item = vault.get_item_opt(path).unwrap().unwrap();

        assert!(item.has_tag(&vault, &fields::image::NAMESPACE.id).unwrap());
    }
}
