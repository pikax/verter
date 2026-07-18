//! Exact membership types for workspace ownership.
//!
//! These types model TypeScript's `files`/`include`/`exclude` membership
//! semantics exactly:
//!
//! - `files` entries are ALWAYS members — `exclude` does NOT affect them
//! - `exclude` only filters what `include` finds
//! - `include` defaults to `["**/*"]` only when BOTH `files` AND `include` are absent
//! - If `files` IS present but `include` is absent → no implicit include
//! - `exclude` defaults to `["node_modules", "bower_components", "jspm_packages", outDir]`
//! - Solution-style `{ files: [], references: [...] }` → matches nothing

use crate::canonical_path::CanonicalPath;
use crate::normalized_glob::{CompiledGlob, NormalizedGlob};
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::{Arc, LazyLock};

/// The set of file extensions a configured TypeScript project treats as
/// members, modelled exactly the way TypeScript filters its default `**/*`
/// include.
///
/// The set is:
/// - the standard TS family — `.ts`, `.tsx`, `.d.ts`, `.cts`, `.mts`,
///   `.d.cts`, `.d.mts` — always present;
/// - the JS family — `.js`, `.jsx`, `.cjs`, `.mjs` — present ONLY when
///   `allowJs`/`checkJs` is set;
/// - each adapter CARRIER extension (`.vue`, `.svelte`, …) acting as a
///   declared `extraFileExtensions`. Carrier extensions are passed in by the
///   caller, which sources them from
///   `verter_language::LanguageRegistry::global().carrier_extensions()` — they
///   are NEVER hardcoded here, so the model is framework-agnostic.
#[derive(Debug, Clone)]
pub struct SupportedExtensions {
    /// Dotted extensions (e.g. `".ts"`, `".vue"`), longest-first so a probe
    /// against a path matches the most specific extension.
    extensions: Vec<String>,
}

/// The standard TypeScript extension family, always a member of the supported
/// set regardless of `allowJs`/`checkJs`.
const TS_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".cts", ".mts", ".d.cts", ".d.mts"];

/// The JavaScript extension family, a member of the supported set only when
/// `allowJs`/`checkJs` is enabled.
const JS_EXTENSIONS: &[&str] = &[".js", ".jsx", ".cjs", ".mjs"];

impl SupportedExtensions {
    /// Build the supported set: TS family + (JS family iff `allow_js`) + each
    /// carrier extension (passed WITHOUT a leading dot, e.g. `"vue"`).
    pub fn new(allow_js: bool, carrier_extensions: &[String]) -> Self {
        let mut extensions: Vec<String> = TS_EXTENSIONS.iter().map(|e| (*e).to_string()).collect();
        if allow_js {
            extensions.extend(JS_EXTENSIONS.iter().map(|e| (*e).to_string()));
        }
        for carrier in carrier_extensions {
            let dotted = format!(".{carrier}");
            if !extensions.contains(&dotted) {
                extensions.push(dotted);
            }
        }
        // Longest-first: a `.d.ts` extension must win over `.ts` when probing.
        extensions.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { extensions }
    }

    /// The dotted extensions in the supported set (longest-first).
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }
}

/// Classify a resolved include glob and expand it per the TS extension rules.
///
/// - An EXTENSION-SPECIFIC glob (whose final path segment ends in a literal
///   `.<ext>`, e.g. `…/**/*.ts`, `…/foo.tsx`) is returned UNCHANGED — TypeScript
///   matches it literally and never expands it.
/// - A DIRECTORY / BARE-STAR glob (whose final segment is `*` / `**` / a bare
///   directory name with no extension) is EXPANDED into one glob per supported
///   extension by appending `.<ext>` to the trailing `*` (or `/**/*.<ext>` for a
///   bare directory name).
///
/// There is NO brace expansion anywhere — multi-extension coverage is one glob
/// per extension, never `*.{a,b}`.
fn expand_include_glob(
    raw: &NormalizedGlob,
    supported: &SupportedExtensions,
) -> Vec<NormalizedGlob> {
    let pattern = raw.as_str();
    let final_segment = pattern.rsplit('/').next().unwrap_or(pattern);

    if segment_is_extension_specific(final_segment) {
        // Extension-specific — matched literally, never expanded.
        return vec![raw.clone()];
    }

    // Directory / bare-star — expand to one glob per supported extension.
    // If the final segment already ends in `*` (the common `**/*` / `*` form),
    // append the extension directly to the star; otherwise (a bare directory
    // name) append a recursive `/**/*` first.
    let base: String = if final_segment.ends_with('*') {
        pattern.to_string()
    } else {
        format!("{}/**/*", pattern.trim_end_matches('/'))
    };

    supported
        .extensions
        .iter()
        .map(|ext| NormalizedGlob::new(&format!("{base}{ext}")))
        .collect()
}

