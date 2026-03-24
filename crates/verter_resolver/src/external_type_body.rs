use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_core::utils::oxc::vue::resolve_type::{
    collect_required_import_names_for_external_type, extract_imported_type_bindings,
    imported_member_name_for_required_alias, required_import_alias_names_for_binding,
    resolve_external_type_with_companion, ResolvedElements,
};
use verter_vfs::ResolveRequestKind;

pub type ExternalTypeBodyCache = FxHashMap<(String, String), Option<ResolvedElements>>;

pub trait ExternalTypeBodyResolver {
    type Error;

    fn max_external_type_resolve_steps(&self) -> usize;

    fn step_limit_exceeded(&self, type_name: &str, last_dep: &str) -> Self::Error;

    fn debug_enabled(&self) -> bool {
        false
    }

    fn debug_log(&self, _message: String) {}

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

    #[allow(clippy::too_many_arguments)]
    fn resolve_type_through_barrel(
        &self,
        barrel_canonical: &str,
        type_name: &str,
        wildcard_sources: &[String],
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut ExternalTypeBodyCache,
        visiting: &mut FxHashSet<(String, String)>,
        kind: ResolveRequestKind,
        use_host_cache: bool,
        profile_hash: Option<u64>,
        depth: usize,
        debug_enabled: bool,
    ) -> Result<Option<ResolvedElements>, Self::Error>;
}

#[allow(clippy::too_many_arguments)]
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
        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type cycle dep={} type={}",
                dep_canonical, type_name
            ));
        }
        return Ok(None);
    }

    let alloc = oxc_allocator::Allocator::new();
    let extracted = extract_imported_type_bindings(effective_source, &alloc);
    let required_import_names =
        collect_required_import_names_for_external_type(type_name, effective_source, &alloc);
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

    let direct_reexport = extracted
        .reexport_bindings
        .iter()
        .find(|binding| binding.local_name == type_name);
    if let Some(target) = direct_reexport {
        if resolver.debug_enabled() {
            resolver.debug_log(format!(
                "resolve_external_type direct-reexport dep={} type={} -> {}:{}",
                dep_canonical, type_name, target.source, target.imported_name
            ));
        }
        if let Some(resolved) = resolver.resolve_external_type_recursive(
            dep_canonical,
            &target.source,
            &target.imported_name,
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
            visiting.remove(&cache_key);
            return Ok(Some(resolved));
        }
    }

    let mut companion_types = FxHashMap::default();
    for binding in &extracted.bindings {
        let required_aliases =
            required_import_alias_names_for_binding(binding, &required_import_names);
        for required_alias in required_aliases {
            let Some(imported_name) =
                imported_member_name_for_required_alias(binding, &required_alias)
            else {
                continue;
            };
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

    let resolve_alloc = oxc_allocator::Allocator::new();
    let mut resolved = resolve_external_type_with_companion(
        type_name,
        effective_source,
        &companion_types,
        &resolve_alloc,
    );
    if resolver.debug_enabled() {
        resolver.debug_log(format!(
            "resolve_external_type local-eval dep={} type={} companion_keys={} resolved={}",
            dep_canonical,
            type_name,
            companion_types.len(),
            resolved.is_some(),
        ));
    }

    if resolved.is_none() && !extracted.wildcard_reexport_sources.is_empty() {
        resolved = resolver.resolve_type_through_barrel(
            dep_canonical,
            type_name,
            &extracted.wildcard_reexport_sources,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            kind,
            use_host_cache,
            profile_hash,
            depth,
            resolver.debug_enabled(),
        )?;
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

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_external_type_from_source_body, ExternalTypeBodyCache, ExternalTypeBodyResolver,
    };
    use rustc_hash::FxHashSet;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use verter_core::utils::oxc::vue::resolve_type::{ResolvedElements, RuntimeType};
    use verter_vfs::ResolveRequestKind;

    #[derive(Default)]
    struct TestResolver {
        recursive_results: BTreeMap<(String, String, String), Option<ResolvedElements>>,
        barrel_results: BTreeMap<(String, String), Option<ResolvedElements>>,
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

        fn resolve_type_through_barrel(
            &self,
            barrel_canonical: &str,
            type_name: &str,
            _wildcard_sources: &[String],
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut ExternalTypeBodyCache,
            _visiting: &mut FxHashSet<(String, String)>,
            _kind: ResolveRequestKind,
            _use_host_cache: bool,
            _profile_hash: Option<u64>,
            _depth: usize,
            _debug_enabled: bool,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
            Ok(self
                .barrel_results
                .get(&(barrel_canonical.to_string(), type_name.to_string()))
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
    fn resolve_external_type_from_source_body_follows_direct_reexport() {
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
        .expect("direct reexport resolution should succeed");

        assert!(actual.is_some());
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
}
