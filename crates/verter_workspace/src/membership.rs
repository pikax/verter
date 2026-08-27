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
use rustc_hash::FxHashSet;
use std::sync::Arc;
use verter_semantic::resolver_core::{
    typescript_default_excludes, CompiledGlob, ConfiguredMembership, NormalizedGlob,
    StaticMembershipSpec,
};

/// Membership filter accepted from tsconfig project configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectMembership {
    #[default]
    MatchAll,
    IncludeExclude {
        files: Vec<String>,
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

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

#[must_use]
pub fn configured_membership_match_all_under_root(root: &CanonicalPath) -> ConfiguredMembership {
    ConfiguredMembership {
        spec: static_membership_with_typescript_defaults(root),
        materialized_files: FxHashSet::default(),
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

#[must_use]
pub fn static_membership_with_typescript_defaults(root: &CanonicalPath) -> StaticMembershipSpec {
    StaticMembershipSpec {
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
pub fn static_membership_from_includes(
    root: &CanonicalPath,
    files: &[&str],
    include: &[&str],
    exclude: &[&str],
    supported: &SupportedExtensions,
) -> StaticMembershipSpec {
    // A relative `files`/`include`/`exclude` entry is rooted at the project
    // root; an already-absolute entry is kept verbatim. `NormalizedGlob`
    // anchors its match against the full canonical path, so an un-rooted
    // relative pattern (`src/**/*`) can never match an absolute canonical id
    // — the pattern MUST be rooted for the static spec (`contains`) to agree
    // with the materialized set. This is the single rooting point.
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
    StaticMembershipSpec {
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
pub fn static_membership_with_supported_extension_defaults(
    root: &CanonicalPath,
    supported: &SupportedExtensions,
) -> StaticMembershipSpec {
    let default_glob = NormalizedGlob::from_root_and_pattern(root, "**/*");
    StaticMembershipSpec {
        files: Vec::new(),
        include: expand_include_glob(&default_glob, supported)
            .into_iter()
            .map(CompiledGlob::new)
            .collect(),
        exclude: typescript_default_excludes(root),
    }
}

#[cfg(test)]
#[path = "membership_tests.rs"]
mod tests;
