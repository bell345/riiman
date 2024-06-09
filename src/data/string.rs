use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub struct Utf32CachedString {
    inner: String,
    utf32: nucleo_matcher::Utf32String,
}

impl Default for Utf32CachedString {
    fn default() -> Self {
        String::default().into()
    }
}

impl Hash for Utf32CachedString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}

impl PartialEq for Utf32CachedString {
    fn eq(&self, other: &Self) -> bool {
        self.utf32() == other.utf32()
    }
}

impl Eq for Utf32CachedString {}

impl PartialOrd for Utf32CachedString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Utf32CachedString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.utf32.cmp(&other.utf32)
    }
}

impl Serialize for Utf32CachedString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.inner.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Utf32CachedString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.into())
    }
}

impl From<String> for Utf32CachedString {
    fn from(value: String) -> Self {
        Self {
            utf32: nucleo_matcher::Utf32String::from(value.as_str()),
            inner: value,
        }
    }
}

impl From<&str> for Utf32CachedString {
    fn from(value: &str) -> Self {
        Self::from(value.to_string())
    }
}

impl From<Utf32CachedString> for String {
    fn from(value: Utf32CachedString) -> Self {
        value.inner
    }
}

impl Deref for Utf32CachedString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> AsRef<T> for Utf32CachedString
where
    T: ?Sized,
    <Utf32CachedString as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl Display for Utf32CachedString {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Utf32CachedString {
    pub fn utf32(&self) -> &nucleo_matcher::Utf32String {
        &self.utf32
    }
}
