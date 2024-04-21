use serde::{Deserialize, Serialize};

use super::field::FieldPair;

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    path: Box<str>,
    fields: Vec<FieldPair>,
}
