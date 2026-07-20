//! Top-level types, traces, and helpers shared across the host_resolve
//! sub-modules.
//!
//! Owns:
//! - Request-local route shallow-state cache.
//! - The `DirectComponentMetaDeclarationResolver` adapter used when
//!   building imported macro declarations.
//! - Wildcard re-export ranking helpers.

use std::sync::Arc;

use crate::VerterHost;

/// A routed shallow-state serve plus its publication status — the
/// value-flow carrier for the ReturnOnly discriminant on the frontier
/// route readers (`route_shallow_state_serve` /
/// `routed_shallow_state_serve`), the shallow-state sibling of
/// `host_manage::prepared_decl::IndexedReadyServe`.
///
/// `store_published == false` means the state came from a FENCED
/// `IndexedReady` flight (a workspace or route-resolution mutation
/// landed mid-flight; the artifact was built against superseded state
/// and published nothing). The serve is valid for the requesting
/// caller's read, but any derived value entering a SHARED cache must
/// consult the flag and decline admission — the derived value's fact
/// stamps are read from the LIVE post-mutation state while its payload
/// was computed FROM the superseded surface, an entry the read-side
/// fact rail cannot reject.
#[derive(Clone)]
pub(crate) struct RoutedShallowServe {
    pub(crate) state: Arc<crate::resolver_core::ShallowFileState>,
    pub(crate) store_published: bool,
}

/// Request-scoped frontier shallow-state memo (per-walk de-dupe of
/// repeated `Arc` clones — NOT a host-side mirror).
///
/// Besides the memo it accumulates whether ANY serve observed through
/// it was fenced (`store_published == false`). The route walk threads
/// one cache per route entry, so the accumulator is per-entry-precise:
/// `build_named_type_export_route_entry` consults it to route a result
/// computed from a superseded surface through the strict-admission
/// negative-cache pattern (empty fact signature — served, never
/// persisted). Every serve exit in `route_shallow_state_serve` records
/// itself here, including the edge-stale rebuild arm that bypasses the
/// memo, so the accumulator cannot under-report.
#[derive(Default)]
pub(crate) struct RouteShallowStateCache {
    states: rustc_hash::FxHashMap<String, RoutedShallowServe>,
    fenced_serve_observed: bool,
}

impl RouteShallowStateCache {
    pub(crate) fn get(&self, canonical: &str) -> Option<&RoutedShallowServe> {
        let cached = self.states.get(canonical)?;
        if !cached.store_published {
            // A memoized FENCED serve consumed by a traced cold compute
            // that opened AFTER the original serve was recorded would
            // otherwise miss the chokepoint flag — re-flag on every
            // memo read so the by-value rail cannot under-report.
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
            );
        }
        Some(cached)
    }

    pub(crate) fn insert(&mut self, canonical: String, serve: RoutedShallowServe) {
        self.states.insert(canonical, serve);
    }

    pub(crate) fn observe_serve(&mut self, serve: &RoutedShallowServe) {
        self.fenced_serve_observed |= !serve.store_published;
    }

    /// TRUE when any serve recorded through this cache was fenced
    /// (ReturnOnly) — the walk's result must not enter a shared cache.
    pub(crate) fn fenced_serve_observed(&self) -> bool {
        self.fenced_serve_observed
    }
}
pub(crate) struct DirectComponentMetaDeclarationResolver<'a> {
    pub host: &'a VerterHost,
}

impl crate::resolver_core::DeclarationMetadataResolver
    for DirectComponentMetaDeclarationResolver<'_>
{
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _dep_owner: verter_type_expr::TopLevelOwnerId,
        _requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<verter_span::Span> {
        None
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        (owner == verter_type_expr::TopLevelOwnerId::ordinary_file())
            .then(|| {
                self.host
                    .local_type_declaration_id(canonical_source, resolved_name)
            })
            .flatten()
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let state = self.host.shallow_file_state(canonical_source)?;
        let (symbol_kind, span) = state.type_symbol_metadata_in(owner, resolved_name)?;
        let kind = match symbol_kind {
            verter_semantic::analysis::type_eval::TypeDeclKind::Alias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_semantic::analysis::type_eval::TypeDeclKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_semantic::analysis::type_eval::TypeDeclKind::Class => {
                crate::resolver_core::ResolvedDeclarationKind::Class
            }
        };
        Some(crate::resolver_core::ResolvedLocalTypeSymbolMetadata { kind, span })
    }
}

