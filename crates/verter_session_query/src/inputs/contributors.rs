//! Global symbol contributors: module-kind classification, contributor entries and their
//! population fact keys.

use crate::analysis::types::Hash16;
use crate::facts::SymbolSpace;
use crate::{
    facts::registry::{InternedName, InternedSpecifier},
    resolution::augmentation_key::AugmentationTargetKind,
    source::artifact_key::FileArtifactKey,
};
use std::sync::Arc;
use verter_type_expr::TopLevelOwnerId;

/// TypeScript script vs external-module classification of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileModuleKind {
    /// No import/export syntax: top-level interfaces and namespaces
    /// contribute to the global object.
    Script,
    /// Has import or export syntax: only `declare global` / module
    /// augmentations contribute globally.
    Module,
}

/// How one file contributes one global (or ambient-module) symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContributorOrigin {
    DeclareGlobal,
    ModuleAugmentation,
    FileScopeInterface,
    /// Top-level `namespace N` in a script (or automatic lib). Lowered
    /// through the same retained-body path as file-scope interfaces.
    FileScopeNamespace,
    /// Top-level TYPE declaration other than an interface (`class`,
    /// `type`, `enum`) in a script: global by name, the one declaration
    /// of its type (no other file's declaration merges into it).
    FileScopeType,
    /// Top-level VALUE declaration (`var` / `let` / `const` / `function` /
    /// `class` / `enum`) in a script: global by name. An automatic lib's
    /// values are the lib environment's own and are not recorded.
    FileScopeValue,
}

/// One contributor in a published population. Precedence is applied at
/// lookup from the project's configured `files` sequence when present,
/// otherwise a stable canonical-path normalize of unordered discovery.
#[derive(Debug, Clone)]
pub struct ContributorEntry {
    pub artifact_key: FileArtifactKey,
    pub parse_stable_hash: Hash16,
    pub owner: TopLevelOwnerId,
    pub origin: ContributorOrigin,
    pub specifier: Option<InternedSpecifier>,
    pub symbol: InternedName,
    pub space: SymbolSpace,
    pub is_automatic_lib: bool,
    pub module_kind: FileModuleKind,
    pub contribution_fingerprint: Hash16,
}

/// Per-symbol contributor set plus its fingerprint (empty set included).
#[derive(Debug, Clone)]
pub struct SymbolContributors {
    pub entries: Arc<[ContributorEntry]>,
    pub fingerprint: Hash16,
}

impl SymbolContributors {
    pub fn empty() -> Self {
        Self {
            entries: Arc::from(Vec::new().into_boxed_slice()),
            fingerprint: fingerprint_of(&[]),
        }
    }
}

impl ContributorEntry {
    pub fn matches_overlay(&self, overlay_discriminator: Option<Hash16>) -> bool {
        match overlay_discriminator {
            None => self.artifact_key.is_base(),
            Some(discriminator) => {
                self.artifact_key.is_base() || self.artifact_key.parse_env_hash == discriminator
            }
        }
    }
}

pub fn fingerprint_of(entries: &[ContributorEntry]) -> Hash16 {
    use std::hash::{BuildHasher, Hasher};
    let salt_lo = rustc_hash::FxBuildHasher;
    let salt_hi = rustc_hash::FxBuildHasher;
    let mut h_lo = salt_lo.build_hasher();
    let mut h_hi = salt_hi.build_hasher();
    h_lo.write_u64(0xC4A1_C4A1_4A1C_4A1C);
    h_hi.write_u64(0x9E37_79B9_7F4A_7C15);
    for entry in entries {
        h_lo.write(entry.artifact_key.canonical.as_bytes());
        h_lo.write(&entry.contribution_fingerprint);
        h_lo.write_u8(entry.space.tag());
        h_lo.write_u8(entry.module_kind as u8);
        h_lo.write_u8(entry.origin as u8);
        h_hi.write(entry.artifact_key.canonical.as_bytes());
        h_hi.write(&entry.contribution_fingerprint);
        h_hi.write_u8(entry.space.tag());
        h_hi.write_u8(entry.module_kind as u8);
        h_hi.write_u8(entry.origin as u8);
    }
    let lo = h_lo.finish();
    let hi = h_hi.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&lo.to_le_bytes());
    out[8..].copy_from_slice(&hi.to_le_bytes());
    out
}

/// [`classify_module_kind`] over the retained shallow inventory alone.
#[must_use]
pub fn classify_shallow_module_kind(
    shallow: &crate::inputs::shallow::ShallowInputAssembly,
) -> FileModuleKind {
    if !shallow.exports.is_empty()
        || !shallow.wildcard_reexports.is_empty()
        || !shallow.import_targets.is_empty()
        || shallow.export_assignment_target().is_some()
    {
        return FileModuleKind::Module;
    }
    let routes = shallow.route_inventory.as_ref();
    if !routes.imports.is_empty()
        || !routes.bindingless_imports.is_empty()
        || !routes.reexports.is_empty()
        || !routes.wildcard_reexports.is_empty()
        || !routes.local_exports.is_empty()
        || !routes.export_assignments.is_empty()
        || routes.has_module_syntax
    {
        FileModuleKind::Module
    } else {
        FileModuleKind::Script
    }
}

/// Automatic library files are ambient-lib virtual ids (`ambient:/…`).
/// `noLib` disables these only; a user file whose basename happens to
/// look like `lib.*.d.ts` stays a program declaration.
#[must_use]
pub fn is_automatic_lib_canonical(canonical: &str) -> bool {
    canonical.starts_with("ambient:/")
}

/// Route-surface fact key for the per-symbol population fingerprint.
/// Extra optional fields carry `decl_name` so this is distinct from the
/// target-wide augmenter-set `ModuleAugmentationIndexShape` observation.
#[must_use]
pub fn population_contributor_fact_key(
    target: &AugmentationTargetKind,
    decl_name: &str,
) -> crate::facts::FactKey {
    use crate::facts::registry::{
        AugmentationTargetKindTag, InternedGlobPattern, InternedSpecifier,
    };
    use crate::facts::FactKey;
    match target {
        AugmentationTargetKind::GlobalAugmentation => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::GlobalAugmentation,
            external_specifier: Some(InternedSpecifier::from(decl_name)),
            resolved_relative_canonical: None,
            wildcard_pattern: None,
        },
        AugmentationTargetKind::ExternalSpecifier(spec) => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
            external_specifier: Some(spec.clone()),
            resolved_relative_canonical: None,
            wildcard_pattern: Some(InternedGlobPattern::from(decl_name)),
        },
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => {
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
                external_specifier: None,
                resolved_relative_canonical: Some(Arc::clone(canon)),
                wildcard_pattern: Some(InternedGlobPattern::from(decl_name)),
            }
        }
        AugmentationTargetKind::WildcardAmbient(pat) => FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::WildcardAmbient,
            external_specifier: Some(InternedSpecifier::from(decl_name)),
            resolved_relative_canonical: None,
            wildcard_pattern: Some(pat.clone()),
        },
    }
}
