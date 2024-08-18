use crate::data::{FieldStore, TagLike, Utf32CachedString};
use dashmap::{DashMap, DashSet};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::OnceLock;
use uuid::Uuid;

#[allow(clippy::module_name_repetitions)]
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub id: Uuid,
    pub name: Utf32CachedString,
    pub field_type: Type,
    parents: DashSet<Uuid>,
    children: DashSet<Uuid>,
    fields: DashMap<Uuid, Value>,
}

impl Definition {
    pub fn known<T: TagLike>(known_field: &KnownField<T>) -> Self {
        Self {
            id: known_field.id,
            name: known_field.name.to_string().into(),
            field_type: T::get_type(),
            ..Default::default()
        }
    }

    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            ..Default::default()
        }
    }

    pub fn tag(id: Uuid, name: String) -> Self {
        Self {
            id,
            name: name.into(),
            field_type: kind::Tag::get_type(),
            ..Default::default()
        }
    }

    pub fn with_parent(self, parent_id: Uuid) -> Self {
        self.add_parent(parent_id);
        self
    }

    pub fn with_tag_id(self, tag_id: Uuid) -> Self {
        self.fields.insert(tag_id, Value::Tag);
        self
    }

    pub fn iter_parent_ids(&self) -> impl Iterator<Item = impl Deref<Target = Uuid> + '_> {
        self.parents.iter()
    }

    pub fn iter_child_ids(&self) -> impl Iterator<Item = impl Deref<Target = Uuid> + '_> {
        self.children.iter()
    }

    pub fn add_parent(&self, parent_id: Uuid) {
        self.parents.insert(parent_id);
    }

    pub fn remove_parent(&self, parent_id: Uuid) {
        self.parents.remove(&parent_id);
    }

    pub fn add_child(&self, child_id: Uuid) {
        self.children.insert(child_id);
    }

    pub fn remove_child(&self, child_id: Uuid) {
        self.children.remove(&child_id);
    }
}

impl FieldStore for Definition {
    fn fields(&self) -> &DashMap<Uuid, Value> {
        &self.fields
    }
}

