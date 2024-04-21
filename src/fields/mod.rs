use std::sync::OnceLock;

use crate::data::FieldDefinition;

#[macro_export]
macro_rules! field_defs {
    { $( $name:ident : ( $type:ident , $id:literal ) ),* } => {
        #[allow(unused_imports)]
        use $crate::data::{FieldDefinition, FieldType};
        use std::sync::OnceLock;
        #[allow(unused_imports)]
        use uuid::{uuid, Uuid};

        paste::paste! {
            $(
                const [<$name:upper>]: Uuid = uuid!($id);
            )*

            #[allow(dead_code)]
            pub fn defs() -> Vec<&'static FieldDefinition> {
                static DEFS: OnceLock<Vec<FieldDefinition>> = OnceLock::new();
                DEFS.get_or_init(|| vec![
                    $(
                        FieldDefinition::new(
                            [<$name:upper>],
                            stringify!($name).into(),
                            FieldType::$type
                        ),
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
