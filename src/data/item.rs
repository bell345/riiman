use std::collections::HashMap;

use anyhow::Context;
use eframe::egui;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::field::KnownField;
use crate::data::{FieldKind, FieldValue, FieldValueKind};
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
        return Ok(Some(egui::Vec2::new(width as f32, height as f32)));
    }
}
