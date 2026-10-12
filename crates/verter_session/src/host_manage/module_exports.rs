//! Whole-module value export routing. Tables share terminal bindings instead
//! of replaying a barrel search for every namespace property.

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::analysis::route_inventory::RouteCapability;
use verter_session_query::declarations::module_exports::{ModuleExportBinding, ModuleExportTable};
use verter_session_query::facts::fact_cache::{signature_entries, FactVersionRef};
use verter_session_query::facts::fact_read_set::{seal_canonical_signature, FactReadSetFinalise};
use verter_session_query::facts::store_view::StoreView;
use verter_session_query::inputs::shallow::{ExportTarget, LexicalValueBinding};
use verter_session_query::type_solver::ResolvedRootIdentity;
use verter_type_engine::fact_signature_helpers::{
    install_fact_tracer, with_cacheability_scope, FactTracerBasisSource,
};
use verter_type_engine::resolver_core::ResolverContext;

use crate::resolver_core::route_db::{BarrelSurfaceKey, ExportTableFlight};
use crate::resolver_core::{HostCapabilities, SingleflightRole};
use crate::VerterHost;

type Context<'a> = &'a dyn ResolverContext<HostCapabilities>;
type Bindings = FxHashMap<Arc<str>, Option<ModuleExportBinding>>;

#[derive(Default)]
struct ModuleInputs {
    masked: FxHashSet<Arc<str>>,
    local: Bindings,
    named: Vec<(Arc<str>, String, Arc<str>)>,
    stars: Vec<String>,
    facts: FxHashSet<FactVersionRef>,
    evidence: Option<FactReadSetFinalise>,
}

fn table_key(ctx: Context<'_>, canonical: &str) -> BarrelSurfaceKey {
    let env = ctx.host_view_env_hashes_for(canonical);
    BarrelSurfaceKey::new(
        canonical,
        ctx.host_view_project_identity_for(canonical),
        env.resolve_env_hash,
        env.lib_env_hash,
    )
}

impl VerterHost {
    pub(crate) fn module_export_table_with_context(
        &self,
        ctx: Context<'_>,
        view: &dyn StoreView,
        canonical: &str,
    ) -> Option<Arc<ModuleExportTable>> {
        let canonical = ctx.normalized_analysis_canonical(canonical);
        let db = &self.resolver.runtime.routes;
        let key = table_key(ctx, &canonical);
        let source = FactTracerBasisSource::from_ctx(ctx);
        with_cacheability_scope(&source, |probe| {
            if let Some(table) =
                db.module_exports
                    .get_if_valid_self_rooted(&key, view, &[canonical.as_str()])
            {
                return Some(table);
            }
            let build = || {
                let _build = db.module_export_build_lock.lock();
                if let Some(table) =
                    db.module_exports
                        .get_if_valid_self_rooted(&key, view, &[canonical.as_str()])
                {
                    return Ok(ExportTableFlight {
                        table,
                        admitted: true,
                    });
                }
                let (table, cacheable) =
                    self.build_module_export_tables(ctx, view, &canonical, || {
                        !probe.non_cacheable()
                    })?;
                Ok(ExportTableFlight {
                    table,
                    admitted: cacheable && !probe.non_cacheable(),
                })
            };
            let run = db
                .module_export_singleflight
                .run_retaining(key.clone(), view.compat_token(), build, |outcome| {
                    outcome.admitted
                })
                .ok()?;
            if run.value.admitted {
                if let Some(table) =
                    db.module_exports
                        .get_if_valid_self_rooted(&key, view, &[canonical.as_str()])
                {
                    return Some(table);
                }
            }
            // A follower cannot adopt an unrootable leader's answer: compute
            // it from its own snapshot and carry non-admission to its caller.
            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
            if matches!(run.role, SingleflightRole::Leader) {
                Some(Arc::clone(&run.value.table))
            } else {
                let _build = db.module_export_build_lock.lock();
                self.build_module_export_tables(ctx, view, &canonical, || false)
                    .ok()
                    .map(|result| result.0)
            }
        })
        .0
    }

