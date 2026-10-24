//! Owned module augmentation facts and the predicate that matches one against an
//! augmentation target.
use crate::facts::registry::InternedName;
use crate::facts::registry::InternedSpecifier;
use crate::facts::registry::SymbolSpace;
use crate::inputs::contributors::FileModuleKind;
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
/// - `declaring_file` — whether the file holding the block is a module or a
///   script. In a module the block augments the module its specifier names;
///   in a script it declares an ambient module of that name instead.
#[derive(Debug, Clone, verter_retention::heap_size::HeapSize)]
pub struct ModuleAugmentationFact {
    pub specifier: InternedSpecifier,
    pub owner: TopLevelOwnerId,
    pub augmented_name: InternedName,
    pub space: SymbolSpace,
    pub augmented_member_shape_fingerprint: Hash16,
    pub declaring_file: FileModuleKind,
}

/// Special marker the parse-domain emission uses for `declare global
/// { ... }` blocks (see `fact_emission::GLOBAL_AUGMENTATION_TAG`).
/// Duplicated here to keep the matcher free-standing of fact_emission.
pub const GLOBAL_AUGMENTATION_TAG: &str = "$global";

/// Whether a `declare module` block augments the module FILE its specifier
/// resolves to from the augmenter. A relative specifier does; a package
/// specifier does only when the declaring file is a module, as the checker
/// merges a module augmentation into the module its name resolves to while
/// a script's `declare module "pkg"` declares an ambient module, not an
/// augmentation. A wildcard pattern and the global tag never do.
#[must_use]
pub fn fact_augments_resolved_module(fact: &ModuleAugmentationFact) -> bool {
    let specifier: &str = fact.specifier.as_ref();
    !specifier.contains('*')
        && specifier != GLOBAL_AUGMENTATION_TAG
        && (crate::resolution::is_relative_specifier(specifier)
            || fact.declaring_file == FileModuleKind::Module)
}

/// Classify an owned augmentation fact against a target. The module file the
/// fact's specifier resolves to from the augmenter is supplied by the request
/// owner for a [`AugmentationTargetKind::ResolvedRelativeCanonical`] target;
/// this predicate performs no source work.
pub fn augmenter_matches_target(
    fact: &ModuleAugmentationFact,
    target_key: &AugmentationTargetKey,
    resolved_module_file: Option<&str>,
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
            if !fact_augments_resolved_module(fact) {
                return false;
            }
            match resolved_module_file {
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
