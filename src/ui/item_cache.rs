use crate::data::{FilterExpression, Item, ItemId, Vault};
use crate::state::AppStateRef;
use crate::tasks::sort::{get_filtered_and_sorted_items, SortExpression};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemCacheParams {
    pub(crate) vault_name: String,
    pub(crate) last_updated: DateTime<Utc>,
    pub(crate) sorts: Vec<SortExpression>,
    pub(crate) filter: FilterExpression,
}

#[derive(Default)]
pub struct ItemCache {
    item_ids: Vec<ItemId>,
    pub(crate) params: ItemCacheParams,
}

impl ItemCache {
    fn new_params_opt(&self, state: AppStateRef, vault: &Vault) -> Option<ItemCacheParams> {
        let make_params = || ItemCacheParams {
            vault_name: vault.name.to_string(),
            last_updated: vault.last_updated(),
            filter: state.filter().clone(),
            sorts: state.sorts().clone(),
        };

        if self.params.vault_name != vault.name {
            return Some(make_params());
        }
        if self.params.last_updated != vault.last_updated() {
            return Some(make_params());
        }
        if self.params.filter != *state.filter() {
            return Some(make_params());
        }
        if self.params.sorts != *state.sorts() {
            return Some(make_params());
        }

        None
    }

    pub fn update(&mut self, state: AppStateRef) -> Option<bool> {
        let vault = state.current_vault_opt()?;

        let Some(params) = self.new_params_opt(state.clone(), &vault) else {
            return Some(false);
        };

        // TODO: handle errors sanely and properly
        let items = get_filtered_and_sorted_items(&vault, &state.filter(), &state.sorts()).ok()?;
        self.params = params;
        self.item_ids = items
            .iter()
            .map(|item| ItemId::from_item(&vault, item))
            .collect();

        Some(true)
    }

    pub fn resolve_all_refs(&self, vault: &Vault) -> Vec<Arc<Item>> {
        self.item_ids
            .iter()
            .filter_map(|id| vault.get_item_opt_by_id(*id))
            .collect()
    }

    pub fn item_id_set(&self) -> HashSet<ItemId> {
        self.item_ids.iter().copied().collect()
    }

    pub fn len_items(&self) -> usize {
        self.item_ids.len()
    }
}
