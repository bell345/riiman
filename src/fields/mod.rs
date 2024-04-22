use std::sync::OnceLock;

use crate::data::FieldDefinition;

#[macro_export]
macro_rules! field_def {
    { $name:ident , $type:ident , $id:literal } => {
        paste::paste! {
            FieldDefinition::new(
                [<$name:upper>],
                stringify!($name).into(),
                FieldType::$type
            )
        }
    };
    { $name:ident , $type:ident , $id:literal , $parent:ident } => {
        paste::paste! {
            FieldDefinition::new_child(
                [<$name:upper>],
                stringify!($name).into(),
                FieldType::$type,
                [<$parent:upper>]
            )
        }
    }
}

#[macro_export]
macro_rules! field_defs {
    { $ns_name:ident : $ns_id:literal { $( $name:ident : ( $type:ident , $id:literal ) ),* } } => {
        #[allow(unused_imports)]
        use $crate::data::{FieldDefinition, FieldType};
        use std::sync::OnceLock;
        #[allow(unused_imports)]
        use uuid::{uuid, Uuid};

        paste::paste! {
            const [<$ns_name:upper>]: Uuid = uuid!($ns_id);
            $(
                const [<$name:upper>]: Uuid = uuid!($id);
            )*

            #[allow(dead_code)]
            pub fn defs() -> Vec<&'static FieldDefinition> {
                static DEFS: OnceLock<Vec<FieldDefinition>> = OnceLock::new();
                DEFS.get_or_init(|| vec![
                    field_def!{ $ns_name , Dictionary , $ns_id },
                    $(
                        field_def!{ $name , $type , $id , [<$ns_name:upper>] },
                    )*
                ]).iter().collect()
            }
        }
    };
}

mod image;
mod general;

pub fn defs() -> &'static Vec<&'static FieldDefinition> {
    static DEFS: OnceLock<Vec<&'static FieldDefinition>> = OnceLock::new();
    DEFS.get_or_init(|| [
        general::defs(),
        image::defs()
    ].concat())
}
