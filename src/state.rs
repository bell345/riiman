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

    pub fn get_current_vault(&self) -> Option<dashmap::mapref::one::Ref<String, Vault>> {
        let name = self.current_vault.as_ref()?;
        let vault = self.vaults.get(name)?;
        Some(vault)
    }
}

pub type AppStateRef = Arc<tokio::sync::RwLock<AppState>>;
