//! Module-resolution design vocabulary (content-free SHAPE types).
//!
//! These are the typed vocabulary the module-resolution KEYING contract
//! (`### Module-Resolution Keying (CRITICAL)`) is expressed in. They are
//! **content-free shape** — no resolver, no walker, no condition-evaluation
//! semantics. The resolution-matrix walker that consumes them, and the
//! broken-input taint producers, live in `verter_session::resolver_core`
//! (see `docs/arch/native-typeinfo-parity-u2-reducers.md`).
//!
//! Two of these types ([`ModuleResolutionMode`] and [`ConditionSet`]) are
//! resolve-domain ENV inputs: they ride in [`crate::env_hash::EnvHashInputs`]
//! and hash into `resolve_env_hash` (and ONLY `resolve_env_hash`). They are
//! orthogonal to the lib dimension — TS lib selection / `typeRoots` / the
//! ambient corpus feed `lib_env_hash`, NEVER `resolve_env_hash` (R21 scoping
//! rule). [`SpecifierKind`] is a per-specifier classification used by the
//! resolution-matrix walker in `verter_session::resolver_core`; it is NOT a
//! project-env input and does not key any env hash.

/// TypeScript `moduleResolution` strategy.
///
/// Content-free shape mirroring the closed TS taxonomy. This is a
/// resolve-domain ENV input — changing it changes where a bare/relative
/// specifier resolves, so it hashes into `resolve_env_hash`.
///
/// The discriminants are pinned (`#[repr(u8)]` + explicit values) because the
/// mode is hashed via `mode as u8` into `resolve_env_hash`; pinning prevents a
/// future variant reorder from silently changing the hash for the same
/// semantic mode.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModuleResolutionMode {
    /// Legacy `classic` resolution.
    Classic = 0,
    /// `node` / `node10` CommonJS-style resolution.
    Node10 = 1,
    /// `node16` ESM-aware resolution.
    Node16 = 2,
    /// `nodenext` ESM-aware resolution.
    NodeNext = 3,
    /// `bundler` resolution (the Vite / Vue workspace default).
    #[default]
    Bundler = 4,
}

/// Classification of an import specifier by its syntactic shape.
///
/// Content-free vocabulary used by the resolution-matrix walker in
/// `verter_session::resolver_core` to pick a resolution lane. NOT an env-hash
/// input — a specifier's kind is a property of the specifier being resolved,
/// not of the project environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecifierKind {
    /// `./foo`, `../bar` — relative path.
    Relative,
    /// `/abs/path` — absolute path.
    Absolute,
    /// `vue`, `lodash` — bare package name (package root).
    BarePackage,
    /// `lodash/fp` — bare package name with a subpath.
    PackageSubpath,
    /// `#imports`, `#app/foo` — package-internal `imports` specifier.
    PackageImport,
}

/// Ordered, deduplicated set of `package.json` `exports`/`imports` condition
/// tokens consulted during resolution (e.g. `["types", "import", "default"]`).
///
/// Content-free shape: conditions are open-ended / user-defined, so this is a
/// string set rather than a closed enum. The set is a resolve-domain ENV input
/// (different active condition orderings resolve a conditional `exports` map to
/// different targets) and hashes into `resolve_env_hash`.
///
/// Order is significant — `["import", "default"]` and `["default", "import"]`
/// are different resolution behaviours — so the constructor preserves first-seen
/// order while removing later duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ConditionSet(Box<[String]>);

impl ConditionSet {
    /// Build a condition set from an iterator of condition tokens, preserving
    /// first-seen order and dropping later duplicates.
    ///
    /// Framing-ambiguous tokens are rejected at construction: an empty token
    /// (`""`) or a token containing the NUL byte (`'\0'`, the hash-buffer
    /// separator) is dropped entirely. Real `package.json`
    /// `exports`/`imports` condition keys are identifiers and never contain
    /// NUL, so this drops only impossible/degenerate tokens and keeps the
    /// content-free shape unambiguous when hashed.
    #[must_use]
    pub fn new<I, S>(conditions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut seen: Vec<String> = Vec::new();
        for cond in conditions {
            let cond = cond.into();
            if cond.is_empty() || cond.contains('\0') {
                continue;
            }
            if !seen.iter().any(|existing| existing == &cond) {
                seen.push(cond);
            }
        }
        Self(seen.into_boxed_slice())
    }

    /// The ordered condition tokens.
    #[must_use]
    pub fn conditions(&self) -> &[String] {
        &self.0
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_set_preserves_order_and_dedupes() {
        let set = ConditionSet::new(["types", "import", "default", "import"]);
        assert_eq!(
            set.conditions(),
            &[
                "types".to_string(),
                "import".to_string(),
                "default".to_string()
            ],
            "first-seen order preserved, later duplicate `import` dropped"
        );
        assert!(!set.is_empty());
    }

    #[test]
    fn condition_set_order_is_significant_for_equality() {
        let a = ConditionSet::new(["import", "default"]);
        let b = ConditionSet::new(["default", "import"]);
        assert_ne!(a, b, "condition order is a distinct resolution behaviour");
    }

    #[test]
    fn module_resolution_mode_default_is_bundler() {
        assert_eq!(
            ModuleResolutionMode::default(),
            ModuleResolutionMode::Bundler
        );
    }

    #[test]
    fn module_resolution_mode_discriminants_are_stable() {
        // These discriminants are hashed via `mode as u8` into
        // `resolve_env_hash`; a future variant reorder must NOT silently
        // change the hash for the same semantic mode.
        assert_eq!(ModuleResolutionMode::Classic as u8, 0);
        assert_eq!(ModuleResolutionMode::Node10 as u8, 1);
        assert_eq!(ModuleResolutionMode::Node16 as u8, 2);
        assert_eq!(ModuleResolutionMode::NodeNext as u8, 3);
        assert_eq!(ModuleResolutionMode::Bundler as u8, 4);
    }

    #[test]
    fn condition_set_drops_empty_and_nul_tokens() {
        // Empty tokens and tokens containing the SEP/NUL byte are
        // framing-ambiguous and can never be real `package.json` condition
        // keys, so the constructor drops them entirely.
        let set = ConditionSet::new(["a", "", "b\0c", "d"]);
        assert_eq!(
            set.conditions(),
            &["a".to_string(), "d".to_string()],
            "empty token and NUL-containing token dropped, real tokens kept"
        );
    }
}
