//! The fact key of a module augmentation index's shape.

use crate::resolution::augmentation_key::AugmentationTargetKind;
use std::sync::Arc;

/// Build the parse-domain `FactKey::ModuleAugmentationIndexShape`
/// payload an augmentation-index consumer observes for the queried
/// target — the sole `RouteSurface` fact shape. The parallel optional
/// fields hold the concrete target value; the `target_kind_tag`
/// discriminates.
pub fn build_module_augmentation_index_shape_fact_key(
    target: &AugmentationTargetKind,
) -> crate::facts::FactKey {
    use crate::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => {
            crate::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(spec.clone()),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => {
            crate::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
                external_specifier: None,
                resolved_relative_canonical: Some(Arc::clone(canon)),
                wildcard_pattern: None,
            }
        }
        AugmentationTargetKind::WildcardAmbient(pat) => {
            crate::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::WildcardAmbient,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: Some(pat.clone()),
            }
        }
        AugmentationTargetKind::GlobalAugmentation => {
            crate::facts::FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
                external_specifier: None,
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            }
        }
    }
}
