//! Name and ordinal facts of a signature-scoped type-parameter list.

use std::sync::Arc;
use verter_type_expr::facts::{NarrowTypeParam, TypeParamVariance};
use verter_type_expr::TypeParam;

/// Narrow a SIGNATURE-scoped type-parameter list (a function declaration's /
/// method's own `<T extends C>` list) to name + ordinal facts. Signature-scoped
/// bounds live ON the signature's authored position: the closed path vocabulary
/// addresses type-parameter bounds only on TYPE-space declaration headers
/// (a value / method signature's bound is recovered whole-signature when the
/// signature position is demanded), so no independent bound slot exists to
/// mint — deliberately NOT a fabricated locator.
pub fn narrow_signature_type_params(params: &[TypeParam]) -> Arc<[NarrowTypeParam]> {
    params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ordinal = u32::try_from(index).ok()?;
            Some(NarrowTypeParam {
                name: param.name.clone(),
                ordinal,
                // `TypeParamBound` is a type-space DECL-HEADER first-step-only
                // position — not addressable for a signature-scoped parameter.
                // Honest typed miss: an authored `extends` / `=` bound here is
                // recovered whole-signature on demand, never through a fabricated
                // slot.
                constraint: None,
                default: None,
                is_const: param.is_const,
                variance: TypeParamVariance::Unannotated,
            })
        })
        .collect()
}
