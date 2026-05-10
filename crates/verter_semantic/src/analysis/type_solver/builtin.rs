//! Built-in TypeScript utility type registry.
//!
//! Owns the `BuiltinUtility` enum and its name/arity/intrinsic
//! metadata. Utility expansion logic itself lives on the shared
//! semantic dispatch layer
//! (`verter_session::project_semantic_dispatch`); callers such as
//! `IntrinsicRegistry`, `component_meta_query_engine`, and dispatch
//! lower only need to classify whether a name is a recognized utility.

// ---------------------------------------------------------------------------
// Built-in utility registry
// ---------------------------------------------------------------------------

/// Recognized built-in TypeScript utility types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinUtility {
    // -- Object utilities --
    Partial,
    Required,
    Readonly,
    Pick,
    Omit,
    Record,

    // -- Union/extraction utilities --
    Extract,
    Exclude,
    NonNullable,

    // -- Function utilities --
    ReturnType,
    Parameters,
    ConstructorParameters,
    InstanceType,

    // -- Promise utilities --
    Awaited,

    // -- Compiler string intrinsics (not shadowable) --
    Uppercase,
    Lowercase,
    Capitalize,
    Uncapitalize,

    // -- Inference utilities --
    NoInfer,
}

impl BuiltinUtility {
    /// Look up a utility by name. Returns `None` for non-utility names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Partial" => Some(Self::Partial),
            "Required" => Some(Self::Required),
            "Readonly" => Some(Self::Readonly),
            "Pick" => Some(Self::Pick),
            "Omit" => Some(Self::Omit),
            "Record" => Some(Self::Record),
            "Extract" => Some(Self::Extract),
            "Exclude" => Some(Self::Exclude),
            "NonNullable" => Some(Self::NonNullable),
            "ReturnType" => Some(Self::ReturnType),
            "Parameters" => Some(Self::Parameters),
            "ConstructorParameters" => Some(Self::ConstructorParameters),
            "InstanceType" => Some(Self::InstanceType),
            "Awaited" => Some(Self::Awaited),
            "Uppercase" => Some(Self::Uppercase),
            "Lowercase" => Some(Self::Lowercase),
            "Capitalize" => Some(Self::Capitalize),
            "Uncapitalize" => Some(Self::Uncapitalize),
            "NoInfer" => Some(Self::NoInfer),
            _ => None,
        }
    }

    /// Whether this is a compiler intrinsic that cannot be shadowed by user code.
    pub fn is_compiler_intrinsic(self) -> bool {
        matches!(
            self,
            Self::Uppercase | Self::Lowercase | Self::Capitalize | Self::Uncapitalize
        )
    }

    /// Expected number of type arguments for this utility.
    pub fn expected_arity(self) -> usize {
        match self {
            Self::Partial
            | Self::Required
            | Self::Readonly
            | Self::NonNullable
            | Self::ReturnType
            | Self::Parameters
            | Self::ConstructorParameters
            | Self::InstanceType
            | Self::Awaited
            | Self::Uppercase
            | Self::Lowercase
            | Self::Capitalize
            | Self::Uncapitalize
            | Self::NoInfer => 1,
            Self::Pick | Self::Omit | Self::Record | Self::Extract | Self::Exclude => 2,
        }
    }

    /// The name of this utility as a string.
    pub fn name(self) -> &'static str {
        match self {
            Self::Partial => "Partial",
            Self::Required => "Required",
            Self::Readonly => "Readonly",
            Self::Pick => "Pick",
            Self::Omit => "Omit",
            Self::Record => "Record",
            Self::Extract => "Extract",
            Self::Exclude => "Exclude",
            Self::NonNullable => "NonNullable",
            Self::ReturnType => "ReturnType",
            Self::Parameters => "Parameters",
            Self::ConstructorParameters => "ConstructorParameters",
            Self::InstanceType => "InstanceType",
            Self::Awaited => "Awaited",
            Self::Uppercase => "Uppercase",
            Self::Lowercase => "Lowercase",
            Self::Capitalize => "Capitalize",
            Self::Uncapitalize => "Uncapitalize",
            Self::NoInfer => "NoInfer",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_recognizes_all_utilities() {
        let names = [
            "Partial",
            "Required",
            "Readonly",
            "Pick",
            "Omit",
            "Record",
            "Extract",
            "Exclude",
            "NonNullable",
            "ReturnType",
            "Parameters",
            "ConstructorParameters",
            "InstanceType",
            "Awaited",
            "Uppercase",
            "Lowercase",
            "Capitalize",
            "Uncapitalize",
            "NoInfer",
        ];
        for name in names {
            let util = BuiltinUtility::from_name(name)
                .unwrap_or_else(|| panic!("{name} should be a recognized utility"));
            assert_eq!(util.name(), name);
        }
    }

    #[test]
    fn from_name_rejects_unknown_names() {
        assert!(BuiltinUtility::from_name("NotAUtility").is_none());
        assert!(BuiltinUtility::from_name("partial").is_none()); // case sensitive
        assert!(BuiltinUtility::from_name("").is_none());
    }

    #[test]
    fn compiler_intrinsics_are_not_shadowable() {
        assert!(BuiltinUtility::Uppercase.is_compiler_intrinsic());
        assert!(BuiltinUtility::Lowercase.is_compiler_intrinsic());
        assert!(BuiltinUtility::Capitalize.is_compiler_intrinsic());
        assert!(BuiltinUtility::Uncapitalize.is_compiler_intrinsic());
        assert!(!BuiltinUtility::Partial.is_compiler_intrinsic());
        assert!(!BuiltinUtility::NoInfer.is_compiler_intrinsic());
    }

    #[test]
    fn expected_arity_is_correct() {
        assert_eq!(BuiltinUtility::Partial.expected_arity(), 1);
        assert_eq!(BuiltinUtility::Pick.expected_arity(), 2);
        assert_eq!(BuiltinUtility::Omit.expected_arity(), 2);
        assert_eq!(BuiltinUtility::Record.expected_arity(), 2);
        assert_eq!(BuiltinUtility::NoInfer.expected_arity(), 1);
    }
}
