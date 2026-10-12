//! Whole-module value export routing. Tables share terminal bindings instead
//! of replaying a barrel search for every namespace property.

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::analysis::route_inventory::RouteCapability;
use verter_session_query::declarations::module_exports::{
    ModuleExportBinding, ModuleExportBindings, ModuleExportRetention, ModuleExportTable,
};
use verter_session_query::facts::fact_cache::{signature_entries, FactVersionRef, ResultReceipt};
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
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
type Bindings = ModuleExportBindings;

#[derive(Default)]
struct ModuleInputs {
    masked: FxHashSet<Arc<str>>,
    local: Bindings,
    named: Vec<(Arc<str>, String, Arc<str>)>,
    stars: Vec<String>,
    facts: FxHashSet<FactVersionRef>,
    evidence: Option<FactReadSetFinalise>,
}

impl ModuleInputs {
    fn dependencies(&self) -> FxHashSet<&String> {
        self.stars
            .iter()
            .chain(self.named.iter().map(|(_, module, _)| module))
            .collect()
    }
}

/// Iterative Kosaraju decomposition. The result is dependency-first, so
/// proofs can share already-complete children without cyclic Arc ownership.
fn export_components(
    inputs: &FxHashMap<String, ModuleInputs>,
    parents: &FxHashMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    let mut visited = FxHashSet::default();
    let mut finished = Vec::new();
    for module in inputs.keys() {
        let mut pending = vec![(module.clone(), false)];
        while let Some((module, finish)) = pending.pop() {
            if finish {
                finished.push(module);
                continue;
            }
            if !visited.insert(module.clone()) {
                continue;
            }
            pending.push((module.clone(), true));
            for dependency in inputs[&module].dependencies() {
                if inputs.contains_key(dependency) {
                    pending.push((dependency.clone(), false));
                }
            }
        }
    }
    visited.clear();
    let mut components = Vec::new();
    for module in finished.into_iter().rev() {
        if visited.contains(&module) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![module];
        while let Some(module) = pending.pop() {
            if !visited.insert(module.clone()) {
                continue;
            }
            pending.extend(parents.get(&module).into_iter().flatten().cloned());
            component.push(module);
        }
        components.push(component);
    }
    components.reverse();
    components
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
                        rootable: true,
                    });
                }
                let (table, admitted, rootable) =
                    self.build_module_export_tables(ctx, view, &canonical, || {
                        !probe.non_cacheable()
                    })?;
                Ok(ExportTableFlight {
                    table,
                    admitted: admitted && !probe.non_cacheable(),
                    rootable: rootable && !probe.non_cacheable(),
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
            if !run.value.rootable {
                verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                    verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
                );
            }
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
                                } else {
                                    inputs
                                        .local
                                        .insert(name, Some(ModuleExportBinding::Value(root)));
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
                                } else {
                                    // Ambient imports resolve through TypeOf's
                                    // ambient owner, not a file export route.
                                    inputs
                                        .local
                                        .insert(name, Some(ModuleExportBinding::Value(root)));
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
    ) -> Result<(Arc<ModuleExportTable>, bool, bool), ()> {
        let db = &self.resolver.runtime.routes;
        let mut inputs: FxHashMap<String, ModuleInputs> = FxHashMap::default();
        let mut tables: FxHashMap<String, Arc<ModuleExportTable>> = FxHashMap::default();
        let mut parents: FxHashMap<String, Vec<String>> = FxHashMap::default();
        let mut pending = vec![canonical.to_owned()];
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
            for dependency in input.dependencies() {
                parents
                    .entry(dependency.clone())
                    .or_default()
                    .push(module.clone());
                pending.push(dependency.clone());
            }
            inputs.insert(module, input);
        }
        // Dependency-first SCCs permit one fold per acyclic module. Only
        // cyclic components need the finite absent/binding/ambiguous worklist.
        let components = export_components(&inputs, &parents);
        let rootable = inputs
            .values()
            .all(|input| matches!(input.evidence, Some(FactReadSetFinalise::Ok(_))));
        let cacheable = rootable && can_admit();
        let mut admitted = false;
        for component in components {
            let component_set: FxHashSet<_> = component.iter().cloned().collect();
            let mut bindings: FxHashMap<String, Bindings> = component
                .iter()
                .map(|module| (module.clone(), inputs[module].local.clone()))
                .collect();
            let mut writes: FxHashMap<String, usize> = FxHashMap::default();
            let mut queue: VecDeque<_> = component.iter().cloned().collect();
            let mut queued: FxHashSet<_> = component.iter().cloned().collect();
            let mut folded = FxHashSet::default();
            while let Some(module) = queue.pop_front() {
                queued.remove(&module);
                let input = &inputs[&module];
                let child = |dependency: &String| {
                    bindings
                        .get(dependency)
                        .unwrap_or_else(|| &tables[dependency].bindings)
                };
                // Start with the largest star's HAMT instead of copying its
                // entire suffix. Only authored masks and additions change it.
                let largest = input
                    .stars
                    .iter()
                    .max_by_key(|dependency| child(dependency).len());
                let mut next =
                    largest.map_or_else(Bindings::default, |dependency| child(dependency).clone());
                let mut owned = 0;
                if next.remove("default").is_some() {
                    owned += 1;
                }
                for name in &input.masked {
                    self.count_module_export_work(1);
                    if next.remove(name).is_some() {
                        owned += 1;
                    }
                }
                for dependency in &input.stars {
                    self.count_module_export_work(1);
                    if Some(dependency) == largest {
                        continue;
                    }
                    for (name, binding) in child(dependency) {
                        self.count_module_export_work(1);
                        if name.as_ref() == "default" || input.masked.contains(name) {
                            continue;
                        }
                        match next.get(name) {
                            Some(existing) if existing != binding && existing.is_some() => {
                                next.insert(Arc::clone(name), None);
                                owned += 1;
                            }
                            None => {
                                next.insert(Arc::clone(name), binding.clone());
                                owned += 1;
                            }
                            _ => {}
                        }
                    }
                }
                for (name, binding) in &input.local {
                    self.count_module_export_work(1);
                    next.insert(Arc::clone(name), binding.clone());
                    owned += 1;
                }
                for (name, dependency, original) in &input.named {
                    self.count_module_export_work(1);
                    if let Some(binding) = child(dependency).get(original) {
                        next.insert(Arc::clone(name), binding.clone());
                        owned += 1;
                    }
                }
                // A first acyclic fold needs no whole-map equality scan.
                let first = folded.insert(module.clone());
                if first || next != bindings[&module] {
                    bindings.insert(module.clone(), next);
                    writes.insert(module.clone(), owned);
                    if component.len() > 1 {
                        for parent in parents.get(&module).into_iter().flatten() {
                            if component_set.contains(parent) && queued.insert(parent.clone()) {
                                queue.push_back(parent.clone());
                            }
                        }
                    }
                }
            }
            // One shared proof for the component: own observations plus a
            // reference to each consumed component, never expanded suffixes.
            let mut receipts = Vec::new();
            let mut dependencies = FxHashSet::default();
            for module in &component {
                let input = &inputs[module];
                let receipt = match input.evidence.as_ref() {
                    Some(
                        FactReadSetFinalise::Ok(facts) | FactReadSetFinalise::NonCacheable(facts),
                    ) => ResultReceipt::retained_signature(facts, db.module_export_account.get()),
                    _ => ResultReceipt::new(input.facts.iter().cloned().collect()),
                };
                receipts.push(receipt);
                dependencies.extend(
                    inputs[module]
                        .dependencies()
                        .into_iter()
                        .filter(|dependency| !component_set.contains(*dependency))
                        .cloned(),
                );
            }
            let mut dependency_charges = Vec::new();
            let mut dependencies_charged = true;
            for dependency in dependencies {
                let table = &tables[&dependency];
                receipts.extend(table.facts.iter().filter_map(|fact| match fact {
                    FactVersionRef::Receipt(receipt) => Some(receipt.clone()),
                    _ => None,
                }));
                if let Some(charge) = &table.retention {
                    dependency_charges.push(Arc::clone(charge));
                } else {
                    dependencies_charged = false;
                }
            }
            let receipt = ResultReceipt::retained_join(receipts, db.module_export_account.get());
            let facts: Arc<[FactVersionRef]> = Arc::from([FactVersionRef::Receipt(receipt)]);
            let mut own_bytes = std::mem::size_of::<ModuleExportRetention>()
                + dependency_charges.len() * std::mem::size_of::<Arc<ModuleExportRetention>>();
            for module in &component {
                let input = &inputs[module];
                let map = &bindings[module];
                let owned = writes.get(module).copied().unwrap_or(0);
                // A 64-bit HAMT has at most 13 levels of 32 slots. Bound
                // copied paths by either those paths or the entire map.
                let slots = (map.len() * 2 + 32).min(owned * 13 * 32);
                own_bytes += std::mem::size_of::<ModuleExportTable>()
                    + 8 * std::mem::size_of::<usize>()
                    + 2 * std::mem::size_of::<FactVersionRef>()
                    + 2 * module.len()
                    + slots * std::mem::size_of::<(Arc<str>, Option<ModuleExportBinding>)>()
                    + input
                        .local
                        .iter()
                        .map(|(name, binding)| {
                            name.len()
                                + match binding {
                                    Some(ModuleExportBinding::Value(root)) => {
                                        root.canonical_id.len() + root.symbol_name.len()
                                    }
                                    Some(ModuleExportBinding::Namespace(module)) => module.len(),
                                    None => 0,
                                }
                        })
                        .sum::<usize>();
            }
            let retention = if cacheable && dependencies_charged {
                verter_session_query::facts::receipt::reserve_retained_with_evidence(
                    db.module_export_account.get(),
                    own_bytes,
                    &[&facts],
                )
                .admitted()
                .map(|charge| {
                    Arc::new(ModuleExportRetention {
                        charge,
                        dependencies: dependency_charges,
                    })
                })
            } else {
                None
            };
            for module in component {
                let table = Arc::new(ModuleExportTable {
                    bindings: bindings.remove(&module).expect("folded component member"),
                    facts: Arc::clone(&facts),
                    retention: retention.clone(),
                    owned_binding_slots: writes.get(&module).copied().unwrap_or(0),
                });
                if table.retention.is_some() {
                    let stored = db.module_exports.insert_arc_with_kind(
                        table_key(ctx, &module),
                        Arc::clone(&table),
                        table.facts.to_vec(),
                        "route_db.module_exports",
                    );
                    if module == canonical {
                        admitted = stored.is_some();
                    }
                }
                tables.insert(module, table);
            }
        }
        Ok((Arc::clone(&tables[canonical]), admitted, rootable))
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
