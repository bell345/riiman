use std::fs::Metadata;

use crate::data::ThumbnailParams;
use crate::errors::AppError;
use crate::tasks::image::{read_and_resize, read_image, wand_to_image};
use crate::tasks::transform::transform_wand;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};
use anyhow::Context;
use chrono::{DateTime, Utc};
use magick_rust::MagickWand;
use tokio::task::block_in_place;

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

fn load_image_thumbnail_from_file(params: &ThumbnailParams) -> anyhow::Result<MagickWand> {
    let (mut wand, full_size) = read_and_resize(&params.abs_path, params.height)?;
    if let Some(tf_params) = params.transform_params.as_ref() {
        transform_wand(&mut wand, tf_params, Some(full_size))?;
    }
    Ok(wand)
}

pub async fn load_image_thumbnail_with_fs(
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
                params.abs_path.display()
            )
        })?;

    let wand = match tokio::fs::metadata(&hash_file).await {
        Ok(meta) if !thumbnail_needs_updating(&params, &meta) => {
            block_in_place(|| read_image(&hash_file))?
        }
        _ => block_in_place(|| -> anyhow::Result<MagickWand> {
            let wand = load_image_thumbnail_from_file(&params)?;
            wand.write_image(hash_file_str).with_context(|| {
                format!(
                    "while writing thumbnail at {} for {}",
                    hash_file_str,
                    params.abs_path.display()
                )
            })?;
            Ok(wand)
        })?,
    };

    let image = block_in_place(|| wand_to_image(&wand))?;
    Ok(AsyncTaskResult::ThumbnailLoaded { image, params })
}

pub async fn load_image_thumbnail(
    params: ThumbnailParams,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(TaskError::WasmNotImplemented);
    }

    progress.send(ProgressState::Indeterminate);

    block_in_place(|| {
        let wand = load_image_thumbnail_from_file(&params)?;
        let image = wand_to_image(&wand)?;
        Ok(AsyncTaskResult::ThumbnailLoaded { image, params })
    })
}

pub async fn commit_thumbnail_to_fs(params: &ThumbnailParams) -> AsyncTaskReturn {
    let hash_file = std::env::temp_dir().join(params.hash_path());
    let hash_file_str = hash_file.to_str().ok_or(AppError::InvalidUnicode)?;
    tokio::fs::create_dir_all(hash_file.parent().unwrap()).await?;

    block_in_place(|| {
        let wand = load_image_thumbnail_from_file(params)?;
        wand.write_image(hash_file_str).with_context(|| {
            format!(
                "while committing thumbnail for {}",
                params.abs_path.display()
            )
        })?;

        Ok(AsyncTaskResult::None)
    })
}
