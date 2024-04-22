use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub id: Uuid,
    pub name: String,
    pub field_type: FieldType,
    parents: Vec<Uuid>,
    children: Vec<Uuid>,
    fields: HashMap<Uuid, FieldValue>,
}

impl FieldDefinition {
    pub fn new(id: Uuid, name: String, field_type: FieldType) -> Self {
        Self {
            id,
            name,
            field_type,
            ..Default::default()
        }
    }

    pub fn new_child(id: Uuid, name: String, field_type: FieldType, parent_id: Uuid) -> Self {
        Self {
            id,
            name,
            field_type,
            parents: vec![parent_id],
            ..Default::default()
        }
    }
}

#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum FieldType {
    #[default]
    Tag,
    Boolean,
    Int,
    UInt,
    Float,
    String,
    ItemRef,
    Array,
    Dictionary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldValue {
    Tag,
    Boolean(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    ItemRef(String),
    Array(u64),
    Dictionary,
}
