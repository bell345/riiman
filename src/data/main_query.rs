use crate::data::parse::FilterExpressionParseResult;
use crate::data::vault_ui::VaultViewModel;
use crate::data::{FieldStore, FilterExpression, Vault};
use crate::tasks::sort::{SortDirection, SortExpression, SortType};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct MainQueryParams {
    pub last_updated: DateTime<Utc>,
    pub search_text: String,
    pub sort_type: SortType,
    pub sort_field_id: Option<Uuid>,
    pub sort_direction: SortDirection,
}

#[derive(Default)]
pub struct MainQuery {
    pub params: MainQueryParams,
    filter: FilterExpression,
    sorts: Vec<SortExpression>,
    is_new: bool,
}

impl MainQuery {
    fn new_params_opt(&self, vault: &Vault, vm: &VaultViewModel) -> Option<MainQueryParams> {
        let make_params = || MainQueryParams {
            last_updated: vault.last_modified(),
            search_text: vm.search_text.clone(),
            sort_type: vm.sort_type,
            sort_field_id: vm.sort_field_id,
            sort_direction: vm.sort_direction,
        };

        if self.params.last_updated != vault.last_modified() {
            return Some(make_params());
        }
        if self.params.search_text != vm.search_text {
            return Some(make_params());
        }
        if self.params.sort_type != vm.sort_type {
            return Some(make_params());
        }
        if self.params.sort_field_id != vm.sort_field_id {
            return Some(make_params());
        }
        if self.params.sort_direction != vm.sort_direction {
            return Some(make_params());
        }

        None
    }

    pub fn update(&mut self, vault: &Vault, vm: &VaultViewModel) {
        let Some(params) = self.new_params_opt(vault, vm) else {
            return;
        };

        self.params = params;

        self.sorts = match vm.sort_type {
            SortType::Path => vec![SortExpression::Path(vm.sort_direction)],
            SortType::Field => {
                if let Some(field_id) = vm.sort_field_id {
                    vec![SortExpression::Field(field_id, vm.sort_direction)]
                } else {
                    vec![]
                }
            }
        };

        self.filter = vm
            .search_text
            .parse::<FilterExpressionParseResult>()
            .map_or(FilterExpression::None, |r| r.expr);

        self.is_new = true;
    }

    pub fn filter(&self) -> &FilterExpression {
        &self.filter
    }

    pub fn sorts(&self) -> &[SortExpression] {
        &self.sorts
    }

    pub fn take_freshness(&mut self) -> bool {
        let is_new = self.is_new;
        self.is_new = false;
        is_new
    }
}
