//! Fact registry types — the parse-domain / resolve-domain fact schema
//! that backs `verter_session::file_artifact_store::FileFacts`.
//!
//! The fact-based cache architecture (see
//! `.claude/skills/type-cache-architecture/SKILL.md`) splits cache
//! validation into per-fact observations. Each fact carries a stable
//! [`FactKey`] (R10), a `SymbolSpace` (R11), and `semantic_hash`
//! plus `display_hash` (R13). Cache entries record exactly the facts
//! they read, and read-side validators check `current(fact) ==
//! recorded(fact)` rather than running an eager cascade invalidator.
//!
//! Three orthogonal fact domains exist (see [`FactDomain`]):
//!
//! - **Parse-file** (`ParseFile`): syntactic, parse-env keyed,
//!   `content_hash`-derived (`parse_stable_hash` for semantic_hash,
//!   `content_hash` for display_hash). Emitted at parse time by the
//!   shallow walk in `verter_session`.
//! - **Resolve-imports** (`ResolveImports`): one-step import resolution
//!   facts, resolve-env keyed. Populated by the resolver producer
//!   downstream — the variants are defined here so the substrate is
//!   closed.
//! - **Route-surface** (`RouteSurface`): post-wildcard, post-augmentation
//!   effective export surface facts, resolve_env + lib_env keyed.
//!   Populated by the `RouteDb` producer.
//!
//! Variant taxonomy: parse-time producers populate the parse-domain
//! variants eagerly. The lazy member-body producers populate
//! `Member` semantic / display facts on first member-access query.
//! Resolve-domain variants stay UNPOPULATED until the resolver wires
//! into the fact graph downstream.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::TopLevelOwnerId;

/// Truncated SHA-256 hash used everywhere as a content / structural
/// fingerprint. Re-exported from
/// [`verter_semantic::analysis::types::Hash16`].
pub type FactHash = crate::analysis::Hash16;

/// An interned module-specifier string (e.g. `"vue"`, `"./local"`,
/// `"*.css"`).
///
/// Wraps an `Arc<str>` so the type is movable into data structures
/// without back-references; later stages can swap in a crate-wide
/// interner if profiling shows hot-path duplication.
///
/// This type lives in `verter_semantic` so the fact registry can
/// reference it without taking a back-edge on `verter_session`.
/// `verter_session::file_artifact_store` re-imports it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct InternedSpecifier(pub Arc<str>);

