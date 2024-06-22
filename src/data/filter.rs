use crate::data::{FieldValue, Utf32CachedString};
use eframe::egui::text::{CCursor, CCursorRange, CursorRange};
use eframe::egui::TextBuffer;
use eframe::epaint::text::cursor::{Cursor, PCursor, RCursor};
use itertools::Itertools;
use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take_while, take_while_m_n};
use nom::character::complete::{none_of, one_of};
use nom::combinator::{map, map_opt};
use nom::error::ParseError;
use nom::multi::{count, fold_many0, many0, many1};
use nom::sequence::{delimited, pair, preceded, separated_pair, terminated, tuple};
use nom::{Compare, IResult, InputLength, InputTake, InputTakeAtPosition, Parser};
use nom_locate::LocatedSpan;
use regex::Regex;
use serde::{Deserializer, Serializer};
use serde_regex::Serde;
use std::collections::HashSet;
use std::fmt::{Debug, Formatter};
use std::iter::Filter;
use std::ops::{Deref, DerefMut, Not};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

pub fn new_matcher() -> nucleo_matcher::Matcher {
    nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerdeRegex(#[serde(with = "serde_regex")] Regex);

impl Deref for SerdeRegex {
    type Target = Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Regex> for SerdeRegex {
    fn from(value: Regex) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, strum::EnumDiscriminants, serde::Serialize, serde::Deserialize)]
pub enum ValueMatchExpression {
    Equals(FieldValue),
    NotEquals(FieldValue),
    IsOneOf(HashSet<FieldValue>),
    Contains(FieldValue),
    LessThan(FieldValue),
    LessThanOrEqual(FieldValue),
    GreaterThan(FieldValue),
    GreaterThanOrEqual(FieldValue),
    StartsWith(FieldValue),
    EndsWith(FieldValue),
    Regex(SerdeRegex),
}

impl PartialEq for ValueMatchExpression {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Equals(x), Self::Equals(y))
            | (Self::NotEquals(x), Self::NotEquals(y))
            | (Self::Contains(x), Self::Contains(y))
            | (Self::LessThan(x), Self::LessThan(y))
            | (Self::GreaterThan(x), Self::GreaterThan(y))
            | (Self::LessThanOrEqual(x), Self::LessThanOrEqual(y))
            | (Self::GreaterThanOrEqual(x), Self::GreaterThanOrEqual(y))
            | (Self::StartsWith(x), Self::StartsWith(y))
            | (Self::EndsWith(x), Self::EndsWith(y)) => x.eq(y),
            (Self::IsOneOf(x), Self::IsOneOf(y)) => {
                x.len() == y.len() && x.intersection(y).count() == x.len()
            }
            (Self::Regex(x), Self::Regex(y)) => x.as_str().eq(y.as_str()),
            _ => false,
        }
    }
}

impl Eq for ValueMatchExpression {}

#[derive(Default)]
pub struct TextSearchQuery {
    string: String,
    pattern: nucleo_matcher::pattern::Pattern,
    matcher: OnceLock<Mutex<nucleo_matcher::Matcher>>,
    temp_idx_buf: Mutex<Vec<u32>>,
}

impl Debug for TextSearchQuery {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextSearchQuery({:?})", self.string)
    }
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

impl<T: Into<String>> From<T> for TextSearchQuery {
    fn from(value: T) -> Self {
        Self::new(value.into())
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

    pub fn indices(&self, haystack: &Utf32CachedString) -> Option<(u32, Vec<u32>)> {
        let mut l_idx_buf = self.temp_idx_buf.lock().unwrap();
        let mut l_matcher = self
            .matcher
            .get_or_init(|| Mutex::new(new_matcher()))
            .lock()
            .unwrap();
        l_idx_buf.deref_mut().clear();
        self.pattern
            .indices(haystack.utf32().slice(..), &mut l_matcher, &mut l_idx_buf)
            .map(|score| (score, l_idx_buf.deref().clone()))
    }

    pub fn score(&self, haystack: &Utf32CachedString) -> Option<u32> {
        let mut l_matcher = self
            .matcher
            .get_or_init(|| Mutex::new(new_matcher()))
            .lock()
            .unwrap();
        self.pattern
            .score(haystack.utf32().slice(..), &mut l_matcher)
    }

    pub fn matches(&self, haystack: &Utf32CachedString) -> bool {
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

#[derive(Default, Clone)]
pub struct ExactTextSearchQuery {
    original: String,
    lowercase: Vec<char>,
    char_len: usize,
}

impl Debug for ExactTextSearchQuery {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExactTextSearchQuery({:?})", self.original)
    }
}

impl<T: Into<String>> From<T> for ExactTextSearchQuery {
    fn from(value: T) -> Self {
        Self::new(value.into())
    }
}

impl PartialEq for ExactTextSearchQuery {
    fn eq(&self, other: &Self) -> bool {
        self.original == other.original
    }
}

impl Eq for ExactTextSearchQuery {}

impl serde::Serialize for ExactTextSearchQuery {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.original.serialize(serializer)
    }
}

impl<'a> serde::Deserialize<'a> for ExactTextSearchQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        Ok(Self::new(String::deserialize(deserializer)?))
    }
}

impl ExactTextSearchQuery {
    pub fn new(query: String) -> Self {
        let lowercase: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
        Self {
            char_len: lowercase.len(),
            lowercase,
            original: query,
        }
    }

    pub fn matches(&self, haystack: &Utf32CachedString) -> bool {
        let mut scan_idx = 0;
        for c in haystack
            .utf32()
            .slice(..)
            .chars()
            .flat_map(|c| c.to_lowercase())
        {
            if scan_idx == self.char_len {
                return true;
            }

            scan_idx = if c == self.lowercase[scan_idx] {
                scan_idx + 1
            } else {
                0
            }
        }
        scan_idx == self.char_len
    }
}

#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub enum FilterExpression {
    #[default]
    None,
    TextSearch(TextSearchQuery),
    ExactTextSearch(ExactTextSearchQuery),
    FolderMatch(Box<Path>),
    TagMatch(Uuid),
    FieldMatch(Uuid, ValueMatchExpression),
    Not(Box<FilterExpression>),
    Or(Box<FilterExpression>, Box<FilterExpression>),
    And(Box<FilterExpression>, Box<FilterExpression>),
}
