use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_compiler::utils::oxc::vue::resolve_type::{
    analyze_external_type_source, imported_member_name_for_required_alias,
    required_import_alias_names_for_binding, AnalyzedExternalTypeSource, ResolvedElements,
};
use verter_workspace::ResolveRequestKind;

#[derive(Debug, Clone, Default)]
pub struct ExternalTypeBodyCache {
    resolved: FxHashMap<(String, String), Option<ResolvedElements>>,
    /// Per-request memo of `(dep_canonical, type_name) → (target_canonical, target_symbol)`.
    /// Populated alongside `resolved` so cache-hit callers avoid a second
    /// `resolve_imported_type_root_in_view` trip through the request view +
    /// imported-root DB. Without this memo every `resolved` hit still pays
    /// a full route walk to derive `target_canonical` for the returned tuple.
    resolved_roots: FxHashMap<(String, String), (String, String)>,
    source_analysis:
        FxHashMap<(String, verter_semantic::analysis::Hash16), AnalyzedExternalTypeSource>,
}

impl ExternalTypeBodyCache {
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    pub fn get(&self, key: &(String, String)) -> Option<&Option<ResolvedElements>> {
        self.resolved.get(key)
    }

    pub fn insert(
        &mut self,
        key: (String, String),
        value: Option<ResolvedElements>,
    ) -> Option<Option<ResolvedElements>> {
        self.resolved.insert(key, value)
    }

    /// Look up a memoized `(target_canonical, target_symbol)` for a previously
    /// resolved `(dep_canonical, type_name)` pair. Returns `None` before a call
    /// to [`record_resolved_root`] for the same key.
    pub fn resolved_root(&self, key: &(String, String)) -> Option<&(String, String)> {
        self.resolved_roots.get(key)
    }

    /// Record the `(target_canonical, target_symbol)` that
    /// `resolve_imported_type_root_in_view` computed for `key`. Idempotent —
    /// re-insertions with the same mapping are safe; different mappings are
    /// overwritten (last-writer-wins matches the resolved-elements semantics).
    pub fn record_resolved_root(&mut self, key: (String, String), target: (String, String)) {
        self.resolved_roots.insert(key, target);
    }

    pub fn source_analysis_len(&self) -> usize {
        self.source_analysis.len()
    }

    pub fn source_analysis(
        &mut self,
        dep_canonical: &str,
        effective_source: &str,
    ) -> (&AnalyzedExternalTypeSource, bool) {
        let key = (
            dep_canonical.to_string(),
            verter_semantic::analysis::hash_16(effective_source.as_bytes()),
        );
        let mut inserted = false;
        let analysis = self.source_analysis.entry(key).or_insert_with(|| {
            inserted = true;
            let alloc = oxc_allocator::Allocator::new();
            analyze_external_type_source(effective_source, &alloc)
        });
        (analysis, inserted)
    }

    pub fn store_source_analysis(
        &mut self,
        dep_canonical: &str,
        effective_source: &str,
        analysis: AnalyzedExternalTypeSource,
    ) -> bool {
        let key = (
            dep_canonical.to_string(),
            verter_semantic::analysis::hash_16(effective_source.as_bytes()),
        );
        match self.source_analysis.entry(key) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(analysis);
                true
            }
        }
    }
}

pub trait ExternalTypeBodyResolver {
    type Error;

    fn max_external_type_resolve_steps(&self) -> usize;

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error;

    fn required_import_names_for_type(
        &self,
        dep_canonical: &str,
        type_name: &str,
        _effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
    ) -> FxHashSet<String> {
        let _ = dep_canonical;
        analysis.required_import_names(type_name)
    }

    fn debug_enabled(&self) -> bool {
        false
    }

    fn debug_log(&self, _message: String) {}

    fn note_cycle_detected(&self) {}

    fn cached_source_analysis(
        &self,
        _dep_canonical: &str,
        _effective_source: &str,
    ) -> Option<AnalyzedExternalTypeSource> {
        None
    }

    /// Called after successfully resolving a type from a source body.
    /// Implementations can use this to populate a host-level cache.
    fn note_resolved_type(
        &self,
        _dep_canonical: &str,
        _type_name: &str,
        _resolved: Option<&ResolvedElements>,
        _tracked_deps: &[String],
    ) {
    }

