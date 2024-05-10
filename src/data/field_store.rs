use crate::data::{FieldDefinition, FieldKind, FieldValue, FieldValueKind, KnownField, Vault};
use crate::errors::AppError;
use crate::fields;
use anyhow::Context;
use dashmap::iter::Iter;
use dashmap::DashMap;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use uuid::Uuid;

pub struct FieldDefRef<'a> {
    inner: dashmap::mapref::multiple::RefMulti<'a, Uuid, FieldValue>,
    definition: dashmap::mapref::one::Ref<'a, Uuid, FieldDefinition>,
}

impl<'a> FieldDefRef<'a> {
    fn try_new<'b: 'a>(
        inner: dashmap::mapref::multiple::RefMulti<'a, Uuid, FieldValue>,
        vault: &'b Vault,
    ) -> Option<Self> {
        Some(Self {
            definition: vault.get_definition(inner.key())?,
            inner,
        })
    }

    pub fn definition(&self) -> &(impl Deref<Target = FieldDefinition> + 'a) {
        &self.definition
    }

    pub fn value(&self) -> &FieldValue {
        self.inner.value()
    }
}

pub trait FieldStore {
    fn fields(&self) -> &DashMap<Uuid, FieldValue>;

    fn set_known_field<T: FieldKind>(&mut self, field: KnownField<T>, value: T) {
        *self
            .fields()
            .entry(field.id)
            .or_insert_with(|| <T as Default>::default().into()) = value.into();
    }

    fn has_known_field<T: FieldKind>(&self, field: KnownField<T>) -> anyhow::Result<Option<()>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields().get(&field.id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|_: T| Some(()))
                .with_context(|| format!("while retrieving field {}", field.name)),
            None => Ok(None),
        }
    }

    fn get_known_field_value<V, T: FieldValueKind<V>>(
        &self,
        field: KnownField<T>,
    ) -> anyhow::Result<Option<V>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields().get(&field.id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|v: T| -> Option<V> { Some(v.into()) })
                .with_context(|| format!("while retrieving field {}", field.name)),
            None => Ok(None),
        }
    }

    fn get_or_insert_known_field_value<V, T: FieldValueKind<V>>(
        &mut self,
        field: KnownField<T>,
        default_value: V,
    ) -> anyhow::Result<V>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        self.fields()
            .entry(field.id)
            .or_insert(T::from(default_value).into())
            .clone()
            .try_into()
            .map(|v: T| -> V { v.into() })
            .with_context(|| format!("while retrieving field {}", field.name))
    }

    fn set_known_field_value<V, T: FieldValueKind<V>>(&mut self, field: KnownField<T>, value: V) {
        *self
            .fields()
            .entry(field.id)
            .or_insert_with(|| <T as Default>::default().into()) = T::from(value).into();
    }

    fn get_image_size(&self) -> anyhow::Result<Option<egui::Vec2>> {
        let Some(width) = self.get_known_field_value(fields::image::WIDTH)? else {
            return Ok(None);
        };
        let Some(height) = self.get_known_field_value(fields::image::HEIGHT)? else {
            return Ok(None);
        };
        Ok(Some(egui::Vec2::new(width as f32, height as f32)))
    }

    fn has_field(&self, field_id: &Uuid) -> bool {
        self.fields().contains_key(field_id)
    }

    fn get_field_value(&self, field_id: &Uuid) -> Option<impl Deref<Target = FieldValue>> {
        self.fields().get(field_id)
    }

    fn get_field_value_typed<V, T: FieldValueKind<V>>(
        &self,
        field_id: &Uuid,
    ) -> anyhow::Result<Option<V>>
    where
        <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
    {
        match self.fields().get(field_id) {
            Some(fv) => fv
                .clone()
                .try_into()
                .map(|v: T| -> Option<V> { Some(v.into()) })
                .with_context(|| format!("while retrieving field with ID {}", field_id)),
            None => Ok(None),
        }
    }

    fn iter_fields(&self) -> Iter<'_, Uuid, FieldValue> {
        self.fields().iter()
    }

    fn iter_field_defs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
    ) -> impl Iterator<Item = FieldDefRef<'a>> {
        self.iter_fields()
            .filter_map(|r| FieldDefRef::try_new(r, vault))
    }

    fn has_tag(&self, vault: &Vault, tag_id: &Uuid) -> anyhow::Result<bool> {
        let mut seen = HashSet::new();
        let mut queue = vec![*tag_id];
        while let Some(curr) = queue.pop() {
            if seen.contains(&curr) {
                return Err(anyhow::Error::from(AppError::FieldTreeLoop {
                    field_id: curr,
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
