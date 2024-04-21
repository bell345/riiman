use std::sync::Arc;

use dashmap::DashMap;

use crate::data::Vault;

#[derive(Default)]
pub(crate) struct AppState {
    vaults: Arc<DashMap<String, Vault>>,
    pub current_vault: Option<String>,
}

impl AppState {
    pub fn load_vault(&mut self, vault: Vault) {
        let name = vault.name.clone();
        self.vaults.insert(vault.name.clone(), vault);
        self.current_vault = Some(name);
    }
}
