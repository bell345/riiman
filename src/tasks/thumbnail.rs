use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use magick_rust::MagickWand;
use sha2::{Digest, Sha256};
use tokio::task::block_in_place;

use crate::data::Vault;
use crate::errors::AppError;
use crate::tasks::image::{read_and_resize, read_image, wand_to_image};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailParams {
    pub path: Box<Path>,
    pub last_modified: Option<DateTime<Utc>>,
    pub height: usize,
}

impl ThumbnailParams {
    pub fn tex_name(&self) -> String {
        format!("{}_{}", self.path.display(), self.height)
    }

    pub fn hash_path(&self) -> Box<Path> {
        let id = format!(
            "{}_{}_{}",
            self.path.display(),
            self.height,
            self.last_modified
                .map_or(String::new(), |dt| dt.to_rfc3339())
        );
        let h = base16ct::lower::encode_string(Sha256::digest(id).as_slice());
        // 6f12a101d9[...] -> riiman/6f/12a101d9[...].jpg
        let root_folder = "riiman";
        let folder = &h[..2];
        let file = &format!("{}.jpg", &h[2..]);
        let buf: PathBuf = [root_folder, folder, file].iter().collect();
        buf.into()
    }
}

fn thumbnail_needs_updating(params: &ThumbnailParams, meta: &Metadata) -> bool {
    if !meta.is_file() {
        return true;
    }

    let Some(source_modified) = params.last_modified else {
        return true;
    };

    let Ok(thumb_modified): Result<DateTime<Utc>, _> = meta.modified().map(|st| st.into()) else {
        return true;
    };

    source_modified > thumb_modified
}

pub async fn load_image_thumbnail_with_fs(
    vault: Arc<Vault>,
    params: ThumbnailParams,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    progress.send(ProgressState::Indeterminate);

    let hash_file = std::env::temp_dir().join(params.hash_path());
    let hash_file_str = hash_file.to_str().ok_or(AppError::InvalidUnicode)?;
    tokio::fs::create_dir_all(hash_file.parent().unwrap())
        .await
        .with_context(|| {
            format!(
                "while ensuring directory for thumbnail at {} for {}",
                hash_file_str,
                params.path.display()
            )
        })?;

    let wand = match tokio::fs::metadata(&hash_file).await {
        Ok(meta) if !thumbnail_needs_updating(&params, &meta) => {
            block_in_place(|| read_image(&hash_file))?
        }
        _ => block_in_place(|| -> anyhow::Result<MagickWand> {
            let abs_path = vault.resolve_abs_path(&params.path)?;
            let wand = read_and_resize(abs_path.as_str(), params.height)?;
            wand.write_image(hash_file_str).with_context(|| {
                format!(
                    "while writing thumbnail at {} for {}",
                    hash_file_str,
                    params.path.display()
                )
            })?;
            Ok(wand)
        })?,
    };

    let image = block_in_place(|| wand_to_image(&wand))?;
    Ok(AsyncTaskResult::ThumbnailLoaded { image, params })
}

pub async fn load_image_thumbnail(
    vault: Arc<Vault>,
    params: ThumbnailParams,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    progress.send(ProgressState::Indeterminate);

    block_in_place(|| {
        let abs_path = vault.resolve_abs_path(&params.path)?;
        let wand = read_and_resize(abs_path.as_str(), params.height)?;
        let image = wand_to_image(&wand)?;
        Ok(AsyncTaskResult::ThumbnailLoaded { image, params })
    })
}

pub async fn commit_thumbnail_to_fs(vault: &Vault, params: &ThumbnailParams) -> AsyncTaskReturn {
    let hash_file = std::env::temp_dir().join(params.hash_path());
    let hash_file_str = hash_file.to_str().ok_or(AppError::InvalidUnicode)?;
    tokio::fs::create_dir_all(hash_file.parent().unwrap()).await?;

    block_in_place(|| {
        let abs_path = vault.resolve_abs_path(&params.path)?;
        let wand = read_and_resize(abs_path.as_str(), params.height)?;
        wand.write_image(hash_file_str)
            .with_context(|| format!("while committing thumbnail for {}", params.path.display()))?;

        Ok(AsyncTaskResult::None)
    })
}