impl From<&str> for InternedSpecifier {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<String> for InternedSpecifier {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl AsRef<str> for InternedSpecifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An interned symbol name (export name, member name, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct InternedName(pub Arc<str>);

impl From<&str> for InternedName {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<String> for InternedName {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl AsRef<str> for InternedName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An interned wildcard pattern (e.g. `"*.css"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct InternedGlobPattern(pub Arc<str>);

impl From<&str> for InternedGlobPattern {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<String> for InternedGlobPattern {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl AsRef<str> for InternedGlobPattern {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The symbol-space namespace a fact identity carries.
///
/// **`BothTypeValue` is forbidden (R11).** A `class Foo` declaration
/// occupies both `Type` and `Value` and MUST emit two distinct facts —
/// one keyed on `(Foo, Type)`, one on `(Foo, Value)`. Adding the
/// `Namespace` variant covers TypeScript `namespace X { ... }` blocks
/// where the declaration also introduces a name in the namespace.
///
/// This is the canonical 3-variant `SymbolSpace` referenced by the
/// fact-based cache architecture. The legacy 2-variant
/// `verter_session::resolver_core::route_demand::SymbolSpace` covers
/// resolve-only call paths that have not yet migrated; mixing the two
/// is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolSpace {
    Type,
    Value,
    Namespace,
}

impl SymbolSpace {
    /// Stable byte tag for serialisation into a structural hash.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Type => 0x01,
            Self::Value => 0x02,
            Self::Namespace => 0x03,
        }
    }
}

/// Member kind discriminator carried by `MemberPresence` and
/// `MemberShape` facts.
///
/// Used by the path-precise consumer (`Pick<Foo, "a">`,
/// `Foo['a']['b']`) to invalidate on header changes that affect
/// the consumer's view, e.g. a property switching from optional to
/// required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MemberKind {
    /// Object property: `{ a: T }` or `{ a?: T }`.
    Property { readonly: bool, optional: bool },
    /// Method signature: `{ a(): T }`.
    Method,
    /// Getter / setter signature on an interface or class.
    Accessor,
    /// Index signature: `{ [k: string]: T }`.
    Index,
    /// Call signature.
    Call,
    /// Construct signature.
    Construct,
    /// Class field — distinct from a plain property because of
    /// `static`, `readonly`, `abstract` modifiers.
    ClassField {
        static_: bool,
        readonly: bool,
        abstract_: bool,
    },
    /// Enum member.
    EnumMember,
}

impl MemberKind {
    /// Stable byte tag for serialisation into a structural hash.
    /// Cosmetic-invariant; never includes display strings.
    #[must_use]
    pub fn tag(self) -> [u8; 4] {
        match self {
            Self::Property { readonly, optional } => {
                [0x10, u8::from(readonly), u8::from(optional), 0x00]
            }
            Self::Method => [0x11, 0, 0, 0],
            Self::Accessor => [0x12, 0, 0, 0],
            Self::Index => [0x13, 0, 0, 0],
            Self::Call => [0x14, 0, 0, 0],
            Self::Construct => [0x15, 0, 0, 0],
            Self::ClassField {
                static_,
                readonly,
                abstract_,
            } => [
                0x16,
                u8::from(static_),
                u8::from(readonly),
                u8::from(abstract_),
            ],
            Self::EnumMember => [0x17, 0, 0, 0],
        }
    }
}

/// Domain a fact belongs to (R12). Domain routes `StoreView`
/// validator dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FactDomain {
    /// Parse-file facts, parse_env_hash keyed, ZERO resolution data.
    /// Producer: the parse-time shallow walk.
    ParseFile,
    /// Resolved-import facts, resolve_env_hash keyed, no lib_env.
    /// Producer: the resolver substrate.
    ResolveImports,
    /// Route-surface facts, resolve_env_hash + lib_env_hash keyed.
    /// Producer: `RouteDb` (post-augmentation-stitched).
    RouteSurface,
}

/// Macro target identifier for `MacroSurface` facts (R28).
///
/// A `<script setup>` block can host multiple macro invocations.
/// The combination of `MacroKind` + `MacroTargetKey` discriminates
/// distinct invocations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MacroTargetKey {
    /// Stable index within the file's macro instance list, in lexical
    /// order. Restart at 0 per `(MacroKind, file)`.
    pub instance: u32,
}

/// Stable identifier for a single fact. Equal facts have equal
/// `FactKey`s; cosmetic edits never change a key.
///
/// `FactKey` is `Hash + Eq + Clone`, suitable as a `FxHashMap` key.
///
/// **Reordering a file does not change `FactKey`s (R10).** Adding
/// a binding adds a new `FactKey`; removing one drops an existing
/// `FactKey`; editing one keeps the `FactKey` but bumps
/// `Fact::semantic_hash` and/or `Fact::display_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FactKey {
    // ────────────────────────────────────────────────────────────────
    // Parse-domain (FileFacts; parse_env_hash keyed)
    // ────────────────────────────────────────────────────────────────
    /// One exported binding: `export type Foo = ...`, `export const x = ...`.
    /// Distinguished by `SymbolSpace`; `class Foo` produces two facts.
    Export {
        name: InternedName,
        space: SymbolSpace,
    },
    /// `export { Foo as Bar }` — separate from `Export(Foo)` because
    /// the alias has its own observable identity.
    ExportAlias {
        exported_as: InternedName,
        space: SymbolSpace,
    },
    /// Whole-file export set fingerprint. Adding/removing an export
    /// shifts this fact; renaming one shifts it too.
    SyntacticExportSet,
    /// One locally-declared, NOT-exported binding inside the file.
    /// Path-precise consumers observe these by name.
    LocalDecl {
        name: InternedName,
        space: SymbolSpace,
    },
    /// Member body fingerprint — lazy, computed once per
    /// `(canonical, parse_stable_hash, exporter, name, space)`.
    Member {
        exporter: InternedName,
        name: InternedName,
        space: SymbolSpace,
    },
    /// Member header (name + kind + exporter salt). Eager parse-time.
    /// Body fingerprint is NOT included (R28); adding sibling
    /// `b` does not force re-walking `a`'s body.
    MemberPresence {
        exporter: InternedName,
        name: InternedName,
        space: SymbolSpace,
    },
    /// Ordered fingerprint over the exporter's full member name +
    /// kind list. Used by whole-surface projections (`keyof Foo`,
    /// `Omit<Foo, "a">`, mapped, `Required<Foo>`).
    MemberShape {
        exporter: InternedName,
        space: SymbolSpace,
    },
    /// One macro invocation surface — captures the parsed type
    /// argument fingerprint + runtime-args shape.
    MacroSurface {
        kind: MacroKind,
        target: MacroTargetKey,
    },
    /// Vue template root reachability fact — captures the root list
    /// shape used by fallthrough / root inheritance.
    TemplateRoot,
    /// One import statement: `import X from "spec"` →
    /// `ImportRef { specifier: "spec", binding: "X", space }`.
    /// **No `resolved_canonical` (R12)** — that's a resolve-domain
    /// fact.
    ImportRef {
        specifier: InternedSpecifier,
        binding: InternedName,
        space: SymbolSpace,
    },
    /// One re-export specifier: `export { Foo as Bar } from "spec"`.
    /// Parse-domain — no resolution recorded.
    SyntacticReexportRef {
        specifier: InternedSpecifier,
        source_name: InternedName,
        target_name: InternedName,
        space: SymbolSpace,
    },
    /// `declare module "spec" { ... }` augmenting declaration (R29).
    ModuleAugmentation {
        specifier: InternedSpecifier,
        owner: TopLevelOwnerId,
        augmented_name: InternedName,
        space: SymbolSpace,
    },

    // ────────────────────────────────────────────────────────────────
    // Resolve-imports domain (ResolvedImportFacts; resolve_env_hash
    // keyed). The resolver producer populates these; the variants are
    // defined here so the substrate is closed.
    // ────────────────────────────────────────────────────────────────
    ResolvedImportClause {
        specifier: InternedSpecifier,
        binding: InternedName,
        space: SymbolSpace,
        resolved_canonical: Arc<str>,
        resolved_source_name: InternedName,
    },
    ResolvedReexportBinding {
        specifier: InternedSpecifier,
        source_name: InternedName,
        target_name: InternedName,
        space: SymbolSpace,
        resolved_canonical: Arc<str>,
        resolved_source_name: InternedName,
    },

    // ────────────────────────────────────────────────────────────────
    // Route-surface domain (RouteDb-owned; resolve_env_hash +
    // lib_env_hash keyed). The route-surface producer populates these.
    // ────────────────────────────────────────────────────────────────
    /// Effective post-augmentation, post-wildcard export surface
    /// fingerprint.
    EffectiveExportSet,
    /// Augmenter-set membership fingerprint per augmentation target.
    /// Observed by `EffectiveExportSet(specifier)` consumers (R29).
    ModuleAugmentationIndexShape {
        target_kind_tag: AugmentationTargetKindTag,
        external_specifier: Option<InternedSpecifier>,
        resolved_relative_canonical: Option<Arc<str>>,
        wildcard_pattern: Option<InternedGlobPattern>,
    },
}

/// Tag-only representation of
/// `verter_session::file_artifact_store::AugmentationTargetKind`. The
/// concrete target value lives in the parallel optional fields of
/// `FactKey::ModuleAugmentationIndexShape` — keeping the discriminant
/// here keeps `FactKey` `Hash + Eq` without dragging in a
/// `Vec<(...)>` per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AugmentationTargetKindTag {
    ExternalSpecifier,
    ResolvedRelativeCanonical,
    WildcardAmbient,
    GlobalAugmentation,
}

/// `defineProps` / `defineEmits` / etc. macro kind, used by
/// `FactKey::MacroSurface`.
///
/// Inlined from `verter_semantic::analysis::template::MacroKind` to
/// avoid leaking that domain's serialisation contract into the fact
/// registry. The variants must remain in lock-step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MacroKind {
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineSlots,
    DefineExpose,
    DefineOptions,
    WithDefaults,
}

impl From<crate::analysis::MacroKind> for MacroKind {
    fn from(value: crate::analysis::MacroKind) -> Self {
        use crate::analysis::MacroKind as TemplateMacroKind;
        match value {
            TemplateMacroKind::DefineProps => Self::DefineProps,
            TemplateMacroKind::DefineEmits => Self::DefineEmits,
            TemplateMacroKind::DefineModel => Self::DefineModel,
            TemplateMacroKind::DefineSlots => Self::DefineSlots,
            TemplateMacroKind::DefineExpose => Self::DefineExpose,
            TemplateMacroKind::DefineOptions => Self::DefineOptions,
            TemplateMacroKind::WithDefaults => Self::WithDefaults,
        }
    }
}

impl FactKey {
    /// Route this fact key to its domain (R12).
    ///
    /// Used by `StoreView`'s per-domain validator dispatch (R26):
    /// `validates(fact)` looks at `fact.key.domain()` and routes to
    /// `validates_parse_domain`, `validates_resolve_imports_domain`,
    /// or `validates_route_surface_domain`.
    #[must_use]
    pub fn domain(&self) -> FactDomain {
        match self {
            Self::Export { .. }
            | Self::ExportAlias { .. }
            | Self::SyntacticExportSet
            | Self::LocalDecl { .. }
            | Self::Member { .. }
            | Self::MemberPresence { .. }
            | Self::MemberShape { .. }
            | Self::MacroSurface { .. }
            | Self::TemplateRoot
            | Self::ImportRef { .. }
            | Self::SyntacticReexportRef { .. }
            | Self::ModuleAugmentation { .. } => FactDomain::ParseFile,

            Self::ResolvedImportClause { .. } | Self::ResolvedReexportBinding { .. } => {
                FactDomain::ResolveImports
            }

            Self::EffectiveExportSet | Self::ModuleAugmentationIndexShape { .. } => {
                FactDomain::RouteSurface
            }
        }
    }
}

/// A single fact entry — `(semantic_hash, display_hash)` pair per
/// `FactKey` (R13).
///
/// Semantic vs display split is physical at the cache layer:
/// `MemberSemanticFactStore` is keyed on `parse_stable_hash` so
/// cosmetic edits do NOT churn it; `MemberDisplayFactStore` is keyed
/// on `content_hash` so cosmetic edits recompute display facts only.
/// At the per-file `FileFacts` level (parse-domain), both fields are
/// stored together because parse-time producers compute both in
/// one pass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fact {
    pub key: FactKey,
    /// Alpha-normalised structural fingerprint. Cosmetic edits
    /// (whitespace, comments, JSDoc, generic param rename, decl
    /// reorder) MUST NOT change `semantic_hash` (R16).
    pub semantic_hash: FactHash,
    /// Display fingerprint. Same input bytes as `semantic_hash` plus
    /// any cosmetic data the display layer needs (JSDoc, identifier
    /// display strings, comment text).
    pub display_hash: FactHash,
}

/// Which lane (`Semantic` or `Display`) a consumer observed a fact
/// under. Recorded in `fact_dep_signature` per-observation so the
/// validator knows whether a cosmetic-only edit invalidates the
/// consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FactLane {
    Semantic,
    Display,
}

/// One entry in a consumer's `fact_dep_signature` — the read
/// observation that the cache layer validates on warm hit.
///
/// `(canonical, key, lane, expected_hash)` tuples are sorted +
/// deduped on signature finalisation. Validation walks the sorted
/// list and short-circuits on the first miss.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObservedFact {
    pub canonical: Arc<str>,
    pub key: FactKey,
    pub lane: FactLane,
    pub expected_hash: FactHash,
}

/// Per-file fact registry — the parse-domain authoritative store.
///
/// Populated by the parse-time shallow walk (O(file_size)).
/// Lazy member-body facts (`Member.semantic_hash`,
/// `Member.display_hash`) live in two SEPARATE host-owned stores
/// keyed differently so a cosmetic edit hits only the display
/// store.
///
/// **Lookup cost contract.** All `FactKey::domain() == ParseFile`
/// lookups MUST be O(1). The registry is backed by an `FxHashMap`.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FactRegistry {
    /// Parse-domain facts. Key uniqueness is guaranteed by R10:
    /// `FactKey` is stable across file edits that preserve the
    /// declaration identity.
    pub facts: FxHashMap<FactKey, Fact>,
    /// Cached `SyntacticExportSet` fact for quick whole-file
    /// surface inspection. Equal to `facts.get(&FactKey::SyntacticExportSet)`.
    pub syntactic_export_set: Option<Fact>,
}

