//! Augmentation population fact keys and publication content identity.

use crate::resolution::augmentation_key::AugmentationTargetKind;
use std::sync::Arc;

/// One augmentation population's source identities, in canonical contributor
/// order. Whole content, exact syntax identity, runtime language and the actual
/// materialization environment are all inputs; the artifact-key environment
/// slot is a base sentinel or overlay discriminator, not that environment.
pub fn augmentation_population_content_fingerprint<'a>(
    sources: impl IntoIterator<
        Item = (
            &'a crate::source::artifact_key::FileArtifactKey,
            crate::analysis::types::Hash16,
        ),
    >,
) -> crate::analysis::types::Hash16 {
    use std::hash::{Hash, Hasher};
    let mut digest = xxhash_rust::xxh3::Xxh3::new();
    digest.write(b"verter:augmentation-population-content:v1");
    let count = sources
        .into_iter()
        .fold(0u64, |count, (key, source_parse_env_hash)| {
            key.canonical.hash(&mut digest);
            key.content_hash.hash(&mut digest);
            key.parse_key.hash(&mut digest);
            key.file_language_id.hash(&mut digest);
            source_parse_env_hash.hash(&mut digest);
            count + 1
        });
    digest.write_u64(count);
    digest.digest128().to_le_bytes()
}

/// Join the two immutable publications consumed by one stitch. A read combines
/// fixed-size identities; the population owners have already hashed sources.
pub fn join_augmentation_population_content(
    augmenters: crate::analysis::types::Hash16,
    contributors: crate::analysis::types::Hash16,
) -> crate::analysis::types::Hash16 {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(&augmenters);
    bytes[16..].copy_from_slice(&contributors);
    xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes()
}

/// The content-free identity of the population a stitch reads.
pub fn build_augmentation_population_content_fact_key(
    target: &AugmentationTargetKind,
    decl_name: &str,
) -> crate::facts::FactKey {
    let crate::facts::FactKey::ModuleAugmentationIndexShape {
        target_kind_tag,
        external_specifier,
        resolved_relative_canonical,
        wildcard_pattern,
    } = build_module_augmentation_index_shape_fact_key(target)
    else {
        unreachable!("an augmentation shape builder returns its shape key")
    };
    crate::facts::FactKey::AugmentationPopulationContent {
        target_kind_tag,
        external_specifier,
        resolved_relative_canonical,
        wildcard_pattern,
        decl_name: crate::facts::registry::InternedName::from(decl_name),
    }
}

/// Build the route-surface `FactKey::ModuleAugmentationIndexShape`
/// payload an augmentation-index consumer observes for the queried
/// target. The parallel optional
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
