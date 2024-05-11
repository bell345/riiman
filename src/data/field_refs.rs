use crate::data::{FieldDefinition, FieldValue, Vault};
use std::ops::Deref;
use uuid::Uuid;

pub struct FieldDefValueRef<'a> {
    inner: dashmap::mapref::multiple::RefMulti<'a, Uuid, FieldValue>,
    definition: dashmap::mapref::one::Ref<'a, Uuid, FieldDefinition>,
}

impl<'a> FieldDefValueRef<'a> {
    pub fn try_new<'b: 'a>(
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
