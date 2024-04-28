use crate::data::kind::KindType;
use crate::data::FieldValue;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
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
}
