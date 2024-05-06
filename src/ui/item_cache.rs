use crate::data::{Item, Vault};
use crate::tasks::sort::{FilterExpression, SortExpression};
use std::ops::Deref;
use std::path::Path;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemCacheParams {
    pub(crate) vault_name: String,
    pub(crate) sorts: Vec<SortExpression>,
    pub(crate) filter: FilterExpression,
}

#[derive(Default)]
pub struct ItemCache {
    item_paths: Vec<String>,
    pub(crate) params: ItemCacheParams,
}

impl ItemCache {
    pub fn from_items(params: ItemCacheParams, items: &[impl Deref<Target = Item>]) -> Self {
        Self {
            params,
            item_paths: items.iter().map(|i| i.path().to_string()).collect(),
        }
    }

    pub fn resolve_refs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
    ) -> Vec<impl Deref<Target = Item> + 'a> {
        self.item_paths
            .iter()
            .filter_map(|p| vault.get_item(Path::new(p)).expect("valid path"))
            .collect()
    }
}