macro_rules! impl_kind {
    { $name:ident } => {
        #[derive(Default, std::fmt::Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name ;

        impl TagLike for $name {
            fn get_type() -> Type {
                Type::$name
            }
        }

        impl TryFrom<Value> for $name {
            type Error = $crate::errors::AppError;

            fn try_from(value: Value) -> Result<Self, Self::Error> {
                match value {
                    Value::$name => Ok(Self),
                    _ => Err($crate::errors::AppError::WrongFieldType {
                        expected: $name ::get_type(),
                        got: value
                    })
                }
            }
        }

        impl From< $name > for Value {
            fn from(_value: $name) -> Self {
                Value::$name
            }
        }
    };
    { $name:ident , $type:ty } => {
        #[derive(Default, std::fmt::Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name ( pub $type );

        impl TagLike for $name {
            fn get_type() -> Type {
                Type::$name
            }
        }

        impl FieldLike< $type > for $name {}

        impl TryFrom<Value> for $name {
            type Error = $crate::errors::AppError;

            fn try_from(value: Value) -> Result<Self, Self::Error> {
                match value {
                    Value::$name (x) => Ok(Self(x.clone())),
                    _ => Err($crate::errors::AppError::WrongFieldType {
                        expected: $name ::get_type(),
                        got: value
                    })
                }
            }
        }

        impl From< $name > for Value {
            fn from(value: $name) -> Self {
                Value::$name (value.0)
            }
        }

        impl From< $name > for $type {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl From< $type > for $name {
            fn from(value: $type) -> Self {
                $name (value)
            }
        }

        impl std::ops::Deref for $name {
            type Target = $type;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        paste::paste! {
            impl Value {
                pub fn [<$name:lower>](x: $type) -> Self {
                    Self::from($name (x))
                }

                pub fn [<as_ $name:lower _opt>] (&self) -> Option<& $type > {
                    match self {
                        Self::$name (x) => Some(x),
                        _ => None
                    }
                }

                pub fn [<as_ $name:lower>] (&self) -> Result<& $type , AppError> {
                    self.[<as_ $name:lower _opt>]().ok_or_else(|| AppError::WrongFieldType {
                        expected: $name ::get_type(),
                        got: self.clone()
                    })
                }
            }
        }
    }
}

macro_rules! define_kinds {
    {
        $(
            $( #[display( $display:literal )] )?
            $( #[alias ( $alias:literal )] )?
            $name:ident $( ( $type:ty ) )?
        ),*
    } => {

        #[derive(
            std::fmt::Debug,
            derive_more::Display,
            Copy,
            Clone,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize
        )]
        pub enum Type {
            $(
                $( #[display($display)] )?
                $( #[serde(alias = $alias )] )?
                $name ,
            )*
        }

        impl Type {
            pub const fn all() -> &'static [Type] {
                &[
                    $(
                        Self:: $name ,
                    )*
                ]
            }
        }

        #[derive(std::fmt::Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum Value {
            $(
                $( #[serde(alias = $alias )] )?
                $name $( ( $type ) )? ,
            )*
        }

        impl Value {
            pub fn get_type(&self) -> Type {
                match self {
                    $(
                        Self:: $name $(
                          (_) if std::any::TypeId::of::< $type >()
                            != std::any::TypeId::of::<()>()
                        )? => Type:: $name ,
                    )*
                    _ => unreachable!()
                }
            }
        }

        $(
            impl_kind!( $name $( , $type )? );
        )*
    };
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Ord,
    PartialOrd,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SerialColour(pub(crate) [u8; 3]);

impl SerialColour {
    pub fn r(self) -> u8 {
        self.0[0]
    }
    pub fn g(self) -> u8 {
        self.0[1]
    }
    pub fn b(self) -> u8 {
        self.0[2]
    }
    pub fn as_slice(&self) -> &[u8; 3] {
        &self.0
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8; 3] {
        &mut self.0
    }
}

impl Display for SerialColour {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:2x}{:2x}{:2x}", self.r(), self.g(), self.b())
    }
}

impl From<SerialColour> for egui::Color32 {
    fn from(value: SerialColour) -> Self {
        egui::Color32::from_rgb(value.r(), value.g(), value.b())
    }
}

impl From<egui::Color32> for SerialColour {
    fn from(value: egui::Color32) -> Self {
        Self([value.r(), value.g(), value.b()])
    }
}

impl From<SerialColour> for [u8; 3] {
    fn from(value: SerialColour) -> Self {
        [value.r(), value.g(), value.b()]
    }
}

impl From<[u8; 3]> for SerialColour {
    fn from(value: [u8; 3]) -> Self {
        Self(value)
    }
}

pub mod kind {
    use super::SerialColour;
    use crate::data::Utf32CachedString;
    use crate::errors::AppError;
    use itertools::Itertools;
    use std::any::TypeId;
    use std::str::FromStr;

    pub trait TagLike:
        std::fmt::Debug + Default + Clone + serde::Serialize + TryFrom<Value> + Into<Value>
    {
        fn get_type() -> Type;
    }

    pub trait FieldLike<T>: TagLike + From<T> + Into<T> + std::ops::Deref<Target = T> {}

    define_kinds! {
        Tag,

        Container,

        Boolean(bool),

        #[display("Integer")]
        #[alias("UInt")]
        Int(i64),

        #[display("Floating Point Decimal")]
        Float(ordered_float::OrderedFloat<f64>),

        #[display("String")]
        #[alias("Str")]
        String(Utf32CachedString),

        #[display("Item Reference")]
        ItemRef((Utf32CachedString, Utf32CachedString)),

        List(Vec<Value>),

        Colour(SerialColour),

        Dictionary(Vec<(Utf32CachedString, Value)>),

        #[display("Date and Time")]
        DateTime(chrono::DateTime<chrono::Utc>)
    }

    impl FromStr for ItemRef {
        type Err = ();

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s.splitn(2, ':').collect_tuple() {
                Some((vault, path)) => {
                    Ok(Self((vault.to_string().into(), path.to_string().into())))
                }
                None => Err(()),
            }
        }
    }

    impl Value {
        pub fn as_str_opt(&self) -> Option<&str> {
            self.as_string_opt().map(|s| s.as_str())
        }

        pub fn as_str(&self) -> Result<&str, AppError> {
            self.as_str_opt().ok_or_else(|| AppError::WrongFieldType {
                expected: Type::String,
                got: self.clone(),
            })
        }
    }

    impl Default for Type {
        fn default() -> Self {
            Self::Tag
        }
    }
}

pub type Value = kind::Value;
pub type Type = kind::Type;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Copy)]
pub struct KnownField<T: kind::TagLike> {
    pub id: Uuid,
    pub name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T: kind::TagLike> KnownField<T> {
    pub const fn new(id: Uuid, name: &'static str) -> KnownField<T> {
        KnownField {
            id,
            name,
            _phantom: PhantomData,
        }
    }
}
