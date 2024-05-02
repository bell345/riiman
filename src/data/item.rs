use std::collections::hash_map::Iter;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;

use anyhow::Context;
use eframe::egui;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::field::KnownField;
use crate::data::{FieldDefinition, FieldKind, FieldValue, FieldValueKind, Vault};
use crate::errors::AppError;
use crate::fields;

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    path: String,
    fields: HashMap<Uuid, FieldValue>,
}

impl Item {
    pub fn new(path: String) -> Item {
        Item {
            path,
            fields: HashMap::new(),
        }
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn set_known_field<T: FieldKind>(&mut self, field: KnownField<T>, value: T) {
        *self
            .fields
            .entry(field.id)
            .or_insert_with(|| <T as Default>::default().into()) = value.into();
    }

    pub fn has_known_field<T: FieldKind>(&self, field: KnownField<T>) -> anyhow::Result<Option<()>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields.get(&field.id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|_: T| Some(()))
                .with_context(|| format!("while retrieving field {}", field.name)),
            None => Ok(None),
        }
    }

    pub fn get_known_field_value<V, T: FieldValueKind<V>>(
        &self,
        field: KnownField<T>,
    ) -> anyhow::Result<Option<V>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields.get(&field.id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|v: T| -> Option<V> { Some(v.into()) })
                .with_context(|| format!("while retrieving field {}", field.name)),
            None => Ok(None),
        }
    }

    pub fn set_known_field_value<V, T: FieldValueKind<V>>(
        &mut self,
        field: KnownField<T>,
        value: V,
    ) {
        *self
            .fields
            .entry(field.id)
            .or_insert_with(|| <T as Default>::default().into()) = T::from(value).into();
    }

    pub fn get_image_size(&self) -> anyhow::Result<Option<egui::Vec2>> {
        let Some(width) = self.get_known_field_value(fields::image::WIDTH)? else {
            return Ok(None);
        };
        let Some(height) = self.get_known_field_value(fields::image::HEIGHT)? else {
            return Ok(None);
        };
        Ok(Some(egui::Vec2::new(width as f32, height as f32)))
    }

    pub fn has_field(&self, field_id: &Uuid) -> bool {
        self.fields.contains_key(field_id)
    }

    pub fn get_field_value(&self, field_id: &Uuid) -> Option<&FieldValue> {
        self.fields.get(field_id)
    }

    pub fn get_field_value_typed<V, T: FieldValueKind<V>>(
        &self,
        field_id: &Uuid,
    ) -> anyhow::Result<Option<V>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields.get(field_id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|v: T| -> Option<V> { Some(v.into()) })
                .with_context(|| format!("while retrieving field with ID {}", field_id)),
            None => Ok(None),
        }
    }

    pub fn iter_fields(&self) -> Iter<'_, Uuid, FieldValue> {
        self.fields.iter()
    }

    pub fn iter_field_defs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
    ) -> impl Iterator<
        Item = (
            impl Deref<Target = FieldDefinition> + 'b,
            impl Deref<Target = FieldValue> + 'a,
        ),
    > {
        self.iter_fields()
            .filter_map(|(id, v)| Some((vault.get_definition(id)?, v)))
    }

    pub fn has_tag(&self, vault: &Vault, tag_id: &Uuid) -> anyhow::Result<bool> {
        let mut seen = HashSet::new();
        let mut queue = vec![*tag_id];
        while let Some(curr) = queue.pop() {
            if seen.contains(&curr) {
                return Err(anyhow::Error::from(AppError::FieldTreeLoop {
                    field_id: curr,
                    item_path: self.path().to_string(),
                }));
            }

            if self.has_field(&curr) {
                return Ok(true);
            }

            let definition = vault.get_definition(&curr).ok_or(anyhow::Error::from(
                AppError::MissingFieldDefinition { id: curr },
            ))?;
            for child_id in definition.iter_child_ids() {
                queue.push(*child_id);
            }

            seen.insert(curr);
        }

        Ok(false)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_has_tag() {
        let vault = Vault::new("test".to_string());
        let mut item = vault.ensure_item_mut(Path::new("path")).unwrap();
        item.set_known_field_value(fields::image::WIDTH, 200);
        assert!(item.has_tag(&vault, &fields::image::NAMESPACE.id).unwrap());
    }
}
