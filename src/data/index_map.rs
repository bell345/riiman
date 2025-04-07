use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

pub struct IndexMapSerial<K, V>(IndexMap<K, V>);

impl<K, V> Default for IndexMapSerial<K, V> {
    fn default() -> Self {
        Self(IndexMap::new())
    }
}

impl<K, V> Deref for IndexMapSerial<K, V> {
    type Target = IndexMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> DerefMut for IndexMapSerial<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V, T> AsRef<T> for IndexMapSerial<K, V>
where
    T: ?Sized,
    <IndexMapSerial<K, V> as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl<K: Serialize, V: Serialize> Serialize for IndexMapSerial<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0
            .iter()
            .collect::<Vec<(&K, &V)>>()
            .serialize(serializer)
    }
}

impl<'de, K: Eq + Hash + Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de>
    for IndexMapSerial<K, V>
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = IndexMap::new();
        for (k, v) in Vec::<(K, V)>::deserialize(deserializer)? {
            map.insert(k, v);
        }
        Ok(IndexMapSerial(map))
    }
}
