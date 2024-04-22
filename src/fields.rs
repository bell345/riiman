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
    { $( #[id( $ns_id:literal )] $ns_name:ident { $( #[id( $id:literal )] $name:ident : $type:ident ),* } ),* } => {
        #[allow(unused_imports)]
        use $crate::data::{FieldDefinition, FieldType};
        use std::sync::OnceLock;
        #[allow(unused_imports)]
        use uuid::{uuid, Uuid};

        paste::paste! {
            $(
                pub mod $ns_name {
                    #[allow(unused_imports)]
                    use $crate::data::{FieldDefinition, FieldType};
                    use std::sync::OnceLock;
                    #[allow(unused_imports)]
                    use uuid::{uuid, Uuid};

                    pub const NAMESPACE: Uuid = uuid!($ns_id);
                    $(
                        pub const [<$name:upper>]: Uuid = uuid!($id);
                    )*

                    #[allow(dead_code)]
                    pub fn defs() -> Vec<&'static FieldDefinition> {
                        static DEFS: OnceLock<Vec<FieldDefinition>> = OnceLock::new();
                        DEFS.get_or_init(|| vec![
                            FieldDefinition::new(
                                NAMESPACE,
                                stringify!($ns_name).into(),
                                FieldType::Dictionary
                            ),
                            $(
                                field_def!{ $name , $type , $id , NAMESPACE },
                            )*
                        ]).iter().collect()
                    }
                }
            )*

            pub fn defs() -> &'static Vec<&'static FieldDefinition> {
                static DEFS: OnceLock<Vec<&'static FieldDefinition>> = OnceLock::new();
                DEFS.get_or_init(|| [
                    $(
                        $ns_name::defs(),
                    )*
                ].concat())
            }
        }
    };
}

field_defs! {
    #[id("20f98f4b-0fb0-4bf2-a633-755e47a8fec7")]
    image {
        #[id("07f4527f-9cec-4310-8d32-ee820bd7f87e")]
        width: UInt,
        #[id("dabb8289-62e3-47b8-bfea-90891cdaf858")]
        height: UInt
    },
    #[id("59589bd3-f9b9-49c1-9969-1d3714fa68db")]
    general {
        #[id("cd1bbe33-c7b0-49a8-a3c4-901ca3ea01fd")]
        media_type: String
    }
}
