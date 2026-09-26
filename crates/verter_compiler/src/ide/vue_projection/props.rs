//! Caller prop checks and default-resolved setup contracts.
//!
//! TypeScript remains the type-answer owner. This module supplies the
//! projection facts that decide which authored contributions are checked and
//! which keys a spread may omit because a later write certainly replaces it.

use crate::framework_common::projection_plan::ComponentUseId;
use crate::ide::vue_projection::attribute_operations::{
    AttributeOperationsProjection, AttributeSyntax, Certainty,
};
use crate::ide::vue_projection::public_constructor::{
    DeclaredSurface, PropsDefaults, VuePublicConstructorContract,
};

/// Policy used for an object `v-bind` contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadCertaintyPolicy {
    /// A finite key set is checked against the component's declared props.
    /// Keys proven overwritten by a later definite write are excluded from
    /// that check.
    CheckKnownKeys,
    /// An index-signature spread is framework-legal and cannot be made exact
    /// without inventing a type answer.
    PreserveOpenDomain,
}

/// One caller-side prop check TypeScript must perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropCheckObligation {
    /// Component use that owns the check.
    pub use_id: ComponentUseId,
    /// Authored `v-bind` operation that contributes the spread.
    pub op_index: u32,
    /// Keys whose values cannot reach the final component props because a
    /// later definite write replaces them.
    pub overwritten_keys: Vec<String>,
    /// Certainty policy supplied to the generated TypeScript helper.
    pub policy: SpreadCertaintyPolicy,
}

impl PropCheckObligation {
    /// Whether this obligation validates finite, statically knowable keys.
    #[must_use]
    pub fn checks_known_keys(&self) -> bool {
        self.policy == SpreadCertaintyPolicy::CheckKnownKeys
    }
}

/// Caller optionality and setup-side default resolution are distinct facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallerAndSetupPropsContract {
    /// Type-declared keys callers may omit because `withDefaults` supplies a
    /// statically named default.
    pub caller_optional_keys: Vec<String>,
    /// The same keys are defined inside setup after Vue resolves defaults.
    pub setup_defined_keys: Vec<String>,
    /// False when the defaults object has an open key set; that shape is
    /// runtime-valid but cannot prove any individual caller optionality.
    pub defaults_are_static: bool,
}

/// Prop-check products of an admitted projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropsProjection {
    /// False when attribute facts were incomplete. Incomplete products never
    /// warm a complete cache entry.
    pub complete: bool,
    /// Spread checks in authored order.
    pub obligations: Vec<PropCheckObligation>,
}

/// Derive caller prop-check obligations from the runtime property plan.
#[must_use]
pub fn project_props(attributes: &AttributeOperationsProjection) -> PropsProjection {
    let mut obligations = Vec::new();
    for (sequence, key_plan) in attributes.sequences.iter().zip(&attributes.key_plans) {
        for operation in &sequence.operations {
            if operation.syntax != AttributeSyntax::BindObject {
                continue;
            }
            let overwritten_keys = key_plan
                .effective
                .iter()
                .filter(|property| property.overridden.contains(&operation.index))
                .filter(|property| {
                    property
                        .contributors
                        .iter()
                        .all(|contribution| contribution.certainty != Certainty::Possible)
                })
                .map(|property| property.key.clone())
                .collect();
            obligations.push(PropCheckObligation {
                use_id: sequence.use_id.clone(),
                op_index: operation.index,
                overwritten_keys,
                policy: SpreadCertaintyPolicy::CheckKnownKeys,
            });
        }
    }
    PropsProjection {
        complete: attributes.complete,
        obligations,
    }
}

/// Derive the caller/setup split from the public constructor's macro facts.
#[must_use]
pub fn caller_and_setup_props(
    contract: &VuePublicConstructorContract,
) -> CallerAndSetupPropsContract {
    let static_defaults = matches!(contract.props, DeclaredSurface::TypeArgument { .. });
    let keys = match &contract.props_defaults {
        Some(PropsDefaults {
            keys: Some(keys), ..
        }) if static_defaults => keys.clone(),
        _ => Vec::new(),
    };
    CallerAndSetupPropsContract {
        caller_optional_keys: keys.clone(),
        setup_defined_keys: keys,
        defaults_are_static: contract
            .props_defaults
            .as_ref()
            .is_none_or(|defaults| defaults.keys.is_some()),
    }
}
