use std::fs::Metadata;
use std::future::Future;
use std::path::{Path, PathBuf};

use crate::data::FieldStore;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use magick_rust::MagickWand;
use tokio::fs::DirEntry;
use tokio::task::JoinSet;
use tracing::info;

use crate::errors::AppError;
use crate::fields;
use crate::state::AppStateRef;
use crate::tasks::image::{commit_thumbnail_to_fs, ThumbnailParams};
use crate::tasks::vault::save_vault;
use crate::tasks::{
    AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, SingleImportResult,
};

const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;

async fn import_single_image(
    state: AppStateRef,
    path: Box<Path>,
    last_modified: DateTime<Utc>,
) -> SingleImportResult {
    let mut item = state
        .read()
        .await
        .current_vault()?
        .get_cloned_item_or_default(&path)?;

    let mime_type = item
        .get_known_field_value(fields::general::MEDIA_TYPE)?
        .unwrap_or_else(|| {
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string()
                .into()
        });
    if !mime_type.starts_with("image/") {
        state.read().await.current_vault()?.remove_item(&path)?;

        return Err(AppError::WrongMimeType {
            expected: "image/*".to_string(),
            got: mime_type.to_string(),
        }
        .into());
    }
    item.set_known_field_value(fields::general::MEDIA_TYPE, mime_type);

    if let Some(item_modified) = item.get_known_field_value(fields::general::LAST_MODIFIED)? {
        if last_modified <= item_modified {
            return Ok(path);
        }
    }

    item.set_known_field_value(fields::general::LAST_MODIFIED, last_modified);

    commit_thumbnail_to_fs(
        state.clone(),
        &ThumbnailParams {
            path: path.clone(),
            last_modified: Some(last_modified),
            height: THUMBNAIL_LOW_QUALITY_HEIGHT,
        },
    )
    .await
    .map_err(|e| anyhow!(e))?;

    {
        let wand = MagickWand::new();
        wand.ping_image(path.to_str().ok_or(AppError::InvalidUnicode)?)
            .with_context(|| format!("while reading image metadata of {}", path.display()))?;

        let width = wand.get_image_width() as u64;
        let height = wand.get_image_height() as u64;
        item.set_known_field_value(fields::image::HEIGHT, height);
        item.set_known_field_value(fields::image::WIDTH, width);
    }

    {
        let r = state.read().await;
        let vault = r.current_vault()?;
        vault.update_item(&path, item)?;
    }

    Ok(path)
}

pub async fn select_and_import_one(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog =
        rfd::AsyncFileDialog::new().add_filter("Image file", &["jpeg", "jpg", "png", "gif"]);

    progress.send(ProgressState::Indeterminate);
    let Some(fp) = dialog.pick_file().await else {
        return Ok(AsyncTaskResult::None);
    };

    #[cfg(target_arch = "wasm32")]
    {
        use crate::tasks::TaskError::WasmNotImplemented;
        return Err(WasmNotImplemented);
    }

    let path = fp.path();
    let last_modified = fp
        .path()
        .metadata()
        .and_then(|m| m.modified())
        .map(|m| m.into())
        .unwrap_or(Utc::now());

    progress.send(ProgressState::DeterminateWithMessage(
        0.0,
        path.display().to_string(),
    ));
    let res = import_single_image(state.clone(), path.into(), last_modified).await;

    Ok(AsyncTaskResult::ImportComplete {
        path: path.into(),
        results: vec![res],
    })
}

pub async fn scan_recursively<T>(
    root_dir: &Path,
    progress: ProgressSenderRef,
    process_file: impl Fn(DirEntry, Metadata) -> Option<T>,
) -> anyhow::Result<Vec<T>> {
    progress.send(ProgressState::Indeterminate);

    let mut entries: Vec<_> = vec![];
    let mut dir_queue: Vec<PathBuf> = vec![root_dir.to_owned()];

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
                if let Some(x) = process_file(item, metadata) {
                    entries.push(x);
                }
            }
        }
    }

    entries.reverse();
    let total = entries.len();

    progress.send(ProgressState::Completed);
    info!("completed scan: {} items", total);

    Ok(entries)
}

pub async fn process_many<
    EntryT,
    ResultT: Send + 'static,
    FutureT: Future<Output = ResultT> + Send + 'static,
>(
    mut entries: Vec<EntryT>,
    progress: ProgressSenderRef,
    task_factory: impl Fn(EntryT) -> FutureT,
    on_result: impl Fn(&ResultT, &ProgressSenderRef, f32),
    concurrency_limit: usize,
) -> anyhow::Result<Vec<ResultT>> {
    let total = entries.len();
    let mut join_set = JoinSet::new();

    while join_set.len() < concurrency_limit {
        if let Some(entry) = entries.pop() {
            join_set.spawn(task_factory(entry));
        } else {
            break;
        }
    }

    let mut results = vec![];
    while let Some(res) = join_set.join_next().await {
        let task_res = res?;

        let p = results.len() as f32 / total as f32;
        on_result(&task_res, &progress, p);

        results.push(task_res);

        if let Some(entry) = entries.pop() {
            join_set.spawn(task_factory(entry));
        }
    }

    Ok(results)
}

pub fn on_import_result_send_progress(
    result: &SingleImportResult,
    progress: &ProgressSenderRef,
    p: f32,
) {
    if let Ok(path) = result {
        let msg = path.to_str().unwrap_or("").to_string();
        progress.send(ProgressState::DeterminateWithMessage(p, msg));
    }
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

    let root_dir = state.read().await.current_vault()?.root_dir()?;

    let entries = scan_recursively(
        root_dir.as_path(),
        progress.sub_task("Scan", 0.05),
        |item, metadata| {
            Some((
                item.path().into_boxed_path(),
                metadata
                    .modified()
                    .map(|m| -> DateTime<Utc> { m.into() })
                    .unwrap_or(Utc::now()),
            ))
        },
    )
    .await?;

    const CONCURRENT_TASKS_LIMIT: usize = 16;

    let results = process_many(
        entries,
        progress.sub_task("Import", 0.90),
        |(path, last_modified)| import_single_image(state.clone(), path, last_modified),
        on_import_result_send_progress,
        CONCURRENT_TASKS_LIMIT,
    )
    .await?;

    {
        let r = state.read().await;
        let curr_vault = r.current_vault()?;
        save_vault(curr_vault, progress.sub_task("Save", 0.05)).await?;
    }

    Ok(AsyncTaskResult::ImportComplete {
        path: root_dir.into(),
        results,
    })
}