pub(crate) fn wildcard_source_stem_for_matching(path: &str) -> Option<String> {
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut stem = segments.pop()?;

    // Strip a known declaration / script suffix, then any registered framework
    // CARRIER extension (`.vue`, `.svelte`, …), so a `.svelte` wildcard route
    // source derives the same bare component stem as a `.vue` one. The carrier
    // extensions come from the language registry — never a hardcoded `.vue`
    // arm that would leave other carriers' extensions in the stem.
    const SCRIPT_SUFFIXES: [&str; 9] = [
        ".d.ts", ".d.mts", ".d.cts", ".tsx", ".ts", ".jsx", ".js", ".mts", ".cts",
    ];
    if let Some(stripped) = SCRIPT_SUFFIXES.iter().find_map(|s| stem.strip_suffix(s)) {
        stem = stripped;
    } else {
        for ext in verter_language::LanguageRegistry::global().carrier_extensions() {
            let suffix = format!(".{ext}");
            if let Some(stripped) = stem.strip_suffix(suffix.as_str()) {
                stem = stripped;
                break;
            }
        }
    }

    if stem == "index" {
        stem = segments.pop()?;
    }

    let mut normalized = String::new();
    let mut uppercase_next = true;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                normalized.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                normalized.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) fn wildcard_match_score(
    exported_name: &str,
    wildcard: &crate::resolver_core::WildcardReexport,
) -> usize {
    let candidate = if wildcard.canonical_id.is_empty() {
        wildcard.source_specifier.as_str()
    } else {
        wildcard.canonical_id.as_str()
    };
    let Some(stem) = wildcard_source_stem_for_matching(candidate) else {
        return 0;
    };
    if exported_name.starts_with(stem.as_str()) {
        stem.len()
    } else {
        0
    }
}

pub(crate) fn ordered_wildcard_indices_for_exported_name(
    wildcards: &[crate::resolver_core::WildcardReexport],
    exported_name: &str,
) -> Vec<usize> {
    let mut scored = wildcards
        .iter()
        .enumerate()
        .map(|(index, wildcard)| (index, wildcard_match_score(exported_name, wildcard)))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_index.cmp(right_index))
    });
    scored.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod wildcard_stem_tests {
    use super::wildcard_source_stem_for_matching;

    // F4: the wildcard route source-stem derivation must strip ANY registered
    // carrier extension, not just `.vue`. Pre-fix, a `.svelte` source kept its
    // extension in the stem (`Widget.svelte` → `WidgetSvelte`), stranding it
    // below the `.vue` parity.

    #[test]
    fn strips_svelte_carrier_extension_like_vue() {
        // `.vue` → bare PascalCase stem…
        assert_eq!(
            wildcard_source_stem_for_matching("/src/widget.vue").as_deref(),
            Some("Widget")
        );
        // …and `.svelte` strips identically (the F4 fix). Pre-fix the `.svelte`
        // extension survived into the normalized stem.
        assert_eq!(
            wildcard_source_stem_for_matching("/src/widget.svelte").as_deref(),
            Some("Widget")
        );
    }

    #[test]
    fn strips_script_suffixes_and_handles_index() {
        assert_eq!(
            wildcard_source_stem_for_matching("/routes/profile.ts").as_deref(),
            Some("Profile")
        );
        assert_eq!(
            wildcard_source_stem_for_matching("/routes/user.d.ts").as_deref(),
            Some("User")
        );
        // `index.<ext>` falls back to the parent directory segment.
        assert_eq!(
            wildcard_source_stem_for_matching("/routes/settings/index.svelte").as_deref(),
            Some("Settings")
        );
    }
}
