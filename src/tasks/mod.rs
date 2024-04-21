use anyhow::Context;
use poll_promise::Promise;

use crate::data::Vault;

pub enum TaskResult {
    None,
    VaultLoaded(Box<Vault>),
}

pub enum TaskError {
    UserCancelled,
    Error(anyhow::Error),
}

impl<T: Into<anyhow::Error>> From<T> for TaskError {
    fn from(value: T) -> Self {
        Self::Error(value.into())
    }
}

type TaskReturn = Result<TaskResult, TaskError>;

type Task = Promise<TaskReturn>;

#[derive(Default)]
pub(crate) struct TaskState {
    running_tasks: Vec<Task>,
}

impl TaskState {
    pub fn add_task(&mut self, func: Task) {
        self.running_tasks.push(func);
    }

    pub fn iter_ready(&mut self) -> Vec<TaskReturn> {
        let mut results = vec![];
        let mut still_running_tasks = vec![];
        for task in self.running_tasks.drain(..) {
            match task.try_take() {
                Ok(result) => results.push(result),
                Err(task) => still_running_tasks.push(task)
            }
        }

        self.running_tasks = still_running_tasks;
        results
    }
}

pub async fn choose_and_load_vault() -> TaskReturn {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("riiman vault file", &["riiman"]);

    let fp = dialog.pick_file().await.ok_or(TaskError::UserCancelled)?;
    let vault: Vault;

    #[cfg(target_arch = "wasm32")]
    {
        let contents = fp.read().await;
        vault = serde_json::from_slice::<Vault>(&contents)
            .context("while reading from vault file")?;
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = fp.path();

        let contents = tokio::fs::read_to_string(path).await
            .with_context(|| format!("while reading from vault file at {}", path.display()))?;

        vault = serde_json::from_str::<Vault>(contents.as_str())
            .with_context(|| format!("while deserialising vault file at {}", path.display()))?
            .with_file_path(path)
    };

    Ok(TaskResult::VaultLoaded(vault.into()))
}

pub async fn save_vault(vault: &mut Vault) -> TaskReturn {
    let data = serde_json::to_vec(vault)?;
    match &vault.file_path {
        Some(path) => tokio::fs::write(path, data).await?,
        None => {
            let dialog = rfd::AsyncFileDialog::new()
                .add_filter("riiman vault file", &["riiman"]);

            if let Some(fp) = dialog.save_file().await {
                #[cfg(target_arch = "wasm32")]
                {
                    fp.write(&data).await?;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let path = fp.path();
                    vault.set_file_path(path);

                    tokio::fs::write(fp.path(), data).await
                        .with_context(|| format!("while writing to vault file at {}", path.display()))?;
                }
            }
        }
    }
    Ok(TaskResult::None)
}

pub async fn save_new_vault(mut vault: Vault) -> TaskReturn {
    save_vault(&mut vault).await?;
    Ok(TaskResult::VaultLoaded(vault.into()))
}
