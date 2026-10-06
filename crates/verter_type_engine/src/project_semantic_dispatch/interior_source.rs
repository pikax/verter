//! The interior position path a failed required dereference of a composed source
//! shell reports.

/// One step of the interior position path within a composed source shell —
/// the typed breadcrumb a failed REQUIRED interior dereference carries so
/// the output error names the exact nested position that failed. Produced
/// by the strict raise entry
/// (`ProjectSemanticDispatch::raise_semantic_type_source_to_hot_strict`);
/// defined here (next to the public output error that transports it) so the
/// public error surface stays fully nameable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteriorSourceStep {
    /// An exact authored object / surface / synthesized / leaf-object member
    /// key.
    Member(verter_type_expr::facts::FactAuthoredPropertyKey),
    /// A function parameter position (source order).
    Parameter { ordinal: u32 },
    /// The function return-type position.
    ReturnType,
    /// A type-parameter constraint position (source order).
    TypeParamConstraint { ordinal: u32 },
    /// A type-parameter default position (source order).
    TypeParamDefault { ordinal: u32 },
    /// A tuple element position (source order).
    TupleElement { ordinal: u32 },
    /// A closed leaf-union arm (source order).
    UnionArm { ordinal: u32 },
    /// An index-signature KEY position (declaration order).
    IndexSignatureKey { ordinal: u32 },
    /// An index-signature VALUE position (declaration order).
    IndexSignatureValue { ordinal: u32 },
    /// The object position of a path-precise indexed access.
    IndexedAccessObject,
    /// A call-signature position (declaration order).
    CallSignature { ordinal: u32 },
    /// A construct-signature position (declaration order).
    ConstructSignature { ordinal: u32 },
}

impl std::fmt::Display for InteriorSourceStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InteriorSourceStep::Member(key) => {
                let encoded = serde_json::to_string(key).map_err(|_| std::fmt::Error)?;
                write!(f, ".member[{encoded}]")
            }
            InteriorSourceStep::Parameter { ordinal } => write!(f, ".param[{ordinal}]"),
            InteriorSourceStep::ReturnType => write!(f, ".return"),
            InteriorSourceStep::TypeParamConstraint { ordinal } => {
                write!(f, ".typeParam[{ordinal}].constraint")
            }
            InteriorSourceStep::TypeParamDefault { ordinal } => {
                write!(f, ".typeParam[{ordinal}].default")
            }
            InteriorSourceStep::TupleElement { ordinal } => write!(f, ".tuple[{ordinal}]"),
            InteriorSourceStep::UnionArm { ordinal } => write!(f, ".unionArm[{ordinal}]"),
            InteriorSourceStep::IndexSignatureKey { ordinal } => {
                write!(f, ".indexSignature[{ordinal}].key")
            }
            InteriorSourceStep::IndexSignatureValue { ordinal } => {
                write!(f, ".indexSignature[{ordinal}].value")
            }
            InteriorSourceStep::IndexedAccessObject => write!(f, ".indexedAccessObject"),
            InteriorSourceStep::CallSignature { ordinal } => write!(f, ".callSignature[{ordinal}]"),
            InteriorSourceStep::ConstructSignature { ordinal } => {
                write!(f, ".constructSignature[{ordinal}]")
            }
        }
    }
}
