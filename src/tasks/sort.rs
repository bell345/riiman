use std::cmp::Ordering;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;

use uuid::Uuid;

use crate::data::kind::{KindType, Value};
use crate::data::{kind, FieldDefinition, FieldValue, FieldValueKind, Item, Vault};
use crate::errors::AppError;

#[derive(Debug, Clone)]
pub enum ValueMatchExpression {
    Equals(FieldValue),
    NotEquals(FieldValue),
    IsOneOf(HashSet<FieldValue>),
    LessThan(FieldValue),
    GreaterThan(FieldValue),
    Regex(regex::Regex),
}

impl PartialEq for ValueMatchExpression {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Equals(x), Self::Equals(y))
            | (Self::NotEquals(x), Self::NotEquals(y))
            | (Self::LessThan(x), Self::LessThan(y))
            | (Self::GreaterThan(x), Self::GreaterThan(y)) => x.eq(y),
            (Self::IsOneOf(x), Self::IsOneOf(y)) => x.iter().eq(y.iter()),
            (Self::Regex(x), Self::Regex(y)) => x.as_str().eq(y.as_str()),
            _ => false,
        }
    }
}

impl Eq for ValueMatchExpression {}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub enum FilterExpression {
    #[default]
    None,
    TextSearch(String),
    FolderMatch(Box<Path>),
    TagMatch(Uuid),
    FieldMatch(Uuid, ValueMatchExpression),
    Not(Box<FilterExpression>),
    Or(Box<FilterExpression>, Box<FilterExpression>),
    And(Box<FilterExpression>, Box<FilterExpression>),
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SortExpression {
    Path(SortDirection),
    Field(Uuid, SortDirection),
}

fn evaluate_match_expression_string(
    value: &str,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    let e_factory = |v: &FieldValue| AppError::WrongFieldType {
        expected: KindType::Str,
        got: v.clone(),
    };
    Ok(match expr {
        ValueMatchExpression::Equals(x) => x.as_str().ok_or_else(|| e_factory(x))? == value,
        ValueMatchExpression::NotEquals(x) => x.as_str().ok_or_else(|| e_factory(x))? != value,
        ValueMatchExpression::IsOneOf(xs) => {
            for x in xs {
                if x.as_str().ok_or_else(|| e_factory(x))? == value {
                    return Ok(true);
                }
            }

            false
        }
        ValueMatchExpression::LessThan(x) => value < x.as_str().ok_or_else(|| e_factory(x))?,
        ValueMatchExpression::GreaterThan(x) => value > x.as_str().ok_or_else(|| e_factory(x))?,
        ValueMatchExpression::Regex(x) => x.is_match(value),
    })
}

fn evaluate_match_expression_typed<V, T: FieldValueKind<V>>(
    value: &V,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool>
where
    V: Eq + Ord + Copy,
    <T as TryFrom<FieldValue>>::Error: std::error::Error + Send + Sync + 'static,
{
    Ok(match expr {
        ValueMatchExpression::Equals(x) => &*T::try_from(x.clone())? == value,
        ValueMatchExpression::NotEquals(x) => &*T::try_from(x.clone())? != value,
        ValueMatchExpression::IsOneOf(xs) => {
            for x in xs {
                if &*T::try_from(x.clone())? == value {
                    return Ok(true);
                }
            }
            false
        }
        ValueMatchExpression::LessThan(x) => value < &*T::try_from(x.clone())?,
        ValueMatchExpression::GreaterThan(x) => value > &*T::try_from(x.clone())?,
        ValueMatchExpression::Regex(_) => todo!(),
    })
}

fn evaluate_match_expression(
    value: &FieldValue,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    Ok(match value {
        FieldValue::Tag => true,
        FieldValue::Boolean(v) => evaluate_match_expression_typed::<bool, kind::Boolean>(v, expr)?,
        FieldValue::Int(v) => evaluate_match_expression_typed::<i64, kind::Int>(v, expr)?,
        FieldValue::UInt(v) => evaluate_match_expression_typed::<u64, kind::UInt>(v, expr)?,
        FieldValue::Str(v) => evaluate_match_expression_string(v, expr)?,
        FieldValue::ItemRef(v) => evaluate_match_expression_string(v, expr)?,
        FieldValue::List(_) => todo!(),
        FieldValue::Dictionary => true,
        FieldValue::DateTime(v) => evaluate_match_expression_typed::<
            chrono::DateTime<chrono::Utc>,
            kind::DateTime,
        >(v, expr)?,
    })
}

fn evaluate_filter(item: &Item, vault: &Vault, filter: &FilterExpression) -> anyhow::Result<bool> {
    Ok(match filter {
        FilterExpression::None => true,
        FilterExpression::TextSearch(text) => {
            if item.path().contains(text) {
                return Ok(true);
            }

            for (def, v) in item.iter_field_defs(vault) {
                if match def.field_type {
                    KindType::Tag => def.name.contains(text),
                    KindType::Str => String::from(kind::Str::try_from(v.clone())?).contains(text),
                    KindType::ItemRef => {
                        String::from(kind::ItemRef::try_from(v.clone())?).contains(text)
                    }
                    KindType::List => return Err(AppError::NotImplemented.into()),
                    KindType::Dictionary => return Err(AppError::NotImplemented.into()),
                    _ => false,
                } {
                    return Ok(true);
                }
            }

            false
        }
        FilterExpression::FolderMatch(x) => Path::new(item.path()).starts_with(x),
        FilterExpression::TagMatch(id) => item.has_tag(vault, id)?,
        FilterExpression::FieldMatch(id, expr) => {
            if let Some(v) = item.get_field_value(&id) {
                return evaluate_match_expression(v, expr);
            }

            false
        }
        FilterExpression::Not(a) => !evaluate_filter(item, vault, a)?,
        FilterExpression::Or(a, b) => {
            evaluate_filter(item, vault, a)? || evaluate_filter(item, vault, b)?
        }
        FilterExpression::And(a, b) => {
            evaluate_filter(item, vault, a)? && evaluate_filter(item, vault, b)?
        }
    })
}

fn cmp_by_field(
    item1: &Item,
    item2: &Item,
    vault: &Vault,
    field_def: &FieldDefinition,
) -> Option<Ordering> {
    let id = &field_def.id;
    Some(match field_def.field_type {
        KindType::Tag => item1
            .has_tag(vault, id)
            .unwrap_or(false)
            .cmp(&item2.has_tag(vault, id).ok()?),
        KindType::Boolean => item1
            .get_field_value_typed::<bool, kind::Boolean>(id)
            .ok()??
            .cmp(
                &item2
                    .get_field_value_typed::<bool, kind::Boolean>(id)
                    .ok()??,
            ),
        KindType::Int => item1
            .get_field_value_typed::<i64, kind::Int>(id)
            .ok()??
            .cmp(&item2.get_field_value_typed::<i64, kind::Int>(id).ok()??),
        KindType::UInt => item1
            .get_field_value_typed::<u64, kind::UInt>(id)
            .ok()??
            .cmp(&item2.get_field_value_typed::<u64, kind::UInt>(id).ok()??),
        KindType::Str | KindType::ItemRef => item1
            .get_field_value(id)?
            .as_str()?
            .cmp(item2.get_field_value(id)?.as_str()?),
        KindType::DateTime => item1
            .get_field_value_typed::<chrono::DateTime<chrono::Utc>, kind::DateTime>(id)
            .ok()??
            .cmp(
                &item2
                    .get_field_value_typed::<chrono::DateTime<chrono::Utc>, kind::DateTime>(id)
                    .ok()??,
            ),
        KindType::List => todo!(),
        KindType::Dictionary => todo!(),
    })
}

pub fn get_filtered_and_sorted_items<'a, 'b>(
    vault: &'a Vault,
    filter: &'b FilterExpression,
    sorts: &'b [SortExpression],
) -> anyhow::Result<Vec<impl Deref<Target = Item> + 'a>> {
    let mut items = vec![];
    for item in vault.iter_items() {
        if evaluate_filter(&item, vault, filter)? {
            items.push(item);
        }
    }

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
