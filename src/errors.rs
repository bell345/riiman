use crate::data::kind::KindType;
use crate::data::FieldValue;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("not yet implemented")]
    NotImplemented,
    #[error("invalid unicode")]
    InvalidUnicode,
    #[error("no current vault")]
    NoCurrentVault,
    #[error("vault has no parent")]
    VaultNoParent,
    #[error("wrong field type, expected {expected:?}, got {got:?}")]
    WrongFieldType { expected: KindType, got: FieldValue },
    #[error("wrong mime type, expected {expected:?}, got {got:?}")]
    WrongMimeType { expected: String, got: String },
    #[error("missing field definition with ID {id}")]
    MissingFieldDefinition { id: Uuid },
    #[error(
        "found infinite loop that contains \
        field ID {field_id} while recursing fields of {item_path}"
    )]
    FieldTreeLoop { item_path: String, field_id: Uuid },
}
