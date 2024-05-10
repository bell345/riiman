#[macro_export]
macro_rules! field_def {
    { $name:ident } => {
        paste::paste! {
            FieldDefinition::known([<$name:upper>])
        }
    };
    { $name:ident , $parent:ident } => {
        paste::paste! {
            FieldDefinition::known([<$name:upper>])
                .with_parent($parent.id)
        }
    }
}

#[macro_export]
macro_rules! field_defs {
    {
        $(
            #[id( $ns_id:literal )]
            $ns_name:ident {
                $(
                    #[id( $id:literal )]
                    $name:ident : $type:ident
                ),*
            }
        ),*
    } => {
        #[allow(unused_imports)]
        use $crate::data::{FieldDefinition, FieldType, KnownField};
        use std::sync::OnceLock;
        #[allow(unused_imports)]
        use uuid::{uuid, Uuid};

        paste::paste! {
            $(
                pub mod $ns_name {
                    #[allow(unused_imports)]
                    use $crate::data::{FieldDefinition, FieldType, KnownField};
                    use $crate::data::kind;
                    use std::sync::OnceLock;
                    #[allow(unused_imports)]
                    use uuid::{uuid, Uuid};

                    pub const NAMESPACE: KnownField<kind::Dictionary> =
                        KnownField::<kind::Dictionary>::new(
                            uuid!($ns_id), stringify!($ns_name)
                        );
                    $(
                        pub const [<$name:upper>]: KnownField<kind::$type> =
                            KnownField::<kind::$type>::new(
                                uuid!($id), stringify!($name)
                            );
                    )*

                    #[allow(dead_code)]
                    pub fn defs() -> Vec<&'static FieldDefinition> {
                        static DEFS: OnceLock<Vec<FieldDefinition>> = OnceLock::new();
                        DEFS.get_or_init(|| vec![
                            FieldDefinition::known(NAMESPACE),
                            $(
                                field_def!{ $name , NAMESPACE },
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
        media_type: Str,
        #[id("2ee79ce5-9ddc-4115-8f95-e7c028cf495f")]
        last_modified: DateTime
    },
    #[id("49b61dab-ce73-4ac9-ac3a-fb20f928e1e3")]
    meta {
        #[id("5ea86c5a-1458-4977-97b5-bc03bce0354b")]
        colour: Colour,
        #[id("be17c008-c9ba-4691-8e15-44bf76a28a8b")]
        aliases: List
    }
}
