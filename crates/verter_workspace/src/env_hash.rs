//! 5-way env hash split (R21).
//!
//! Verter's cache substrate keys cache entries on **five orthogonal env
//! dimensions**, each derived from a strict subset of the project + host
//! configuration. The five dimensions are:
//!
//! | Dimension          | Captures                                                                              |
//! |--------------------|---------------------------------------------------------------------------------------|
//! | `parse_env_hash`   | The workspace parser-flag string (`EnvHashInputs.parser_flags`) — TODAY the sole consumed input; syntax mode / language target are NOT yet folded in (each new parse-relevant input must be added here AND to `parse_env_hash`). |
//! | `resolve_env_hash` | `base_url`, `paths`, workspace aliases, project references, `moduleResolution` mode, `exports`/`imports` condition set, extension order. |
//! | `type_env_hash`    | TS semantic options that change type meaning (`strict`, `noImplicitAny`, ...).        |
//! | `lib_env_hash`     | TS built-in lib selection, `types`, `typeRoots`, ambient corpus identity.             |
//! | `project_identity` | Project root, tsconfig path, provider root, workspace root, membership.               |
//!
//! Every cache layer keys on **only** the dimensions it actually depends on
//! (R21 scoping rule). A single bundled `project_config_hash` is forbidden.
//! See `docs/arch/fact-based-cache.md` for the per-field audit table and the
//! per-cache-layer key composition.
//!
//! The hash functions are pure: same inputs → same `Hash16`. Each dimension
//! mixes a **per-dimension salt** byte into its hash input so the five
//! dimensions derived from identical baseline state never collide (the
//! `env_hashes_distinguish_across_dimensions` test asserts this).
//!
//! ## R21 scoping rule
//!
//! `lib_env_hash` enters a cache key only when the cached value depends on
//! lib data. Concretely:
//!
//! - `ResolvedImportFacts` (base syntactic import → file canonical resolution)
//!   does **NOT** include `lib_env_hash`. A lib update does not change where
//!   `./theme` resolves.
//! - `RouteDb` per-name and effective-set caches **DO** include `lib_env_hash`
//!   because module augmentations (which live in libs / ambient corpora)
//!   stitch into the effective surface.
//! - Typed-IR resolve, `MaterializeStructureDb`, `RefCycleResultDb`,
//!   `SemanticGraphStore`, `ComponentMetaResultDb` **DO** include
//!   `lib_env_hash` because semantic meaning depends on intrinsic types
//!   (`Array<T>`, `HTMLElement`, etc.).
//!
//! See `/type-cache-architecture` skill for the full rule set.

use verter_scheduler::invalidation::Hash16;
use xxhash_rust::xxh3::xxh3_128;

use crate::membership::ConfiguredMembership;
use crate::module_resolution::{ConditionSet, ModuleResolutionMode};
use crate::resolver::{IdeProjectCompilerOptions, IdeProjectConfig};

/// Per-call inputs to the env-hash functions that are NOT part of
/// [`IdeProjectConfig`].
///
/// `IdeProjectConfig` captures the project-shape data (root, tsconfig,
/// references, paths, aliases, membership). The remaining inputs — parser
/// flags, resolve extensions, TS semantic options, and TS lib data — are
/// surfaced through this borrowed-view struct so callers can pass them by
/// reference without cloning.
///
/// All fields are stable input projections, not raw deserialised structures.
/// Callers that hold richer types should produce minimal stable forms before
/// passing them here.
#[derive(Debug, Clone, Copy)]
pub struct EnvHashInputs<'a> {
    /// Parser / SFC compiler feature flag identifiers in declaration order.
    /// Reordering them DOES change the hash; callers MUST normalise to a
    /// canonical order before hashing. Currently order-sensitive so a
    /// feature-flag rename is observable.
    pub parser_flags: &'a [&'a str],

    /// Extension-priority order for extensionless import specifiers.
    /// Order matters — `(.ts, .tsx)` and `(.tsx, .ts)` are different
    /// resolve behaviours.
    pub resolve_extensions: &'a [&'a str],

    /// TS `strict` flag.
    pub type_strict: bool,

    /// TS `noImplicitAny` flag.
    pub type_no_implicit_any: bool,

    /// TS lib selection (e.g. `lib.dom.d.ts`, `lib.es2022.d.ts`).
    /// Includes user-declared `lib` from tsconfig plus any default lib names.
    pub lib_names: &'a [&'a str],

    /// TS `typeRoots` — directories scanned for ambient `@types` packages.
    pub type_roots: &'a [&'a str],

    /// TS `moduleResolution` strategy. A resolve-domain ENV input — changing
    /// it changes where a bare/relative specifier resolves, so it hashes into
    /// `resolve_env_hash` (and NEVER into `lib_env_hash` / `type_env_hash`).
    pub module_resolution_mode: ModuleResolutionMode,

    /// Ordered, deduplicated `package.json` `exports`/`imports` condition set
    /// consulted during resolution (e.g. `["types", "import", "default"]`).
    /// A resolve-domain ENV input — different active condition orderings
    /// resolve a conditional `exports` map to different targets — so it hashes
    /// into `resolve_env_hash` ONLY. Orthogonal to the lib dimension (R21).
    pub export_conditions: &'a ConditionSet,

    /// Fingerprint of the resolved ambient library corpus (the set of
    /// `lib*.d.ts` declarations, ambient `@types`, registered globals, and
    /// module-augmentation declarations visible to this project).
    ///
    /// Producers compute this from the workspace ambient registration table
    /// plus the active TS SDK; consumers pass it through unchanged. Any
    /// observable change to the ambient surface MUST change this
    /// fingerprint.
    pub ambient_corpus_fingerprint: u64,
}

