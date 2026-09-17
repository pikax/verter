//! Per-project (tsconfig-backed) resolver configuration.
//!
//! The DTO lives with the resolver core. Workspace-specific default membership
//! construction remains in the workspace config-ingress function, while this
//! module accepts the resulting dependency-neutral membership value.

use super::membership::ConfiguredMembership;
use verter_span::path::CanonicalPath;

/// A workspace alias maps a prefix (e.g. `@/`) to a filesystem replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

/// Compiler options extracted from a tsconfig for resolution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdeProjectCompilerOptions {
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
    /// `compilerOptions.allowJs` — when set (or `checkJs`), `.js`/`.jsx`/
    /// `.cjs`/`.mjs` join the project's supported-extension set.
    pub allow_js: bool,
    /// `compilerOptions.checkJs` — implies `allowJs` for membership purposes
    /// (TypeScript treats `checkJs` as turning on JS type-checking, which
    /// requires the JS files to be project members).
    pub check_js: bool,
    /// `compilerOptions.allowImportingTsExtensions` — when explicitly true,
    /// tsserver barrel publication preserves authored `.vue`/`.svelte`
    /// specifiers. Missing/false projects receive the `.verter.ts`
    /// compatibility rewrite.
    pub allow_importing_ts_extensions: bool,
    /// `compilerOptions.disableSolutionSearching` — when a solution config
    /// sets it, default-project selection does NOT climb from that solution
    /// to its ancestor solution (mirrors tsgo `DisableSolutionSearching`).
    /// Default `false`.
    pub disable_solution_searching: bool,
    /// The EFFECTIVE type-semantic option set (strictness family, exact
    /// optionality, lib selection, target) — canonical values, never raw
    /// spellings. Populated by the tsconfig loader after the whole `extends`
    /// chain is merged; defaults to TypeScript's own defaults for a project
    /// that declares nothing.
    pub semantic: SemanticCompilerOptions,
}

impl IdeProjectCompilerOptions {
    /// Whether JavaScript files are project members (either `allowJs` or
    /// `checkJs` is set).
    #[must_use]
    pub fn js_is_member(&self) -> bool {
        self.allow_js || self.check_js
    }
}

/// TypeScript `target` — the ECMAScript level a project compiles for.
///
/// Its only type-layer consequence is the DEFAULT lib file selected when
/// `lib` is unset ([`Self::default_lib_file_name`]), so it enters the
/// lib env dimension through that derived name and never the type env
/// dimension.
///
/// Accepted spellings follow the 7.0.2 binary (`tsc --listFilesOnly`
/// with a one-option `tsconfig.json`): `es5` (deprecated but still
/// honoured), `es6`/`es2015`, `es2016`…`es2025`, `esnext`. `es3` and
/// `es7` are rejected by the binary (`TS6046`) and fall back to the
/// default target here, exactly as the rejected option falls back there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScriptTarget {
    Es5,
    Es2015,
    Es2016,
    Es2017,
    Es2018,
    Es2019,
    Es2020,
    Es2021,
    Es2022,
    Es2023,
    Es2024,
    Es2025,
    EsNext,
}

impl Default for ScriptTarget {
    /// TypeScript 7.0.2 default. Probe: `tsc --listFilesOnly` over an
    /// empty `{}` tsconfig loads `lib.es2025.full.d.ts`, the default lib of
    /// exactly this target (`--target es2025` loads the same file;
    /// `--target esnext` loads `lib.esnext.full.d.ts`).
    fn default() -> Self {
        Self::Es2025
    }
}

