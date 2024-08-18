use crate::data::transform::DestinationExistingBehaviour;
use crate::data::FieldValue;
use crate::data::{FieldType, ItemId};
use std::path::PathBuf;
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
    #[error("vault with name {name} does not exist")]
    VaultDoesNotExist { name: String },
    #[error("wrong field type; expected {expected:?}, got {got:?}")]
    WrongFieldType {
        expected: FieldType,
        got: FieldValue,
    },
    #[error("wrong mime type; expected {expected}, got {got}")]
    WrongMimeType { expected: String, got: String },
    #[error("missing field definition with ID {id}")]
    MissingFieldDefinition { id: Uuid },
    #[error("missing item with path {path}")]
    MissingItem { path: String },
    #[error("missing file associated with item at path {abs_path}")]
    MissingFile { abs_path: PathBuf },
    #[error("missing item with ID {id:?}")]
    MissingItemId { id: ItemId },
    #[error("found infinite loop that contains field ID {field_id}")]
    FieldTreeLoop { field_id: Uuid },
    #[error("error when executing command {command}: {error}")]
    CommandError { command: String, error: String },
    #[error("missing executable; expected {expected}")]
    MissingExecutable { expected: String },
    #[error("unexpected executable; expected {expected}, got {got}")]
    UnexpectedExecutable { expected: String, got: String },
    #[error("unexpected format for JSON sidecar with path {path}, error: {error:?}")]
    UnexpectedJsonSidecar { path: String, error: Option<String> },
    #[error("missing fields width, height expected of images for item with path {path}")]
    MissingImageFields { path: String },
    #[error("the destination existing behaviour is invalid for this destination: {behaviour}")]
    InvalidDestinationExistingBehaviour {
        behaviour: DestinationExistingBehaviour,
    },
    #[error("cannot remove current vault (name: {current_vault_name})")]
    CannotRemoveCurrentVault { current_vault_name: String },
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
