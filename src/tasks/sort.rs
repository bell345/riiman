use std::cmp::Ordering;
use std::ops::{Deref, Not};

use derive_more::Display;
use uuid::Uuid;

use crate::data::kind::KindType;
use crate::data::{kind, FieldDefinition, FieldStore, FieldValue, FieldValueKind, Item, Vault};
use crate::errors::AppError;
use crate::tasks::filter::{evaluate_filter, evaluate_items_filter, new_matcher, FilterExpression};

#[derive(
    Default, Display, Debug, Eq, PartialEq, Copy, Clone, serde::Serialize, serde::Deserialize,
)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    pub(crate) fn to_icon(self) -> &'static str {
        match self {
            SortDirection::Ascending => "\u{23f6}",
            SortDirection::Descending => "\u{23f7}",
        }
    }
}

impl Not for SortDirection {
    type Output = SortDirection;

    fn not(self) -> Self::Output {
        match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum SortExpression {
    Path(SortDirection),
    Field(Uuid, SortDirection),
}

#[derive(Default, Debug, Display, Eq, PartialEq)]
pub enum SortType {
    #[default]
    Path,
    Field,
}

impl From<SortExpression> for SortType {
    fn from(value: SortExpression) -> Self {
        match value {
            SortExpression::Path(_) => SortType::Path,
            SortExpression::Field(_, _) => SortType::Field,
        }
    }
}

fn cmp_by_field(
    item1: &Item,
    item2: &Item,
    vault: &Vault,
    field_def: &FieldDefinition,
) -> Option<Ordering> {
    let id = &field_def.id;
    macro_rules! cmp_typed {
        ($t:ty, $kind:ident) => {
            item1
                .get_field_value_typed::<$t, kind::$kind>(id)
                .ok()??
                .cmp(&item2.get_field_value_typed::<$t, kind::$kind>(id).ok()??)
        };
    }
    Some(match field_def.field_type {
        KindType::Tag => item1
            .has_tag(vault, id)
            .unwrap_or(false)
            .cmp(&item2.has_tag(vault, id).ok()?),
        KindType::Boolean => cmp_typed!(bool, Boolean),
        KindType::Int => cmp_typed!(i64, Int),
        KindType::UInt => cmp_typed!(u64, UInt),
        KindType::Float => cmp_typed!(ordered_float::OrderedFloat<f64>, Float),
        KindType::Colour => cmp_typed!([u8; 3], Colour),
        KindType::Str | KindType::ItemRef => item1
            .get_field_value(id)?
            .as_str_opt()?
            .cmp(item2.get_field_value(id)?.as_str_opt()?),
        KindType::DateTime => cmp_typed!(chrono::DateTime<chrono::Utc>, DateTime),
        KindType::List => Ordering::Equal,
        KindType::Dictionary => Ordering::Equal,
    })
}

pub fn get_filtered_and_sorted_items<'a, 'b>(
    vault: &'a Vault,
    filter: &'b FilterExpression,
    sorts: &'b [SortExpression],
) -> anyhow::Result<Vec<impl Deref<Target = Item> + 'a>> {
    let mut items = evaluate_items_filter(vault, filter)?;

    for sort in sorts.iter().rev() {
        let sort_dir: &SortDirection;
        match sort {
            SortExpression::Path(dir) => {
                sort_dir = dir;
                items.sort_unstable_by(|a, b| a.path().cmp(b.path()));
            }
            SortExpression::Field(id, dir) => {
                sort_dir = dir;
                let field_def = vault.get_definition(id).ok_or(anyhow::Error::from(
                    AppError::MissingFieldDefinition { id: *id },
                ))?;
                items.sort_unstable_by(|a, b| {
                    cmp_by_field(a, b, vault, &field_def).unwrap_or(Ordering::Equal)
                });
            }
        }

        if *sort_dir == SortDirection::Descending {
            items.reverse();
        }
    }

    Ok(items)
}