/// Per-dimension salt bytes mixed into each hash so the five dimensions
/// derived from the same baseline never collide.
const SALT_PARSE: &[u8] = b"verter-env:parse";
const SALT_RESOLVE: &[u8] = b"verter-env:resolve";
const SALT_TYPE: &[u8] = b"verter-env:type";
const SALT_LIB: &[u8] = b"verter-env:lib";
const SALT_PROJECT_IDENTITY: &[u8] = b"verter-env:project-identity";

const SEP: u8 = 0u8;

impl IdeProjectConfig {
    /// `parse_env_hash` — the parse dimension. TODAY it consumes exactly
    /// one input: the workspace parser-flag string
    /// (`EnvHashInputs.parser_flags`). Syntax mode / language target are
    /// NOT yet folded in — any new parse-relevant configuration MUST be
    /// added to this hash (and the module doc table) the moment it can
    /// vary per project, or the value-side `parse_env_hash` reuse gates
    /// go blind to it.
    ///
    /// Does NOT include project root, tsconfig paths, alias maps, TS
    /// semantic options, or lib data. Editing `paths` or flipping `strict`
    /// MUST NOT change this hash.
    ///
    /// Bound by: `FileArtifactStore`, `MemberSemanticFactStore`,
    /// `MemberDisplayFactStore` keys.
    #[must_use]
    pub fn parse_env_hash(&self, inputs: &EnvHashInputs<'_>) -> Hash16 {
        let mut buf: Vec<u8> = Vec::with_capacity(64);
        buf.extend_from_slice(SALT_PARSE);
        buf.push(SEP);
        write_str_slice(&mut buf, inputs.parser_flags);
        compute_hash16(&buf)
    }

    /// `resolve_env_hash` — captures `base_url`, `paths`, workspace aliases,
    /// project references, default extension order, the `moduleResolution`
    /// mode ([`ModuleResolutionMode`]), and the active `exports`/`imports`
    /// condition set ([`ConditionSet`]).
    ///
    /// Does NOT include lib data (R21 scoping rule). The lib corpus
    /// (`lib_names` / `typeRoots` / ambient corpus) is NEVER folded into this
    /// hash — `resolve_env` and `lib_env` are orthogonal dimensions. A TS lib
    /// update MUST NOT change this hash. See `### Module-Resolution Keying
    /// (CRITICAL)` in the `/type-cache-architecture` skill.
    ///
    /// Bound by: `ResolvedImportFacts` (NOT `lib_env_hash`), `RouteDb`
    /// (combined with `lib_env_hash` because of module augmentations).
    #[must_use]
    pub fn resolve_env_hash(&self, inputs: &EnvHashInputs<'_>) -> Hash16 {
        let mut buf: Vec<u8> = Vec::with_capacity(256);
        buf.extend_from_slice(SALT_RESOLVE);
        buf.push(SEP);

        // workspace aliases (order-sensitive — overlap precedence matters)
        for alias in &self.workspace_aliases {
            buf.extend_from_slice(alias.find.as_bytes());
            buf.push(SEP);
            buf.extend_from_slice(alias.replacement.as_bytes());
            buf.push(SEP);
        }
        buf.push(SEP);

        // compiler options that affect resolution
        write_compiler_options(&mut buf, &self.compiler_options);

        // project references
        for r in &self.references {
            buf.extend_from_slice(r.as_bytes());
            buf.push(SEP);
        }
        buf.push(SEP);

        // resolve extensions (order matters)
        write_str_slice(&mut buf, inputs.resolve_extensions);

        // module resolution mode (TS `moduleResolution`) — a resolve-domain
        // input that changes where bare/relative specifiers resolve.
        buf.push(inputs.module_resolution_mode as u8);
        buf.push(SEP);

        // active `exports`/`imports` condition set (order is significant —
        // a different ordering resolves a conditional `exports` map to a
        // different target). NEVER mixes lib data (R21). Framed via the shared
        // `write_str_slice` helper so the framing matches every other slice.
        let conditions: Vec<&str> = inputs
            .export_conditions
            .conditions()
            .iter()
            .map(String::as_str)
            .collect();
        write_str_slice(&mut buf, &conditions);

        compute_hash16(&buf)
    }