/// Root a raw `files`/`include`/`exclude` entry at `root`, mirroring TypeScript's
/// tsconfig-relative resolution: an absolute entry (leading `/` or a `x:` drive)
/// is kept; a relative entry is joined onto the project root. When
/// `allow_directory_glob` is set (the `include`/`exclude` case), a bare directory
/// name — no wildcard, no extension in its last segment — expands to a recursive
/// `…/**/*`. Producers that already resolve tsconfig-relative membership (the
/// on-disk `config::resolve_membership_path`) emit absolute entries, so this is a
/// no-op for them and only roots the raw relative entries a bridge-mode caller
/// supplies.
fn root_membership_entry(root: &CanonicalPath, value: &str, allow_directory_glob: bool) -> String {
    let normalized = value.replace('\\', "/");
    let is_absolute = normalized.starts_with('/') || normalized.as_bytes().get(1) == Some(&b':');
    let resolved = if is_absolute {
        normalized
    } else {
        format!(
            "{}/{}",
            root.as_str().trim_end_matches('/'),
            normalized.trim_start_matches("./").trim_start_matches('/')
        )
    };

    if !allow_directory_glob
        || resolved.contains(['*', '?', '['])
        || resolved
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.contains('.'))
    {
        return resolved;
    }

    format!("{resolved}/**/*")
}

/// Is the final glob segment extension-specific (ends in a literal `.<ext>`
/// where `<ext>` carries no further wildcard)?
///
/// `*.ts` → yes (ext `ts`); `foo.tsx` → yes; `*` / `**` / `src` → no;
/// `*.d.ts` → yes (ext after the LAST dot is `ts`).
fn segment_is_extension_specific(segment: &str) -> bool {
    match segment.rfind('.') {
        Some(dot) => {
            let ext = &segment[dot + 1..];
            // A non-empty extension token with no wildcard char is "specific".
            !ext.is_empty() && !ext.contains(['*', '?', '[', ']'])
        }
        None => false,
    }
}

/// Static membership specification parsed from a tsconfig.
///
/// Always explicit — no `MatchAll` variant. When a tsconfig has no
/// `files`, no `include`, no `exclude`, the builder fills in TypeScript
/// defaults: `include: ["{dir}/**/*"]`, `exclude: ["{dir}/node_modules/**", ...]`.
///
/// Glob patterns are stored precompiled ([`CompiledGlob`]): membership
/// match loops run per ownership query, and compiling on every match
/// dominated the query cost. Compilation happens once, at membership
/// construction (snapshot build) time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticMembershipSpec {
    /// Exact file paths. **Immune to exclude** — always members.
    pub files: Vec<CanonicalPath>,
    /// Glob patterns. Builder fills default `["**/*"]` when needed.
    pub include: Vec<CompiledGlob>,
    /// Only filters `include`. Builder fills TS defaults when needed.
    ///
    /// A shared slice: the TS-default exclude set is memoized per root
    /// ([`typescript_default_excludes`]) and shared by every membership
    /// built for that root, so cloning a spec never recompiles globs.
    pub exclude: Arc<[CompiledGlob]>,
}

/// Configured project membership: static spec + materialized file set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredMembership {
    pub spec: StaticMembershipSpec,
    /// Exact set of files determined to be members of this project.
    /// Populated during snapshot build by expanding the static spec.
    pub materialized_files: FxHashSet<CanonicalPath>,
}

impl ConfiguredMembership {
    /// A membership that claims every file under `root` except the TypeScript
    /// default excludes (`node_modules`, …). It carries no materialized set by
    /// design — its member domain is unbounded, so the compiled
    /// `with_typescript_defaults` spec is its permanent ownership authority
    /// (see [`ConfiguredMembership::contains`]).
    ///
    /// This is the resolver-config default (a freshly-constructed
    /// [`IdeProjectConfig`](crate::resolver::IdeProjectConfig)) and the
    /// root-containment membership a fallback (tsconfig-less) config carries:
    /// its `contains` reduces to "under root, not excluded" through the one
    /// shared glob engine — no second glob evaluator.
    ///
    /// [`IdeProjectConfig`]: crate::resolver::IdeProjectConfig
    #[must_use]
    pub fn match_all_under_root(root: &CanonicalPath) -> Self {
        Self {
            spec: StaticMembershipSpec::with_typescript_defaults(root),
            materialized_files: FxHashSet::default(),
        }
    }

