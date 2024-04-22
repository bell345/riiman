use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use rand::random;
use tokio::task::JoinSet;
use tracing::info;

use crate::state::AppStateRef;
use crate::tasks::{ProgressSender, ProgressState, SingleImportResult, TaskError, TaskResult, TaskReturn};

async fn import_single_image(state: AppStateRef, item: tokio::fs::DirEntry) -> SingleImportResult {
    let path: Box<Path> = item.path().into();
    info!("fake loading path {}", path.display());
    tokio::time::sleep(Duration::from_millis((random::<f64>() * 5000.0) as u64)).await;
    Ok(path)
}

pub async fn import_images_recursively(state: AppStateRef, progress: ProgressSender) -> TaskReturn {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::tasks::TaskError::WasmNotImplemented;
        return Err(WasmNotImplemented);
    }

    let state_read = state.read().await;
    let curr_vault = state_read.get_current_vault().context("there is no current vault")?;
    let vault_path = curr_vault.file_path.as_ref().context("vault has no file path")?;
    let root_dir = vault_path.parent().context("getting vault directory")?;

    let mut read_dir = tokio::fs::read_dir(root_dir).await.with_context(|| format!("reading from directory {}", root_dir.display()))?;
    let mut join_set = JoinSet::new();
    while let Some(item) = read_dir.next_entry().await.with_context(|| format!("iterating in directory {}", root_dir.display()))? {
        join_set.spawn(import_single_image(state.clone(), item));
    }

    let total = join_set.len();
    let mut results = vec![];
    while let Some(res) = join_set.join_next().await {
        let task_res = res.with_context(|| format!("awaiting import within directory {}", root_dir.display()))?;
        results.push(task_res);
        progress.send(ProgressState::Determinate(results.len() as f32 / total as f32)).context("sending progress")?;
    }

    Ok(TaskResult::ImportComplete { path: root_dir.into(), results })
}
