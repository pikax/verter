//! Structural metadata for a JS-facing object union, and the macros that
//! declare one.
//!
//! A JS-facing union is an enum whose every arm crosses a binding boundary
//! as one plain object: either told apart by a tag property (`kind`,
//! `framework`) or, for an untagged union, by which of its slots holds
//! `null`. The published TypeScript declaration of such a union is
//! RENDERED from the same declaration list the binding serialises or
//! decodes, so an arm cannot exist on one side without the other.
//!
//! Each binding owns the macro that emits its own (de)serialisation —
//! serde here, through [`serialized_js_union`], and the native adapter's
//! own conversion in `verter_napi`. All of them hand their declaration list
//! to [`js_union_ts`], which derives the metadata the renderer reads and
//! the [`ts_rs::TS`] implementation that names the union inside every other
//! projected declaration and reaches the types its arms carry.

/// Re-exported so the exported macros resolve `ts-rs` through this crate
/// whatever their caller depends on.
#[doc(hidden)]
pub use ts_rs as __ts_rs;

/// The shape an arm of a union carries beside its tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmShape {
    /// A newtype arm: the tag object intersected with a named payload type.
    Payload(String),
    /// A struct arm's own fields. Empty means the arm is the tag alone.
    Fields(Vec<ArmField>),
}

/// One named field of a struct arm, with its projected TypeScript type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmField {
    /// The wire key, which is the Rust field identifier. The two are one
    /// value, not two that agree: the macros read the field through
    /// `stringify!` — the same identifier the serialisation reads — and
    /// their grammar admits no attribute on a variant field, so a rename
    /// that could separate them has nowhere to be written.
    pub name: &'static str,
    /// The projected TypeScript type, resolved through `ts_rs`.
    pub ts_type: String,
}

/// One arm of a union: its published name, its tag value, its shape and
/// its documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedArm {
    /// The published TypeScript name of this arm. Arms that share a name
    /// and a shape publish as ONE declaration whose tag is the union of
    /// their tag values.
    pub ts_name: &'static str,
    /// The tag value that selects this arm, or `None` for an arm of an
    /// untagged union.
    pub tag_value: Option<&'static str>,
    /// Documentation lines, in source order.
    pub docs: &'static [&'static str],
    /// What the arm carries beside its tag.
    pub shape: ArmShape,
}

/// A JS-facing union: the arms plus the key that discriminates them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedUnion {
    /// The published TypeScript name of the union.
    pub ts_name: &'static str,
    /// The property every arm carries to discriminate itself, or `None`
    /// when the arms are told apart by which of their slots is `null`.
    pub tag_key: Option<&'static str>,
    /// Documentation lines, in source order.
    pub docs: &'static [&'static str],
    /// The arms, in declaration order.
    pub arms: Vec<TaggedArm>,
}

