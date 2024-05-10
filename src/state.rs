use std::ops::Deref;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use poll_promise::Promise;

use crate::data::Vault;
use crate::errors::AppError;
use crate::fields;
use crate::tasks::sort::{FilterExpression, SortDirection, SortExpression};
use crate::tasks::{AsyncTaskReturn, ProgressSenderRef, TaskFactory};

pub(crate) struct AppState {
    task_queue: Mutex<Vec<(String, TaskFactory)>>,
    error_queue: Mutex<Vec<anyhow::Error>>,
    vaults: DashMap<String, Vault>,
    pub current_vault_name: Option<String>,
    pub vault_loading: bool,

    pub filter: FilterExpression,
    pub sorts: Vec<SortExpression>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            task_queue: Default::default(),
            error_queue: Default::default(),
            vaults: Default::default(),
            current_vault_name: None,
            vault_loading: false,
            filter: FilterExpression::TagMatch(fields::image::NAMESPACE.id),
            sorts: vec![SortExpression::Path(SortDirection::Ascending)],
        }
    }
}

impl AppState {
    pub fn load_vault(&mut self, vault: Vault) {
        let name = vault.name.clone();
        self.vaults.insert(vault.name.clone(), vault);
        self.current_vault_name = Some(name);
    }

    pub fn current_vault_opt(&self) -> Option<impl Deref<Target = Vault> + '_> {
        let name = self.current_vault_name.as_ref()?;
        let vault = self.vaults.get(name)?;
        Some(vault)
    }

    pub fn current_vault(&self) -> Result<impl Deref<Target = Vault> + '_, AppError> {
        self.current_vault_opt().ok_or(AppError::NoCurrentVault)
    }

    pub fn add_task(
        &self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        let mut l = self.task_queue.lock().unwrap();
        l.push((name, Box::new(task_factory)));
    }

    pub fn catch<T, E: Into<anyhow::Error>>(
        &self,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, ()> {
        match f() {
            Ok(r) => Ok(r),
            Err(e) => {
                let mut queue = self.error_queue.lock().unwrap();
                queue.push(e.into());
                Err(())
            }
        }
    }

    pub fn drain_tasks(&mut self, n: usize) -> Vec<(String, TaskFactory)> {
        let mut l = self.task_queue.lock().unwrap();
        let mut v: Vec<(String, TaskFactory)> = vec![];
        for _ in 0..n {
            if let Some(x) = l.pop() {
                v.push(x);
            } else {
                break;
            }
        }

        v
    }
}

pub type AppStateRef = Arc<tokio::sync::RwLock<AppState>>;
