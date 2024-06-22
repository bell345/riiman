use std::cmp::{Ordering, Reverse};
use std::collections::HashSet;
use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;
use uuid::Uuid;

use crate::data::{
    kind, FieldDefinition, FieldLike, FieldStore, FieldType, FieldValue, FilterExpression, Item,
    SerialColour, TextSearchQuery, Utf32CachedString, ValueMatchExpression, Vault,
};
use crate::errors::AppError;
use crate::{fields, time_us};

fn evaluate_match_expression_string(
    value: &str,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    Ok(match expr {
        ValueMatchExpression::Equals(x) => x.as_str()? == value,
        ValueMatchExpression::NotEquals(x) => x.as_str()? != value,
        ValueMatchExpression::IsOneOf(xs) => {
            for x in xs {
                if x.as_str()? == value {
                    return Ok(true);
                }
            }

            false
        }
        ValueMatchExpression::Contains(x) => x.as_str()?.contains(x.as_str()?),
        ValueMatchExpression::LessThan(x) => value < x.as_str()?,
        ValueMatchExpression::LessThanOrEqual(x) => value <= x.as_str()?,
        ValueMatchExpression::GreaterThanOrEqual(x) => value >= x.as_str()?,
        ValueMatchExpression::GreaterThan(x) => value > x.as_str()?,
        ValueMatchExpression::StartsWith(x) => value.starts_with(x.as_str()?),
        ValueMatchExpression::EndsWith(x) => value.ends_with(x.as_str()?),
        ValueMatchExpression::Regex(x) => x.is_match(value),
    })
}

fn evaluate_match_expression_typed<V, T: FieldLike<V>>(
    value: &V,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool>
where
    V: Eq + Ord + Copy + Display,
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
        ValueMatchExpression::Contains(x) => value.to_string().contains(x.as_str()?),
        ValueMatchExpression::LessThan(x) => value < &*T::try_from(x.clone())?,
        ValueMatchExpression::GreaterThan(x) => value > &*T::try_from(x.clone())?,
        ValueMatchExpression::LessThanOrEqual(x) => value <= &*T::try_from(x.clone())?,
        ValueMatchExpression::GreaterThanOrEqual(x) => value >= &*T::try_from(x.clone())?,
        ValueMatchExpression::StartsWith(x) => value.to_string().starts_with(x.as_str()?),
        ValueMatchExpression::EndsWith(x) => value.to_string().ends_with(x.as_str()?),
        ValueMatchExpression::Regex(x) => x.is_match(&value.to_string()),
    })
}

fn evaluate_match_expression_list(
    value: &[FieldValue],
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    Ok(match expr {
        ValueMatchExpression::Contains(x) => value.contains(x),
        _ => value
            .iter()
            .map(|v| evaluate_match_expression(v, expr))
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .any(|b| b),
    })
}

