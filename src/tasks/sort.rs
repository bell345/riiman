use std::cmp::Ordering;
use std::ops::{Deref, Not};

use derive_more::Display;
use uuid::Uuid;

use crate::data::FieldType;
use crate::data::{kind, FieldDefinition, FieldStore, Item, SerialColour, Vault};
use crate::errors::AppError;
use crate::tasks::filter::{evaluate_items_filter, FilterExpression};

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

fn cmp_option_refs<Ref: Deref<Target = T>, T: Ord>(
    val1: Option<Ref>,
    val2: Option<Ref>,
) -> Ordering {
    match (val1, val2) {
        (Some(x), Some(y)) => (*x).cmp(&*y),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn cmp_by_field(
    item1: &Item,
    item2: &Item,
    vault: &Vault,
    field_def: &FieldDefinition,
) -> Ordering {
    let id = &field_def.id;

    macro_rules! cmp_typed {
        ($t:ty, $kind:ident) => {{
            let val1 = item1
                .get_field_value_typed::<$t, kind::$kind>(id)
                .ok()
                .flatten();
            let val2 = item2
                .get_field_value_typed::<$t, kind::$kind>(id)
                .ok()
                .flatten();
            val1.cmp(&val2)
        }};
    }

    match field_def.field_type {
        FieldType::Container | FieldType::Tag => item1
            .has_tag(vault, id)
            .ok()
            .cmp(&item2.has_tag(vault, id).ok()),
        FieldType::Boolean => cmp_typed!(bool, Boolean),
        FieldType::Int => cmp_typed!(i64, Int),
        FieldType::UInt => cmp_typed!(u64, UInt),
        FieldType::Float => cmp_typed!(ordered_float::OrderedFloat<f64>, Float),
        FieldType::Colour => cmp_typed!(SerialColour, Colour),
        FieldType::String | FieldType::ItemRef => cmp_option_refs(
            item1.get_field_value_as_str(id),
            item2.get_field_value_as_str(id),
        ),
        FieldType::DateTime => cmp_typed!(chrono::DateTime<chrono::Utc>, DateTime),
        FieldType::List | FieldType::Dictionary => Ordering::Equal,
    }
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
                items.sort_unstable_by(|a, b| cmp_by_field(a, b, vault, &field_def));
            }
        }

        if *sort_dir == SortDirection::Descending {
            items.reverse();
        }
    }

    Ok(items)
}
