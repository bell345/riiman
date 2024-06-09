#[macro_export]
macro_rules! field_def {
    { $name:ident } => {
        paste::paste! {
            FieldDefinition::known(&[<$name:upper>])
        }
    };
    { $name:ident , $parent:ident } => {
        paste::paste! {
            FieldDefinition::known(&[<$name:upper>])
                .with_parent($parent.id)
        }
    };
    { $name:ident , $parent:ident , $( $tag_ns:ident :: $tag:ident ),+ } => {
        paste::paste! {
            FieldDefinition::known(&[<$name:upper>])
                .with_parent($parent.id)
            $(
                .with_tag_id(super::$tag_ns::[<$tag:upper>].id)
            )+
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
                    $( #[tag( $tag_ns:ident :: $tag:ident )] )*
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

                    pub const NAMESPACE: KnownField<kind::Container> =
                        KnownField::<kind::Container>::new(
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
                            FieldDefinition::known(&NAMESPACE),
                            $(
                                field_def!{ $name , NAMESPACE $(,  $tag_ns :: $tag ),* },
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
        #[tag(meta::no_link)]
        width: UInt,
        #[id("dabb8289-62e3-47b8-bfea-90891cdaf858")]
        #[tag(meta::no_link)]
        height: UInt
    },
    #[id("59589bd3-f9b9-49c1-9969-1d3714fa68db")]
    general {
        #[id("cd1bbe33-c7b0-49a8-a3c4-901ca3ea01fd")]
        #[tag(meta::no_link)]
        media_type: String,
        #[id("2ee79ce5-9ddc-4115-8f95-e7c028cf495f")]
        #[tag(meta::no_link)]
        last_modified: DateTime,
        #[id("26b71e5f-6397-479d-ae85-bab8a47c0ab4")]
        #[tag(meta::no_link)]
        sidecar_last_updated: DateTime,
        #[id("7bb22e14-0ae3-481a-a85b-b5b826384297")]
        #[tag(meta::no_link)]
        link: ItemRef
    },
    #[id("49b61dab-ce73-4ac9-ac3a-fb20f928e1e3")]
    meta {
        #[id("5ea86c5a-1458-4977-97b5-bc03bce0354b")]
        colour: Colour,
        #[id("be17c008-c9ba-4691-8e15-44bf76a28a8b")]
        aliases: List,
        #[id("df82a9fb-6afe-4fad-8a1d-35067cc6a409")]
        no_link: Tag
    },
    #[id("f194b5a3-d623-4a28-a91a-c6ed6affff53")]
    tweet {
        #[id("b3515371-db8e-42e5-9f78-a55dfb682be1")]
        id: UInt,
        #[id("dd8e96dd-375a-48a5-b1b7-7b5aecfdbc53")]
        content: String,
        #[id("49d87b9c-bf8c-4560-a745-eaa64b7af965")]
        hashtags: List,
        #[id("f0c1b538-53b5-408a-bf90-c9d169f4f13d")]
        author_id: UInt,
        #[id("3daf7d4d-a889-41c7-8ad2-91e4f1cd0a1d")]
        author_handle: String,
        #[id("50297d00-681d-4351-bce1-5fa64dbea128")]
        author_name: String,
        #[id("4e2153e5-df69-45f4-8e6a-acb1040eecec")]
        post_date: DateTime,
        #[id("f1e6c489-e023-4945-bba2-85467b373100")]
        liked_date: DateTime
    }
}