    /// Check if a file is a member of this configured project.
    ///
    /// `materialized_files` is a WALK-TIME POSITIVE CACHE, not a negative
    /// ownership authority. It records the spec-matching files that existed
    /// when the snapshot was built (`materialize_from_spec`), so an exact hit
    /// is the fast, precise "yes". A MISS is NOT terminal: it falls through to
    /// `spec.matches` (the compiled `files`/`include`/`exclude` globs), which
    /// is the ownership authority. This is what lets a file CREATED AFTER the
    /// snapshot walk — absent from the materialized set but matching the
    /// project's globs — still resolve to its owning configured project
    /// instead of failing closed to `NoProject`.
    ///
    /// The fall-through cannot over-include. `materialize_from_spec` inserts
    /// exactly the entries `spec.matches` accepts: its directory prune uses
    /// the same `spec.exclude` set, its per-file filter is the identity, and
    /// the walk applies no `.gitignore` / dotfile / extension filtering of its
    /// own — so `spec.matches` IS the walk's effective membership predicate.
    /// An excluded path (`node_modules/**`, an explicit `exclude` glob) or an
    /// unsupported extension is therefore rejected on the fall-through exactly
    /// as it was skipped by the walk.
    ///
    /// An empty `materialized_files` — a filesystem-less environment (WASM,
    /// in-memory workspace) or a spec-defined match-all membership like
    /// [`Self::match_all_under_root`] — simply never hits the positive cache
    /// and routes straight to `spec.matches`. The fast-positive / spec-authority
    /// split needs no `is_empty` discriminator, which dissolves the old
    /// "walked-empty vs filesystem-less" conflation: both route to the one
    /// correct authority.
    pub fn contains(&self, file_path: &CanonicalPath) -> bool {
        // Fast POSITIVE path: an exact hit on the walk-time materialized set.
        if self.materialized_files.contains(file_path) {
            return true;
        }
        // MISS (incl. a file created after the walk, or an empty set): the
        // compiled `files`/`include`/`exclude` spec is the ownership authority.
        // It never over-includes — the walk's effective filter IS `spec.matches`.
        self.spec.matches(file_path)
    }
}

/// Fallback project membership: root-containment minus exclusions.
#[derive(Debug, Clone)]
pub struct FallbackMembership {
    pub root: CanonicalPath,
    /// Precompiled at membership construction — see [`StaticMembershipSpec`].
    pub exclude: Arc<[CompiledGlob]>,
}

impl FallbackMembership {
    /// Check if a file is covered by this fallback project.
    ///
    /// True if: file is under root AND not excluded.
    pub fn contains(&self, file_path: &CanonicalPath) -> bool {
        if !file_path.starts_with_dir(&self.root) {
            return false;
        }
        !self.exclude.iter().any(|glob| glob.matches(file_path))
    }
}

impl StaticMembershipSpec {
    /// Check whether a path is a static member according to TypeScript rules.
    ///
    /// Order: `files` first (immune to exclude), then `include - exclude`.
    /// This fixes the bug where the old code checked exclude before files.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        // Step 1: files are ALWAYS members — immune to exclude
        if self.files.iter().any(|f| f == path) {
            return true;
        }

        // Step 2: check include patterns
        let included = if self.include.is_empty() {
            false
        } else {
            self.include.iter().any(|glob| glob.matches(path))
        };

        if !included {
            return false;
        }