    fn resolve_external_type_from_analysis(
        &self,
        dep_canonical: &str,
        type_name: &str,
        effective_source: &str,
        analysis: &AnalyzedExternalTypeSource,
        imported_companions: &FxHashMap<String, ResolvedElements>,
    ) -> Option<ResolvedElements>;

    #[allow(clippy::too_many_arguments)]
    fn resolve_external_type_recursive(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
    ) -> Result<Option<ResolvedElements>, Self::Error>;
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_from_source_body<R: ExternalTypeBodyResolver>(
    resolver: &R,
    dep_canonical: &str,
    type_name: &str,
    effective_source: &str,
    tracked_deps: &mut BTreeSet<String>,
    resolution_deps: &mut BTreeSet<String>,
    cache: &mut ExternalTypeBodyCache,
    visiting: &mut FxHashSet<(String, String)>,
    kind: ResolveRequestKind,
    use_host_cache: bool,
    profile_hash: Option<u64>,
    depth: usize,
) -> Result<Option<ResolvedElements>, R::Error> {
    let cache_key = (dep_canonical.to_string(), type_name.to_string());
    let resolve_steps = cache.len() + visiting.len();
    if resolve_steps >= resolver.max_external_type_resolve_steps() {
        return Err(resolver.step_limit_exceeded(type_name, dep_canonical));
    }

    if !visiting.insert(cache_key.clone()) {
        resolver.note_cycle_detected();
        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type cycle dep={} type={}",
                dep_canonical, type_name
            ));
        }
        return Ok(None);
    }

    let analysis_before = cache.source_analysis_len();
    let (analysis, extracted, required_import_names) = {
        let (analysis, inserted) = if let Some(cached) =
            resolver.cached_source_analysis(dep_canonical, effective_source)
        {
            let inserted = cache.store_source_analysis(dep_canonical, effective_source, cached);
            let (analysis, _) = cache.source_analysis(dep_canonical, effective_source);
            (analysis, inserted)
        } else {
            cache.source_analysis(dep_canonical, effective_source)
        };
        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type source-analysis dep={} type={} hit={} cache_entries={}",
                dep_canonical,
                type_name,
                !inserted,
                analysis_before + usize::from(inserted),
            ));
        }
        (
            analysis.clone(),
            analysis.extracted.clone(),
            resolver.required_import_names_for_type(
                dep_canonical,
                type_name,
                effective_source,
                analysis,
            ),
        )
    };
    let projected_steps = cache.len() + visiting.len() + required_import_names.len();
    if projected_steps > resolver.max_external_type_resolve_steps() {
        visiting.remove(&cache_key);
        return Err(resolver.step_limit_exceeded(type_name, dep_canonical));
    }

    if resolver.debug_enabled() {
        let mut required_list = required_import_names.iter().cloned().collect::<Vec<_>>();
        required_list.sort();
        resolver.debug_log(format!(
            "resolve_external_type required-imports dep={} type={} imports=[{}]",
            dep_canonical,
            type_name,
            required_list.join(", "),
        ));
    }

    let mut companion_types = FxHashMap::default();
    let mut attempted_companion_requests = FxHashSet::default();
    for binding in &extracted.bindings {
        let required_aliases =
            required_import_alias_names_for_binding(binding, &required_import_names);
        for required_alias in required_aliases {
            let Some(imported_name) =
                imported_member_name_for_required_alias(binding, &required_alias)
            else {
                continue;
            };
            let request_key = (
                required_alias.clone(),
                binding.source.clone(),
                imported_name.clone(),
            );
            if !attempted_companion_requests.insert(request_key) {
                continue;
            }
            if resolver.debug_enabled() {
                resolver.debug_log(format!(
                    "resolve_external_type companion-binding dep={} type={} binding={} -> {}:{}",
                    dep_canonical, type_name, required_alias, binding.source, imported_name,
                ));
            }
            if let Some(resolved) = resolver.resolve_external_type_recursive(
                dep_canonical,
                &binding.source,
                &imported_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                false,
                kind,
                use_host_cache,
                profile_hash,
                depth + 1,
            )? {
                companion_types.entry(required_alias).or_insert(resolved);
            }
        }
    }

    let used_local_symbol_cache = analysis.has_local_symbol_target(type_name);
    let resolved = resolver.resolve_external_type_from_analysis(
        dep_canonical,
        type_name,
        effective_source,
        &analysis,
        &companion_types,
    );
    if resolver.debug_enabled() {
        resolver.debug_log(format!(
            "resolve_external_type local-eval dep={} type={} strategy={} companion_keys={} resolved={}",
            dep_canonical,
            type_name,
            if used_local_symbol_cache {
                "symbol-cache"
            } else {
                "full-source"
            },
            companion_types.len(),
            resolved.is_some(),
        ));
    }
    visiting.remove(&cache_key);
    if resolver.debug_enabled() {
        resolver.debug_log(format!(
            "resolve_external_type exit dep={} type={} resolved={}",
            dep_canonical,
            type_name,
            resolved.is_some(),
        ));
    }

    resolver.note_resolved_type(
        dep_canonical,
        type_name,
        resolved.as_ref(),
        &tracked_deps.iter().cloned().collect::<Vec<_>>(),
    );

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use verter_compiler::utils::oxc::vue::resolve_type::{
        AnalyzedExternalTypeSource, ResolvedElements, RuntimeType,
    };
    use verter_workspace::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        recursive_results: BTreeMap<(String, String, String), Option<ResolvedElements>>,
        required_import_names: BTreeMap<(String, String), FxHashSet<String>>,
        recursive_calls: RefCell<Vec<(String, String, String)>>,
        logs: RefCell<Vec<String>>,
    }

    impl ExternalTypeBodyResolver for TestResolver {
        type Error = String;

        fn max_external_type_resolve_steps(&self) -> usize {
            64
        }

        fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error {
            format!("step limit exceeded for {type_name} at {last_dep}")
        }

        fn debug_enabled(&self) -> bool {
            true
        }

        fn debug_log(&self, message: String) {
            self.logs.borrow_mut().push(message);
        }

        fn required_import_names_for_type(
            &self,
            dep_canonical: &str,
            type_name: &str,
            _effective_source: &str,
            analysis: &AnalyzedExternalTypeSource,
        ) -> FxHashSet<String> {
            self.required_import_names
                .get(&(dep_canonical.to_string(), type_name.to_string()))
                .cloned()
                .unwrap_or_else(|| analysis.required_import_names(type_name))
        }

        fn resolve_external_type_from_analysis(
            &self,
            _dep_canonical: &str,
            type_name: &str,
            effective_source: &str,
            analysis: &AnalyzedExternalTypeSource,
            imported_companions: &FxHashMap<String, ResolvedElements>,
        ) -> Option<ResolvedElements> {
            let allocator = oxc_allocator::Allocator::new();
            let parsed =
                oxc_parser::Parser::new(&allocator, effective_source, oxc_span::SourceType::ts())
                    .parse();
            (!parsed.panicked).then(|| {
                verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type_in_program_with_analyzed_symbol_companion(
                    type_name,
                    &parsed.program,
                    effective_source.as_bytes(),
                    analysis,
                    imported_companions,
                )
            })?
        }

        fn resolve_external_type_recursive(
            &self,
            owner_canonical: &str,
            import_source: &str,
            type_name: &str,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
            _required_root_dep: bool,
            _kind: ResolveRequestKind,
            _use_host_cache: bool,
            _profile_hash: Option<u64>,
            _depth: usize,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
            self.recursive_calls.borrow_mut().push((
                owner_canonical.to_string(),
                import_source.to_string(),
                type_name.to_string(),
            ));
            Ok(self
                .recursive_results
                .get(&(
                    owner_canonical.to_string(),
                    import_source.to_string(),
                    type_name.to_string(),
                ))
                .cloned()
                .flatten())
        }
    }

    fn empty_elements() -> ResolvedElements {
        ResolvedElements {
            props: Vec::new(),
            emits: Vec::new(),
            has_call_signature: false,
            root_runtime_types: vec![RuntimeType::Object],
        }
    }

    #[test]
    fn resolve_external_type_from_source_body_does_not_follow_direct_reexport_route() {
        let mut resolver = TestResolver::default();
        resolver.recursive_results.insert(
            (
                "/src/types.ts".to_string(),
                "./inner".to_string(),
                "Props".to_string(),
            ),
            Some(empty_elements()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Props",
            "export { Props } from './inner'",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("direct reexport body evaluation should succeed");

        assert!(
            actual.is_none(),
            "body evaluation should not resolve direct reexports on its own"
        );
        assert!(
            resolver.recursive_calls.borrow().is_empty(),
            "body evaluation should not recurse through direct reexports"
        );
    }

    #[test]
    fn resolve_external_type_from_source_body_does_not_scan_wildcard_barrels() {
        let resolver = TestResolver::default();

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Props",
            "export * from './inner'",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("wildcard barrel body evaluation should succeed");

        assert!(
            actual.is_none(),
            "body evaluation should not scan wildcard barrels on its own"
        );
        assert!(
            resolver.recursive_calls.borrow().is_empty(),
            "body evaluation should not recurse when wildcard exports are present"
        );
    }

    #[test]
    fn resolve_external_type_from_source_body_uses_companion_types() {
        let mut resolver = TestResolver::default();
        resolver.recursive_results.insert(
            (
                "/src/types.ts".to_string(),
                "./dep".to_string(),
                "Dep".to_string(),
            ),
            Some(empty_elements()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Props",
            "import type { Dep } from './dep'\nexport type Props = Dep",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("companion resolution should succeed");

        assert!(actual.is_some());
    }

    #[test]
    fn resolve_external_type_from_source_body_reuses_source_analysis_for_same_dep() {
        let mut resolver = TestResolver::default();
        resolver.recursive_results.insert(
            (
                "/src/types.ts".to_string(),
                "./dep".to_string(),
                "Dep".to_string(),
            ),
            Some(empty_elements()),
        );

        let source = "\
import type { Dep } from './dep'\n\
export type Props = Dep\n\
export type Emits = Dep\n";

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let props = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Props",
            source,
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("first external type resolution should succeed");

        assert!(props.is_some());
        assert_eq!(cache.source_analysis_len(), 1);

        let emits = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Emits",
            source,
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("second external type resolution should succeed");

        assert!(emits.is_some());
        assert_eq!(cache.source_analysis_len(), 1);
    }

    #[test]
    fn resolve_external_type_from_source_body_dedupes_duplicate_companion_requests() {
        let mut resolver = TestResolver::default();
        resolver.recursive_results.insert(
            (
                "/src/types.ts".to_string(),
                "./dep".to_string(),
                "Dep".to_string(),
            ),
            Some(empty_elements()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Props",
            "import type { Dep } from './dep'\nimport type { Dep } from './dep'\nexport type Props = Dep",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("duplicate companion resolution should succeed");

        assert!(actual.is_some());
        assert_eq!(
            resolver.recursive_calls.borrow().as_slice(),
            &[(
                "/src/types.ts".to_string(),
                "./dep".to_string(),
                "Dep".to_string(),
            )],
            "duplicate companion requests should only resolve once per alias/source/import tuple",
        );
    }

    #[test]
    fn resolve_external_type_from_source_body_prefers_precise_required_import_names() {
        let mut resolver = TestResolver::default();
        resolver.required_import_names.insert(
            ("/src/types.ts".to_string(), "Avatar".to_string()),
            FxHashSet::from_iter(["Used".to_string()]),
        );
        resolver.recursive_results.insert(
            (
                "/src/types.ts".to_string(),
                "./used".to_string(),
                "Used".to_string(),
            ),
            Some(empty_elements()),
        );

        let mut tracked = BTreeSet::new();
        let mut resolution = BTreeSet::new();
        let mut cache = ExternalTypeBodyCache::default();
        let mut visiting = FxHashSet::default();

        let actual = resolve_external_type_from_source_body(
            &resolver,
            "/src/types.ts",
            "Avatar",
            "import type { Used } from './used'\nimport type { Unused } from './unused'\nexport type Avatar = { active: Used; inactive: Unused }['active']",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("precise companion resolution should succeed");

        assert!(actual.is_some());
        assert_eq!(
            resolver.recursive_calls.borrow().as_slice(),
            &[(
                "/src/types.ts".to_string(),
                "./used".to_string(),
                "Used".to_string(),
            )],
            "precise required import names should prevent resolving unrelated sibling companions",
        );
    }
}
