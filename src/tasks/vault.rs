use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use itertools::Itertools;
use tokio::task::block_in_place;

use crate::data::Vault;
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};

#[tracing::instrument]
pub async fn choose_and_load_vault(
    state: AppStateRef,
    progress: ProgressSenderRef,
    set_as_current: bool,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.pick_file().await.ok_or(AppError::UserCancelled)?;

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
            set_as_current,
        )
        .await
    }
}

#[tracing::instrument]
pub async fn load_vault_from_path(
    path: String,
    state: AppStateRef,
    progress: ProgressSenderRef,
    set_as_current: bool,
) -> AsyncTaskReturn {
    progress.send(ProgressState::Determinate(0.5));

    let contents = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("while reading from vault file at {path}"))?;

    let vault = serde_json::from_str::<Vault>(contents.as_str())
        .with_context(|| format!("while deserialising vault file at {path}"))?
        .with_file_path(Path::new(&path))
        .with_id_lookup()
        .with_standard_defs();

    let name = vault.name.clone();
    state.load_vault(vault, set_as_current);

    Ok(AsyncTaskResult::VaultLoaded {
        name,
        set_as_current,
    })
}

#[tracing::instrument]
pub async fn save_vault(vault: Arc<Vault>, progress: ProgressSenderRef) -> AsyncTaskReturn {
    let file_path = vault.file_path.clone();
    let name = vault.name.clone();
    let data = block_in_place(move || serde_json::to_vec(&vault))?;

    progress.send(ProgressState::Determinate(0.5));

    if let Some(path) = file_path {
        tokio::fs::write(path, data).await?;
    } else {
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

    Ok(AsyncTaskResult::VaultSaved(name))
}

#[tracing::instrument]
pub async fn save_new_vault(
    state: AppStateRef,
    name: String,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("riiman vault file", &["riiman"])
        .set_file_name(format!("{name}.riiman"));

    let fp = dialog.save_file().await.ok_or(AppError::UserCancelled)?;
    let path = fp.path();
    let mut vault = Vault::new(name);
    vault.set_file_path(path);

    let vault = Arc::new(vault);
    save_vault(vault.clone(), progress).await?;

    let name = vault.name.clone();
    state.load_vault(
        Arc::into_inner(vault).expect("No other vault references"),
        true,
    );

    Ok(AsyncTaskResult::VaultLoaded {
        name,
        set_as_current: false,
    })
}

#[tracing::instrument]
pub async fn save_current_vault(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let vault = state.current_vault()?;
    save_vault(vault, progress).await
}

#[tracing::instrument]
pub async fn save_vault_and_links(
    state: AppStateRef,
    vault: Arc<Vault>,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let linked_vault_names = vault.iter_linked_vault_names().into_iter().collect_vec();
    let n_names = linked_vault_names.len();

    let res = save_vault(vault, progress.sub_task("Save current vault", 0.5)).await?;

    let sub_task = progress.sub_task("Save linked vaults", 0.5);
    #[allow(clippy::cast_precision_loss)]
    for (i, vault_name) in linked_vault_names.into_iter().enumerate() {
        let weight = (i as f32) / (n_names as f32);
        if let Ok(linked_vault) = state.get_vault(&vault_name) {
            let task_name = format!("Save linked vault {}", linked_vault.name);
            save_vault(linked_vault, sub_task.sub_task(&task_name, weight)).await?;
        }
    }

    Ok(res)
}

#[tracing::instrument]
pub async fn save_current_and_linked_vaults(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let curr_vault = state.current_vault()?;
    save_vault_and_links(state, curr_vault, progress).await
}