fn evaluate_match_expression_dictionary(
    value: &[(Utf32CachedString, FieldValue)],
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    Ok(match expr {
        ValueMatchExpression::Contains(x) => {
            let s = x.as_str_opt();
            value
                .iter()
                .any(|(k, v)| s.is_some_and(|s| k.contains(s)) || v == x)
        }
        _ => value
            .iter()
            .map(|(k, v)| {
                Ok(evaluate_match_expression_string(k, expr)?
                    || evaluate_match_expression(v, expr)?)
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .any(|b| b),
    })
}

fn evaluate_match_expression(
    value: &FieldValue,
    expr: &ValueMatchExpression,
) -> anyhow::Result<bool> {
    Ok(match value {
        FieldValue::Tag => true,
        FieldValue::Container => false,
        FieldValue::Boolean(v) => evaluate_match_expression_typed::<bool, kind::Boolean>(v, expr)?,
        FieldValue::Int(v) => evaluate_match_expression_typed::<i64, kind::Int>(v, expr)?,
        FieldValue::Float(v) => evaluate_match_expression_typed::<
            ordered_float::OrderedFloat<f64>,
            kind::Float,
        >(v, expr)?,
        FieldValue::Colour(v) => {
            evaluate_match_expression_typed::<SerialColour, kind::Colour>(v, expr)?
        }
        FieldValue::String(v) => evaluate_match_expression_string(v, expr)?,
        FieldValue::ItemRef(v) => {
            evaluate_match_expression_string(&v.0, expr)?
                || evaluate_match_expression_string(&v.1, expr)?
        }
        FieldValue::List(list) => evaluate_match_expression_list(list, expr)?,
        FieldValue::Dictionary(dict) => evaluate_match_expression_dictionary(dict, expr)?,
        FieldValue::DateTime(v) => evaluate_match_expression_typed::<
            chrono::DateTime<chrono::Utc>,
            kind::DateTime,
        >(v, expr)?,
    })
}

fn generic_string_match(
    item: &Item,
    vault: &Vault,
    matches: impl Fn(&Utf32CachedString) -> bool,
) -> anyhow::Result<bool> {
    time_us!(
        format!("Generic string match for item {}", item.path()),
        100,
        {
            if matches(item.path_string()) {
                return Ok(true);
            }

            for r in item.iter_fields_with_defs(vault) {
                let def = r.definition();
                let v = r.value();
                if match def.field_type {
                    FieldType::Tag | FieldType::Container => matches(&def.name),
                    FieldType::String => matches(v.as_string()?),
                    FieldType::ItemRef => {
                        let (v, p) = v.as_itemref()?;
                        matches(v) || matches(p)
                    }
                    FieldType::List => v
                        .as_list()?
                        .iter()
                        .filter_map(|v| v.as_string_opt())
                        .any(&matches),
                    FieldType::Dictionary => v
                        .as_dictionary()?
                        .iter()
                        .any(|(k, v)| v.as_string_opt().map_or_else(|| matches(k), &matches)),
                    _ => false,
                } {
                    return Ok(true);
                }
            }

            Ok(false)
        }
    )
}

pub fn evaluate_filter(
    item: &Item,
    vault: &Vault,
    filter: &FilterExpression,
) -> anyhow::Result<bool> {
    Ok(match filter {
        FilterExpression::None => true,
        FilterExpression::TextSearch(query) => {
            generic_string_match(item, vault, |s| query.matches(s))?
        }
        FilterExpression::ExactTextSearch(query) => {
            generic_string_match(item, vault, |s| query.matches(s))?
        }
        FilterExpression::FolderMatch(x) => Path::new(item.path()).starts_with(x),
        FilterExpression::TagMatch(id) => item.has_tag(vault, id).is_ok_and(|b| b),
        FilterExpression::FieldMatch(id, expr) => {
            if let Some(v) = item.get_field_value(id) {
                return evaluate_match_expression(&v, expr);
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

pub fn evaluate_items_filter<'a>(
    vault: &'a Vault,
    filter: &FilterExpression,
) -> anyhow::Result<Vec<impl Deref<Target = Item> + 'a>> {
    let mut items = vec![];
    for item in vault.iter_items() {
        if evaluate_filter(&item, vault, filter)? {
            items.push(item);
        }
    }

    Ok(items)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum FieldMatchResult {
    Name {
        id: Uuid,
        score: u32,
        indices: Vec<u32>,
    },
    Alias {
        id: Uuid,
        alias: String,
        score: u32,
        indices: Vec<u32>,
    },
    ParentName {
        id: Uuid,
        parent_id: Uuid,
        score: u32,
        indices: Vec<u32>,
    },
    ParentAlias {
        id: Uuid,
        parent_id: Uuid,
        alias: String,
        score: u32,
        indices: Vec<u32>,
    },
}

impl FieldMatchResult {
    fn id(&self) -> Uuid {
        match self {
            FieldMatchResult::Name { id, .. }
            | FieldMatchResult::Alias { id, .. }
            | FieldMatchResult::ParentName { id, .. }
            | FieldMatchResult::ParentAlias { id, .. } => *id,
        }
    }

    fn weighted_score(&self) -> (u8, u32) {
        match self {
            FieldMatchResult::Name { score, .. } | FieldMatchResult::Alias { score, .. } => {
                (1, *score)
            }
            FieldMatchResult::ParentName { score, .. }
            | FieldMatchResult::ParentAlias { score, .. } => (0, *score),
        }
    }

    fn indices(&self) -> &Vec<u32> {
        match self {
            FieldMatchResult::Name { indices, .. }
            | FieldMatchResult::Alias { indices, .. }
            | FieldMatchResult::ParentName { indices, .. }
            | FieldMatchResult::ParentAlias { indices, .. } => indices,
        }
    }
}

impl PartialOrd for FieldMatchResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FieldMatchResult {
    fn cmp(&self, other: &Self) -> Ordering {
        self.weighted_score().cmp(&other.weighted_score())
    }
}

fn evaluate_field_search_one(
    def: &FieldDefinition,
    vault: &Vault,
    query: &TextSearchQuery,
    seen: &[Uuid],
) -> anyhow::Result<Vec<FieldMatchResult>> {
    if seen.contains(&def.id) {
        return Err(AppError::FieldTreeLoop { field_id: def.id }.into());
    }

    let mut results = vec![];
    let mut seen = Vec::from(seen);
    seen.push(def.id);

    if let Some((score, indices)) = query.indices(&def.name) {
        results.push(FieldMatchResult::Name {
            id: def.id,
            score,
            indices,
        });
    }
    if let Some(aliases) = def.get_known_field_value(fields::meta::ALIASES)? {
        for alias in aliases {
            let alias_str = alias.as_string()?;
            if let Some((score, indices)) = query.indices(alias_str) {
                results.push(FieldMatchResult::Alias {
                    id: def.id,
                    alias: alias_str.to_string(),
                    score,
                    indices,
                });
            }
        }
    }

    for parent in vault.resolve_field_defs(def.iter_parent_ids()) {
        for result in evaluate_field_search_one(&parent, vault, query, &seen)? {
            results.push(match result {
                FieldMatchResult::Name { id, score, indices } => FieldMatchResult::ParentName {
                    id: def.id,
                    parent_id: id,
                    score,
                    indices,
                },
                FieldMatchResult::Alias {
                    id,
                    alias,
                    score,
                    indices,
                } => FieldMatchResult::ParentAlias {
                    id: def.id,
                    parent_id: id,
                    alias,
                    score,
                    indices,
                },
                FieldMatchResult::ParentName {
                    parent_id,
                    score,
                    indices,
                    ..
                } => FieldMatchResult::ParentName {
                    id: def.id,
                    parent_id,
                    score,
                    indices,
                },
                FieldMatchResult::ParentAlias {
                    parent_id,
                    alias,
                    score,
                    indices,
                    ..
                } => FieldMatchResult::ParentAlias {
                    id: def.id,
                    parent_id,
                    alias,
                    score,
                    indices,
                },
            });
        }

        #[allow(clippy::explicit_auto_deref)]
        {
            seen.push((*parent).id);
        }
    }

    Ok(results)
}

#[derive(Debug, Clone)]
pub struct MergedFieldMatchResult {
    pub id: Uuid,
    pub matches: Vec<FieldMatchResult>,
}

impl MergedFieldMatchResult {
    pub fn no_matches(id: Uuid) -> Self {
        Self {
            id,
            matches: vec![],
        }
    }

    pub fn with_matches<'a>(
        id: Uuid,
        results: impl Iterator<Item = &'a FieldMatchResult> + 'a,
    ) -> Self {
        Self {
            id,
            matches: results.filter(|r| r.id() == id).cloned().collect(),
        }
    }
}

pub fn evaluate_field_search(
    vault: &Vault,
    query: &TextSearchQuery,
    exclude_ids: Option<&[Uuid]>,
    filter_types: Option<&[FieldType]>,
) -> anyhow::Result<Vec<MergedFieldMatchResult>> {
    let mut results = vec![];
    let exclude_ids = exclude_ids.unwrap_or(&[]);
    let filter_types = filter_types.unwrap_or_else(|| FieldType::all());
    for def in vault.iter_field_defs() {
        if exclude_ids.contains(&def.id) {
            continue;
        }

        if !filter_types.contains(&def.field_type) {
            continue;
        }

        for result in evaluate_field_search_one(&def, vault, query, &[])? {
            results.push(result);
        }
    }

    results.sort_unstable_by_key(|r| Reverse(r.weighted_score()));

    let mut merged_results = vec![];
    let mut processed = HashSet::new();
    for (i, result) in results.iter().enumerate() {
        let id = result.id();
        if processed.contains(&id) {
            continue;
        }

        merged_results.push(MergedFieldMatchResult::with_matches(
            id,
            results.iter().skip(i),
        ));

        processed.insert(id);
    }

    Ok(merged_results)
}

#[cfg(test)]
mod test {
    use crate::data::ExactTextSearchQuery;

    #[test]
    fn test_exact_text_search_query_matches() {
        use crate::data::Utf32CachedString;
        let query = ExactTextSearchQuery::from("");
        assert!(query.matches(&Utf32CachedString::from("")));
        assert!(query.matches(&Utf32CachedString::from("this is a test string")));

        let query = ExactTextSearchQuery::from("cat dog");
        assert!(!query.matches(&Utf32CachedString::from("dog")));
        assert!(!query.matches(&Utf32CachedString::from("cat")));
        assert!(query.matches(&Utf32CachedString::from("cat dog")));
        assert!(query.matches(&Utf32CachedString::from("CAT DOG")));
        assert!(!query.matches(&Utf32CachedString::from("cat dgogg cast dog cat do")));
        assert!(query.matches(&Utf32CachedString::from("cat dgogg cast dog cat dog")));
        assert!(query.matches(&Utf32CachedString::from(
            "there once was a Cat Dog in the street"
        )));
        assert!(query.matches(&Utf32CachedString::from("ends with cat dog")));
        assert!(!query.matches(&Utf32CachedString::from("dog cat")));

        let query = ExactTextSearchQuery::from("crème brûlée");
        assert!(!query.matches(&Utf32CachedString::from("creme brulee")));
        assert!(!query.matches(&Utf32CachedString::from("creme brulee")));
        assert!(query.matches(&Utf32CachedString::from("Crème Brûlée")));
        assert!(query.matches(&Utf32CachedString::from("CRÈME BRÛLÉE")));
        assert!(query.matches(&Utf32CachedString::from("👌👌👌CRÈME BRÛLÉE👌👌👌")));
        assert!(query.matches(&Utf32CachedString::from("CRÈME BRÛLÉE👌👌👌")));
        assert!(query.matches(&Utf32CachedString::from("👌👌👌CRÈME BRÛLÉE")));
    }
}
