use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use magick_rust::MagickWand;
use tokio::task::JoinSet;
use tracing::info;

use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;
use crate::tasks::vault::save_vault;
use crate::tasks::{
    AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, SingleImportResult,
};

async fn import_single_image(
    state: AppStateRef,
    entry: tokio::fs::DirEntry,
    metadata: std::fs::Metadata,
) -> SingleImportResult {
    let path: Box<Path> = entry.path().into();

    let state_ref = state.read().await;
    let vault = state_ref
        .get_current_vault()
        .ok_or(AppError::NoCurrentVault)?;

    let mut item_ref = vault.ensure_item_mut(&path)?;
    let item = item_ref.value_mut();

    let mime_type = item
        .get_known_field_value(fields::general::MEDIA_TYPE)?
        .unwrap_or_else(|| {
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string()
        });
    if !mime_type.starts_with("image/") {
        return Err(AppError::WrongMimeType {
            expected: "image/*".to_string(),
            got: mime_type,
        }
        .into());
    }
    item.set_known_field_value(fields::general::MEDIA_TYPE, mime_type);

    let file_modified = metadata.modified().map(|m| m.into()).unwrap_or(Utc::now());

    if let Some(item_modified) = item.get_known_field_value(fields::general::LAST_MODIFIED)? {
        if file_modified <= item_modified {
            return Ok(path);
        }
    }

    item.set_known_field_value(fields::general::LAST_MODIFIED, file_modified);

    let wand = MagickWand::new();
    wand.ping_image(path.to_str().ok_or(AppError::InvalidUnicode)?)?;

    let width = wand.get_image_width() as u64;
    let height = wand.get_image_height() as u64;
    item.set_known_field_value(fields::image::HEIGHT, height);
    item.set_known_field_value(fields::image::WIDTH, width);

    Ok(path)
}

pub async fn import_images_recursively(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::tasks::TaskError::WasmNotImplemented;
        return Err(WasmNotImplemented);
    }

    let state_read = state.read().await;
    let curr_vault = state_read
        .get_current_vault()
        .context("there is no current vault")?;
    let vault_path = curr_vault
        .file_path
        .as_ref()
        .context("vault has no file path")?;
    let root_dir = vault_path.parent().context("getting vault directory")?;

    let scan_progress = progress.sub_task("Scan", 0.05);
    scan_progress.send(ProgressState::Indeterminate);

    let mut entries: Vec<(tokio::fs::DirEntry, std::fs::Metadata)> = vec![];
    let mut dir_queue: Vec<PathBuf> = vec![root_dir.into()];

    while let Some(dir_path) = dir_queue.pop() {
        let mut read_dir = tokio::fs::read_dir(dir_path)
            .await
            .with_context(|| format!("reading from directory {}", root_dir.display()))?;

        while let Some(item) = read_dir
            .next_entry()
            .await
            .with_context(|| format!("iterating in directory {}", root_dir.display()))?
        {
            let metadata = item
                .metadata()
                .await
                .with_context(|| format!("getting metadata for file {}", item.path().display()))?;
            if metadata.is_dir() {
                dir_queue.push(item.path());
            } else if metadata.is_file() {
                entries.push((item, metadata));
            }
        }
    }

    scan_progress.send(ProgressState::Completed);
    info!("completed scan: {} items", entries.len());

    let mut join_set = JoinSet::new();
    for (entry, metadata) in entries {
        join_set.spawn(import_single_image(state.clone(), entry, metadata));
    }

    let import_progress = progress.sub_task("Import", 0.90);
    let total = join_set.len();
    let mut results = vec![];
    while let Some(res) = join_set.join_next().await {
        let task_res = res
            .with_context(|| format!("awaiting import within directory {}", root_dir.display()))?;

        let p = results.len() as f32 / total as f32;
        if let Ok(path) = &task_res {
            let msg = path.to_str().unwrap_or("").to_string();
            info!("sending msg: {msg}");
            import_progress.send(ProgressState::DeterminateWithMessage(p, msg));
        } else {
            info!("unknown result: {task_res:?}");
            import_progress.send(ProgressState::Determinate(p));
        }

        results.push(task_res);
    }

    save_vault(&curr_vault, progress.sub_task("Save", 0.05)).await?;

    Ok(AsyncTaskResult::ImportComplete {
        path: root_dir.into(),
        results,
    })
}