        // Step 3: exclude only filters what include found
        !self.exclude.iter().any(|glob| glob.matches(path))
    }

    /// Create a spec with TypeScript defaults filled in.
    ///
    /// When tsconfig has no `files`, no `include`, no `exclude`:
    /// - `include` defaults to `["{root}/**/*"]`
    /// - `exclude` defaults to `["{root}/node_modules/**", ...]`
    pub fn with_typescript_defaults(root: &CanonicalPath) -> Self {
        Self {
            files: Vec::new(),
            include: vec![CompiledGlob::new(NormalizedGlob::from_root_and_pattern(
                root, "**/*",
            ))],
            exclude: typescript_default_excludes(root),
        }
    }

    /// Build a configured spec from raw membership inputs, applying the
    /// supported-extension expansion rule to the `include` globs.
    ///
    /// `files` entries are exact and IMMUNE — never expanded. `exclude` is
    /// literal / extension-agnostic — never expanded. Each `include` glob is
    /// classified by [`expand_include_glob`]: a directory / bare-star glob
    /// expands into one glob per supported extension; an extension-specific
    /// glob is kept verbatim.
    pub fn from_includes(
        root: &CanonicalPath,
        files: &[&str],
        include: &[&str],
        exclude: &[&str],
        supported: &SupportedExtensions,
    ) -> Self {
        // A relative `files`/`include`/`exclude` entry is rooted at the project
        // root; an already-absolute entry is kept verbatim. `NormalizedGlob`
        // anchors its match against the full canonical path, so an un-rooted
        // relative pattern (`src/**/*`) can never match an absolute canonical id
        // — the pattern MUST be rooted for the static spec (bridge-mode
        // `contains`) to agree with the materialized set. This is the single
        // rooting point (the former resolver-side `normalize_project_membership_entry`).
        let files = files
            .iter()
            .map(|f| CanonicalPath::new(&root_membership_entry(root, f, false)))
            .collect();
        let include = include
            .iter()
            .map(|g| NormalizedGlob::new(&root_membership_entry(root, g, false)))
            .flat_map(|g| expand_include_glob(&g, supported))
            .map(CompiledGlob::new)
            .collect();
        let exclude = exclude
            .iter()
            .map(|g| CompiledGlob::new(NormalizedGlob::new(&root_membership_entry(root, g, true))))
            .collect();
        Self {
            files,
            include,
            exclude,
        }
    }

    /// Create a spec with TypeScript defaults filled in, applying the
    /// supported-extension expansion to the default `**/*` include.
    ///
    /// This is [`with_typescript_defaults`] under the explicit extension model:
    /// the default include behaves as a bare-star glob over the supported
    /// extension set, so an unknown non-carrier extension is NOT a member.
    ///
    /// [`with_typescript_defaults`]: Self::with_typescript_defaults
    pub fn with_supported_extension_defaults(
        root: &CanonicalPath,
        supported: &SupportedExtensions,
    ) -> Self {
        let default_glob = NormalizedGlob::from_root_and_pattern(root, "**/*");
        Self {
            files: Vec::new(),
            include: expand_include_glob(&default_glob, supported)
                .into_iter()
                .map(CompiledGlob::new)
                .collect(),
            exclude: typescript_default_excludes(root),
        }
    }
}

/// TypeScript's default exclude patterns, relative to a project root.
const TYPESCRIPT_DEFAULT_EXCLUDE_PATTERNS: &[&str] =
    &["node_modules/**", "bower_components/**", "jspm_packages/**"];

/// Bound on distinct roots retained by the default-excludes memo. Real
/// processes see a handful of project roots; the bound only protects
/// long-lived processes that touch many transient roots (e.g. in-process
/// test suites over temp dirs). On overflow the memo is cleared — a pure
/// recompute, never a correctness change.
const DEFAULT_EXCLUDES_MEMO_CAP: usize = 64;

/// Process-wide per-root memo for [`typescript_default_excludes`]: the
/// compiled set is a pure function of the root, and membership
/// construction sites run hot (snapshot rebuilds, `IdeProjectConfig::new`,
/// the workspace-default env-hash fallback), so each root compiles its
/// three default-exclude globs exactly once per process.
static DEFAULT_EXCLUDES_MEMO: LazyLock<RwLock<FxHashMap<CanonicalPath, Arc<[CompiledGlob]>>>> =
    LazyLock::new(|| RwLock::new(FxHashMap::default()));

/// TypeScript's default exclude patterns for a project root, precompiled.
///
/// Returns a shared `Arc` slice memoized per root: repeated calls for the
/// same root hand out the same allocation instead of recompiling the glob
/// set (see [`DEFAULT_EXCLUDES_MEMO`]).
pub fn typescript_default_excludes(root: &CanonicalPath) -> Arc<[CompiledGlob]> {
    if let Some(hit) = DEFAULT_EXCLUDES_MEMO.read().get(root) {
        return Arc::clone(hit);
    }

    let compiled: Arc<[CompiledGlob]> = TYPESCRIPT_DEFAULT_EXCLUDE_PATTERNS
        .iter()
        .map(|pattern| CompiledGlob::new(NormalizedGlob::from_root_and_pattern(root, pattern)))
        .collect();

    let mut memo = DEFAULT_EXCLUDES_MEMO.write();
    if memo.len() >= DEFAULT_EXCLUDES_MEMO_CAP && !memo.contains_key(root) {
        memo.clear();
    }
    // `entry` keeps concurrent first-computers converging on ONE shared
    // allocation: the losing thread returns the winner's Arc.
    Arc::clone(memo.entry(root.clone()).or_insert(compiled))
}

#[cfg(test)]
#[path = "membership_tests.rs"]
mod tests;