impl FactRegistry {
    /// Empty registry — used for placeholder construction and
    /// inputs that bypass the parse-time emitter.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of parse-domain facts stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// `true` if no facts have been emitted yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Look up a fact by key. `None` means "fact not present in this
    /// registry version" — for consumers, that's an invalidation
    /// observation (the binding was removed or never existed).
    #[must_use]
    pub fn get(&self, key: &FactKey) -> Option<&Fact> {
        self.facts.get(key)
    }

    /// Insert (or overwrite) a fact.
    pub fn insert(&mut self, fact: Fact) {
        if matches!(fact.key, FactKey::SyntacticExportSet) {
            self.syntactic_export_set = Some(fact.clone());
        }
        self.facts.insert(fact.key.clone(), fact);
    }

    /// Iterator over `(key, fact)` pairs. Order is undefined.
    pub fn iter(&self) -> impl Iterator<Item = (&FactKey, &Fact)> {
        self.facts.iter()
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn dummy_hash(b: u8) -> FactHash {
        let mut h = [0u8; 16];
        h[0] = b;
        h
    }

    fn fact(key: FactKey, sem: u8, disp: u8) -> Fact {
        Fact {
            key,
            semantic_hash: dummy_hash(sem),
            display_hash: dummy_hash(disp),
        }
    }

    #[test]
    fn empty_registry_round_trip() {
        let r = FactRegistry::empty();
        assert!(r.is_empty(), "empty must be empty");
        assert_eq!(r.len(), 0);
        assert!(
            r.get(&FactKey::SyntacticExportSet).is_none(),
            "empty registry returns None for any key — missing fact = invalidation observation"
        );
    }

    #[test]
    fn insert_and_get_round_trip() {
        let mut r = FactRegistry::empty();
        let key = FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        };
        r.insert(fact(key.clone(), 1, 2));
        assert_eq!(r.len(), 1);
        let got = r.get(&key).expect("must exist");
        assert_eq!(got.semantic_hash[0], 1);
        assert_eq!(got.display_hash[0], 2);
    }

