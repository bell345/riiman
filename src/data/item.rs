use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::FieldValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    path: String,
    fields: HashMap<Uuid, FieldValue>,
}
