use crate::data::field_refs::FieldDefValueRef;
use crate::data::{
    kind, FieldDefinition, FieldLike, FieldValue, KnownField, TagLike, Utf32CachedString, Vault,
};
use crate::errors::AppError;
use crate::fields;
use anyhow::Context;
use dashmap::iter::Iter;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use eframe::egui;
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::ops::Deref;
use uuid::Uuid;

pub trait FieldStore: Debug {
    fn fields(&self) -> &DashMap<Uuid, FieldValue>;

    fn get_known_field_value<V, T: FieldLike<V>>(
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

    fn get_or_insert_known_field_value<V: Debug, T: FieldLike<V>>(
        &self,
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

    fn set_known_field_value<V: Debug, T: FieldLike<V>>(&self, field: KnownField<T>, value: V) {
        *self
            .fields()
            .entry(field.id)
            .or_insert_with(|| <T as Default>::default().into()) = T::from(value).into();
    }

    fn insert_value_into_list(
        &self,
        field: KnownField<kind::List>,
        value: FieldValue,
    ) -> anyhow::Result<()> {
        let mut list = self.get_or_insert_known_field_value(field.clone(), vec![])?;
        if !list.contains(&value) {
            list.push(value);
        }
        self.set_known_field_value(field, list);

        Ok(())
    }

    fn remove_value_from_list(
        &self,
        field: KnownField<kind::List>,
        value: &FieldValue,
    ) -> anyhow::Result<()> {
        if let Some(mut list) = self.get_known_field_value(field.clone())? {
            while let Some((pos, _)) = list.iter().find_position(|i| *i == value) {
                list.swap_remove(pos);
            }
            self.set_known_field_value(field, list);
        }

        Ok(())
    }

    fn list_contains(
        &self,
        field: KnownField<kind::List>,
        value: &FieldValue,
    ) -> anyhow::Result<bool> {
        if let Some(list) = self.get_known_field_value(field)? {
            Ok(list.contains(value))
        } else {
            Ok(false)
        }
    }

    fn has_field(&self, field_id: &Uuid) -> bool {
        self.fields().contains_key(field_id)
    }

    fn remove_field(&self, field_id: &Uuid) -> Option<(Uuid, FieldValue)> {
        self.fields().remove(field_id)
    }

    fn get_field_value(&self, field_id: &Uuid) -> Option<Ref<'_, Uuid, FieldValue>> {
        self.fields().get(field_id)
    }

    fn set_field_value(&self, field_id: Uuid, value: FieldValue) {
        self.fields().insert(field_id, value);
    }

    fn get_field_value_typed<V, T: FieldLike<V>>(
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
                .with_context(|| format!("while retrieving field with ID {field_id}")),
            None => Ok(None),
        }
    }

    fn get_field_value_as_str(
        &self,
        field_id: &Uuid,
    ) -> Option<impl Deref<Target = Utf32CachedString>> {
        self.get_field_value(field_id)?
            .try_map(|v| v.as_string_opt())
            .ok()
    }

    fn get_field_with_def<'a, 'b: 'a>(
        &'a self,
        field_id: &Uuid,
        vault: &'b Vault,
    ) -> Option<FieldDefValueRef<Ref<'b, Uuid, FieldDefinition>, Ref<'a, Uuid, FieldValue>>> {
        Some(FieldDefValueRef::new(
            vault.get_definition(field_id)?,
            self.get_field_value(field_id)?,
        ))
    }

    fn iter_fields(&self) -> Iter<'_, Uuid, FieldValue> {
        self.fields().iter()
    }

    fn iter_fields_with_defs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
    ) -> impl Iterator<Item = FieldDefValueRef<Ref<'a, Uuid, FieldDefinition>, Ref<'a, Uuid, FieldValue>>>
    {
        self.iter_fields()
            .map(|f| *f.key())
            .filter_map(|id| self.get_field_with_def(&id, vault))
    }

    fn cloned_fields_with_defs(&self, vault: &Vault) -> Vec<(FieldDefinition, FieldValue)> {
        self.iter_fields_with_defs(vault)
            .map(|r| (r.definition().clone(), r.value().clone()))
            .collect()
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

    fn clear(&self) {
        let field_ids: Vec<_> = self.iter_fields().map(|f| *f.key()).collect();
        for id in field_ids {
            self.remove_field(&id);
        }
    }

    fn update(&self, src: &impl FieldStore) {
        for field in src.iter_fields() {
            self.set_field_value(*field.key(), field.value().clone());
        }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Default, Clone)]
pub struct SimpleFieldStore {
    fields: DashMap<Uuid, FieldValue>,
}

impl FieldStore for SimpleFieldStore {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}
