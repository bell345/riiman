use std::collections::hash_map::Iter;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
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

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    path: Utf32CachedString,
    fields: DashMap<Uuid, FieldValue>,
}

impl Debug for Item {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Item {
    #[tracing::instrument]
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

    pub fn get_image_size(&self) -> anyhow::Result<Option<egui::Vec2>> {
        let Some(width) = self.get_known_field_value(fields::image::WIDTH)? else {
            return Ok(None);
        };
        let Some(height) = self.get_known_field_value(fields::image::HEIGHT)? else {
            return Ok(None);
        };
        #[allow(clippy::cast_precision_loss)]
        Ok(Some(egui::Vec2::new(width as f32, height as f32)))
    }

    pub fn expect_image_size(&self) -> anyhow::Result<egui::Vec2> {
        self.get_image_size()?.ok_or(
            AppError::MissingImageFields {
                path: self.path().into(),
            }
            .into(),
        )
    }

    pub fn links(&self) -> anyhow::Result<Vec<kind::ItemRef>> {
        let mut links = vec![];
        links.extend(self.get_known_field_value(fields::general::LINK)?);
        links.extend(self.get_known_field_value(fields::general::ORIGINAL)?);
        if let Some(l) = self.get_known_field_value(fields::general::DERIVED)? {
            links.extend(l.into_iter().filter_map(|v| v.as_itemref_opt().cloned()));
        }

        Ok(links.into_iter().map(|l| l.into()).collect())
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
