use anyhow::Context;

use crate::data::Vault;
use crate::state::AppStateRef;
use crate::tasks::{ProgressSender, ProgressState, TaskError, TaskResult, TaskReturn};

pub async fn choose_and_load_vault(progress: ProgressSender) -> TaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.pick_file().await.ok_or(TaskError::UserCancelled)?;
    let vault: Vault;

    #[cfg(target_arch = "wasm32")]
    {
        let contents = fp.read().await;
        progress.send(ProgressState::Determinate(0.5)).expect("progress rx exists");
        vault =
            serde_json::from_slice::<Vault>(&contents).context("while reading from vault file")?;
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = fp.path();
        progress.send(ProgressState::Determinate(0.5)).expect("progress rx exists");

        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("while reading from vault file at {}", path.display()))?;

        vault = serde_json::from_str::<Vault>(contents.as_str())
            .with_context(|| format!("while deserialising vault file at {}", path.display()))?
            .with_file_path(path)
    };

    Ok(TaskResult::VaultLoaded(vault.into()))
}

pub async fn save_vault(vault: &Vault, progress: ProgressSender) -> TaskReturn {
    let data = serde_json::to_vec(vault)?;

    progress.send(ProgressState::Determinate(0.5)).expect("progress rx exists");

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
    Ok(TaskResult::None)
}

pub async fn save_new_vault(mut vault: Vault, progress: ProgressSender) -> TaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.save_file().await.ok_or(TaskError::UserCancelled)?;
    let path = fp.path();
    vault.set_file_path(path);

    save_vault(&vault, progress).await?;

    Ok(TaskResult::VaultLoaded(vault.into()))
}

pub async fn save_current_vault(state: AppStateRef, progress: ProgressSender) -> TaskReturn {
    save_vault(
        state
            .read()
            .await
            .get_current_vault()
            .context("No current vault exists")?
            .value(),
        progress
    )
    .await
}
