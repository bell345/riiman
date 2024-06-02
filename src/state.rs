use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dashmap::mapref::one::Ref;
use dashmap::{DashMap, DashSet};
use poll_promise::Promise;

use crate::data::{FieldStore, Vault};
use crate::errors::AppError;
use crate::fields;
use crate::tasks::filter::FilterExpression;
use crate::tasks::sort::{SortDirection, SortExpression};
use crate::tasks::{AsyncTaskReturn, ProgressSenderRef, TaskFactory};
use crate::ui::AppModal;

pub(crate) struct AppState {
    task_queue: Mutex<Vec<(String, TaskFactory, bool)>>,
    results: DashMap<String, AsyncTaskReturn>,
    error_queue: Mutex<Vec<anyhow::Error>>,
    dialog_queue: Mutex<Vec<Box<dyn AppModal>>>,
    vaults: DashMap<String, Vault>,
    unresolved_vaults: DashSet<String>,
    pub current_vault_name: Mutex<Option<String>>,
    pub vault_loading: Mutex<bool>,

    pub filter: FilterExpression,
    pub sorts: Vec<SortExpression>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            task_queue: Default::default(),
            results: Default::default(),
            error_queue: Default::default(),
            dialog_queue: Default::default(),
            vaults: Default::default(),
            unresolved_vaults: Default::default(),
            current_vault_name: Default::default(),
            vault_loading: Default::default(),
            filter: FilterExpression::TagMatch(fields::image::NAMESPACE.id),
            sorts: vec![SortExpression::Path(SortDirection::Ascending)],
        }
    }
}

impl AppState {
    pub fn load_vault(&self, vault: Vault) {
        let name = vault.name.clone();

        for item in vault.iter_items() {
            if let Ok(Some((ref_name, _))) = item.get_known_field_value(fields::general::LINK) {
                if !self.vaults.contains_key(&ref_name) && ref_name != name {
                    self.unresolved_vaults.insert(ref_name);
                }
            }
        }

        self.unresolved_vaults.remove(&name);
        self.vaults.insert(name.clone(), vault);
        self.set_current_vault_name(name)
            .expect("vault we just added should exist");
    }

    pub fn get_vault(&self, name: &String) -> Result<Ref<'_, String, Vault>, AppError> {
        self.vaults
            .get(name)
            .ok_or(AppError::VaultDoesNotExist { name: name.clone() })
    }

    pub fn current_vault_name(&self) -> Option<String> {
        self.current_vault_name.lock().unwrap().clone()
    }

    pub fn current_vault_opt(&self) -> Option<Ref<'_, String, Vault>> {
        let name = self.current_vault_name()?;
        let vault = self.vaults.get(&name)?;
        Some(vault)
    }

    pub fn current_vault(&self) -> Result<Ref<'_, String, Vault>, AppError> {
        self.current_vault_opt().ok_or(AppError::NoCurrentVault)
    }

    pub fn set_current_vault_name(&self, name: String) -> Result<(), AppError> {
        if !self.vaults.contains_key(&name) {
            return Err(AppError::VaultDoesNotExist { name });
        }

        let mut l = self.current_vault_name.lock().unwrap();
        *l = Some(name);

        Ok(())
    }

    pub fn has_unresolved_vaults(&self) -> bool {
        !self.unresolved_vaults.is_empty()
    }

    pub fn valid_vault_names(&self) -> Vec<String> {
        self.vaults.iter().map(|v| v.name.clone()).collect()
    }

    pub fn known_vault_names(&self) -> Vec<String> {
        self.vaults
            .iter()
            .map(|v| v.name.clone())
            .chain(self.unresolved_vaults.iter().map(|n| n.clone()))
            .collect()
    }

    pub fn vault_name_to_file_paths(&self) -> HashMap<String, String> {
        HashMap::from_iter(
            self.vaults
                .iter()
                .filter_map(|r| Some((r.name.clone(), r.file_path.clone()?.to_str()?.to_string()))),
        )
    }

    fn add_task_impl(
        &self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
        is_request: bool,
    ) {
        let mut l = self.task_queue.lock().unwrap();
        l.push((name, Box::new(task_factory), is_request));
    }

    pub fn add_dialog(&self, dialog: impl AppModal) {
        let mut l = self.dialog_queue.lock().unwrap();
        l.push(Box::new(dialog));
    }

    pub fn add_task(
        &self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.add_task_impl(name, task_factory, false);
    }

    pub fn add_task_request(
        &self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.add_task_impl(name, task_factory, true);
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

    pub fn drain_tasks(&mut self, n: usize) -> Vec<(String, TaskFactory, bool)> {
        let mut l = self.task_queue.lock().unwrap();
        let mut v: Vec<_> = vec![];
        for _ in 0..n {
            if let Some(x) = l.pop() {
                v.push(x);
            } else {
                break;
            }
        }

        v
    }

    pub fn drain_errors(&mut self) -> Vec<anyhow::Error> {
        self.error_queue.lock().unwrap().drain(..).collect()
    }

    pub fn drain_dialogs(&mut self) -> Vec<Box<dyn AppModal>> {
        self.dialog_queue.lock().unwrap().drain(..).collect()
    }

    pub fn try_take_request_result(&self, name: &String) -> Option<AsyncTaskReturn> {
        self.results.remove(name).map(|(_, v)| v)
    }

    pub fn push_request_results(&self, results: Vec<(String, AsyncTaskReturn)>) {
        for (name, result) in results {
            self.results.insert(name, result);
        }
    }

    pub fn vault_loading(&self) -> bool {
        *self.vault_loading.lock().unwrap()
    }

    pub fn set_vault_loading(&self) {
        *self.vault_loading.lock().unwrap() = true;
    }

    pub fn reset_vault_loading(&self) {
        *self.vault_loading.lock().unwrap() = false;
    }

    pub fn save_current_vault(&self) {
        *self.vault_loading.lock().unwrap() = true;
        self.add_task("Save vault".into(), |state, p| {
            Promise::spawn_async(crate::tasks::vault::save_current_vault(state, p))
        });
    }
}

pub type AppStateRef = Arc<tokio::sync::RwLock<AppState>>;
