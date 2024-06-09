use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use tokio::task::block_in_place;

use crate::data::Vault;
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};

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
        .with_standard_defs();

    let name = vault.name.clone();
    state.write().await.load_vault(vault);

    Ok(AsyncTaskResult::VaultLoaded {
        name,
        set_as_current,
    })
}

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

pub async fn save_new_vault(
    state: AppStateRef,
    mut vault: Vault,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.save_file().await.ok_or(AppError::UserCancelled)?;
    let path = fp.path();
    vault.set_file_path(path);

    let vault = Arc::new(vault);
    save_vault(vault.clone(), progress).await?;

    let name = vault.name.clone();
    state
        .write()
        .await
        .load_vault(Arc::into_inner(vault).expect("No other vault references"));

    Ok(AsyncTaskResult::VaultLoaded {
        name,
        set_as_current: false,
    })
}

pub async fn save_current_vault(
    state: AppStateRef,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let r = state.read().await;
    let vault = r.current_vault()?;
    save_vault(vault, progress).await
}

pub async fn save_vault_by_name(
    state: AppStateRef,
    progress: ProgressSenderRef,
    name: String,
) -> AsyncTaskReturn {
    let r = state.read().await;
    let vault = r.get_vault(&name)?;
    save_vault(vault, progress).await
}
