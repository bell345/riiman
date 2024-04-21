use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct FieldDefinition {
    id: Uuid,
    name: Box<str>,
    field_type: FieldType,
    parents: Vec<Uuid>,
    children: Vec<Uuid>,
    fields: Vec<FieldPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldPair(Uuid, FieldValue);

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum FieldType {
    Tag,
    Boolean,
    Number,
    String,
    ItemRef,
    Array,
    Dictionary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldValue {
    Tag,
    Boolean(bool),
    Number(f64),
    String(String),
    ItemRef(String),
    Array(u64),
    Dictionary,
}
