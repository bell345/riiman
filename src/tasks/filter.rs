use crate::data::kind::KindType;
use crate::data::{
    kind, FieldDefinition, FieldStore, FieldValue, FieldValueKind, Item, SerialColour, Vault,
};
use crate::errors::AppError;
use crate::fields;
use serde::{Deserializer, Serializer};
use std::cmp::{Ordering, Reverse};
use std::collections::HashSet;
use std::fmt::Display;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

pub fn new_matcher() -> nucleo_matcher::Matcher {
    nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerdeRegex(#[serde(with = "serde_regex")] regex::Regex);

impl Deref for SerdeRegex {
    type Target = regex::Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ValueMatchExpression {
    Equals(FieldValue),
    NotEquals(FieldValue),
    IsOneOf(HashSet<FieldValue>),
    LessThan(FieldValue),
    GreaterThan(FieldValue),
    Regex(SerdeRegex),
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

#[derive(Debug, Default)]
pub struct TextSearchQuery {
    string: String,
    pattern: nucleo_matcher::pattern::Pattern,
    matcher: OnceLock<Mutex<nucleo_matcher::Matcher>>,
    temp_char_buf: Mutex<Vec<char>>,
    temp_idx_buf: Mutex<Vec<u32>>,
}

impl Clone for TextSearchQuery {
    fn clone(&self) -> Self {
        Self {
            string: self.string.clone(),
            pattern: self.pattern.clone(),
            ..Default::default()
        }
    }
}

impl From<String> for TextSearchQuery {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl TextSearchQuery {
    pub fn new(s: String) -> Self {
        Self {
            pattern: nucleo_matcher::pattern::Pattern::parse(
                s.as_str(),
                nucleo_matcher::pattern::CaseMatching::Smart,
                nucleo_matcher::pattern::Normalization::Smart,
            ),
            string: s,
            ..Default::default()
        }
    }

    pub fn indices(&self, haystack: &str) -> Option<(u32, Vec<u32>)> {
        let mut l_idx_buf = self.temp_idx_buf.lock().unwrap();
        let mut l_tmp = self.temp_char_buf.lock().unwrap();
        let mut l_matcher = self
            .matcher
            .get_or_init(|| Mutex::new(new_matcher()))
            .lock()
            .unwrap();
        let haystack_utf32 = nucleo_matcher::Utf32Str::new(haystack, l_tmp.deref_mut());
        l_idx_buf.deref_mut().clear();
        self.pattern
            .indices(haystack_utf32, l_matcher.deref_mut(), l_idx_buf.deref_mut())
            .map(|score| (score, l_idx_buf.deref().clone()))
    }

    pub fn score(&self, haystack: &str) -> Option<u32> {
        if let Some((score, _)) = self.indices(haystack) {
            Some(score)
        } else {
            None
        }
    }

    pub fn matches(&self, haystack: &str) -> bool {
        self.score(haystack).is_some()
    }
}

impl serde::Serialize for TextSearchQuery {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.string.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for TextSearchQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::new(String::deserialize(deserializer)?))
    }
}

impl PartialEq for TextSearchQuery {
    fn eq(&self, other: &Self) -> bool {
        self.string.eq(&other.string)
    }
}

impl Eq for TextSearchQuery {}

#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub enum FilterExpression {
    #[default]
    None,
    TextSearch(TextSearchQuery),
    FolderMatch(Box<Path>),
    TagMatch(Uuid),
    FieldMatch(Uuid, ValueMatchExpression),
    Not(Box<FilterExpression>),
    Or(Box<FilterExpression>, Box<FilterExpression>),
    And(Box<FilterExpression>, Box<FilterExpression>),
}

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
        ValueMatchExpression::LessThan(x) => value < x.as_str()?,
        ValueMatchExpression::GreaterThan(x) => value > x.as_str()?,
        ValueMatchExpression::Regex(x) => x.is_match(value),
    })
}