impl ScriptTarget {
    /// Parse a tsconfig `target` spelling (case-insensitive). `None` for a
    /// spelling the 7.0.2 binary rejects (`es3`, `es7`, typos): the
    /// project then runs on the default target, as TypeScript does after
    /// reporting the rejected option.
    #[must_use]
    pub fn from_config_value(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "es5" => Self::Es5,
            "es6" | "es2015" => Self::Es2015,
            "es2016" => Self::Es2016,
            "es2017" => Self::Es2017,
            "es2018" => Self::Es2018,
            "es2019" => Self::Es2019,
            "es2020" => Self::Es2020,
            "es2021" => Self::Es2021,
            "es2022" => Self::Es2022,
            "es2023" => Self::Es2023,
            "es2024" => Self::Es2024,
            "es2025" => Self::Es2025,
            "esnext" => Self::EsNext,
            _ => return None,
        })
    }

    /// The lib file TypeScript loads for this target when `lib` is unset
    /// and `noLib` is off. Pinned against the 7.0.2 binary
    /// (`tsc --listFilesOnly` per target): `es5` → `lib.d.ts`,
    /// `es2015` → `lib.es6.d.ts`, `es2016`…`es2025` →
    /// `lib.esYYYY.full.d.ts`, `esnext` → `lib.esnext.full.d.ts`.
    #[must_use]
    pub fn default_lib_file_name(self) -> &'static str {
        match self {
            Self::Es5 => "lib.d.ts",
            Self::Es2015 => "lib.es6.d.ts",
            Self::Es2016 => "lib.es2016.full.d.ts",
            Self::Es2017 => "lib.es2017.full.d.ts",
            Self::Es2018 => "lib.es2018.full.d.ts",
            Self::Es2019 => "lib.es2019.full.d.ts",
            Self::Es2020 => "lib.es2020.full.d.ts",
            Self::Es2021 => "lib.es2021.full.d.ts",
            Self::Es2022 => "lib.es2022.full.d.ts",
            Self::Es2023 => "lib.es2023.full.d.ts",
            Self::Es2024 => "lib.es2024.full.d.ts",
            Self::Es2025 => "lib.es2025.full.d.ts",
            Self::EsNext => "lib.esnext.full.d.ts",
        }
    }
}

/// Canonicalise one tsconfig `lib` entry into the lib FILE name TypeScript
/// loads for it: lowercase, the two historical aliases (`es6` → `es2015`,
/// `es7` → `es2016`) resolved, then `lib.<name>.d.ts`. Pinned against the
/// 7.0.2 binary: `["ES6", "DOM"]` loads `lib.es2015.d.ts` + `lib.dom.d.ts`;
/// `["es7"]` loads `lib.es2016.d.ts`. A spelling the binary rejects
/// (`TS6046`) still canonicalises by the same rule — it distinguishes the
/// configuration without pretending to know the binary's accepted set.
#[must_use]
pub fn canonical_lib_file_name(spelling: &str) -> String {
    let lowered = spelling.trim().to_ascii_lowercase();
    let name = match lowered.as_str() {
        "es6" => "es2015",
        "es7" => "es2016",
        other => other,
    };
    format!("lib.{name}.d.ts")
}

/// The raw, spelling-level semantic options a single tsconfig (or an
/// `extends` chain) DECLARES — every field `None` when the key is absent.
///
/// This is the inheritance carrier: the loader layers each config's
/// declared keys over its ancestors' (last-wins per key) and canonicalises
/// ONCE at the end through [`Self::effective`], so an umbrella `strict`
/// inherited from a base and a member spelled explicitly on the leaf
/// resolve through one rule set. Never stored on a project — projects
/// carry the effective [`SemanticCompilerOptions`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawSemanticCompilerOptions {
    pub strict: Option<bool>,
    pub strict_null_checks: Option<bool>,
    pub strict_function_types: Option<bool>,
    pub strict_bind_call_apply: Option<bool>,
    pub strict_property_initialization: Option<bool>,
    pub no_implicit_any: Option<bool>,
    pub no_implicit_this: Option<bool>,
    pub use_unknown_in_catch_variables: Option<bool>,
    pub always_strict: Option<bool>,
    pub exact_optional_property_types: Option<bool>,
    pub no_unchecked_indexed_access: Option<bool>,
    pub no_lib: Option<bool>,
    /// Raw `lib` spellings exactly as declared; `Some(vec![])` is an
    /// explicit empty selection (TypeScript then loads NO lib file), which
    /// is distinct from `None` (unset — the target's default lib applies).
    pub lib: Option<Vec<String>>,
    /// Raw `target` spelling as declared.
    pub target: Option<String>,
}

impl RawSemanticCompilerOptions {
    /// Layer `leaf` over `self`: every key `leaf` declares replaces the
    /// inherited value (TypeScript's last-wins `extends` semantics per
    /// option; `lib` replaces the whole array, never merges it).
    pub fn layer(&mut self, leaf: Self) {
        macro_rules! take {
            ($($field:ident),* $(,)?) => {
                $( if leaf.$field.is_some() { self.$field = leaf.$field; } )*
            };
        }
        take!(
            strict,
            strict_null_checks,
            strict_function_types,
            strict_bind_call_apply,
            strict_property_initialization,
            no_implicit_any,
            no_implicit_this,
            use_unknown_in_catch_variables,
            always_strict,
            exact_optional_property_types,
            no_unchecked_indexed_access,
            no_lib,
            lib,
            target,
        );
    }

