use crate::data::kind::KindType;
use crate::data::FieldValue;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("user cancelled")]
    UserCancelled,
    #[error("not yet implemented")]
    NotImplemented,
    #[error("invalid unicode")]
    InvalidUnicode,
    #[error("no current vault")]
    NoCurrentVault,
    #[error("vault has no parent")]
    VaultNoParent,
    #[error("vault has no file path")]
    VaultNoPath,
    #[error("wrong field type, expected {expected:?}, got {got:?}")]
    WrongFieldType { expected: KindType, got: FieldValue },
    #[error("wrong mime type, expected {expected:?}, got {got:?}")]
    WrongMimeType { expected: String, got: String },
    #[error("missing field definition with ID {id}")]
    MissingFieldDefinition { id: Uuid },
    #[error("found infinite loop that contains field ID {field_id}")]
    FieldTreeLoop { field_id: Uuid },
}

impl AppError {
    pub fn is_err(&self, e: &anyhow::Error) -> bool {
        if let Some(app_e) = e.downcast_ref::<Self>() {
            app_e == self
        } else {
            false
        }
    }
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum HierarchyError {
    #[error("missing field definition with ID {id}")]
    MissingFieldDefinition { id: Uuid },
    #[error("found infinite loop that contains field ID {field_id}")]
    FieldTreeLoop { field_id: Uuid },
}