fn evaluate_match_expression_typed<V, T: FieldValueKind<V>>(
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
        ValueMatchExpression::LessThan(x) => value < &*T::try_from(x.clone())?,
        ValueMatchExpression::GreaterThan(x) => value > &*T::try_from(x.clone())?,
        ValueMatchExpression::Regex(x) => x.is_match(&format!("{}", value)),
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
        FieldValue::UInt(v) => evaluate_match_expression_typed::<u64, kind::UInt>(v, expr)?,
        FieldValue::Float(v) => evaluate_match_expression_typed::<
            ordered_float::OrderedFloat<f64>,
            kind::Float,
        >(v, expr)?,
        FieldValue::Colour(v) => {
            evaluate_match_expression_typed::<SerialColour, kind::Colour>(v, expr)?
        }
        FieldValue::Str(v) => evaluate_match_expression_string(v, expr)?,
        FieldValue::ItemRef(v) => evaluate_match_expression_string(v, expr)?,
        FieldValue::List(list) => list
            .iter()
            .map(|v| evaluate_match_expression(v, expr))
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .any(|b| b),
        FieldValue::Dictionary(dict) => dict
            .iter()
            .map(|(k, v)| {
                Ok(evaluate_match_expression_string(k, expr)?
                    || evaluate_match_expression(v, expr)?)
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .any(|b| b),
        FieldValue::DateTime(v) => evaluate_match_expression_typed::<
            chrono::DateTime<chrono::Utc>,
            kind::DateTime,
        >(v, expr)?,
    })
}

pub fn evaluate_filter(
    item: &Item,
    vault: &Vault,
    filter: &FilterExpression,
) -> anyhow::Result<bool> {
    Ok(match filter {
        FilterExpression::None => true,
        FilterExpression::TextSearch(query) => {
            if query.matches(item.path()) {
                return Ok(true);
            }

            let matches = |s: &String| query.matches(s.as_str());

            for r in item.iter_fields_with_defs(vault) {
                let def = r.definition();
                let v = r.value();
                if match def.field_type {
                    KindType::Tag | KindType::Container => matches(&def.name),
                    KindType::Str => matches(&String::from(kind::Str::try_from(v.clone())?)),
                    KindType::ItemRef => {
                        matches(&String::from(kind::ItemRef::try_from(v.clone())?))
                    }
                    KindType::List => kind::List::try_from(v.clone())?
                        .iter()
                        .filter_map(|v| v.as_string_opt())
                        .any(matches),
                    KindType::Dictionary => kind::Dictionary::try_from(v.clone())?
                        .iter()
                        .any(|(k, v)| v.as_string_opt().map(matches).unwrap_or(matches(k))),
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
            FieldMatchResult::Name { id, .. } => *id,
            FieldMatchResult::Alias { id, .. } => *id,
            FieldMatchResult::ParentName { id, .. } => *id,
            FieldMatchResult::ParentAlias { id, .. } => *id,
        }
    }

    fn score(&self) -> u32 {
        match self {
            FieldMatchResult::Name { score, .. } => *score,
            FieldMatchResult::Alias { score, .. } => *score,
            FieldMatchResult::ParentName { score, .. } => *score,
            FieldMatchResult::ParentAlias { score, .. } => *score,
        }
    }

    fn weighted_score(&self) -> (u8, u32) {
        match self {
            FieldMatchResult::Name { score, .. } => (1, *score),
            FieldMatchResult::Alias { score, .. } => (1, *score),
            FieldMatchResult::ParentName { score, .. } => (0, *score),
            FieldMatchResult::ParentAlias { score, .. } => (0, *score),
        }
    }

    fn indices(&self) -> &Vec<u32> {
        match self {
            FieldMatchResult::Name { indices, .. } => indices,
            FieldMatchResult::Alias { indices, .. } => indices,
            FieldMatchResult::ParentName { indices, .. } => indices,
            FieldMatchResult::ParentAlias { indices, .. } => indices,
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
        })
    }
    if let Some(aliases) = def.get_known_field_value(fields::meta::ALIASES)? {
        for alias in aliases {
            let alias_str = alias.as_str()?;
            if let Some((score, indices)) = query.indices(alias_str) {
                results.push(FieldMatchResult::Alias {
                    id: def.id,
                    alias: alias_str.into(),
                    score,
                    indices,
                })
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
    filter_types: Option<&[KindType]>,
) -> anyhow::Result<Vec<MergedFieldMatchResult>> {
    let mut results = vec![];
    let exclude_ids = exclude_ids.unwrap_or(&[]);
    let filter_types = filter_types.unwrap_or_else(|| KindType::all());
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
