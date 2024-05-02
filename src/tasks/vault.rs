use anyhow::Context;
use std::path::Path;

use crate::data::Vault;
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskError};

pub async fn choose_and_load_vault(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.pick_file().await.ok_or(TaskError::UserCancelled)?;

    #[cfg(target_arch = "wasm32")]
    {
        let contents = fp.read().await;
        progress
            .send(ProgressState::Determinate(0.5))
            .expect("progress rx exists");
        let vault = serde_json::from_slice::<Vault>(&contents)
            .context("while reading from vault file")?
            .with_standard_defs();

        let name = vault.name.clone();
        state.write().await.load_vault(vault);

        Ok(AsyncTaskResult::VaultLoaded(name))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        load_vault_from_path(
            fp.path()
                .to_str()
                .ok_or(AppError::InvalidUnicode)?
                .to_string(),
            state,
            progress,
        )
        .await
    }
}

pub async fn load_vault_from_path(
    path: String,
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    progress.send(ProgressState::Determinate(0.5));

    let contents = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("while reading from vault file at {}", path))?;

    let vault = serde_json::from_str::<Vault>(contents.as_str())
        .with_context(|| format!("while deserialising vault file at {}", path))?
        .with_file_path(Path::new(&path))
        .with_standard_defs();

    let name = vault.name.clone();
    state.write().await.load_vault(vault);

    Ok(AsyncTaskResult::VaultLoaded(name))
}

pub async fn save_vault(vault: &Vault, progress: ProgressSenderRef) -> AsyncTaskReturn {
    let data = serde_json::to_vec(vault)?;

    progress.send(ProgressState::Determinate(0.5));

    match &vault.file_path {
        Some(path) => tokio::fs::write(path, data).await?,
        None => {
            let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

            if let Some(fp) = dialog.save_file().await {
                #[cfg(target_arch = "wasm32")]
                {
                    fp.write(&data).await?;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let path = fp.path();

                    tokio::fs::write(path, data).await.with_context(|| {
                        format!("while writing to vault file at {}", path.display())
                    })?;
                }
            }
        }
    }

    Ok(AsyncTaskResult::VaultSaved(vault.name.clone()))
}

pub async fn save_new_vault(
    state: AppStateRef,
    mut vault: Vault,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.save_file().await.ok_or(TaskError::UserCancelled)?;
    let path = fp.path();
    vault.set_file_path(path);

    save_vault(&vault, progress).await?;

    let name = vault.name.clone();
    state.write().await.load_vault(vault);

    Ok(AsyncTaskResult::VaultLoaded(name))
}

pub async fn save_current_vault(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    save_vault(
        &*state
            .read()
            .await
            .get_current_vault()
            .ok_or(AppError::NoCurrentVault)?,
        progress,
    )
    .await
}
