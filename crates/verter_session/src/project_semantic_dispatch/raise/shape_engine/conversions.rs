//! Leaf graph→`TypeExpr` conversions used by the shared
//! [`fold_node`](super::fold_node): the semantic primitive-kind and mapped-modifier
//! mappings. Split from the parent for file-size; the fold that calls them lives in
//! `shape_engine`.

use verter_type_expr::{MappedModifier, PrimitiveName};

use crate::semantic_query::{OptionalityMod, ReadonlyMod};

pub(super) fn semantic_primitive_to_primitive_name(
    kind: crate::semantic_query::PrimitiveKind,
) -> PrimitiveName {
    use crate::semantic_query::PrimitiveKind as K;
    match kind {
        K::String => PrimitiveName::String,
        K::Number => PrimitiveName::Number,
        K::Boolean => PrimitiveName::Boolean,
        K::Symbol => PrimitiveName::Symbol,
        K::BigInt => PrimitiveName::BigInt,
        K::Any => PrimitiveName::Any,
        K::Unknown => PrimitiveName::Unknown,
        K::Void => PrimitiveName::Void,
        K::Never => PrimitiveName::Never,
        K::Null => PrimitiveName::Null,
        K::Undefined => PrimitiveName::Undefined,
        K::Object => PrimitiveName::Object,
    }
}

pub(super) fn mapped_modifier_for_optionality(opt: OptionalityMod) -> MappedModifier {
    match opt {
        OptionalityMod::Add => MappedModifier::Add,
        OptionalityMod::Remove => MappedModifier::Remove,
        OptionalityMod::Keep => MappedModifier::None,
    }
}

pub(super) fn mapped_modifier_for_readonly(readonly: ReadonlyMod) -> MappedModifier {
    match readonly {
        ReadonlyMod::Add => MappedModifier::Add,
        ReadonlyMod::Remove => MappedModifier::Remove,
        ReadonlyMod::Keep => MappedModifier::None,
    }
}