    /// Resolve the declared spellings into the effective option set.
    ///
    /// Each `strict`-family member takes its explicit value when declared,
    /// otherwise the umbrella `strict` value, otherwise TypeScript 7.0.2's
    /// default (`strict` ON — see [`SemanticCompilerOptions::default`]).
    /// `exactOptionalPropertyTypes` / `noUncheckedIndexedAccess` / `noLib`
    /// default OFF and are not umbrella members. `lib` entries
    /// canonicalise through [`canonical_lib_file_name`], deduplicate and
    /// sort (a set — declaration order is not semantic); `target` parses
    /// through [`ScriptTarget::from_config_value`] with the rejected
    /// spellings falling back to the default target.
    #[must_use]
    pub fn effective(&self) -> SemanticCompilerOptions {
        let defaults = SemanticCompilerOptions::default();
        let strict = self.strict.unwrap_or(defaults.strict_null_checks);
        let member = |explicit: Option<bool>| explicit.unwrap_or(strict);
        let lib = self.lib.as_ref().map(|entries| {
            let mut names: Vec<String> = entries
                .iter()
                .map(|entry| canonical_lib_file_name(entry))
                .collect();
            names.sort_unstable();
            names.dedup();
            names
        });
        SemanticCompilerOptions {
            strict_null_checks: member(self.strict_null_checks),
            strict_function_types: member(self.strict_function_types),
            strict_bind_call_apply: member(self.strict_bind_call_apply),
            strict_property_initialization: member(self.strict_property_initialization),
            no_implicit_any: member(self.no_implicit_any),
            no_implicit_this: member(self.no_implicit_this),
            use_unknown_in_catch_variables: member(self.use_unknown_in_catch_variables),
            always_strict: member(self.always_strict),
            exact_optional_property_types: self
                .exact_optional_property_types
                .unwrap_or(defaults.exact_optional_property_types),
            no_unchecked_indexed_access: self
                .no_unchecked_indexed_access
                .unwrap_or(defaults.no_unchecked_indexed_access),
            no_lib: self.no_lib.unwrap_or(defaults.no_lib),
            lib,
            target: self
                .target
                .as_deref()
                .and_then(ScriptTarget::from_config_value)
                .unwrap_or(defaults.target),
        }
    }
}

/// The EFFECTIVE TypeScript semantic compiler options of one project —
/// the values the checker actually runs under, after `strict` umbrella
/// expansion, `extends` inheritance and spelling canonicalisation.
///
/// Two projects with identical effective values are the same type
/// environment however they spelled them; the type env dimension hashes
/// exactly the type-meaning fields of this struct, the lib env dimension
/// exactly the lib-selection fields ([`Self::effective_lib_file_names`]).
///
/// [`Default`] is TypeScript 7.0.2's default configuration, pinned by
/// running its `tsc.exe` over a file with no tsconfig options: `strict`
/// is ON (`let a: string = null` → TS2322, an unannotated parameter →
/// TS7006, a `catch` binding → TS18046 `unknown`, an uninitialised class
/// property → TS2564, a narrower callback parameter → the
/// `strictFunctionTypes` TS2345), `exactOptionalPropertyTypes` is OFF
/// (`{ a: undefined }` into `{ a?: number }` is accepted; ON reports
/// TS2375), `noUncheckedIndexedAccess` is OFF (`arr[0]` types as `number`;
/// ON reports TS2322 `number | undefined`), `noLib` is OFF, `lib` is unset
/// and the default target loads `lib.es2025.full.d.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticCompilerOptions {
    /// `strictNullChecks`: `null` / `undefined` are their own types and
    /// relate to nothing but themselves (and `never`-free unions naming
    /// them); OFF makes them assignable to every non-`never` type.
    pub strict_null_checks: bool,
    /// `strictFunctionTypes`: function-type parameters relate
    /// contravariantly; OFF relates them bivariantly.
    pub strict_function_types: bool,
    /// `strictBindCallApply`.
    pub strict_bind_call_apply: bool,
    /// `strictPropertyInitialization`.
    pub strict_property_initialization: bool,
    /// `noImplicitAny`.
    pub no_implicit_any: bool,
    /// `noImplicitThis`.
    pub no_implicit_this: bool,
    /// `useUnknownInCatchVariables`.
    pub use_unknown_in_catch_variables: bool,
    /// `alwaysStrict`.
    pub always_strict: bool,
    /// `exactOptionalPropertyTypes`: an optional property does NOT
    /// implicitly accept `undefined`.
    pub exact_optional_property_types: bool,
    /// `noUncheckedIndexedAccess`: index-signature reads add `undefined`.
    pub no_unchecked_indexed_access: bool,
    /// `noLib`: no lib file is loaded at all.
    pub no_lib: bool,
    /// Explicit `lib` selection as canonical lib FILE names
    /// ([`canonical_lib_file_name`]), deduplicated and sorted. `None` when
    /// the project declares no `lib` (the target's default lib applies);
    /// `Some(vec![])` is an explicit empty selection, which loads nothing.
    pub lib: Option<Vec<String>>,
    /// `target`, consumed only through its default lib file.
    pub target: ScriptTarget,
}

