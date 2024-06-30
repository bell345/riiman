use crate::data::field::Definition;
use crate::data::{FieldDefinition, FieldValue, Vault};
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use std::borrow::Borrow;
use std::ops::Deref;
use std::sync::OnceLock;
use uuid::Uuid;

pub struct FieldDefValueRef<Def: Deref<Target = FieldDefinition>, Value: Deref<Target = FieldValue>>
{
    definition: Def,
    value: Value,
}

impl<'a, Def: Deref<Target = FieldDefinition> + 'a, Value: Deref<Target = FieldValue> + 'a>
    FieldDefValueRef<Def, Value>
{
    pub fn new(definition: Def, value: Value) -> Self {
        Self { definition, value }
    }

    pub fn definition(&self) -> &FieldDefinition {
        &self.definition
    }

    pub fn value(&self) -> &FieldValue {
        &self.value
    }
}

static DEFAULT_DEFINITION: OnceLock<Definition> = OnceLock::new();

pub enum FieldDefRefOrPlaceholder<Def: Deref<Target = FieldDefinition>> {
    Filled(Def),
    Vacant,
}

impl<Def: Deref<Target = FieldDefinition>> Deref for FieldDefRefOrPlaceholder<Def> {
    type Target = FieldDefinition;

    fn deref(&self) -> &Self::Target {
        match self {
            FieldDefRefOrPlaceholder::Filled(def) => def,
            FieldDefRefOrPlaceholder::Vacant => {
                DEFAULT_DEFINITION.get_or_init(|| FieldDefinition::tag(Uuid::nil(), "???".into()))
            }
        }
    }
}

impl<Def: Deref<Target = FieldDefinition>> From<Option<Def>> for FieldDefRefOrPlaceholder<Def> {
    fn from(value: Option<Def>) -> Self {
        match value {
            Some(def) => Self::Filled(def),
            None => Self::Vacant,
        }
    }
}
