use verter_type_expr::{MappedModifier, PrimitiveName};

pub const MEMBER_PROPERTY: u32 = 1;
pub const MEMBER_INDEX_SIGNATURE: u32 = 2;
pub const MEMBER_CALL_SIGNATURE: u32 = 3;
pub const MEMBER_CONSTRUCT_SIGNATURE: u32 = 4;
pub const MEMBER_METHOD: u32 = 5;

pub fn primitive_to_tag(name: PrimitiveName) -> u32 {
    match name {
        PrimitiveName::String => 1,
        PrimitiveName::Number => 2,
        PrimitiveName::Boolean => 3,
        PrimitiveName::Symbol => 4,
        PrimitiveName::BigInt => 5,
        PrimitiveName::Any => 6,
        PrimitiveName::Unknown => 7,
        PrimitiveName::Void => 8,
        PrimitiveName::Never => 9,
        PrimitiveName::Null => 10,
        PrimitiveName::Undefined => 11,
        PrimitiveName::Object => 12,
    }
}

pub fn mapped_modifier_to_tag(modifier: MappedModifier) -> u32 {
    match modifier {
        MappedModifier::None => 1,
        MappedModifier::Add => 2,
        MappedModifier::Remove => 3,
    }
}