    #[test]
    fn type_and_value_namespace_keys_are_distinct() {
        // R11: a `class Foo` declaration emits two facts; they MUST
        // NOT collide.
        let mut r = FactRegistry::empty();
        let key_type = FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        };
        let key_value = FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Value,
        };
        r.insert(fact(key_type.clone(), 1, 1));
        r.insert(fact(key_value.clone(), 9, 9));
        assert_eq!(r.len(), 2, "type + value occupy distinct keys");
        assert_eq!(r.get(&key_type).unwrap().semantic_hash[0], 1);
        assert_eq!(r.get(&key_value).unwrap().semantic_hash[0], 9);
    }

    #[test]
    fn module_augmentation_keys_are_partitioned_by_lexical_owner() {
        let mut registry = FactRegistry::empty();
        let key_for = |owner| FactKey::ModuleAugmentation {
            specifier: InternedSpecifier::from("vue"),
            owner,
            augmented_name: InternedName::from("Shared"),
            space: SymbolSpace::Type,
        };
        let module = key_for(TopLevelOwnerId::module(0));
        let instance = key_for(TopLevelOwnerId::instance(0));

        registry.insert(fact(module.clone(), 1, 1));
        registry.insert(fact(instance.clone(), 2, 2));

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.get(&module).unwrap().semantic_hash[0], 1);
        assert_eq!(registry.get(&instance).unwrap().semantic_hash[0], 2);
    }

    #[test]
    fn syntactic_export_set_cache_is_kept_in_sync() {
        let mut r = FactRegistry::empty();
        assert!(r.syntactic_export_set.is_none());
        r.insert(fact(FactKey::SyntacticExportSet, 7, 7));
        let cached = r.syntactic_export_set.as_ref().expect("must be cached");
        assert_eq!(cached.semantic_hash[0], 7);
    }

    #[test]
    fn fact_key_domain_routes_correctly() {
        // R12 / R26 — every variant routes to exactly one of the
        // three domains.
        let parse_keys = [
            FactKey::Export {
                name: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::ExportAlias {
                exported_as: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::SyntacticExportSet,
            FactKey::LocalDecl {
                name: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::Member {
                exporter: InternedName::from("X"),
                name: InternedName::from("a"),
                space: SymbolSpace::Type,
            },
            FactKey::MemberPresence {
                exporter: InternedName::from("X"),
                name: InternedName::from("a"),
                space: SymbolSpace::Type,
            },
            FactKey::MemberShape {
                exporter: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::MacroSurface {
                kind: MacroKind::DefineProps,
                target: MacroTargetKey { instance: 0 },
            },
            FactKey::TemplateRoot,
            FactKey::ImportRef {
                specifier: InternedSpecifier::from("./x"),
                binding: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::SyntacticReexportRef {
                specifier: InternedSpecifier::from("./x"),
                source_name: InternedName::from("X"),
                target_name: InternedName::from("X"),
                space: SymbolSpace::Type,
            },
            FactKey::ModuleAugmentation {
                specifier: InternedSpecifier::from("vue"),
                owner: TopLevelOwnerId::ordinary_file(),
                augmented_name: InternedName::from("ComponentOptions"),
                space: SymbolSpace::Type,
            },
        ];
        for k in &parse_keys {
            assert_eq!(
                k.domain(),
                FactDomain::ParseFile,
                "parse-domain variant routed wrong: {k:?}"
            );
        }

        let resolve_imports_keys = [
            FactKey::ResolvedImportClause {
                specifier: InternedSpecifier::from("./x"),
                binding: InternedName::from("X"),
                space: SymbolSpace::Type,
                resolved_canonical: Arc::from("/x.ts"),
                resolved_source_name: InternedName::from("X"),
            },
            FactKey::ResolvedReexportBinding {
                specifier: InternedSpecifier::from("./x"),
                source_name: InternedName::from("X"),
                target_name: InternedName::from("X"),
                space: SymbolSpace::Type,
                resolved_canonical: Arc::from("/x.ts"),
                resolved_source_name: InternedName::from("X"),
            },
        ];
        for k in &resolve_imports_keys {
            assert_eq!(
                k.domain(),
                FactDomain::ResolveImports,
                "resolve-imports variant routed wrong: {k:?}"
            );
        }

        let route_surface_keys = [
            FactKey::EffectiveExportSet,
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(InternedSpecifier::from("vue")),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            },
        ];
        for k in &route_surface_keys {
            assert_eq!(
                k.domain(),
                FactDomain::RouteSurface,
                "route-surface variant routed wrong: {k:?}"
            );
        }
    }

    #[test]
    fn symbol_space_tags_are_distinct_and_stable() {
        assert_ne!(SymbolSpace::Type.tag(), SymbolSpace::Value.tag());
        assert_ne!(SymbolSpace::Type.tag(), SymbolSpace::Namespace.tag());
        assert_ne!(SymbolSpace::Value.tag(), SymbolSpace::Namespace.tag());
        // Lock the byte values so a renumber is a deliberate change.
        assert_eq!(SymbolSpace::Type.tag(), 0x01);
        assert_eq!(SymbolSpace::Value.tag(), 0x02);
        assert_eq!(SymbolSpace::Namespace.tag(), 0x03);
    }

    #[test]
    fn member_kind_tags_discriminate_by_modifier() {
        let a = MemberKind::Property {
            readonly: false,
            optional: false,
        };
        let b = MemberKind::Property {
            readonly: true,
            optional: false,
        };
        let c = MemberKind::Property {
            readonly: false,
            optional: true,
        };
        assert_ne!(a.tag(), b.tag(), "readonly modifier changes tag");
        assert_ne!(a.tag(), c.tag(), "optional modifier changes tag");
        // Different kinds get different leading bytes.
        assert_ne!(a.tag()[0], MemberKind::Method.tag()[0]);
        assert_ne!(
            MemberKind::Property {
                readonly: false,
                optional: false
            }
            .tag()[0],
            MemberKind::ClassField {
                static_: false,
                readonly: false,
                abstract_: false
            }
            .tag()[0],
            "property and class field discriminate"
        );
    }

    #[test]
    fn macro_kind_round_trips_with_template_kind() {
        use crate::analysis::MacroKind as TemplateMacroKind;
        let pairs = [
            (TemplateMacroKind::DefineProps, MacroKind::DefineProps),
            (TemplateMacroKind::DefineEmits, MacroKind::DefineEmits),
            (TemplateMacroKind::DefineModel, MacroKind::DefineModel),
            (TemplateMacroKind::DefineSlots, MacroKind::DefineSlots),
            (TemplateMacroKind::DefineExpose, MacroKind::DefineExpose),
            (TemplateMacroKind::DefineOptions, MacroKind::DefineOptions),
            (TemplateMacroKind::WithDefaults, MacroKind::WithDefaults),
        ];
        for (template, fact) in pairs {
            assert_eq!(
                MacroKind::from(template),
                fact,
                "MacroKind From mismatch for {template:?}"
            );
        }
    }
}