    fn read_module_export_inputs(&self, ctx: Context<'_>, canonical: &str) -> Option<ModuleInputs> {
        let source = FactTracerBasisSource::from_ctx(ctx);
        let (inputs, signature) = install_fact_tracer(&source, || {
            let indexed = ctx.ensure_indexed_ready_serve(canonical)?.indexed;
            let shallow = &indexed.shallow_state;
            let witness = crate::host_manage::import_route_witness::ResolutionWitnessScope::enter();
            let mut inputs = ModuleInputs::default();
            let whole = FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_owned(),
                hash: indexed.whole_hash,
            };
            inputs.facts.insert(whole.clone());
            ctx.observe_borrowed_signature(&[whole]);
            let type_only_imports: FxHashSet<_> = shallow
                .route_inventory
                .imports
                .iter()
                .filter(|route| route.capability == RouteCapability::TypeOnly)
                .map(|route| (route.owner, route.local.as_str()))
                .collect();
            let type_only_exports: FxHashSet<_> = shallow
                .route_inventory
                .local_exports
                .iter()
                .filter(|route| route.capability == RouteCapability::TypeOnly)
                .map(|route| route.exported.as_str())
                .collect();
            let namespace_exports: FxHashSet<_> = shallow
                .route_inventory
                .wildcard_reexports
                .iter()
                .filter_map(|route| route.exported_namespace.as_deref())
                .collect();
            for (name, target) in &shallow.exports {
                self.count_module_export_work(1);
                let name: Arc<str> = Arc::from(name.as_str());
                inputs.masked.insert(Arc::clone(&name));
                if type_only_exports.contains(name.as_ref())
                    || namespace_exports.contains(name.as_ref())
                {
                    continue;
                }
                match target {
                    ExportTarget::Local { owner, symbol_name } => {
                        if type_only_imports.contains(&(*owner, symbol_name.as_str())) {
                            continue;
                        }
                        let root = ResolvedRootIdentity::new_in_owner(
                            canonical,
                            *owner,
                            symbol_name.as_str(),
                        );
                        match shallow.visible_value_binding(*owner, symbol_name) {
                            Some(LexicalValueBinding::Import(import))
                                if import.is_namespace && !import.is_import_equals() =>
                            {
                                if let Some(dependency) = ctx.resolve_type_dependency_canonical(
                                    canonical,
                                    &import.source_specifier,
                                ) {
                                    inputs.local.insert(
                                        name,
                                        Some(ModuleExportBinding::Namespace(Arc::from(dependency))),
                                    );
                                }
                            }
                            Some(LexicalValueBinding::Import(import)) if !import.is_namespace => {
                                if let Some(dependency) = ctx.resolve_type_dependency_canonical(
                                    canonical,
                                    &import.source_specifier,
                                ) {
                                    inputs.named.push((
                                        name,
                                        dependency,
                                        Arc::from(import.imported_name.as_str()),
                                    ));
                                }
                            }
                            Some(_) => {
                                inputs
                                    .local
                                    .insert(name, Some(ModuleExportBinding::Value(root)));
                            }
                            None if shallow.headers.namespace_is_instantiated(
                                None,
                                *owner,
                                symbol_name,
                            ) =>
                            {
                                inputs
                                    .local
                                    .insert(name, Some(ModuleExportBinding::Value(root)));
                            }
                            None => {}
                        }
                    }
                    ExportTarget::Reexport {
                        source_specifier,
                        original_name,
                        is_type: false,
                    } => {
                        if let Some(dependency) =
                            ctx.resolve_type_dependency_canonical(canonical, source_specifier)
                        {
                            inputs.named.push((
                                name,
                                dependency,
                                Arc::from(original_name.as_str()),
                            ));
                        }
                    }
                    ExportTarget::Reexport { is_type: true, .. } => {}
                }
            }
            // The authored inventory distinguishes `export type *` and
            // `export * as NS`; neither is an ordinary value star edge.
            let ordinary = verter_type_expr::TopLevelOwnerId::ordinary_file();
            for route in &shallow.route_inventory.wildcard_reexports {
                if route.owner != ordinary || route.capability == RouteCapability::TypeOnly {
                    continue;
                }
                self.count_module_export_work(1);
                let Some(dependency) =
                    ctx.resolve_type_dependency_canonical(canonical, &route.source)
                else {
                    continue;
                };
                if let Some(name) = &route.exported_namespace {
                    inputs.local.insert(
                        Arc::from(name.as_str()),
                        Some(ModuleExportBinding::Namespace(Arc::from(dependency))),
                    );
                } else {
                    inputs.stars.push(dependency);
                }
            }
            let route_facts = witness.collected();
            ctx.observe_borrowed_signature(&route_facts);
            inputs.facts.extend(route_facts);
            Some(inputs)
        });
        let mut inputs = inputs?;
        match &signature {
            FactReadSetFinalise::Ok(facts) | FactReadSetFinalise::NonCacheable(facts) => {
                inputs.facts.extend(signature_entries(facts).cloned());
            }
            FactReadSetFinalise::MutationUnstable => {}
        }
        inputs.evidence = Some(signature);
        Some(inputs)
    }

    fn build_module_export_tables(
        &self,
        ctx: Context<'_>,
        view: &dyn StoreView,
        canonical: &str,
        can_admit: impl Fn() -> bool,
    ) -> Result<(Arc<ModuleExportTable>, bool), ()> {
        let db = &self.resolver.runtime.routes;
        let mut inputs: FxHashMap<String, ModuleInputs> = FxHashMap::default();
        let mut tables: FxHashMap<String, Arc<ModuleExportTable>> = FxHashMap::default();
        let mut parents: FxHashMap<String, Vec<String>> = FxHashMap::default();
        let mut order = Vec::new();
        let mut pending = vec![canonical.to_owned()];
        // Iterative discovery reads each module once, sharing cached children.
        while let Some(module) = pending.pop() {
            if inputs.contains_key(&module) || tables.contains_key(&module) {
                continue;
            }
            if let Some(table) = db.module_exports.get_if_valid_self_rooted(
                &table_key(ctx, &module),
                view,
                &[module.as_str()],
            ) {
                ctx.observe_borrowed_signature(&table.facts);
                tables.insert(module, table);
                continue;
            }
            let input = self.read_module_export_inputs(ctx, &module).ok_or(())?;
            self.count_module_export_work(1);
            let dependencies: FxHashSet<_> = input
                .stars
                .iter()
                .chain(input.named.iter().map(|(_, module, _)| module))
                .collect();
            for dependency in dependencies {
                parents
                    .entry(dependency.clone())
                    .or_default()
                    .push(module.clone());
                pending.push(dependency.clone());
            }
            tables.insert(
                module.clone(),
                Arc::new(ModuleExportTable {
                    bindings: input.local.clone(),
                    facts: seal_canonical_signature(input.facts.iter().cloned().collect()),
                }),
            );
            order.push(module.clone());
            inputs.insert(module, input);
        }
        let mut queue: VecDeque<String> = order.iter().rev().cloned().collect();
        let mut queued: FxHashSet<String> = queue.iter().cloned().collect();
        // Acyclic barrels fold once in postorder. Cycles converge over the
        // finite binding lattice: absent -> one identity -> ambiguous. No
        // recursive descent or partially-built cyclic table is published.
        while let Some(module) = queue.pop_front() {
            queued.remove(&module);
            let input = &inputs[&module];
            let mut bindings = input.local.clone();
            let mut facts = input.facts.clone();
            let mut observed_dependencies = FxHashSet::default();
            for (name, dependency, original) in &input.named {
                self.count_module_export_work(1);
                let table = &tables[dependency];
                if observed_dependencies.insert(dependency) {
                    facts.extend(signature_entries(&table.facts).cloned());
                }
                if let Some(binding) = table.bindings.get(original) {
                    bindings.insert(Arc::clone(name), binding.clone());
                }
            }
            for dependency in &input.stars {
                let table = &tables[dependency];
                if observed_dependencies.insert(dependency) {
                    facts.extend(signature_entries(&table.facts).cloned());
                }
                for (name, binding) in &table.bindings {
                    self.count_module_export_work(1);
                    if name.as_ref() == "default" || input.masked.contains(name) {
                        continue;
                    }
                    bindings
                        .entry(Arc::clone(name))
                        .and_modify(|existing| {
                            if existing != binding {
                                *existing = None;
                            }
                        })
                        .or_insert_with(|| binding.clone());
                }
            }
            let facts = seal_canonical_signature(facts.into_iter().collect());
            let old = &tables[&module];
            if bindings != old.bindings || facts != old.facts {
                tables.insert(
                    module.clone(),
                    Arc::new(ModuleExportTable { bindings, facts }),
                );
                for parent in parents.get(&module).into_iter().flatten() {
                    if queued.insert(parent.clone()) {
                        queue.push_back(parent.clone());
                    }
                }
            }
        }
        // A refused child makes every derived table return-only. Sampling the
        // outer scope also closes fences raised during dependency resolution.
        let cacheable = inputs
            .values()
            .all(|input| matches!(input.evidence, Some(FactReadSetFinalise::Ok(_))))
            && can_admit();
        let mut admitted = false;
        if cacheable {
            for module in &order {
                let table = Arc::clone(&tables[module]);
                let stored = db.module_exports.insert_arc_with_kind(
                    table_key(ctx, module),
                    Arc::clone(&table),
                    table.facts.to_vec(),
                    "route_db.module_exports",
                );
                if module == canonical {
                    admitted = stored.is_some();
                }
            }
        }
        Ok((Arc::clone(&tables[canonical]), admitted))
    }

    fn count_module_export_work(&self, units: u64) {
        #[cfg(any(test, feature = "test-support"))]
        self.resolver
            .runtime
            .routes
            .module_export_work
            .fetch_add(units, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = units;
    }
}
