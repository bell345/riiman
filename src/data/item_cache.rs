use crate::data::{FieldStore, FilterExpression, ItemId, Vault};
use crate::tasks::sort::{get_filtered_and_sorted_items, SortExpression};
use chrono::{DateTime, Utc};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemCacheParams {
    pub(crate) last_updated: DateTime<Utc>,
    pub(crate) sorts: Vec<SortExpression>,
    pub(crate) filter: FilterExpression,
}

#[derive(Default)]
pub struct ItemCache {
    item_ids: Vec<ItemId>,
    pub(crate) params: ItemCacheParams,
    is_new: bool,
    refresh_requested: bool,
}

fn take(x: &mut bool) -> bool {
    let value = *x;
    *x = false;
    value
}

impl ItemCache {
    fn new_params_opt(
        &self,
        vault: &Vault,
        filter: &FilterExpression,
        sorts: &[SortExpression],
    ) -> Option<ItemCacheParams> {
        let make_params = || ItemCacheParams {
            last_updated: vault.last_modified(),
            filter: filter.to_owned(),
            sorts: sorts.to_owned(),
        };

        if self.params.last_updated != vault.last_modified() {
            return Some(make_params());
        }
        if self.params.filter != *filter {
            return Some(make_params());
        }
        if self.params.sorts != *sorts {
            return Some(make_params());
        }

        None
    }

    pub fn update(
        &mut self,
        vault: &Vault,
        filter: &FilterExpression,
        sorts: &[SortExpression],
    ) -> anyhow::Result<()> {
        let Some(params) = self.new_params_opt(vault, filter, sorts) else {
            return Ok(());
        };

        // TODO: handle errors sanely and properly
        self.params = params;
        let items = get_filtered_and_sorted_items(vault, filter, sorts)?;
        self.item_ids = items
            .iter()
            .map(|item| ItemId::from_item(vault, item))
            .collect();

        self.is_new = true;
        Ok(())
    }

    pub fn item_ids(&self) -> &[ItemId] {
        &self.item_ids
    }

    pub fn request_update(&mut self) {
        self.refresh_requested = true;
    }

    pub fn consume_refresh_request(&mut self) -> bool {
        take(&mut self.is_new) | take(&mut self.refresh_requested)
    }
}
