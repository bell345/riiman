use crate::data::{FilterExpression, Item, ItemId, Vault};
use crate::tasks::sort::{get_filtered_and_sorted_items, SortExpression};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemCacheParams {
    pub(crate) vault_name: String,
    pub(crate) last_updated: DateTime<Utc>,
    pub(crate) sorts: Vec<SortExpression>,
    pub(crate) filter: FilterExpression,
}

#[derive(Default)]
pub struct ItemCache {
    item_ids: RwLock<Vec<ItemId>>,
    pub(crate) params: Mutex<ItemCacheParams>,
}

impl ItemCache {
    fn new_params_opt(
        &self,
        vault: &Vault,
        filter: &FilterExpression,
        sorts: &[SortExpression],
    ) -> Option<ItemCacheParams> {
        let make_params = || ItemCacheParams {
            vault_name: vault.name.to_string(),
            last_updated: vault.last_updated(),
            filter: filter.to_owned(),
            sorts: sorts.to_owned(),
        };

        let params = self.params.lock().unwrap();

        if params.vault_name != vault.name {
            return Some(make_params());
        }
        if params.last_updated != vault.last_updated() {
            return Some(make_params());
        }
        if params.filter != *filter {
            return Some(make_params());
        }
        if params.sorts != *sorts {
            return Some(make_params());
        }

        None
    }

    pub fn update(
        &self,
        vault: &Vault,
        filter: &FilterExpression,
        sorts: &[SortExpression],
    ) -> anyhow::Result<bool> {
        let Some(params) = self.new_params_opt(vault, filter, sorts) else {
            return Ok(false);
        };

        // TODO: handle errors sanely and properly
        let items = get_filtered_and_sorted_items(&vault, filter, sorts)?;
        *self.params.lock().unwrap() = params;
        *self.item_ids.write().unwrap() = items
            .iter()
            .map(|item| ItemId::from_item(&vault, item))
            .collect();

        Ok(true)
    }

    pub fn item_ids(&self) -> Vec<ItemId> {
        self.item_ids.read().unwrap().iter().copied().collect()
    }

    pub fn len_items(&self) -> usize {
        self.item_ids.read().unwrap().len()
    }
}