impl Default for SemanticCompilerOptions {
    fn default() -> Self {
        Self {
            strict_null_checks: true,
            strict_function_types: true,
            strict_bind_call_apply: true,
            strict_property_initialization: true,
            no_implicit_any: true,
            no_implicit_this: true,
            use_unknown_in_catch_variables: true,
            always_strict: true,
            exact_optional_property_types: false,
            no_unchecked_indexed_access: false,
            no_lib: false,
            lib: None,
            target: ScriptTarget::default(),
        }
    }
}

impl SemanticCompilerOptions {
    /// The lib files TypeScript loads for this configuration, in sorted
    /// canonical-name order: nothing under `noLib`; the explicit `lib`
    /// selection when declared (possibly empty); otherwise the target's
    /// single default lib file.
    #[must_use]
    pub fn effective_lib_file_names(&self) -> Vec<&str> {
        if self.no_lib {
            return Vec::new();
        }
        match &self.lib {
            Some(names) => names.iter().map(String::as_str).collect(),
            None => vec![self.target.default_lib_file_name()],
        }
    }
}

/// Configuration for a single IDE project (tsconfig-backed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeProjectConfig {
    pub root: String,
    pub workspace_root: String,
    pub tsconfig_path: Option<String>,
    pub provider_root: String,
    pub workspace_aliases: Vec<WorkspaceAlias>,
    pub compiler_options: IdeProjectCompilerOptions,
    pub references: Vec<String>,
    /// Exact configured membership — the SAME [`ConfiguredMembership`] the
    /// host's ownership authority consults, so the resolver and the
    /// ownership authority never diverge on a glob-vs-exact membership
    /// answer. A fallback (tsconfig-less) config carries a match-all
    /// membership under its root.
    pub membership: ConfiguredMembership,
}

impl IdeProjectConfig {
    #[cfg(test)]
    pub(crate) fn new(root: String, workspace_root: String, tsconfig_path: Option<String>) -> Self {
        use rustc_hash::FxHashSet;

        let membership = ConfiguredMembership {
            spec: crate::resolver_core::StaticMembershipSpec {
                files: Vec::new(),
                include: vec![crate::resolver_core::CompiledGlob::new(
                    crate::resolver_core::NormalizedGlob::from_root_and_pattern(
                        &CanonicalPath::new(&root),
                        "**/*",
                    ),
                )],
                exclude: crate::resolver_core::typescript_default_excludes(&CanonicalPath::new(
                    &root,
                )),
            },
            materialized_files: FxHashSet::default(),
        };
        let provider_root = root.clone();
        Self {
            root,
            workspace_root,
            tsconfig_path,
            provider_root,
            workspace_aliases: Vec::new(),
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            membership,
        }
    }

    /// Whether `file_id` is a member of this project, per the exact
    /// [`ConfiguredMembership`] (its materialized file set, or the compiled
    /// spec globs for a match-all / filesystem-less membership). One
    /// membership engine — no second glob evaluator.
    pub fn matches_file(&self, file_id: &str) -> bool {
        self.membership.contains(&CanonicalPath::new(file_id))
    }
}

#[cfg(test)]
#[path = "project_config_tests.rs"]
mod tests;
