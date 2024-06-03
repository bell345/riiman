use crate::data::{Item, Vault};
use crate::state::AppStateRef;
use crate::tasks::filter::FilterExpression;
use crate::tasks::sort::{get_filtered_and_sorted_items, SortExpression};
use chrono::{DateTime, Utc};
use dashmap::mapref::one::Ref;
use std::collections::HashSet;
use std::path::Path;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemCacheParams {
    pub(crate) vault_name: String,
    pub(crate) last_updated: DateTime<Utc>,
    pub(crate) sorts: Vec<SortExpression>,
    pub(crate) filter: FilterExpression,
}

#[derive(Default)]
pub struct ItemCache {
    item_paths: Vec<String>,
    pub(crate) params: ItemCacheParams,
}

impl ItemCache {
    fn new_params_opt(&self, state: AppStateRef, vault: &Vault) -> Option<ItemCacheParams> {
        let r = state.blocking_read();

        let make_params = || ItemCacheParams {
            vault_name: vault.name.to_string(),
            last_updated: vault.last_updated(),
            filter: r.filter().clone(),
            sorts: r.sorts().clone(),
        };

        if self.params.vault_name != vault.name {
            return Some(make_params());
        }
        if self.params.last_updated != vault.last_updated() {
            return Some(make_params());
        }
        if self.params.filter != *r.filter() {
            return Some(make_params());
        }
        if self.params.sorts != *r.sorts() {
            return Some(make_params());
        }

        None
    }

    pub fn update(&mut self, state: AppStateRef) -> anyhow::Result<(bool, bool)> {
        let r = state.blocking_read();
        let current_vault = r.current_vault()?;

        let Some(params) = self.new_params_opt(state.clone(), &current_vault) else {
            return Ok((false, false));
        };
        let vault_is_new = self.params.vault_name == params.vault_name;

        let items = get_filtered_and_sorted_items(&current_vault, &r.filter(), &r.sorts())?;
        self.params = params;
        self.item_paths = items.iter().map(|i| i.path().to_string()).collect();

        Ok((true, vault_is_new))
    }

    pub fn resolve_all_refs<'a, 'b: 'a>(&'a self, vault: &'b Vault) -> Vec<Ref<String, Item>> {
        self.item_paths
            .iter()
            .filter_map(|p| vault.get_item_opt(Path::new(p)).expect("valid path"))
            .collect()
    }

    pub fn resolve_refs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
        paths: Vec<&String>,
    ) -> Vec<Ref<String, Item>> {
        let existing_items = self.item_path_set();
        paths
            .into_iter()
            .filter(|p| existing_items.contains(*p))
            .filter_map(|p| vault.get_item_opt(Path::new(p)).ok().flatten())
            .collect()
    }

    pub fn item_path_set(&self) -> HashSet<&String> {
        self.item_paths.iter().collect()
    }

    pub fn len_items(&self) -> usize {
        self.item_paths.len()
    }
}