/// Derives a union's projection from its declaration list: the
/// `ts_union` metadata the renderer reads, and a [`ts_rs::TS`]
/// implementation under the union's published name.
///
/// The `TS` implementation names the union where another projected
/// declaration carries it (`products: Array<HostCompiledProduct>`) and
/// visits every type its arms carry, so the renderer reaches them through
/// the union the same way it reaches a derived declaration's fields. It
/// has no declaration of its own: the renderer publishes the union from
/// its metadata, and a union it was not told to render fails the render
/// rather than vanishing from it.
#[doc(hidden)]
#[macro_export]
macro_rules! js_union_ts {
    (
        $Name:ident [$($tag_key:literal)?] as $union_ts:literal docs [$($union_doc:literal),*] {
            $(
                $body:tt => [$($tag_value:literal)?] as $arm_ts:literal docs [$($arm_doc:literal),*]
            ),* $(,)?
        }
    ) => {
        impl $Name {
            /// The structural metadata the TypeScript projection renders
            /// from, in declaration order.
            pub fn ts_union(
                cfg: &$crate::js_union::__ts_rs::Config,
            ) -> $crate::js_union::TaggedUnion {
                $crate::js_union::TaggedUnion {
                    ts_name: $union_ts,
                    tag_key: $crate::__js_union_opt!($($tag_key)?),
                    docs: &[$($union_doc),*],
                    arms: ::std::vec![$(
                        $crate::js_union::TaggedArm {
                            ts_name: $arm_ts,
                            tag_value: $crate::__js_union_opt!($($tag_value)?),
                            docs: &[$($arm_doc),*],
                            shape: $crate::__js_union_shape!(cfg, $body),
                        }
                    ),*],
                }
            }
        }

        impl $crate::js_union::__ts_rs::TS for $Name {
            type WithoutGenerics = Self;
            type OptionInnerType = Self;

            fn name(_: &$crate::js_union::__ts_rs::Config) -> ::std::string::String {
                ::std::string::String::from($union_ts)
            }

            fn inline(cfg: &$crate::js_union::__ts_rs::Config) -> ::std::string::String {
                <Self as $crate::js_union::__ts_rs::TS>::name(cfg)
            }

            fn visit_dependencies(v: &mut impl $crate::js_union::__ts_rs::TypeVisitor)
            where
                Self: 'static,
            {
                $( $crate::__js_union_visit!(v, $body); )*
            }

            fn output_path() -> ::std::option::Option<::std::path::PathBuf> {
                ::std::option::Option::Some(::std::path::PathBuf::from(::core::concat!(
                    $union_ts, ".ts"
                )))
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_union_opt {
    () => {
        ::std::option::Option::None
    };
    ($value:literal) => {
        ::std::option::Option::Some($value)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_union_shape {
    ($cfg:ident, ( $payload:ty )) => {
        $crate::js_union::ArmShape::Payload(
            <$payload as $crate::js_union::__ts_rs::TS>::name($cfg),
        )
    };
    ($cfg:ident, { $($field:ident : $ty:ty),* $(,)? }) => {
        $crate::js_union::ArmShape::Fields(::std::vec![$(
            $crate::js_union::ArmField {
                name: ::core::stringify!($field),
                ts_type: <$ty as $crate::js_union::__ts_rs::TS>::name($cfg),
            }
        ),*])
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __js_union_visit {
    ($v:ident, ( $payload:ty )) => {
        $v.visit::<$payload>();
        <$payload as $crate::js_union::__ts_rs::TS>::visit_generics($v);
    };
    ($v:ident, { $($field:ident : $ty:ty),* $(,)? }) => {
        $(
            $v.visit::<$ty>();
            <$ty as $crate::js_union::__ts_rs::TS>::visit_generics($v);
        )*
    };
}

/// Declares a union the browser binding SERIALISES: the serde enum and its
/// projection come out of one declaration list, so an arm cannot exist
/// without a row and a row cannot name an arm that does not exist.
///
/// Arms are a newtype over a payload whose fields serde flattens beside the
/// tag, or a struct arm with named fields. The declared tag literal is
/// serde's tag: it is emitted as a per-variant `rename`, and the container
/// carries no `rename_all` to derive a competing spelling from the variant
/// identifier. A field's wire key is its identifier, so a camelCase key is
/// written as one.
#[macro_export]
macro_rules! serialized_js_union {
    (
        $(#[doc = $union_doc:literal])*
        $vis:vis enum $Name:ident tagged $tag_key:literal as $union_ts:literal {
            $(
                $(#[doc = $arm_doc:literal])*
                $variant:ident $body:tt => $tag_value:literal as $arm_ts:literal
            ),* $(,)?
        }
    ) => {
        #[derive(::serde::Serialize)]
        #[serde(tag = $tag_key)]
        #[allow(non_snake_case)]
        $(#[doc = $union_doc])*
        $vis enum $Name {
            $(
                $(#[doc = $arm_doc])*
                #[serde(rename = $tag_value)]
                $variant $body,
            )*
        }

        $crate::js_union_ts! {
            $Name [$tag_key] as $union_ts docs [$($union_doc),*] {
                $( $body => [$tag_value] as $arm_ts docs [$($arm_doc),*] ),*
            }
        }
    };
}

/// Names a type whose TypeScript declaration is written elsewhere — the
/// shared host types a projection refers to without restating.
///
/// The named type has no declaration in the render that refers to it: the
/// renderer imports every name it references but does not declare.
#[macro_export]
macro_rules! ts_declared_elsewhere {
    ($($ty:ty => $name:literal),* $(,)?) => {$(
        impl $crate::js_union::__ts_rs::TS for $ty {
            type WithoutGenerics = Self;
            type OptionInnerType = Self;

            fn name(_: &$crate::js_union::__ts_rs::Config) -> ::std::string::String {
                ::std::string::String::from($name)
            }

            fn inline(cfg: &$crate::js_union::__ts_rs::Config) -> ::std::string::String {
                <Self as $crate::js_union::__ts_rs::TS>::name(cfg)
            }
        }
    )*};
}