    /// `type_env_hash` — captures TS semantic options that change type
    /// meaning (`strict`, `noImplicitAny`, etc.).
    ///
    /// Bound by: typed-IR resolve, `MaterializeStructureDb`,
    /// `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb`.
    #[must_use]
    pub fn type_env_hash(&self, inputs: &EnvHashInputs<'_>) -> Hash16 {
        let mut buf: Vec<u8> = Vec::with_capacity(32);
        buf.extend_from_slice(SALT_TYPE);
        buf.push(SEP);
        buf.push(inputs.type_strict as u8);
        buf.push(inputs.type_no_implicit_any as u8);
        // Future TS-semantic flags (e.g. `strictNullChecks`,
        // `useUnknownInCatchVariables`) extend this body in declaration
        // order. Adding a flag is a producer-side change; existing keys
        // re-hash automatically.
        compute_hash16(&buf)
    }

    /// `lib_env_hash` — captures TS built-in lib selection
    /// (`lib.dom.d.ts`, `lib.es*.d.ts`), `types`, `typeRoots`, registered
    /// ambient libs, the global / module-augmentation corpus identity.
    ///
    /// R21 scoping rule: enters a cache key only when the cached value
    /// depends on lib data. `ResolvedImportFacts` MUST NOT key on this
    /// hash; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`,
    /// `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb`
    /// MUST.
    #[must_use]
    pub fn lib_env_hash(&self, inputs: &EnvHashInputs<'_>) -> Hash16 {
        let mut buf: Vec<u8> = Vec::with_capacity(256);
        buf.extend_from_slice(SALT_LIB);
        buf.push(SEP);
        write_str_slice(&mut buf, inputs.lib_names);
        write_str_slice(&mut buf, inputs.type_roots);
        buf.extend_from_slice(&inputs.ambient_corpus_fingerprint.to_le_bytes());
        compute_hash16(&buf)
    }

    /// `project_identity` — captures project root, tsconfig path, provider
    /// root, workspace root, membership / owner selection.
    ///
    /// Used as the `ProjectId`-equivalent dimension on cache keys that
    /// must distinguish entries from different projects observing the
    /// same file (e.g., the `AugmentationTargetKey` in
    /// `FileArtifactStore::augmentation_index`).
    ///
    /// Independent of parse / resolve / type / lib content — two projects
    /// with identical configurations under different roots produce
    /// distinct `project_identity` values.
    #[must_use]
    pub fn project_identity(&self) -> Hash16 {
        let mut buf: Vec<u8> = Vec::with_capacity(128);
        buf.extend_from_slice(SALT_PROJECT_IDENTITY);
        buf.push(SEP);
        buf.extend_from_slice(self.workspace_root.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(self.root.as_bytes());
        buf.push(SEP);
        buf.extend_from_slice(self.provider_root.as_bytes());
        buf.push(SEP);
        match &self.tsconfig_path {
            Some(path) => {
                buf.push(1u8);
                buf.extend_from_slice(path.as_bytes());
            }
            None => buf.push(0u8),
        }
        buf.push(SEP);
        write_membership(&mut buf, &self.membership);
        compute_hash16(&buf)
    }
}

fn write_str_slice(buf: &mut Vec<u8>, items: &[&str]) {
    for s in items {
        buf.extend_from_slice(s.as_bytes());
        buf.push(SEP);
    }
    buf.push(SEP);
}

fn write_compiler_options(buf: &mut Vec<u8>, opts: &IdeProjectCompilerOptions) {
    if let Some(base) = &opts.base_url {
        buf.push(1u8);
        buf.extend_from_slice(base.as_bytes());
    } else {
        buf.push(0u8);
    }
    buf.push(SEP);
    for (key, candidates) in &opts.paths {
        buf.extend_from_slice(key.as_bytes());
        buf.push(SEP);
        for c in candidates {
            buf.extend_from_slice(c.as_bytes());
            buf.push(SEP);
        }
        buf.push(SEP);
    }
    buf.push(SEP);
}

fn write_membership(buf: &mut Vec<u8>, membership: &ConfiguredMembership) {
    // Hash the static spec (files / include / exclude) — the identity-bearing
    // membership definition. The materialized set is a disk-derived expansion of
    // the spec, tracked by content generation elsewhere, so the spec alone keys
    // the project-identity contribution.
    let spec = &membership.spec;
    for f in &spec.files {
        buf.extend_from_slice(f.as_str().as_bytes());
        buf.push(SEP);
    }
    buf.push(SEP);
    for inc in &spec.include {
        buf.extend_from_slice(inc.as_str().as_bytes());
        buf.push(SEP);
    }
    buf.push(SEP);
    for exc in spec.exclude.iter() {
        buf.extend_from_slice(exc.as_str().as_bytes());
        buf.push(SEP);
    }
    buf.push(SEP);
}

fn compute_hash16(bytes: &[u8]) -> Hash16 {
    xxh3_128(bytes).to_le_bytes()
}

#[cfg(test)]
#[path = "env_hash_tests.rs"]
mod env_hash_tests;
