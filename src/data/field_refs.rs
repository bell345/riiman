use crate::data::{FieldDefinition, FieldValue, Vault};
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use std::ops::Deref;
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
