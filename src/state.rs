use std::ops::Deref;
use std::sync::Arc;

use dashmap::DashMap;

use crate::data::Vault;

#[derive(Default)]
pub(crate) struct AppState {
    vaults: DashMap<String, Vault>,
    current_vault_name: Option<String>,
}

impl AppState {
    pub fn load_vault(&mut self, vault: Vault) {
        let name = vault.name.clone();
        self.vaults.insert(vault.name.clone(), vault);
        self.current_vault_name = Some(name);
    }

    pub fn set_current_vault_name(&mut self, name: String) {
        self.current_vault_name = Some(name);
    }

    pub fn get_current_vault(&self) -> Option<impl Deref<Target = Vault> + '_> {
        let name = self.current_vault_name.as_ref()?;
        let vault = self.vaults.get(name)?;
        Some(vault)
    }
}

pub type AppStateRef = Arc<tokio::sync::RwLock<AppState>>;
