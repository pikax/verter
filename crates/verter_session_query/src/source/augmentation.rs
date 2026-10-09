//! Owned module augmentation facts and the predicate that matches one against an
//! augmentation target.
use crate::facts::registry::InternedName;
use crate::facts::registry::InternedSpecifier;
use crate::facts::registry::SymbolSpace;
use crate::resolution::AugmentationTargetKey;
use crate::resolution::AugmentationTargetKind;

use crate::analysis::types::Hash16;
use verter_type_expr::TopLevelOwnerId;

/// A single `declare module "<specifier>" { ... }` block emitted by the
/// parser during shallow analysis.
///
/// The type is defined here; the shallow walk populates it.
/// Augmenting declarations are stitched into the consumer's merged
/// declaration surface for that specifier.
///
/// Fields:
///
/// - `specifier` — the syntactic specifier inside `declare module "X" {}`.
/// - `owner` — the lexical top-level owner that authored the declaration.
/// - `augmented_name` — the name of an augmented binding inside the block.
/// - `space` — which symbol space the augmented binding occupies.
/// - `augmented_member_shape_fingerprint` — alpha-normalised fingerprint
///   over the augmenting block's member set; used to detect
///   when an augmenter's contribution to the effective surface changes
///   without changing the augmenter set itself.
#[derive(Debug, Clone)]
pub struct ModuleAugmentationFact {
    pub specifier: InternedSpecifier,
    pub owner: TopLevelOwnerId,
    pub augmented_name: InternedName,
    pub space: SymbolSpace,
    pub augmented_member_shape_fingerprint: Hash16,
}

/// Special marker the parse-domain emission uses for `declare global
/// { ... }` blocks (see `fact_emission::GLOBAL_AUGMENTATION_TAG`).
/// Duplicated here to keep the matcher free-standing of fact_emission.
pub const GLOBAL_AUGMENTATION_TAG: &str = "$global";

/// Classify an owned augmentation fact against a target. Relative resolution is
/// supplied by the request owner; this predicate performs no source work.
pub fn augmenter_matches_target(
    fact: &ModuleAugmentationFact,
    target_key: &AugmentationTargetKey,
    resolved_relative_canonical: Option<&str>,
) -> bool {
    use crate::resolution::is_relative_specifier;
    let specifier: &str = fact.specifier.as_ref();
    match &target_key.target {
        AugmentationTargetKind::ExternalSpecifier(target_spec) => {
            // Bare external: not relative, not wildcard, not global.
            let is_relative = is_relative_specifier(specifier);
            let is_wildcard = specifier.contains('*');
            let is_global = specifier == GLOBAL_AUGMENTATION_TAG;
            !is_relative && !is_wildcard && !is_global && specifier == target_spec.as_ref()
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(target_canon) => {
            if !is_relative_specifier(specifier) {
                return false;
            }
            match resolved_relative_canonical {
                Some(resolved) => resolved == target_canon.as_ref(),
                None => false,
            }
        }
        AugmentationTargetKind::WildcardAmbient(target_pattern) => {
            specifier.contains('*') && specifier == target_pattern.as_ref()
        }
        AugmentationTargetKind::GlobalAugmentation => specifier == GLOBAL_AUGMENTATION_TAG,
    }
}
