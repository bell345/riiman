use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::data::FieldStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub id: Uuid,
    pub name: String,
    pub field_type: kind::KindType,
    parents: HashSet<Uuid>,
    children: HashSet<Uuid>,
    fields: DashMap<Uuid, FieldValue>,
}

impl FieldDefinition {
    pub fn known<T: kind::TagKind>(known_field: KnownField<T>) -> Self {
        Self {
            id: known_field.id,
            name: known_field.name.to_string(),
            field_type: T::get_type(),
            ..Default::default()
        }
    }

    pub fn new(id: Uuid, name: String, field_type: kind::KindType) -> Self {
        Self {
            id,
            name,
            field_type,
            ..Default::default()
        }
    }

    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.add_parent(parent_id);
        self
    }

    pub fn iter_parent_ids(&self) -> impl Iterator<Item = impl Deref<Target = Uuid> + '_> {
        self.parents.iter()
    }

    pub fn iter_child_ids(&self) -> impl Iterator<Item = impl Deref<Target = Uuid> + '_> {
        self.children.iter()
    }

    pub fn add_parent(&mut self, parent_id: Uuid) {
        self.parents.insert(parent_id);
    }

    pub fn add_child(&mut self, child_id: Uuid) {
        self.children.insert(child_id);
    }
}

impl FieldStore for FieldDefinition {
    fn fields(&self) -> &DashMap<Uuid, FieldValue> {
        &self.fields
    }
}

macro_rules! impl_kind {
    { $name:ident } => {
        #[derive(Default, std::fmt::Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name ;

        impl TagKind for $name {
            fn get_type() -> KindType {
                KindType::$name
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
        pub struct $name ( $type );

        impl TagKind for $name {
            fn get_type() -> KindType {
                KindType::$name
            }
        }

        impl FieldKind< $type > for $name {}

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
    }
}

macro_rules! define_kinds {
    {
        $(
            $( #[display( $display:literal )] )?
            $name:ident $( ( $type:ty ) )?
        ),*
    } => {

        #[derive(
            std::fmt::Debug,
            derive_more::Display,
            Clone,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize
        )]
        pub enum KindType {
            $(
                $( #[display(fmt = $display )] )?
                $name ,
            )*
        }

        #[derive(std::fmt::Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum Value {
            $( $name $( ( $type ) )? , )*
        }

        $(
            impl_kind!( $name $( , $type )? );
        )*
    };
}

pub mod kind {
    pub trait TagKind:
        std::fmt::Debug + Default + Clone + serde::Serialize + TryFrom<Value> + Into<Value>
    {
        fn get_type() -> KindType;
    }

    pub trait FieldKind<T>: TagKind + From<T> + Into<T> + std::ops::Deref<Target = T> {}

    define_kinds! {
        Tag,

        Boolean(bool),

        #[display("Signed Integer")]
        Int(i64),

        #[display("Unsigned Integer")]
        UInt(u64),

        #[display("Floating Point Decimal")]
        Float(ordered_float::OrderedFloat<f64>),

        #[display("String")]
        Str(String),

        #[display("Item Reference")]
        ItemRef(String),

        List(Vec<Value>),

        Colour([u8; 3]),

        Dictionary(Vec<(String, Value)>),

        #[display("Date and Time")]
        DateTime(chrono::DateTime<chrono::Utc>)
    }

    impl Value {
        pub fn as_str(&self) -> Option<&str> {
            match self {
                Value::Str(x) => Some(x.as_str()),
                Value::ItemRef(x) => Some(x.as_str()),
                _ => None,
            }
        }
    }

    impl Default for KindType {
        fn default() -> Self {
            Self::Tag
        }
    }
}

pub type FieldValue = kind::Value;
pub type FieldType = kind::KindType;

pub struct KnownField<T: kind::TagKind> {
    pub id: Uuid,
    pub name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T: kind::TagKind> KnownField<T> {
    pub const fn new(id: Uuid, name: &'static str) -> KnownField<T> {
        KnownField {
            id,
            name,
            _phantom: PhantomData,
        }
    }
}
