//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;
use verter_resolver::{
    run_stable_request, RequestSource, SingleflightRole, StableRequestExecutor, StoreView,
};

pub(crate) fn component_meta_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
    })
}

pub(crate) fn component_meta_debug(message: impl AsRef<str>) {
    if component_meta_debug_enabled() {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "[verter-meta] {}", message.as_ref());
        let _ = stderr.flush();
    }
}

const COMPONENT_META_MAX_SYMBOLIC_STEPS: usize = 2_000;
const COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS: usize = 2_000;
const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

struct FallthroughRequestExecutor<'a, 'b> {
    host: &'a VerterHost,
    canonical_id: String,
    prop_type_overrides:
        Option<&'a rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>>,
    visiting: &'b mut rustc_hash::FxHashSet<String>,
    last_snapshot_epoch: Option<u64>,
}

impl<'a, 'b> FallthroughRequestExecutor<'a, 'b> {
    fn new(
        host: &'a VerterHost,
        canonical_id: String,
        prop_type_overrides: Option<
            &'a rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &'b mut rustc_hash::FxHashSet<String>,
    ) -> Self {
        Self {
            host,
            canonical_id,
            prop_type_overrides,
            visiting,
            last_snapshot_epoch: None,
        }
    }
}

impl<'a, 'b>
    StableRequestExecutor<
        verter_resolver::FallthroughNodeKey,
        Option<crate::types::FallthroughResolution>,
    > for FallthroughRequestExecutor<'a, 'b>
{
    type View = crate::resolver_store::HostStoreView;
    type Error = ();

    fn cache_key(&self) -> verter_resolver::FallthroughNodeKey {
        fallthrough_cache_key(
            &self.canonical_id,
            self.host.config.generic_root_propagation,
            self.prop_type_overrides,
        )
    }

    fn snapshot_view(&mut self) -> Self::View {
        let view = self.host.resolver_store_view();
        self.last_snapshot_epoch = Some(view.mutation_epoch());
        view
    }

    fn try_get_cached(
        &mut self,
        store_view: &Self::View,
    ) -> Option<Option<crate::types::FallthroughResolution>> {
        let cache_key = self.cache_key();
        if let Some(cached) = self
            .host
            .fallthrough_cache
            .get_if_valid(&cache_key, store_view)
        {
            if self.prop_type_overrides.is_none() {
                self.host
                    .mirror_cached_fallthrough_arc(&self.canonical_id, cached.clone());
            }
            return Some(Some(cached.as_ref().clone()));
        }

        if self.prop_type_overrides.is_none() {
            #[cfg(feature = "scheduler")]
            {
                if let Some(cc) = self.host.compile_cache.get(&self.canonical_id) {
                    if let Some(ref cached) = cc.cached_fallthrough {
                        if cached.generic_root_propagation
                            == self.host.config.generic_root_propagation
                            && cached
                                .fact_versions
                                .iter()
                                .all(|fact| store_view.validates(fact))
                        {
                            self.host.fallthrough_cache.insert_arc(
                                cache_key,
                                cached.resolution.clone(),
                                cached.fact_versions.clone(),
                            );
                            self.host.mirror_cached_fallthrough_arc(
                                &self.canonical_id,
                                cached.resolution.clone(),
                            );
                            return Some(Some((*cached.resolution).clone()));
                        }
                    }
                }
            }
        }

        None
    }

    fn compute(
        &mut self,
        _view: &Self::View,
    ) -> Result<Option<crate::types::FallthroughResolution>, Self::Error> {
        Ok(self.host.compute_fallthrough_surface_uncached(
            &self.canonical_id,
            self.prop_type_overrides,
            self.visiting,
        ))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        self.last_snapshot_epoch
            .is_some_and(|epoch| self.host.current_store_view_epoch() == epoch)
    }

    fn store_stable(&mut self, value: &Option<crate::types::FallthroughResolution>) {
        if let Some(result) = value.as_ref() {
            self.host.cache_fallthrough_result(
                &self.canonical_id,
                self.prop_type_overrides,
                result,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        STORE_VIEW_STABILITY_MAX_ATTEMPTS
    }
}

pub(crate) fn component_meta_symbolic_step_budget() -> usize {
    COMPONENT_META_MAX_SYMBOLIC_STEPS
}

fn component_meta_expansion_budget() -> verter_analysis::type_expand::ExpansionBudget {
    let mut budget = verter_analysis::type_expand::ExpansionBudget::default();
    budget.max_symbolic_work = COMPONENT_META_MAX_SYMBOLIC_STEPS;
    budget
}

fn macro_debug_summary(snapshot: &FileAnalysisSnapshot) -> String {
    snapshot
        .macros
        .iter()
        .map(|mac| {
            format!(
                "{:?}(refs=[{}], props={}, emits={}, slots={})",
                mac.kind,
                mac.type_references.join(","),
                mac.prop_fields.len(),
                mac.emit_fields.len(),
                mac.slot_fields.len(),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn log_snapshot_debug(
    stage: &str,
    canonical: &str,
    started: Instant,
    snapshot: &FileAnalysisSnapshot,
) {
    component_meta_debug(format!(
        "{stage} {canonical} took {:?} imports={} macro_type_deps={} macros=[{}]",
        started.elapsed(),
        snapshot.imports.len(),
        snapshot.macro_type_deps.len(),
        macro_debug_summary(snapshot),
    ));
}

/// Imported eval inputs collected during component-meta resolution.
/// Type is `pub` so cross-crate code can reference `Option<Arc<ImportedEvalInputs>>`
/// (e.g., on `ResolvedComponentMetaState`), but fields are crate-private —
/// only `verter_host` constructs and reads the contents.
#[derive(Debug)]
pub struct ImportedEvalInputs {
    pub(crate) sources: Vec<ImportedEvalSource>,
    pub(crate) type_aliases: Vec<ImportedTypeAlias>,
    pub(crate) canonical_dependencies: std::collections::BTreeSet<String>,
    pub(crate) overflow: Option<ImportedEvalOverflow>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedEvalSource {
    pub(crate) canonical_id: String,
    pub(crate) source: Arc<str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedTypeAlias {
    pub(crate) local_name: String,
    pub(crate) source_canonical_id: String,
    pub(crate) exported_name: String,
    pub(crate) decl: verter_analysis::type_eval::TypeDeclInfo,
    pub(crate) requires_source_merge: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedEvalOverflow {
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct ComputedEvaluatedTypes {
    pub(crate) evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    pub(crate) discovered_dependencies: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ImportedTypeLookupTarget {
    import_source: String,
    dep_canonical_id: String,
    imported_name: String,
    local_name: String,
}

#[derive(Debug, Clone)]
struct ImportedValueLookupTarget {
    dep_canonical_id: String,
    source_canonical_id: String,
    source_name: String,
    local_name: String,
    remaining_path: Vec<String>,
}

struct ImportedEvalLookup<'a> {
    host: &'a VerterHost,
    owner_canonical_id: &'a str,
    snapshot: &'a FileAnalysisSnapshot,
    dep_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    discovered_dependencies: std::collections::BTreeSet<String>,
    alias_env_stack: rustc_hash::FxHashSet<String>,
    budget: ImportedEvalTraversalBudget,
    type_decl_cache:
        rustc_hash::FxHashMap<String, Option<verter_analysis::type_eval::TypeDeclInfo>>,
    value_decl_cache:
        rustc_hash::FxHashMap<Vec<String>, Option<verter_analysis::type_eval::ValueDeclInfo>>,
}

impl<'a> ImportedEvalLookup<'a> {
    fn new(
        host: &'a VerterHost,
        owner_canonical_id: &'a str,
        snapshot: &'a FileAnalysisSnapshot,
    ) -> Self {
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        Self {
            host,
            owner_canonical_id,
            snapshot,
            dep_resolutions: host.dependency_resolutions_for_eval(owner_canonical_id),
            discovered_dependencies: std::collections::BTreeSet::new(),
            alias_env_stack,
            budget: ImportedEvalTraversalBudget::new(owner_canonical_id),
            type_decl_cache: rustc_hash::FxHashMap::default(),
            value_decl_cache: rustc_hash::FxHashMap::default(),
        }
    }

    fn into_discovered_dependencies(self) -> std::collections::BTreeSet<String> {
        self.discovered_dependencies
    }

    fn resolve_import_canonical_id(
        &self,
        import: &verter_analysis::AnalyzedImport,
    ) -> Option<String> {
        import
            .resolved_canonical_id
            .clone()
            .or_else(|| {
                self.dep_resolutions
                    .get(&import.source)
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
            })
            .or_else(|| {
                import
                    .source
                    .starts_with('.')
                    .then(|| crate::id::resolve_external(self.owner_canonical_id, &import.source))
            })
    }

    fn resolve_type_lookup_target(&self, name: &str) -> Option<ImportedTypeLookupTarget> {
        let (root_name, imported_name) = if let Some((root, member)) = name.split_once('.') {
            (root, Some(member.to_string()))
        } else {
            (name, None)
        };

        self.snapshot.imports.iter().find_map(|import| {
            let binding = import.bindings.iter().find(|binding| {
                binding.name == root_name
                    && (binding.is_type_only || import.is_type_only)
                    && match (&imported_name, binding.kind) {
                        (Some(_), verter_analysis::types::ImportBindingKind::Namespace) => true,
                        (None, verter_analysis::types::ImportBindingKind::Namespace) => false,
                        (Some(_), _) => false,
                        (None, _) => true,
                    }
            })?;
            let dep_canonical_id = self.resolve_import_canonical_id(import)?;
            let imported_name = imported_name.clone().unwrap_or_else(|| {
                binding
                    .imported_name
                    .clone()
                    .unwrap_or_else(|| binding.name.clone())
            });
            Some(ImportedTypeLookupTarget {
                import_source: import.source.clone(),
                dep_canonical_id,
                imported_name,
                local_name: name.to_string(),
            })
        })
    }

    fn resolve_value_lookup_target(&self, path: &[String]) -> Option<ImportedValueLookupTarget> {
        let root_name = path.first()?;

        self.snapshot.imports.iter().find_map(|import| {
            let binding = import.bindings.iter().find(|binding| {
                !binding.is_type_only
                    && !import.is_type_only
                    && binding.name == *root_name
                    && match binding.kind {
                        verter_analysis::types::ImportBindingKind::Namespace => path.len() >= 2,
                        _ => true,
                    }
            })?;
            let dep_canonical_id = self.resolve_import_canonical_id(import)?;
            let (imported_name, remaining_path) = match binding.kind {
                verter_analysis::types::ImportBindingKind::Namespace => {
                    (path.get(1)?.clone(), path[2..].to_vec())
                }
                _ => (
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| binding.name.clone()),
                    path[1..].to_vec(),
                ),
            };
            let resolved_export = self
                .host
                .resolve_exports(&dep_canonical_id)
                .into_iter()
                .find(|export| !export.is_type && export.name == imported_name);
            let (source_canonical_id, source_name) = if let Some(export) = resolved_export {
                (
                    export
                        .source_canonical_id
                        .unwrap_or_else(|| dep_canonical_id.clone()),
                    export.source_name,
                )
            } else {
                (dep_canonical_id.clone(), imported_name)
            };
            Some(ImportedValueLookupTarget {
                dep_canonical_id,
                source_canonical_id,
                source_name,
                local_name: path.join("."),
                remaining_path,
            })
        })
    }

    fn dependency_eval_env(
        &self,
        dep_canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.host.base_eval_env(dep_canonical_id).or_else(|| {
            self.host
                .load_eval_dependency_source_text_with_fallback(dep_canonical_id)
                .map(|source| {
                    verter_analysis::type_eval_build::parse_and_build_env(source.as_ref())
                })
        })
    }

    fn project_value_member_path(
        &mut self,
        dep_env: &mut verter_analysis::type_eval::EvalEnv,
        decl: &verter_analysis::type_eval::ValueDeclInfo,
        remaining_path: &[String],
    ) -> Option<verter_analysis::type_expr::TypeExpr> {
        use verter_analysis::type_expr::{FunctionExpr, TypeExpr};

        let mut current = if let Some(type_annotation) = decl.type_annotation.as_ref() {
            verter_analysis::type_eval::evaluate(type_annotation, dep_env)
        } else if let Some(function_signature) = decl.function_signature.as_ref() {
            TypeExpr::Function(FunctionExpr {
                parameters: function_signature.parameters.clone(),
                return_type: function_signature.return_type.clone().map(Box::new),
                type_parameters: function_signature.type_parameters.clone(),
            })
        } else if let Some(object_shape) = decl.object_shape.as_ref() {
            TypeExpr::Object(object_shape.clone())
        } else {
            return None;
        };

        for segment in remaining_path {
            current = verter_analysis::type_eval::evaluate(
                &TypeExpr::IndexedAccess {
                    object: Box::new(current),
                    index: Box::new(TypeExpr::string_literal(segment.as_str())),
                },
                dep_env,
            );
        }

        Some(current)
    }
}

impl verter_analysis::type_eval::EvalLookup for ImportedEvalLookup<'_> {
    fn resolve_type_decl(
        &mut self,
        name: &str,
    ) -> Option<verter_analysis::type_eval::TypeDeclInfo> {
        if let Some(cached) = self.type_decl_cache.get(name) {
            return cached.clone();
        }

        let resolved = self.resolve_type_lookup_target(name).and_then(|target| {
            let declaration = crate::meta_resolve::resolve_type_declaration(
                self.host,
                &target.dep_canonical_id,
                &target.imported_name,
            );
            let source_canonical_id = if declaration.canonical_source.is_empty() {
                target.dep_canonical_id.clone()
            } else {
                declaration.canonical_source
            };
            let exported_name = if declaration.resolved_name.is_empty() {
                target.imported_name.clone()
            } else {
                declaration.resolved_name
            };

            self.discovered_dependencies
                .insert(target.dep_canonical_id.clone());
            self.discovered_dependencies
                .insert(source_canonical_id.clone());

            self.host
                .prepare_imported_type_alias(
                    ImportedTypeAliasRequest {
                        owner_canonical_id: self.owner_canonical_id,
                        import_source: &target.import_source,
                        local_name: &target.local_name,
                        imported_name: &target.imported_name,
                        source_canonical_id: &source_canonical_id,
                        exported_name: &exported_name,
                    },
                    &mut self.discovered_dependencies,
                    &mut self.alias_env_stack,
                    &mut self.budget,
                )
                .map(|alias| alias.decl)
        });

        self.type_decl_cache
            .insert(name.to_string(), resolved.clone());
        resolved
    }

    fn resolve_value_decl(
        &mut self,
        path: &[String],
    ) -> Option<verter_analysis::type_eval::ValueDeclInfo> {
        if let Some(cached) = self.value_decl_cache.get(path) {
            return cached.clone();
        }

        let resolved = self.resolve_value_lookup_target(path).and_then(|target| {
            self.discovered_dependencies
                .insert(target.dep_canonical_id.clone());
            self.discovered_dependencies
                .insert(target.source_canonical_id.clone());
            let mut dep_env = self.dependency_eval_env(&target.source_canonical_id)?;
            let mut decl = dep_env.value_symbols.get(&target.source_name).cloned()?;
            decl.name = target.local_name.clone();

            if target.remaining_path.is_empty() {
                return Some(decl);
            }

            let projected =
                self.project_value_member_path(&mut dep_env, &decl, &target.remaining_path)?;
            Some(verter_analysis::type_eval::ValueDeclInfo {
                name: target.local_name,
                declaration_id: 0,
                kind: decl.kind,
                type_annotation: Some(projected),
                function_signature: None,
                object_shape: None,
            })
        });

        self.value_decl_cache
            .insert(path.to_vec(), resolved.clone());
        resolved
    }

    fn utility_source(&mut self, name: &str) -> verter_analysis::type_eval::BuiltinUtilitySource {
        if self
            .snapshot
            .imports
            .iter()
            .flat_map(|import| import.bindings.iter())
            .any(|binding| binding.name == name)
        {
            return verter_analysis::type_eval::BuiltinUtilitySource::Shadowed;
        }

        if matches!(
            name,
            "Partial"
                | "Required"
                | "Readonly"
                | "Pick"
                | "Omit"
                | "Record"
                | "Extract"
                | "Exclude"
                | "NonNullable"
                | "ReturnType"
                | "Parameters"
                | "ConstructorParameters"
                | "InstanceType"
                | "Awaited"
        ) {
            verter_analysis::type_eval::BuiltinUtilitySource::Builtin
        } else {
            verter_analysis::type_eval::BuiltinUtilitySource::Unknown
        }
    }
}

#[derive(Debug)]
struct ImportedEvalTraversalBudget {
    owner_canonical_id: String,
    max_type_roots: usize,
    overflow: Option<ImportedEvalOverflow>,
}

impl ImportedEvalTraversalBudget {
    fn new(owner_canonical_id: &str) -> Self {
        Self {
            owner_canonical_id: owner_canonical_id.to_string(),
            max_type_roots: COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            overflow: None,
        }
    }

    fn is_exhausted(&self) -> bool {
        self.overflow.is_some()
    }

    fn overflow(&self) -> Option<ImportedEvalOverflow> {
        self.overflow.clone()
    }

    fn set_overflow(&mut self, message: impl Into<String>) {
        if self.overflow.is_none() {
            self.overflow = Some(ImportedEvalOverflow {
                message: message.into(),
            });
        }
    }

    fn try_enter_type_root(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
        visited_count: usize,
    ) -> bool {
        if self.is_exhausted() {
            return false;
        }
        if visited_count >= self.max_type_roots {
            self.set_overflow(format!(
                "component-meta imported type traversal budget exceeded (maxSteps={}) while resolving '{}#{}' for '{}'",
                self.max_type_roots,
                canonical_id,
                exported_name,
                self.owner_canonical_id,
            ));
            return false;
        }
        true
    }
}

struct OwnerEvalEnvBuild {
    env: verter_analysis::type_eval::EvalEnv,
    requested_binding_names: rustc_hash::FxHashSet<String>,
}

struct ImportedTypeAliasRequest<'a> {
    owner_canonical_id: &'a str,
    import_source: &'a str,
    local_name: &'a str,
    imported_name: &'a str,
    source_canonical_id: &'a str,
    exported_name: &'a str,
}

impl VerterHost {
    fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    fn read_analysis_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.get_source(canonical_id)
            .or_else(|| self.ws().read_file(canonical_id))
    }

    fn clone_cached_eval_env(
        &self,
        cache_key: &str,
        whole_hash: Hash16,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.eval_env_cache
            .lock()
            .get(cache_key)
            .and_then(|(cached_hash, cached_env)| {
                (*cached_hash == whole_hash).then(|| (**cached_env).clone())
            })
    }

    fn cache_eval_env(
        &self,
        cache_keys: &[String],
        whole_hash: Hash16,
        env: verter_analysis::type_eval::EvalEnv,
    ) -> verter_analysis::type_eval::EvalEnv {
        let mut cache = self.eval_env_cache.lock();
        for cache_key in cache_keys {
            if let Some((cached_hash, cached_env)) = cache.get(cache_key) {
                if *cached_hash == whole_hash {
                    return (**cached_env).clone();
                }
            }
        }

        let cached_env = Arc::new(env.clone());
        for cache_key in cache_keys {
            cache.insert(cache_key.clone(), (whole_hash, Arc::clone(&cached_env)));
        }
        env
    }

    pub(crate) fn base_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        if let Some((source, cached_parse, whole_hash)) = self.current_eval_state(canonical_id) {
            if let Some(cached_env) = self.clone_cached_eval_env(canonical_id, whole_hash) {
                return Some(cached_env);
            }

            let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
            let env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
            return Some(self.cache_eval_env(&[canonical_id.to_string()], whole_hash, env));
        }

        let (resolved_canonical_id, eval_source) =
            self.load_eval_dependency_source_with_fallback(canonical_id)?;
        let whole_hash = crate::hash::hash_16(eval_source.as_bytes());

        if let Some(cached_env) = self.clone_cached_eval_env(&resolved_canonical_id, whole_hash) {
            return Some(cached_env);
        }

        let env = verter_analysis::type_eval_build::parse_and_build_env(eval_source.as_ref());
        Some(self.cache_eval_env(
            &[resolved_canonical_id, canonical_id.to_string()],
            whole_hash,
            env,
        ))
    }

    fn build_snapshot_from_parse(parse: crate::ParseSnapshot) -> FileAnalysisSnapshot {
        let script_analysis = parse.script_analysis;
        FileAnalysisSnapshot {
            imports: script_analysis.imports,
            bindings: script_analysis.bindings,
            module_references: Arc::new(script_analysis.module_references),
            macros: Arc::new(script_analysis.macros),
            macro_type_deps: Arc::new(script_analysis.macro_type_deps),
            script_flags: script_analysis.flags.bits(),
            styles: Arc::new(parse.style_analyses),
            template: None,
            vue_api_calls: Arc::new(script_analysis.vue_api_calls),
            dom_query_calls: Arc::new(script_analysis.dom_query_calls),
            css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
            script_binding_occurrences: Arc::new(script_analysis.script_binding_occurrences),
            export_signatures: Arc::new(parse.export_signatures),
            options_api: script_analysis.options_api,
            store_usages: Arc::new(script_analysis.store_usages),
            store_definitions: Arc::new(script_analysis.store_definitions),
            is_typescript: script_analysis.is_typescript,
        }
    }

    fn build_snapshot_from_source(
        &self,
        canonical: &str,
        source: &Arc<str>,
    ) -> FileAnalysisSnapshot {
        if canonical.ends_with(".vue") {
            let (parse, _) =
                crate::parse::parse_vue_snapshot(canonical, source, self.config.effective_scope());
            Self::build_snapshot_from_parse(parse)
        } else {
            let parse = crate::parse::parse_non_sfc_snapshot(canonical, source);
            Self::build_snapshot_from_parse(parse)
        }
    }

    fn finalize_analysis_snapshot(
        &self,
        canonical: &str,
        mut snapshot: FileAnalysisSnapshot,
        needs_template_analysis: bool,
        analysis_started: Option<Instant>,
    ) -> FileAnalysisSnapshot {
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if needs_template_analysis {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
        }
        if let Some(started) = analysis_started {
            log_snapshot_debug("get_analysis", canonical, started, &snapshot);
        }
        snapshot
    }

    fn is_expanded_types_empty(
        result: &verter_analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_core::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        #[cfg(feature = "scheduler")]
        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                Some((state.source, state.cached_parse, state.whole_hash))
            } else {
                let source = self.read_analysis_source(canonical_id)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                Some((
                    source.clone(),
                    cached_parse,
                    crate::hash::hash_16(source.as_bytes()),
                ))
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical_id) {
                Some((
                    Arc::clone(&entry.source),
                    entry.cached_parse.clone(),
                    entry.whole_hash,
                ))
            } else {
                drop(files);
                let source = self.read_analysis_source(canonical_id)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                Some((
                    source.clone(),
                    cached_parse,
                    crate::hash::hash_16(source.as_bytes()),
                ))
            }
        }
    }

    pub(crate) fn dependency_resolutions_for_eval(
        &self,
        canonical_id: &str,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        #[cfg(feature = "scheduler")]
        {
            self.compile_cache
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default()
        }
    }

    /// Load an evaluation dependency source, hydrating workspace-owned files into
    /// host state when necessary before reading them.
    fn load_eval_dependency_source_with_fallback(
        &self,
        dep_canonical: &str,
    ) -> Option<(String, Arc<str>)> {
        let read_candidate = |candidate: &str| -> Option<Arc<str>> {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = self.ensure_loaded(candidate);
            }

            if let Some((source, cached_parse, _)) = self.current_eval_state(candidate) {
                return Some(Arc::<str>::from(Self::build_eval_script_source(
                    &source,
                    cached_parse.as_deref(),
                )));
            }

            self.read_dep_source_for_type_resolution(candidate, None)
                .map(|source| Arc::<str>::from(Self::build_eval_script_source(&source, None)))
        };

        if let Some(source) = read_candidate(dep_canonical) {
            return Some((dep_canonical.to_string(), source));
        }

        let mut candidates = Vec::new();
        if let Some(stem) = dep_canonical.strip_suffix(".js") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".jsx") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".mjs") {
            candidates.push(format!("{stem}.d.mts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".cjs") {
            candidates.push(format!("{stem}.d.cts"));
        }
        candidates.extend([
            format!("{dep_canonical}.d.ts"),
            format!("{dep_canonical}.ts"),
            format!("{dep_canonical}.tsx"),
            format!("{dep_canonical}/index.d.ts"),
            format!("{dep_canonical}/index.ts"),
            format!("{dep_canonical}/index.tsx"),
        ]);

        for candidate in candidates {
            if let Some(source) = read_candidate(&candidate) {
                return Some((candidate, source));
            }
        }

        None
    }

    fn load_eval_dependency_canonical_with_fallback(&self, dep_canonical: &str) -> Option<String> {
        self.load_eval_dependency_source_with_fallback(dep_canonical)
            .map(|(canonical, _)| canonical)
    }

    fn load_eval_dependency_source_text_with_fallback(
        &self,
        dep_canonical: &str,
    ) -> Option<Arc<str>> {
        self.load_eval_dependency_source_with_fallback(dep_canonical)
            .map(|(_, source)| source)
    }

    pub(crate) fn imported_eval_inputs(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> ImportedEvalInputs {
        self.provenance
            .imported_eval_inputs_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        let mut budget = ImportedEvalTraversalBudget::new(owner_canonical_id);
        self.imported_eval_inputs_inner(
            owner_canonical_id,
            snapshot,
            dep_resolutions,
            None,
            &mut alias_env_stack,
            &mut budget,
        )
    }

    fn imported_eval_inputs_inner(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        additional_required_import_names: Option<&rustc_hash::FxHashSet<String>>,
        alias_env_stack: &mut rustc_hash::FxHashSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> ImportedEvalInputs {
        let started = component_meta_debug_enabled().then(Instant::now);
        let mut seen = rustc_hash::FxHashSet::default();
        let mut inputs = Vec::new();
        let mut alias_names = rustc_hash::FxHashSet::default();
        let mut type_aliases = Vec::new();
        let mut canonical_dependencies = std::collections::BTreeSet::new();
        let mut visited_type_roots = rustc_hash::FxHashSet::default();
        let mut snapshot_cache = rustc_hash::FxHashMap::default();
        let mut eval_source_cache = rustc_hash::FxHashMap::default();
        let owner_eval_source = self
            .current_eval_state(owner_canonical_id)
            .map(|(source, cached_parse, _)| {
                Self::build_eval_script_source(&source, cached_parse.as_deref())
            })
            .unwrap_or_default();
        let owner_env = self.base_eval_env(owner_canonical_id).unwrap_or_else(|| {
            verter_analysis::type_eval_build::parse_and_build_env(&owner_eval_source)
        });
        let mut required_import_names =
            collect_required_owner_import_names(snapshot, owner_eval_source.as_str(), &owner_env);
        if let Some(additional) = additional_required_import_names {
            required_import_names.extend(additional.iter().cloned());
        }
        if let Some(started) = started {
            component_meta_debug(format!(
                "imported_eval_inputs:start owner={} imports={} required_bindings=[{}] prework_took {:?}",
                owner_canonical_id,
                snapshot.imports.len(),
                required_import_names
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                started.elapsed(),
            ));
        }
        self.track_direct_eval_dependencies(
            owner_canonical_id,
            snapshot,
            dep_resolutions,
            &mut canonical_dependencies,
        );

        for import in &snapshot.imports {
            if budget.is_exhausted() {
                break;
            }
            for binding in &import.bindings {
                if budget.is_exhausted() {
                    break;
                }
                let required_alias_names = required_type_alias_names_for_import_binding(
                    binding.name.as_str(),
                    matches!(
                        binding.kind,
                        verter_analysis::types::ImportBindingKind::Namespace
                    ),
                    &required_import_names,
                );
                if required_alias_names.is_empty() {
                    continue;
                }

                let Some(dep_canonical) = self
                    .resolve_type_dependency_canonical(owner_canonical_id, &import.source)
                    .or_else(|| import.resolved_canonical_id.clone())
                    .or_else(|| {
                        dep_resolutions
                            .get(&import.source)
                            .and_then(|resolution| resolution.resolved_canonical_id.clone())
                    })
                    .or_else(|| {
                        import.source.starts_with('.').then(|| {
                            crate::id::resolve_external(owner_canonical_id, &import.source)
                        })
                    })
                else {
                    continue;
                };

                canonical_dependencies.insert(dep_canonical.clone());
                for required_alias_name in required_alias_names {
                    let Some(imported_name) = imported_member_name_for_type_alias(
                        binding.name.as_str(),
                        binding.imported_name.as_deref(),
                        matches!(
                            binding.kind,
                            verter_analysis::types::ImportBindingKind::Namespace
                        ),
                        &required_alias_name,
                    ) else {
                        continue;
                    };

                    let declaration = crate::meta_resolve::resolve_type_declaration(
                        self,
                        &dep_canonical,
                        &imported_name,
                    );
                    let source_canonical_id = if declaration.canonical_source.is_empty() {
                        dep_canonical.clone()
                    } else {
                        declaration.canonical_source.clone()
                    };
                    let exported_name = if declaration.resolved_name.is_empty() {
                        imported_name.clone()
                    } else {
                        declaration.resolved_name.clone()
                    };

                    if alias_names.insert(required_alias_name.clone()) {
                        if let Some(alias) = self.prepare_imported_type_alias(
                            ImportedTypeAliasRequest {
                                owner_canonical_id,
                                import_source: &import.source,
                                local_name: &required_alias_name,
                                imported_name: &imported_name,
                                source_canonical_id: &source_canonical_id,
                                exported_name: &exported_name,
                            },
                            &mut canonical_dependencies,
                            alias_env_stack,
                            budget,
                        ) {
                            if alias.requires_source_merge {
                                self.record_relevant_type_eval_inputs_recursive(
                                    &source_canonical_id,
                                    &exported_name,
                                    &mut seen,
                                    &mut inputs,
                                    &mut canonical_dependencies,
                                    &mut visited_type_roots,
                                    &mut snapshot_cache,
                                    &mut eval_source_cache,
                                    budget,
                                );
                            }
                            type_aliases.push(alias);
                        }
                    }
                }
            }
        }

        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "imported_eval_inputs:end owner={} required_bindings=[{}] type_aliases=[{}] sources={} total_took={:?}",
                owner_canonical_id,
                required_import_names
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                type_aliases
                    .iter()
                    .map(|alias| format!(
                        "{}<-{}#{}",
                        alias.local_name, alias.source_canonical_id, alias.exported_name
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                inputs.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }

        ImportedEvalInputs {
            sources: inputs,
            type_aliases,
            canonical_dependencies,
            overflow: budget.overflow(),
        }
    }

    fn track_direct_eval_dependencies(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        for dep in snapshot.macro_type_deps.iter() {
            if let Some(dep_canonical) = self
                .resolve_type_dependency_canonical(owner_canonical_id, &dep.import_source)
                .or_else(|| {
                    dep_resolutions
                        .get(&dep.import_source)
                        .and_then(|resolution| resolution.resolved_canonical_id.clone())
                })
                .or_else(|| {
                    dep.import_source.starts_with('.').then(|| {
                        crate::id::resolve_external(owner_canonical_id, &dep.import_source)
                    })
                })
            {
                canonical_dependencies.insert(dep_canonical);
            }
        }

        for import in snapshot
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
        {
            if let Some(dep_canonical) = import
                .resolved_canonical_id
                .clone()
                .or_else(|| {
                    dep_resolutions
                        .get(&import.source)
                        .and_then(DependencyResolution::effective_target)
                        .map(str::to_string)
                })
                .or_else(|| {
                    import
                        .source
                        .starts_with('.')
                        .then(|| crate::id::resolve_external(owner_canonical_id, &import.source))
                })
            {
                canonical_dependencies.insert(dep_canonical);
            }
        }
    }

    fn record_eval_input_source(
        &self,
        canonical_id: &str,
        seen_sources: &mut rustc_hash::FxHashSet<String>,
        inputs: &mut Vec<ImportedEvalSource>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        let Some((resolved_canonical_id, source)) =
            self.load_eval_dependency_source_with_fallback(canonical_id)
        else {
            canonical_dependencies.insert(canonical_id.to_string());
            return;
        };

        canonical_dependencies.insert(resolved_canonical_id.clone());
        if !seen_sources.insert(resolved_canonical_id.clone()) {
            return;
        }

        inputs.push(ImportedEvalSource {
            canonical_id: resolved_canonical_id,
            source,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record_relevant_type_eval_inputs_recursive(
        &self,
        canonical_id: &str,
        exported_name: &str,
        seen_sources: &mut rustc_hash::FxHashSet<String>,
        inputs: &mut Vec<ImportedEvalSource>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
        visited_type_roots: &mut rustc_hash::FxHashSet<(String, String)>,
        snapshot_cache: &mut rustc_hash::FxHashMap<String, Option<FileAnalysisSnapshot>>,
        eval_source_cache: &mut rustc_hash::FxHashMap<String, Option<String>>,
        budget: &mut ImportedEvalTraversalBudget,
    ) {
        if budget.is_exhausted() {
            return;
        }
        let visit_key = (canonical_id.to_string(), exported_name.to_string());
        if visited_type_roots.contains(&visit_key) {
            return;
        }
        if !budget.try_enter_type_root(canonical_id, exported_name, visited_type_roots.len()) {
            canonical_dependencies.insert(canonical_id.to_string());
            return;
        }
        visited_type_roots.insert(visit_key);

        self.record_eval_input_source(canonical_id, seen_sources, inputs, canonical_dependencies);

        let eval_source = eval_source_cache
            .entry(canonical_id.to_string())
            .or_insert_with(|| {
                self.current_eval_state(canonical_id)
                    .map(|(source, cached_parse, _)| {
                        Self::build_eval_script_source(&source, cached_parse.as_deref())
                    })
            })
            .clone();
        let Some(eval_source) = eval_source else {
            return;
        };

        let alloc = oxc_allocator::Allocator::new();
        let required_import_names =
            verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
                exported_name,
                eval_source.as_str(),
                &alloc,
            );
        if required_import_names.is_empty() || budget.is_exhausted() {
            return;
        }

        // Try the analysis snapshot first for import declarations.
        // If no snapshot exists (dep loaded as raw source, not through normal pipeline),
        // fall back to lightweight OXC extraction from the eval source.
        let snapshot = snapshot_cache
            .entry(canonical_id.to_string())
            .or_insert_with(|| self.get_analysis_snapshot_internal(canonical_id, None))
            .clone();

        struct LightweightImportBinding {
            name: String,
            imported_name: Option<String>,
            source: String,
            resolved_canonical_id: Option<String>,
            is_namespace: bool,
        }

        let lightweight_bindings: Vec<LightweightImportBinding>;
        let bindings_ref: &[LightweightImportBinding];

        if let Some(ref snapshot) = snapshot {
            lightweight_bindings = snapshot
                .imports
                .iter()
                .flat_map(|import| {
                    import
                        .bindings
                        .iter()
                        .map(move |binding| LightweightImportBinding {
                            name: binding.name.clone(),
                            imported_name: binding.imported_name.clone(),
                            source: import.source.clone(),
                            resolved_canonical_id: import.resolved_canonical_id.clone(),
                            is_namespace: matches!(
                                binding.kind,
                                verter_analysis::types::ImportBindingKind::Namespace
                            ),
                        })
                })
                .collect();
            bindings_ref = &lightweight_bindings;
        } else {
            // No analysis snapshot — extract imports from the eval source directly.
            let extracted =
                verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
                    eval_source.as_str(),
                    &alloc,
                );
            lightweight_bindings = extracted
                .bindings
                .into_iter()
                .map(|b| LightweightImportBinding {
                    name: b.local_name.clone(),
                    imported_name: if b.is_namespace {
                        None
                    } else if b.imported_name != b.local_name {
                        Some(b.imported_name)
                    } else {
                        None
                    },
                    source: b.source,
                    resolved_canonical_id: None,
                    is_namespace: b.is_namespace,
                })
                .collect();
            bindings_ref = &lightweight_bindings;
        }

        for binding in bindings_ref.iter() {
            if budget.is_exhausted() {
                break;
            }
            let required_alias_names = required_type_alias_names_for_import_binding(
                binding.name.as_str(),
                binding.is_namespace,
                &required_import_names,
            );
            if required_alias_names.is_empty() {
                continue;
            }
            let Some(dep_canonical) = binding
                .resolved_canonical_id
                .clone()
                .or_else(|| self.resolve_type_dependency_canonical(canonical_id, &binding.source))
                .or_else(|| {
                    binding
                        .source
                        .starts_with('.')
                        .then(|| crate::id::resolve_external(canonical_id, &binding.source))
                })
            else {
                continue;
            };

            canonical_dependencies.insert(dep_canonical.clone());
            for required_alias_name in required_alias_names {
                if budget.is_exhausted() {
                    break;
                }
                let Some(imported_name) = imported_member_name_for_type_alias(
                    binding.name.as_str(),
                    binding.imported_name.as_deref(),
                    binding.is_namespace,
                    &required_alias_name,
                ) else {
                    continue;
                };

                let declaration = crate::meta_resolve::resolve_type_declaration(
                    self,
                    &dep_canonical,
                    &imported_name,
                );
                let next_canonical = if declaration.canonical_source.is_empty() {
                    dep_canonical.clone()
                } else {
                    declaration.canonical_source
                };
                let next_exported_name = if declaration.resolved_name.is_empty() {
                    imported_name
                } else {
                    declaration.resolved_name
                };

                self.record_relevant_type_eval_inputs_recursive(
                    &next_canonical,
                    &next_exported_name,
                    seen_sources,
                    inputs,
                    canonical_dependencies,
                    visited_type_roots,
                    snapshot_cache,
                    eval_source_cache,
                    budget,
                );
            }
        }
    }

    fn prepare_imported_type_alias(
        &self,
        request: ImportedTypeAliasRequest<'_>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
        alias_env_stack: &mut rustc_hash::FxHashSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> Option<ImportedTypeAlias> {
        if budget.is_exhausted() {
            return None;
        }
        let resolved_source_canonical_id = self
            .load_eval_dependency_canonical_with_fallback(request.source_canonical_id)
            .unwrap_or_else(|| request.source_canonical_id.to_string());

        let mut dep_env = self.base_eval_env(&resolved_source_canonical_id)?;
        let mut decl = dep_env.type_symbols.get(request.exported_name).cloned()?;

        let mut tracked_deps = std::collections::BTreeSet::new();
        let mut resolution_deps = std::collections::BTreeSet::new();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();
        let resolved_body = match self.resolve_external_type_from_loaded_files(
            request.owner_canonical_id,
            request.import_source,
            request.imported_name,
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_vfs::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        ) {
            Ok(resolved) => {
                resolved.map(|resolved| resolved_elements_to_type_expr_via_type_text(&resolved))
            }
            Err(crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit,
                type_name,
                last_dep,
            }) => {
                budget.set_overflow(format!(
                    "component-meta external type resolution step budget exceeded (maxSteps={}) while resolving '{}#{}' for '{}' (lastDep='{}')",
                    limit,
                    resolved_source_canonical_id,
                    type_name,
                    request.owner_canonical_id,
                    last_dep,
                ));
                None
            }
            Err(_) => None,
        };
        let resolved_decl_body = should_attempt_owner_env_resolution(&decl, resolved_body.as_ref())
            .then(|| {
                self.evaluate_imported_decl_with_owner_env(
                    &resolved_source_canonical_id,
                    request.exported_name,
                    canonical_dependencies,
                    alias_env_stack,
                    budget,
                )
            })
            .flatten();

        canonical_dependencies.extend(tracked_deps);
        canonical_dependencies.extend(resolution_deps);
        canonical_dependencies.insert(resolved_source_canonical_id.clone());

        if budget.is_exhausted() {
            return None;
        }

        // When the original body is an Intersection with Ref nodes (i.e., interface extends
        // imported types), the flattened resolved_body from the OXC resolver may be incomplete —
        // it might only contain directly declared members, missing inherited ones.
        // In this case, keep the structural Intersection body and force source merge so the
        // EvalEnv can properly resolve the extends chain with loaded dependency sources.
        let body_has_structural_extends = body_has_structural_intersection_refs(&decl.body);
        let preferred_body =
            choose_preferred_imported_type_body(resolved_body.clone(), resolved_decl_body.clone());
        let selected_body =
            choose_preferred_imported_type_body(Some(decl.body.clone()), preferred_body.clone())
                .or(preferred_body.clone());
        let requires_source_merge = if body_has_structural_extends {
            resolved_decl_body.is_none()
                && match selected_body.as_ref() {
                    Some(body) => {
                        is_empty_object_surface(body) || has_non_object_top_level_surface(body)
                    }
                    None => true,
                }
        } else {
            selected_body.is_none()
        };

        if body_has_structural_extends && requires_source_merge {
            // Keep the raw Intersection body WITHOUT evaluating it against the dep env.
            // The dep env only has local types — imported Refs (e.g., LinkProps in
            // ButtonProps extends chain) would be lost if evaluated now. The final
            // EvalEnv (built in build_owner_eval_env_with_inputs) will have all
            // 262+ symbols from dep source merge and can resolve everything.
        } else if let Some(body) = selected_body {
            let mut normalized_env = dep_env.clone();
            for param in &decl.type_parameters {
                normalized_env.type_bindings.insert(
                    param.name.clone(),
                    verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
                );
            }
            let normalized_body = verter_analysis::type_eval::evaluate(&body, &mut normalized_env);
            decl.body = choose_preferred_imported_type_body(Some(body), Some(normalized_body))
                .expect("preferred imported type body should exist");
        } else {
            for param in &decl.type_parameters {
                dep_env.type_bindings.insert(
                    param.name.clone(),
                    verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
                );
            }
            decl.body = verter_analysis::type_eval::evaluate(&decl.body, &mut dep_env);
        }
        decl.name = request.local_name.to_string();

        Some(ImportedTypeAlias {
            local_name: request.local_name.to_string(),
            source_canonical_id: resolved_source_canonical_id,
            exported_name: request.exported_name.to_string(),
            decl,
            requires_source_merge,
        })
    }

    fn evaluate_imported_decl_with_owner_env(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
        alias_env_stack: &mut rustc_hash::FxHashSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> Option<verter_analysis::type_expr::TypeExpr> {
        if budget.is_exhausted() {
            return None;
        }
        let resolved_source_canonical_id = self
            .load_eval_dependency_canonical_with_fallback(source_canonical_id)
            .unwrap_or_else(|| source_canonical_id.to_string());

        if !alias_env_stack.insert(resolved_source_canonical_id.clone()) {
            return None;
        }

        let result = (|| {
            let snapshot = self.get_raw_analysis_snapshot(&resolved_source_canonical_id)?;
            let dep_resolutions =
                self.dependency_resolutions_for_eval(&resolved_source_canonical_id);
            let dep_eval_source =
                self.load_eval_dependency_source_text_with_fallback(&resolved_source_canonical_id)?;
            let dep_env = self.base_eval_env(&resolved_source_canonical_id)?;
            let decl = dep_env.type_symbols.get(exported_name)?.clone();
            let import_alloc = oxc_allocator::Allocator::new();
            let mut decl_required_import_names =
                verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
                    exported_name,
                    dep_eval_source.as_ref(),
                    &import_alloc,
                );
            if decl_required_import_names.is_empty() && !snapshot.imports.is_empty() {
                decl_required_import_names =
                    collect_required_import_names_for_type_decl(&decl, &dep_env);
            }
            let imported_inputs = self.imported_eval_inputs_inner(
                &resolved_source_canonical_id,
                &snapshot,
                &dep_resolutions,
                Some(&decl_required_import_names),
                alias_env_stack,
                budget,
            );
            canonical_dependencies.extend(imported_inputs.canonical_dependencies.iter().cloned());
            if imported_inputs.overflow.is_some() {
                return None;
            }
            let mut dep_env = self
                .build_owner_eval_env_with_inputs(
                    &resolved_source_canonical_id,
                    &snapshot,
                    &imported_inputs,
                    None,
                )?
                .env;
            let decl = dep_env.type_symbols.get(exported_name)?.clone();
            for param in &decl.type_parameters {
                dep_env.type_bindings.insert(
                    param.name.clone(),
                    verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
                );
            }
            let evaluated = verter_analysis::type_eval::evaluate(&decl.body, &mut dep_env);
            Some(evaluated)
        })();

        alias_env_stack.remove(&resolved_source_canonical_id);
        result
    }

    pub(crate) fn cache_dependency_candidates_from_snapshot(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        let mut candidates = std::collections::BTreeSet::new();

        for import in &snapshot.imports {
            if let Some(resolved) = import.resolved_canonical_id.as_deref() {
                candidates.insert(resolved.to_string());
                continue;
            }

            if let Some(target) = dep_resolutions
                .get(&import.source)
                .and_then(DependencyResolution::effective_target)
            {
                candidates.insert(target.to_string());
                continue;
            }

            if import.source.starts_with('.') {
                candidates
                    .extend(self.expand_relative_candidates(owner_canonical_id, &import.source));
            }
        }

        candidates
    }

    /// Compute evaluated types using pre-computed imported eval inputs.
    /// Avoids redundant `imported_eval_inputs()` calls when the caller
    /// already has them (e.g., `resolve_component_meta`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn compute_evaluated_types_with_inputs(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<verter_analysis::type_expand::ExpandedComponentTypes> {
        self.compute_evaluated_types_with_tracking(canonical, snapshot, imported_inputs)
            .and_then(|computed| computed.evaluated_types)
    }

    pub(crate) fn compute_evaluated_types_with_tracking(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<ComputedEvaluatedTypes> {
        let (source, cached_parse, _) = self.current_eval_state(canonical)?;
        let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
        let built =
            self.build_owner_eval_env_with_inputs(canonical, snapshot, imported_inputs, None)?;
        let mut env = built.env;
        let mut lookup = ImportedEvalLookup::new(self, canonical, snapshot);

        let budget = component_meta_expansion_budget();
        let result = verter_analysis::type_eval_build::expand_macro_types_with_lookup(
            snapshot.macros.as_ref(),
            Some(&eval_source),
            &mut env,
            Some(&built.requested_binding_names),
            &budget,
            &mut lookup,
        );
        let discovered_dependencies = lookup.into_discovered_dependencies();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} props={} define_props={} emits={} slot_bindings={} bindings={} discovered_deps={}",
                canonical,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
                discovered_dependencies.len(),
            ));
        }
        Some(ComputedEvaluatedTypes {
            evaluated_types: (!Self::is_expanded_types_empty(&result)).then_some(result),
            discovered_dependencies,
        })
    }

    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::type_expand::ExpandedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        resolved.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Uses `resolve_component_meta(Expanded)` as the single enrichment owner,
    /// then projects the result through the analysis-owned `extract_component_meta`.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        let include_fallthrough = !resolved
            .cached_eval_inputs
            .as_ref()
            .is_some_and(|inputs| inputs.overflow.is_some());
        let meta = extract_component_meta_from_resolved(
            self,
            canonical_or_alias,
            &resolved,
            include_fallthrough,
        );
        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} took {:?}",
                self.resolve_alias_or_canonical(canonical_or_alias),
                started.elapsed(),
            ));
        }
        Some(meta)
    }

    /// Combined query: resolves component-meta once and returns both the
    /// analysis projection and the resolved-meta sidecar. Avoids the
    /// double `resolve_component_meta(Expanded)` that happens if callers
    /// invoke `get_component_meta()` + `resolve_component_meta()` separately.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        let include_fallthrough = !resolved
            .cached_eval_inputs
            .as_ref()
            .is_some_and(|inputs| inputs.overflow.is_some());
        let analysis = extract_component_meta_from_resolved(
            self,
            canonical_or_alias,
            &resolved,
            include_fallthrough,
        );
        Some((analysis, resolved))
    }

    /// Resolve the accepted surface for a component's fallthrough inheritance.
    ///
    /// This is an internal method — the host owns all inheritance semantics.
    /// Returns `None` if the file doesn't exist or has no analysis.
    pub fn resolve_fallthrough_surface(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::FallthroughResolution> {
        let mut visiting = rustc_hash::FxHashSet::default();
        self.resolve_fallthrough_surface_internal(canonical_id, &mut visiting)
    }

    /// Internal recursive method with cycle detection.
    fn resolve_fallthrough_surface_internal(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        self.resolve_fallthrough_surface_internal_with_overrides(canonical_id, None, visiting)
    }

    fn resolve_fallthrough_surface_internal_with_overrides(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_analysis::component_meta::*;
        let started = component_meta_debug_enabled().then(Instant::now);

        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
            return Some(crate::types::FallthroughResolution {
                accepted_props: Vec::new(),
                accepted_events: Vec::new(),
                accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: vec![FallthroughBranch {
                        branch_key: "0".to_string(),
                        condition_text: None,
                        props: Vec::new(),
                        events: Vec::new(),
                        root_chain: vec![ResolvedRootStep::Unresolved {
                            tag: "component".to_string(),
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        }],
                        status: BranchStatus::Unresolved {
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        },
                    }],
                },
                fact_versions: self.current_dependency_fact_versions(
                    canonical_id,
                    &std::collections::BTreeSet::new(),
                ),
            });
        }

        let mut executor = FallthroughRequestExecutor::new(
            self,
            canonical_id.to_string(),
            prop_type_overrides,
            visiting,
        );
        let result = run_stable_request(&self.fallthrough_singleflight, &mut executor)
            .expect("fallthrough request execution is infallible");

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        visiting.remove(canonical_id);
        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_fallthrough owner={} cached attempt={} took {:?}",
                    canonical_id,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_fallthrough owner={} role={:?} stable attempt={} took {:?}",
                    canonical_id,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_fallthrough owner={} retries_exhausted took {:?}",
                    canonical_id,
                    started.elapsed(),
                )),
            }
        }
        result.value
    }

    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_analysis::component_meta::*;

        let resolved =
            self.resolve_component_meta(canonical_id, crate::types::ResolverMode::Expanded)?;
        let mut fallthrough_fact_versions = resolved.fact_versions.clone();

        let resolved_macros =
            component_meta_resolved_macros(&resolved.snapshot, &resolved.resolved_macros);
        let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
        let input = ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
                resolved.snapshot.script_flags,
            ),
            styles: &resolved.snapshot.styles,
            vue_api_calls: &resolved.snapshot.vue_api_calls,
            store_usages: &resolved.snapshot.store_usages,
            resolved_macros: &resolved_macros,
            resolved_type_registry: &resolved_type_registry,
            evaluated_types: resolved.evaluated_types.as_ref(),
            file_path: canonical_id,
        };
        let base_meta = extract_component_meta(input);

        let declared_prop_names: rustc_hash::FxHashSet<String> =
            base_meta.props.iter().map(|p| p.name.clone()).collect();
        let declared_event_names: rustc_hash::FxHashSet<String> =
            base_meta.events.iter().map(|e| e.name.clone()).collect();
        let declared_listener_aliases: rustc_hash::FxHashSet<String> = base_meta
            .props
            .iter()
            .filter_map(|p| verter_analysis::html_intrinsics::on_prop_to_event_name(&p.name))
            .collect();

        let mut accepted_props: Vec<AcceptedPropAnalysis> = base_meta
            .props
            .iter()
            .map(|p| AcceptedPropAnalysis {
                name: p.name.clone(),
                type_expr: p.type_expr.clone(),
                raw_type: p.raw_type.clone(),
                required: p.required,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            })
            .collect();

        let mut accepted_events: Vec<AcceptedEventAnalysis> = base_meta
            .events
            .iter()
            .map(|e| AcceptedEventAnalysis {
                name: e.name.clone(),
                payload: e.payload.clone(),
                raw_signature: e.raw_signature.clone(),
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedEventKind::DeclaredEmit,
            })
            .collect();

        match &base_meta.root_reachability {
            RootReachability::NoFallthrough { reason } => {
                Some(crate::types::FallthroughResolution {
                    accepted_props,
                    accepted_events,
                    accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
                    fallthrough_surface: FallthroughSurface::None {
                        reason: reason.clone(),
                    },
                    fact_versions: fallthrough_fact_versions,
                })
            }
            RootReachability::Branches { branches } => {
                let mut fallthrough_branches = Vec::new();
                let mut any_partial = false;
                let mut any_unresolved = false;
                let mut eval_env = if let Some(ref cached_inputs) = resolved.cached_eval_inputs {
                    self.build_fallthrough_eval_env_with_inputs(
                        canonical_id,
                        &resolved.snapshot,
                        prop_type_overrides,
                        cached_inputs,
                    )
                } else {
                    self.build_fallthrough_eval_env(
                        canonical_id,
                        &resolved.snapshot,
                        prop_type_overrides,
                    )
                };

                for branch in branches {
                    let branch_key = branch.branch_index.to_string();
                    let element_index = match &branch.target {
                        RootTargetRef::NativeElement { element_index, .. }
                        | RootTargetRef::DynamicComponentUsage { element_index, .. }
                        | RootTargetRef::ComponentUsage { element_index, .. }
                        | RootTargetRef::UnresolvedTarget { element_index, .. } => *element_index,
                    };
                    let resolved_consumed = self.resolve_root_consumption(
                        &resolved.snapshot,
                        element_index,
                        &branch.consumed,
                        branch.has_unknown_spread,
                        &mut eval_env,
                    );
                    let consumed = &resolved_consumed.bindings;
                    let parent_partial_reasons = resolved_consumed.partial_reasons.clone();

                    match &branch.target {
                        RootTargetRef::NativeElement { tag, .. } => {
                            push_native_candidate_branch(
                                self,
                                tag,
                                branch_key,
                                branch.condition_text.clone(),
                                consumed,
                                &parent_partial_reasons,
                                &declared_prop_names,
                                &declared_event_names,
                                &declared_listener_aliases,
                                &mut fallthrough_branches,
                                &mut any_partial,
                            );
                        }

                        RootTargetRef::DynamicComponentUsage { usage_index, .. } => {
                            let child_prop_overrides = self.build_generic_child_prop_overrides(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );
                            let candidates = self.resolve_dynamic_root_candidates(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );

                            if candidates.is_empty() {
                                any_unresolved = true;
                                fallthrough_branches.push(FallthroughBranch {
                                    branch_key,
                                    condition_text: branch.condition_text.clone(),
                                    props: Vec::new(),
                                    events: Vec::new(),
                                    root_chain: vec![ResolvedRootStep::Unresolved {
                                        tag: "component".to_string(),
                                        reason: UnresolvedBranchReason::DynamicComponentIs,
                                    }],
                                    status: BranchStatus::Unresolved {
                                        reason: UnresolvedBranchReason::DynamicComponentIs,
                                    },
                                });
                                continue;
                            }

                            let multiple_candidates = candidates.len() > 1;
                            for (candidate_index, candidate) in candidates.into_iter().enumerate() {
                                let candidate_key = if multiple_candidates {
                                    format!("{}.{}", branch_key, candidate_index)
                                } else {
                                    branch_key.clone()
                                };
                                match candidate {
                                    DynamicRootCandidate::NativeTag { tag } => {
                                        push_native_candidate_branch(
                                            self,
                                            &tag,
                                            candidate_key,
                                            branch.condition_text.clone(),
                                            consumed,
                                            &parent_partial_reasons,
                                            &declared_prop_names,
                                            &declared_event_names,
                                            &declared_listener_aliases,
                                            &mut fallthrough_branches,
                                            &mut any_partial,
                                        );
                                    }
                                    DynamicRootCandidate::ComponentImport {
                                        component_name,
                                        import_source,
                                    } => {
                                        append_component_candidate_branches(
                                            self,
                                            canonical_id,
                                            &component_name,
                                            &import_source,
                                            candidate_key,
                                            branch.condition_text.clone(),
                                            consumed,
                                            &parent_partial_reasons,
                                            child_prop_overrides.as_ref(),
                                            &declared_prop_names,
                                            &declared_event_names,
                                            &declared_listener_aliases,
                                            &mut fallthrough_branches,
                                            &mut any_partial,
                                            &mut any_unresolved,
                                            &mut fallthrough_fact_versions,
                                            visiting,
                                        );
                                    }
                                }
                            }
                        }

                        RootTargetRef::ComponentUsage {
                            usage_index,
                            name,
                            import_source,
                            ..
                        } => {
                            let child_prop_overrides = self.build_generic_child_prop_overrides(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );

                            match import_source.as_deref() {
                                Some(import_source) => {
                                    append_component_candidate_branches(
                                        self,
                                        canonical_id,
                                        name,
                                        import_source,
                                        branch_key,
                                        branch.condition_text.clone(),
                                        consumed,
                                        &parent_partial_reasons,
                                        child_prop_overrides.as_ref(),
                                        &declared_prop_names,
                                        &declared_event_names,
                                        &declared_listener_aliases,
                                        &mut fallthrough_branches,
                                        &mut any_partial,
                                        &mut any_unresolved,
                                        &mut fallthrough_fact_versions,
                                        visiting,
                                    );
                                }
                                None => {
                                    any_unresolved = true;
                                    fallthrough_branches.push(FallthroughBranch {
                                        branch_key,
                                        condition_text: branch.condition_text.clone(),
                                        props: Vec::new(),
                                        events: Vec::new(),
                                        root_chain: vec![ResolvedRootStep::Unresolved {
                                            tag: name.clone(),
                                            reason: UnresolvedBranchReason::UnresolvedChildImport {
                                                import_source: None,
                                            },
                                        }],
                                        status: BranchStatus::Unresolved {
                                            reason: UnresolvedBranchReason::UnresolvedChildImport {
                                                import_source: None,
                                            },
                                        },
                                    });
                                }
                            }
                        }

                        RootTargetRef::UnresolvedTarget { tag, reason, .. } => {
                            any_unresolved = true;
                            fallthrough_branches.push(FallthroughBranch {
                                branch_key,
                                condition_text: branch.condition_text.clone(),
                                props: Vec::new(),
                                events: Vec::new(),
                                root_chain: vec![ResolvedRootStep::Unresolved {
                                    tag: tag.clone(),
                                    reason: UnresolvedBranchReason::RootTarget {
                                        reason: reason.clone(),
                                    },
                                }],
                                status: BranchStatus::Unresolved {
                                    reason: UnresolvedBranchReason::RootTarget {
                                        reason: reason.clone(),
                                    },
                                },
                            });
                        }
                    }
                }

                fallthrough_branches.sort_by(|a, b| a.branch_key.cmp(&b.branch_key));
                let total_branches = fallthrough_branches.len();
                let force_conditional = any_partial || any_unresolved;

                let mut inherited_prop_map: rustc_hash::FxHashMap<
                    String,
                    (AcceptedPropAnalysis, Vec<String>),
                > = rustc_hash::FxHashMap::default();
                let mut inherited_event_map: rustc_hash::FxHashMap<
                    String,
                    (AcceptedEventAnalysis, Vec<String>),
                > = rustc_hash::FxHashMap::default();

                for fb in &fallthrough_branches {
                    if matches!(fb.status, BranchStatus::Unresolved { .. }) {
                        continue;
                    }

                    for fp in &fb.props {
                        let entry =
                            inherited_prop_map
                                .entry(fp.name.clone())
                                .or_insert_with(|| {
                                    (
                                        AcceptedPropAnalysis {
                                            name: fp.name.clone(),
                                            type_expr: fp.type_expr.clone(),
                                            raw_type: fp.raw_type.clone(),
                                            required: false,
                                            provenance: MemberProvenance::Inherited {
                                                sources: fp.sources.clone(),
                                            },
                                            availability: MemberAvailability::Always,
                                            kind: AcceptedPropKind::Attr,
                                        },
                                        Vec::new(),
                                    )
                                });
                        merge_type_expr(&mut entry.0.type_expr, &fp.type_expr);
                        if entry.0.raw_type != fp.raw_type {
                            entry.0.raw_type = None;
                        }
                        if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                            merge_inherited_sources(sources, &fp.sources);
                        }
                        entry.1.push(fb.branch_key.clone());
                    }

                    for fe in &fb.events {
                        let entry =
                            inherited_event_map
                                .entry(fe.name.clone())
                                .or_insert_with(|| {
                                    (
                                        AcceptedEventAnalysis {
                                            name: fe.name.clone(),
                                            payload: fe.payload.clone(),
                                            raw_signature: fe.raw_signature.clone(),
                                            provenance: MemberProvenance::Inherited {
                                                sources: fe.sources.clone(),
                                            },
                                            availability: MemberAvailability::Always,
                                            kind: AcceptedEventKind::Listener,
                                        },
                                        Vec::new(),
                                    )
                                });
                        merge_type_expr(&mut entry.0.payload, &fe.payload);
                        if entry.0.raw_signature != fe.raw_signature {
                            entry.0.raw_signature = None;
                        }
                        if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                            merge_inherited_sources(sources, &fe.sources);
                        }
                        entry.1.push(fb.branch_key.clone());
                    }
                }

                for (_, (prop, branch_keys)) in inherited_prop_map.iter_mut() {
                    branch_keys.sort();
                    branch_keys.dedup();
                    if force_conditional || branch_keys.len() < total_branches {
                        prop.availability = MemberAvailability::Conditional {
                            branch_keys: branch_keys.clone(),
                        };
                    }
                }
                for (_, (event, branch_keys)) in inherited_event_map.iter_mut() {
                    branch_keys.sort();
                    branch_keys.dedup();
                    if force_conditional || branch_keys.len() < total_branches {
                        event.availability = MemberAvailability::Conditional {
                            branch_keys: branch_keys.clone(),
                        };
                    }
                }

                let mut inherited_props: Vec<AcceptedPropAnalysis> =
                    inherited_prop_map.into_values().map(|(p, _)| p).collect();
                inherited_props.sort_by(|a, b| a.name.cmp(&b.name));
                accepted_props.extend(inherited_props);

                let mut inherited_events: Vec<AcceptedEventAnalysis> =
                    inherited_event_map.into_values().map(|(e, _)| e).collect();
                inherited_events.sort_by(|a, b| a.name.cmp(&b.name));
                accepted_events.extend(inherited_events);

                let completeness = if any_partial || any_unresolved {
                    AcceptedSurfaceCompleteness::LowerBound
                } else {
                    AcceptedSurfaceCompleteness::Exact
                };

                Some(crate::types::FallthroughResolution {
                    accepted_props,
                    accepted_events,
                    accepted_surface_completeness: completeness,
                    fallthrough_surface: FallthroughSurface::Branches {
                        branches: fallthrough_branches,
                    },
                    fact_versions: fallthrough_fact_versions,
                })
            }
        }
    }

    fn build_fallthrough_eval_env(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        let dep_resolutions = self.dependency_resolutions_for_eval(canonical_id);
        let imported_inputs = self.imported_eval_inputs(canonical_id, snapshot, &dep_resolutions);
        self.build_fallthrough_eval_env_with_inputs(
            canonical_id,
            snapshot,
            prop_type_overrides,
            &imported_inputs,
        )
    }

    fn build_fallthrough_eval_env_with_inputs(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        Some(
            self.build_owner_eval_env_with_inputs(
                canonical_id,
                snapshot,
                imported_inputs,
                prop_type_overrides,
            )?
            .env,
        )
    }

    fn build_owner_eval_env_with_inputs(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
    ) -> Option<OwnerEvalEnvBuild> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let mut env = self.base_eval_env(canonical_id)?;
        let local_type_names: rustc_hash::FxHashSet<String> =
            env.type_symbols.keys().cloned().collect();
        let local_value_names: rustc_hash::FxHashSet<String> =
            env.value_symbols.keys().cloned().collect();
        let requested_binding_names = collect_requested_binding_names(snapshot);
        for dep_source in &imported_inputs.sources {
            let dep_env = self
                .base_eval_env(dep_source.canonical_id.as_str())
                .unwrap_or_else(|| {
                    verter_analysis::type_eval_build::parse_and_build_env(
                        dep_source.source.as_ref(),
                    )
                });
            env.extend_missing(dep_env);
        }
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "build_owner_eval_env owner={} after_dep_merge dep_sources={} type_symbols={} value_symbols={} took {:?}",
                canonical_id,
                imported_inputs.sources.len(),
                env.type_symbols.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
        self.inject_imported_type_aliases(&mut env, &local_type_names, imported_inputs);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "build_owner_eval_env owner={} after_type_aliases type_symbols={} value_symbols={} took {:?}",
                canonical_id,
                env.type_symbols.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
        self.materialize_imported_runtime_values_into_env(snapshot, &local_value_names, &mut env);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "build_owner_eval_env owner={} after_runtime_values type_symbols={} value_symbols={} took {:?}",
                canonical_id,
                env.type_symbols.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
        if let Some(overrides) = prop_type_overrides {
            inject_prop_type_overrides(&mut env, overrides);
        }
        Some(OwnerEvalEnvBuild {
            env,
            requested_binding_names,
        })
    }

    fn inject_imported_type_aliases(
        &self,
        env: &mut verter_analysis::type_eval::EvalEnv,
        owner_local_type_names: &rustc_hash::FxHashSet<String>,
        imported_inputs: &ImportedEvalInputs,
    ) {
        for alias in &imported_inputs.type_aliases {
            if owner_local_type_names.contains(&alias.local_name) {
                continue;
            }
            env.type_symbols
                .insert(alias.local_name.clone(), alias.decl.clone());
        }
    }

    fn materialize_imported_runtime_values_into_env(
        &self,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        env: &mut verter_analysis::type_eval::EvalEnv,
    ) {
        let started = component_meta_debug_enabled().then(Instant::now);
        let mut dep_env_cache: rustc_hash::FxHashMap<
            String,
            Option<verter_analysis::type_eval::EvalEnv>,
        > = rustc_hash::FxHashMap::default();

        for import in &snapshot.imports {
            if import.is_type_only {
                continue;
            }
            let Some(dep_canonical_id) = import.resolved_canonical_id.as_deref() else {
                continue;
            };

            let dep_env = dep_env_cache
                .entry(dep_canonical_id.to_string())
                .or_insert_with(|| {
                    self.base_eval_env(dep_canonical_id).or_else(|| {
                        self.load_eval_dependency_source_text_with_fallback(dep_canonical_id)
                            .map(|source| {
                                verter_analysis::type_eval_build::parse_and_build_env(
                                    source.as_ref(),
                                )
                            })
                    })
                });
            let Some(dep_env) = dep_env.as_ref() else {
                continue;
            };

            for binding in &import.bindings {
                if binding.is_type_only
                    || matches!(
                        binding.kind,
                        verter_analysis::types::ImportBindingKind::Namespace
                    )
                {
                    continue;
                }
                if owner_local_value_names.contains(&binding.name) {
                    continue;
                }
                let Some(imported_name) = binding.imported_name.as_deref() else {
                    continue;
                };
                let Some(dep_value) = dep_env.value_symbols.get(imported_name).cloned() else {
                    continue;
                };

                let mut alias = dep_value;
                alias.name = binding.name.clone();
                env.add_value(alias);
            }
        }
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "materialize_runtime_values imports={} value_symbols={} took {:?}",
                snapshot.imports.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
    }

    fn build_generic_child_prop_overrides(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>> {
        if !self.config.generic_root_propagation {
            return None;
        }

        let template = snapshot.template.as_deref()?;
        let usage = template.components.get(usage_index as usize)?;
        let mut overrides = rustc_hash::FxHashMap::default();

        for prop in &usage.props {
            if prop.from_spread {
                continue;
            }
            if usage.is_dynamic && prop.name == "is" {
                continue;
            }

            let Some(prop_type) = resolve_usage_prop_type(prop, eval_env) else {
                continue;
            };
            overrides.insert(prop.name.clone(), prop_type);
        }

        if overrides.is_empty() {
            None
        } else {
            Some(overrides)
        }
    }

    fn resolve_root_consumption(
        &self,
        snapshot: &FileAnalysisSnapshot,
        element_index: u32,
        base: &verter_analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        use verter_analysis::component_meta::PartialBranchReason;

        let mut resolved = ResolvedConsumedBindings {
            bindings: verter_analysis::component_meta::ConsumedRootBindings {
                attrs: base.attrs.clone(),
                listeners: base.listeners.clone(),
                has_dynamic_attr_name: base.has_dynamic_attr_name,
                has_dynamic_listener_name: base.has_dynamic_listener_name,
            },
            partial_reasons: Vec::new(),
        };

        if base.has_dynamic_attr_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicAttrName,
            );
        }
        if base.has_dynamic_listener_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicListenerName,
            );
        }

        if has_unknown_spread {
            let Some(template) = snapshot.template.as_deref() else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let Some(element) = template.elements.get(element_index as usize) else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let spread_directives: Vec<_> = element
                .directives
                .iter()
                .filter(|directive| directive.name == "bind" && directive.argument.is_none())
                .collect();

            if spread_directives.is_empty() {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
            }

            for directive in spread_directives {
                let Some(expression) = directive.expression.as_deref() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(env) = eval_env.as_mut() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(ty) =
                    verter_analysis::type_eval_build::evaluate_value_expression(expression, env)
                else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(summary) = known_spread_keys_from_type_expr(&ty) else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                resolved.bindings.attrs.extend(summary.attrs.into_iter());
                resolved
                    .bindings
                    .listeners
                    .extend(summary.listeners.into_iter());
                if !summary.exact {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                }
            }
        }

        resolved.bindings.attrs.sort();
        resolved.bindings.attrs.dedup();
        resolved.bindings.listeners.sort();
        resolved.bindings.listeners.dedup();
        resolved.partial_reasons.sort();
        resolved.partial_reasons.dedup();
        resolved
    }

    fn resolve_dynamic_root_candidates(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Vec<DynamicRootCandidate> {
        let Some(template) = snapshot.template.as_deref() else {
            return Vec::new();
        };
        let Some(usage) = template.components.get(usage_index as usize) else {
            return Vec::new();
        };
        let Some(is_prop) = usage.props.iter().find(|prop| prop.name == "is") else {
            return Vec::new();
        };

        let expression = is_prop
            .expression
            .clone()
            .or_else(|| is_prop.is_shorthand.then(|| is_prop.name.clone()));
        let Some(expression) = expression else {
            return Vec::new();
        };

        let mut candidates = Vec::new();
        if let Some(lowered) =
            verter_analysis::type_eval_build::parse_value_expression_type(&expression)
        {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &lowered, snapshot,
            ));
        }
        if let Some(env) = eval_env.as_mut() {
            if let Some(evaluated) =
                verter_analysis::type_eval_build::evaluate_value_expression(&expression, env)
            {
                candidates.extend(collect_dynamic_root_candidates_from_type(
                    &evaluated, snapshot,
                ));
            }
        }

        candidates.sort_by(|left, right| match (left, right) {
            (
                DynamicRootCandidate::NativeTag { tag: left_tag },
                DynamicRootCandidate::NativeTag { tag: right_tag },
            ) => left_tag.cmp(right_tag),
            (
                DynamicRootCandidate::NativeTag { .. },
                DynamicRootCandidate::ComponentImport { .. },
            ) => std::cmp::Ordering::Less,
            (
                DynamicRootCandidate::ComponentImport { .. },
                DynamicRootCandidate::NativeTag { .. },
            ) => std::cmp::Ordering::Greater,
            (
                DynamicRootCandidate::ComponentImport {
                    component_name: left_name,
                    import_source: left_source,
                },
                DynamicRootCandidate::ComponentImport {
                    component_name: right_name,
                    import_source: right_source,
                },
            ) => (left_name, left_source).cmp(&(right_name, right_source)),
        });
        candidates.dedup();
        candidates
    }

    /// Store fallthrough resolution in the compile cache.
    fn cache_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        result: &crate::types::FallthroughResolution,
    ) {
        let resolution = Arc::new(result.clone());
        self.fallthrough_cache.insert_arc(
            fallthrough_cache_key(
                canonical_id,
                self.config.generic_root_propagation,
                prop_type_overrides,
            ),
            resolution.clone(),
            result.fact_versions.clone(),
        );
        if prop_type_overrides.is_none() {
            self.mirror_cached_fallthrough_arc(canonical_id, resolution);
        }
    }

    fn mirror_cached_fallthrough_arc(
        &self,
        canonical_id: &str,
        resolution: Arc<crate::types::FallthroughResolution>,
    ) {
        #[cfg(feature = "scheduler")]
        {
            if self.effective_file_state(canonical_id, None).is_some() {
                let mut cc = self
                    .compile_cache
                    .entry(canonical_id.to_string())
                    .or_default();
                cc.cached_fallthrough = Some(crate::types::CachedFallthroughEntry {
                    fact_versions: resolution.fact_versions.clone(),
                    generic_root_propagation: self.config.generic_root_propagation,
                    resolution,
                });
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (canonical_id, resolution);
        }
    }

    fn parse_dependency_set_for_file(
        &self,
        canonical_id: &str,
    ) -> std::collections::BTreeSet<String> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let Some(source) = self.scheduler.try_get_source(canonical_id) else {
                return std::collections::BTreeSet::new();
            };
            let Some(hd) = source.downcast_data::<HostSourceData>() else {
                return std::collections::BTreeSet::new();
            };

            hd.parse
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    hd.parse
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical_id) else {
                return std::collections::BTreeSet::new();
            };

            entry
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    entry
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }
    }

    fn resolved_dependency_targets(
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        dep_resolutions
            .values()
            .filter_map(|res| res.effective_target().map(|s| s.to_string()))
            .collect()
    }

    pub(crate) fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        transitive_deps: &std::collections::BTreeSet<String>,
    ) {
        let mut new_deps = self.parse_dependency_set_for_file(canonical_id);

        #[cfg(feature = "scheduler")]
        let old_deps = {
            let mut cc_ref = self
                .compile_cache
                .entry(canonical_id.to_string())
                .or_default();
            let cc = cc_ref.value_mut();
            new_deps.extend(Self::resolved_dependency_targets(
                &cc.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = cc.dependencies.clone();
            cc.dependencies = new_deps.clone();
            old_deps
        };

        #[cfg(not(feature = "scheduler"))]
        let old_deps = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(canonical_id) else {
                return;
            };
            new_deps.extend(Self::resolved_dependency_targets(
                &entry.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = entry.dependencies.clone();
            entry.dependencies = new_deps.clone();
            old_deps
        };

        if old_deps != new_deps {
            self.update_reverse_deps(canonical_id, &old_deps, &new_deps);
        }
    }

    /// Returns the original source for a file by canonical ID or alias.
    /// Returns `None` when the file does not exist in the host.
    pub fn get_source(&self, canonical_or_alias: &str) -> Option<std::sync::Arc<str>> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            self.scheduler
                .try_get_source(&canonical)
                .map(|s| s.source.clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.source.clone())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_template_analysis(
        &self,
        canonical: &str,
        source: &Arc<str>,
        cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
        src_blocks: &[crate::SrcBlockInfo],
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_analysis::AnalyzedImport],
        macros: &[verter_analysis::AnalyzedMacro],
        bindings: &[verter_analysis::AnalyzedBinding],
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in external_requests {
                let dep_source =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier);
                if let Some(source) = dep_source {
                    map.insert(req.resolved_canonical_id.clone(), source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        for req in external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return None;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                source, src_blocks, &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_core::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return None;
        }

        let raw = compiled.template_data?;
        let (imports, unions, props_name) =
            crate::host_resolve::template_converter_inputs(imports, macros, bindings);
        Some(Arc::new(crate::template_convert::convert_raw_to_analysis(
            &raw,
            &imports,
            &unions,
            props_name.as_deref(),
        )))
    }

    /// Lazily compute template analysis for a VueSfc file that hasn't been compiled.
    ///
    /// Uses `CompileTarget::META` (= SCRIPT + TEMPLATE_DATA) via the core
    /// `compile_from_parsed()` — bypassing the host `compile_entry()` which fails
    /// on unresolved macro type deps. External-src blocks are merged using the
    /// same `merge_external_sources()` helper. Results are persisted on the entry
    /// for inline-template files to avoid recomputation.
    pub(crate) fn compute_template_analysis_if_missing(
        &self,
        canonical: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        if snapshot.template.is_some() {
            return;
        }

        #[cfg(feature = "scheduler")]
        let (source, cached_parse, src_blocks, external_requests) = {
            use crate::host_executor::HostSourceData;
            if let Some(snap) = self.scheduler.try_get_source(canonical) {
                let Some(hd) = snap.downcast_data::<HostSourceData>() else {
                    return;
                };
                if hd.file_kind != FileKind::VueSfc {
                    return;
                }
                (
                    snap.source.clone(),
                    hd.cached_parse.clone(),
                    hd.parse.src_blocks.clone(),
                    hd.parse.external_requests.clone(),
                )
            } else {
                let Some(source) = self.read_analysis_source(canonical) else {
                    return;
                };
                if !canonical.ends_with(".vue") {
                    return;
                }
                let (parse, parsed) = crate::parse::parse_vue_snapshot(
                    canonical,
                    &source,
                    self.config.effective_scope(),
                );
                (
                    source,
                    Some(Arc::new(parsed)),
                    parse.src_blocks,
                    parse.external_requests,
                )
            }
        };

        #[cfg(not(feature = "scheduler"))]
        let (source, cached_parse, src_blocks, external_requests) = {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical) {
                if entry.file_kind != FileKind::VueSfc {
                    return;
                }
                (
                    entry.source.clone(),
                    entry.cached_parse.clone(),
                    entry.src_blocks.clone(),
                    entry.external_requests.clone(),
                )
            } else {
                drop(files);
                let Some(source) = self.read_analysis_source(canonical) else {
                    return;
                };
                if !canonical.ends_with(".vue") {
                    return;
                }
                let (parse, parsed) = crate::parse::parse_vue_snapshot(
                    canonical,
                    &source,
                    self.config.effective_scope(),
                );
                (
                    source,
                    Some(Arc::new(parsed)),
                    parse.src_blocks,
                    parse.external_requests,
                )
            }
        };

        // Resolve external src blocks (e.g., <template src="./tpl.html">)
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in &external_requests {
                if let Some(dep_source) =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier)
                {
                    map.insert(req.resolved_canonical_id.clone(), dep_source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        // Abort if any external src blocks are unresolved (same guard as compile_entry)
        for req in &external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                &source,
                &src_blocks,
                &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        // Parse SFC (reuse cached parse when no external src)
        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        // Compile with META target — script codegen + template data, no JS/TSX output
        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        // Bail on structural compile errors that would invalidate template data.
        // Skip type-resolution errors (XInvalidMacroType, XMissingMacroType) since
        // template slot extraction doesn't depend on type resolution.
        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_core::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return;
        }

        // Convert RawTemplateData → TemplateAnalysisSnapshot using existing converter
        if let Some(raw) = compiled.template_data {
            // Build converter inputs from snapshot (already computed, not stale entry)
            let imports: Vec<(String, String)> = snapshot
                .imports
                .iter()
                .flat_map(|imp| {
                    imp.bindings
                        .iter()
                        .map(|b| (b.name.clone(), imp.source.clone()))
                })
                .collect();

            // Build binding_class_unions + props_binding_name from snapshot
            let mut unions: Vec<(String, Vec<String>)> = Vec::new();
            let define_props = snapshot
                .macros
                .iter()
                .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps);
            if let Some(dp) = define_props {
                for field in &dp.prop_fields {
                    if let Some(type_ann) = &field.type_annotation {
                        let classes = verter_analysis::parse_string_literal_union(type_ann);
                        if !classes.is_empty() {
                            unions.push((field.name.clone(), classes));
                        }
                    }
                }
            }
            for binding in &snapshot.bindings {
                if let Some(type_ann) = &binding.type_annotation {
                    let effective =
                        verter_analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
                    let classes = verter_analysis::parse_string_literal_union(effective);
                    if !classes.is_empty() {
                        unions.push((binding.name.clone(), classes));
                    }
                }
            }
            let props_name = define_props.and_then(|dp| dp.binding_name.clone());

            let tpl = crate::template_convert::convert_raw_to_analysis(
                &raw,
                &imports,
                &unions,
                props_name.as_deref(),
            );
            let tpl_arc = Arc::new(tpl);
            snapshot.template = Some(Arc::clone(&tpl_arc));

            // Persist for inline templates only. Files with external src
            // blocks are NOT persisted to avoid stale cache when the external
            // dep changes (reverse-dep invalidation only clears compile_slots).
            if src_blocks.is_empty() {
                #[cfg(feature = "scheduler")]
                if let Some(mut cc) = self.compile_cache.get_mut(canonical) {
                    cc.raw_template_analysis = Some(tpl_arc);
                }

                #[cfg(not(feature = "scheduler"))]
                {
                    let mut files = write_lock(&self.files);
                    if let Some(entry) = files.get_mut(canonical) {
                        entry.template_analysis = Some(tpl_arc);
                    }
                }
            }
        }
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    /// Returns `None` if the file doesn't exist.
    /// When `eager_analysis` is false, computes analysis on demand from stored source.
    ///
    /// Template analysis is lazily computed via `CompileTarget::META` when the scope
    /// includes template analysis and no prior compilation has populated it.
    ///
    /// Import `resolved_canonical_id` fields are populated lazily using the host's
    /// file map, alias map, and parent dependency set.
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        self.provenance
            .get_analysis_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let analysis_started = component_meta_debug_enabled().then(Instant::now);
        self.get_analysis_snapshot_internal(&canonical, analysis_started)
    }

    fn get_analysis_snapshot_internal(
        &self,
        canonical: &str,
        analysis_started: Option<Instant>,
    ) -> Option<FileAnalysisSnapshot> {
        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let Some(source_snap) = self.scheduler.try_get_source(canonical) else {
                let source = self.read_analysis_source(canonical)?;
                let snapshot = self.build_snapshot_from_source(canonical, &source);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    analysis_started,
                ));
            };
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            let source = source_snap.source.clone();
            let cached_parse = hd.cached_parse.clone();

            let scope = self.config.effective_scope();
            if file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let stored_script = hd.parse.script_analysis.clone();
                let stored_styles = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| Arc::clone(&ad.style_analyses))
                    })
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                let template = self
                    .compile_cache
                    .get(canonical)
                    .and_then(|cc| cc.raw_template_analysis.clone());
                let export_sigs = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| ad.export_signatures.clone())
                    })
                    .unwrap_or_default();
                drop(source_snap);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        crate::parse::build_script_analysis_from_parsed(parsed, &source)
                    } else {
                        crate::parse::build_script_analysis_from_source(&source)
                    }
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        Arc::new(crate::parse::build_style_analyses_from_parsed(
                            parsed, &source, canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let snapshot = FileAnalysisSnapshot {
                    imports: script_analysis.imports,
                    module_references: Arc::new(script_analysis.module_references),
                    bindings: script_analysis.bindings,
                    macros: Arc::new(script_analysis.macros),
                    macro_type_deps: Arc::new(script_analysis.macro_type_deps),
                    script_flags: script_analysis.flags.bits(),
                    styles: style_analyses,
                    template,
                    vue_api_calls: Arc::new(script_analysis.vue_api_calls),
                    dom_query_calls: Arc::new(script_analysis.dom_query_calls),
                    css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
                    script_binding_occurrences: Arc::new(
                        script_analysis.script_binding_occurrences,
                    ),
                    export_signatures: Arc::new(export_sigs),
                    options_api: script_analysis.options_api,
                    store_usages: Arc::new(script_analysis.store_usages),
                    store_definitions: Arc::new(script_analysis.store_definitions),
                    is_typescript: script_analysis.is_typescript,
                };
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    scope.needs_template_analysis(),
                    analysis_started,
                ));
            }
            drop(source_snap);

            let snapshot = self.build_snapshot_from_scheduler(canonical)?;
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical) else {
                drop(files);
                let source = self.read_analysis_source(canonical)?;
                let snapshot = self.build_snapshot_from_source(canonical, &source);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    analysis_started,
                ));
            };

            let scope = self.config.effective_scope();
            if entry.file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let source = entry.source.clone();
                let stored_script = entry.script_analysis.clone();
                let stored_styles = Arc::clone(&entry.style_analyses);
                let template = entry.template_analysis.clone();
                let cached_parse = entry.cached_parse.clone();
                let export_sigs = entry.export_signatures.clone();
                drop(files);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        crate::parse::build_script_analysis_from_parsed(parsed, &source)
                    } else {
                        crate::parse::build_script_analysis_from_source(&source)
                    }
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        Arc::new(crate::parse::build_style_analyses_from_parsed(
                            parsed, &source, &canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, &canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let snapshot = FileAnalysisSnapshot {
                    imports: script_analysis.imports,
                    module_references: Arc::new(script_analysis.module_references),
                    bindings: script_analysis.bindings,
                    macros: Arc::new(script_analysis.macros),
                    macro_type_deps: Arc::new(script_analysis.macro_type_deps),
                    script_flags: script_analysis.flags.bits(),
                    styles: style_analyses,
                    template,
                    vue_api_calls: Arc::new(script_analysis.vue_api_calls),
                    dom_query_calls: Arc::new(script_analysis.dom_query_calls),
                    css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
                    script_binding_occurrences: Arc::new(
                        script_analysis.script_binding_occurrences,
                    ),
                    export_signatures: Arc::new(export_sigs),
                    options_api: script_analysis.options_api,
                    store_usages: Arc::new(script_analysis.store_usages),
                    store_definitions: Arc::new(script_analysis.store_definitions),
                    is_typescript: script_analysis.is_typescript,
                };
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    scope.needs_template_analysis(),
                    analysis_started,
                ));
            }

            let snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ))
        }
    }

    /// Get the current whole_hash for a file.
    pub(crate) fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            let snap = self.scheduler.try_get_source(canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.whole_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(canonical).map(|entry| entry.whole_hash)
        }
    }

    /// Returns the semantic hash for a file by canonical ID or alias.
    ///
    /// The semantic hash changes when the file's semantically significant content
    /// changes (script, template, scoped styles). Returns `None` for missing files.
    pub fn get_semantic_hash(&self, canonical_or_alias: &str) -> Option<Hash16> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.semantic_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.semantic_hash)
        }
    }

    /// Returns the compile-blocking dependencies for a Vue SFC.
    ///
    /// This exposes the SFC's external `src` blocks and macro type dependencies
    /// so embedding environments can resolve/load them before triggering codegen.
    pub fn get_compile_blockers(
        &self,
        canonical_or_alias: &str,
    ) -> Option<CompileBlockersSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            // Use pre-built AnalysisArcs for cheap pointer clone instead of Vec clone
            let macro_type_deps = self
                .scheduler
                .try_get_analysis(&canonical)
                .and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| Arc::clone(&ad.arcs.macro_type_deps))
                })
                .unwrap_or_else(|| Arc::new(hd.parse.script_analysis.macro_type_deps.clone()));
            Some(CompileBlockersSnapshot {
                external_source_requests: hd.parse.external_requests.clone(),
                macro_type_deps,
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            if entry.file_kind != FileKind::VueSfc {
                return None;
            }
            Some(CompileBlockersSnapshot {
                external_source_requests: entry.external_requests.clone(),
                macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            })
        }
    }

    /// Returns analysis snapshots for multiple files in a single lock acquisition.
    ///
    /// More efficient than calling `get_analysis()` in a loop: acquires the
    /// files read-lock once for all files instead of N separate acquisitions.
    ///
    /// Accepts canonical IDs, aliases, or `None` to return all files.
    /// When `canonical_ids` is `None`, returns snapshots for every file in the host.
    pub fn get_analysis_batch(
        &self,
        canonical_ids: &[&str],
    ) -> Vec<(String, FileAnalysisSnapshot)> {
        let mut results = Vec::with_capacity(canonical_ids.len());

        #[cfg(feature = "scheduler")]
        {
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(cc) = self.compile_cache.get(&canonical) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&canonical) {
                    results.push((canonical, snapshot));
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(entry) = files.get(&canonical) {
                    let snapshot = Self::build_snapshot_from_entry(entry);
                    results.push((canonical, snapshot));
                }
            }
        }

        // Post-process: resolve imports and enrich bindings for all
        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Returns analysis snapshots for all files in the host.
    ///
    /// Single lock acquisition for the entire file map. Use instead of
    /// `list_files()` + loop when you need analysis for every file.
    pub fn get_analysis_all(&self) -> Vec<(String, FileAnalysisSnapshot)> {
        #[cfg(feature = "scheduler")]
        let mut results = {
            let ids = self.scheduler.node_ids();
            let mut results = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(cc) = self.compile_cache.get(&id) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&id) {
                    results.push((id, snapshot));
                }
            }
            results
        };

        #[cfg(not(feature = "scheduler"))]
        let mut results = {
            let files = read_lock(&self.files);
            let mut results = Vec::with_capacity(files.len());
            for (canonical, entry) in files.iter() {
                let snapshot = Self::build_snapshot_from_entry(entry);
                results.push((canonical.clone(), snapshot));
            }
            results
        };

        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Build a `FileAnalysisSnapshot` from a `FileEntry` using Arc::clone
    /// for immutable fields and deep clone for mutable fields (imports, bindings).
    #[cfg(not(feature = "scheduler"))]
    pub(crate) fn build_snapshot_from_entry(entry: &crate::FileEntry) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            // Arc::clone — cheap pointer bump, no deep copy
            module_references: Arc::clone(&entry.arc_script_cache.module_references),
            macros: Arc::clone(&entry.arc_script_cache.macros),
            macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            script_flags: entry.script_analysis.flags.bits(),
            styles: Arc::clone(&entry.style_analyses),
            template: entry.template_analysis.clone(),
            vue_api_calls: Arc::clone(&entry.arc_script_cache.vue_api_calls),
            dom_query_calls: Arc::clone(&entry.arc_script_cache.dom_query_calls),
            css_var_manipulations: Arc::clone(&entry.arc_script_cache.css_var_manipulations),
            script_binding_occurrences: Arc::clone(
                &entry.arc_script_cache.script_binding_occurrences,
            ),
            export_signatures: Arc::new(entry.export_signatures.clone()),
            options_api: entry.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&entry.arc_script_cache.store_usages),
            store_definitions: Arc::clone(&entry.arc_script_cache.store_definitions),
            is_typescript: entry.script_analysis.is_typescript,
        }
    }

    /// Build a `FileAnalysisSnapshot` from scheduler snapshots and compile_cache.
    ///
    /// Reads `HostAnalysisData` for script analysis, export signatures, styles,
    /// and pre-computed `AnalysisArcs`. Template analysis comes from compile_cache
    /// (raw_template_analysis). Uses Arc::clone for all immutable fields.
    #[cfg(feature = "scheduler")]
    pub(crate) fn build_snapshot_from_scheduler(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

        let template = self
            .compile_cache
            .get(canonical)
            .and_then(|cc| cc.raw_template_analysis.clone());

        Some(FileAnalysisSnapshot {
            imports: ad.script_analysis.imports.clone(),
            bindings: ad.script_analysis.bindings.clone(),
            module_references: Arc::clone(&ad.arcs.module_references),
            macros: Arc::clone(&ad.arcs.macros),
            macro_type_deps: Arc::clone(&ad.arcs.macro_type_deps),
            script_flags: ad.script_analysis.flags.bits(),
            styles: Arc::clone(&ad.style_analyses),
            template,
            vue_api_calls: Arc::clone(&ad.arcs.vue_api_calls),
            dom_query_calls: Arc::clone(&ad.arcs.dom_query_calls),
            css_var_manipulations: Arc::clone(&ad.arcs.css_var_manipulations),
            script_binding_occurrences: Arc::clone(&ad.arcs.script_binding_occurrences),
            export_signatures: Arc::new(ad.export_signatures.clone()),
            options_api: ad.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&ad.arcs.store_usages),
            store_definitions: Arc::clone(&ad.arcs.store_definitions),
            is_typescript: ad.script_analysis.is_typescript,
        })
    }

    /// Resolve the source code of a dependency file.
    ///
    /// Tries scheduler (native) or files map (WASM) first, falling back to
    /// VFS resolution + disk read. Used by template analysis and external src
    /// block resolution.
    pub(crate) fn resolve_dep_source(
        &self,
        owner_canonical: &str,
        resolved_canonical_id: &str,
        specifier: &str,
    ) -> Option<Arc<str>> {
        #[cfg(feature = "scheduler")]
        {
            // Try scheduler first (dep may be loaded)
            if let Some(snap) = self.scheduler.try_get_source(resolved_canonical_id) {
                return Some(snap.source.clone());
            }
            // Try VFS resolution fallback (handles aliases like @/... and bare modules)
            let dep_id = self.resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_vfs::ResolveRequestKind::EsmImport,
            );
            if let Some(ref id) = dep_id {
                // File resolved but not yet in scheduler — try loading from disk
                if self.scheduler.try_get_source(id).is_none() {
                    self.ensure_loaded(id);
                }
            }
            dep_id.and_then(|id| self.scheduler.try_get_source(&id).map(|s| s.source.clone()))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let dep_id = files
                .contains_key(resolved_canonical_id)
                .then(|| resolved_canonical_id.to_string())
                .or_else(|| {
                    self.resolve_loaded_dependency_canonical(
                        owner_canonical,
                        specifier,
                        verter_vfs::ResolveRequestKind::EsmImport,
                    )
                });
            dep_id.and_then(|id| files.get(&id).map(|e| e.source.clone()))
        }
    }

    /// Populate `resolved_canonical_id` on each import in the snapshot
    /// using the host's file map, alias map, and parent's dependency set.
    pub(crate) fn resolve_snapshot_imports(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                let ctx = verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::CodegenBlocker,
                    kind: if import.is_type_only {
                        verter_vfs::ResolveRequestKind::TypeImport
                    } else {
                        verter_vfs::ResolveRequestKind::EsmImport
                    },
                };
                import.resolved_canonical_id =
                    self.resolve_via_vfs(parent_canonical_id, &import.source, ctx);
            }
        }
    }

    /// Enrich destructured composable bindings with per-field reactivity info.
    ///
    /// When a binding has `reactivity_kind: MaybeRef` and its initializer is a
    /// `FunctionCall` to a composable, look up the composable's `return_shape`
    /// from the resolved file's `exported_functions`. If it's `Object(fields)`,
    /// match binding names to field names and replace `MaybeRef` with the
    /// field's actual `ReactivityKind`.
    pub(crate) fn enrich_destructured_bindings(&self, snapshot: &mut FileAnalysisSnapshot) {
        use verter_analysis::types::{BindingInitializer, ComposableReturn, ReactivityKind};

        // Build a map of import source → resolved canonical ID from the snapshot
        let import_resolved: rustc_hash::FxHashMap<&str, &str> = snapshot
            .imports
            .iter()
            .filter_map(|imp| {
                imp.resolved_canonical_id
                    .as_deref()
                    .map(|resolved| (imp.source.as_str(), resolved))
            })
            .collect();

        for binding in &mut snapshot.bindings {
            if binding.reactivity_kind != ReactivityKind::MaybeRef {
                continue;
            }

            let Some(BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            }) = &binding.initializer
            else {
                continue;
            };

            let import_source = match callee_import_source {
                Some(src) => src.as_str(),
                None => continue,
            };

            let canonical_id = match import_resolved.get(import_source) {
                Some(id) => *id,
                None => continue,
            };

            // Look up exported_functions from the dep's analysis
            #[cfg(feature = "scheduler")]
            let composable_info = self.scheduler.try_get_analysis(canonical_id).and_then(|a| {
                a.downcast_data::<crate::host_executor::HostAnalysisData>()
                    .and_then(|ad| {
                        ad.script_analysis
                            .exported_functions
                            .iter()
                            .find(|f| f.name == *callee)
                            .and_then(|f| f.composable.clone())
                    })
            });

            #[cfg(not(feature = "scheduler"))]
            let composable_info = {
                let files = read_lock(&self.files);
                files.get(canonical_id).and_then(|entry| {
                    entry
                        .script_analysis
                        .exported_functions
                        .iter()
                        .find(|f| f.name == *callee)
                        .and_then(|f| f.composable.clone())
                })
            };

            let Some(info) = composable_info else {
                continue;
            };

            match &info.return_shape {
                ComposableReturn::Object(fields) => {
                    if let Some(field) = fields.iter().find(|f| f.name == binding.name) {
                        binding.reactivity_kind = field.reactivity;
                        binding.is_reactive = !matches!(field.reactivity, ReactivityKind::None);
                    }
                }
                ComposableReturn::Single(kind) => {
                    binding.reactivity_kind = *kind;
                    binding.is_reactive = !matches!(kind, ReactivityKind::None);
                }
                _ => {}
            }
        }
    }

    /// Returns stored diagnostics for a file+profile without triggering compilation.
    /// Returns `None` if the file doesn't exist or has no diagnostics for this profile.
    pub fn get_diagnostics(
        &self,
        canonical_or_alias: &str,
        profile: &CompileProfile,
    ) -> Option<DiagnosticsSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let profile_hash = compile_profile_hash(profile);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            cc.latest_diagnostics.get(&profile_hash).cloned()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            entry.latest_diagnostics.get(&profile_hash).cloned()
        }
    }

    /// Returns the monotonic diagnostics generation counter for a file.
    /// Incremented on every write to `latest_diagnostics`. Used by the LSP
    /// cache to detect host-driven recompiles without a document version change.
    pub fn get_diagnostics_generation(&self, canonical_or_alias: &str) -> Option<u64> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            Some(cc.diagnostics_generation)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|e| e.diagnostics_generation)
        }
    }

    /// Bump the diagnostics generation counter for a file without changing
    /// its diagnostics.
    pub fn bump_diagnostics_generation(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.diagnostics_generation += 1;
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.diagnostics_generation += 1;
            }
        }
    }

    /// Clear all compile slots for a specific file.
    pub fn invalidate_compile_slots(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.compile_slots.clear();
            cc.cached_resolved_meta.clear();
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
                entry.cached_resolved_meta.clear();
            }
        }
    }

    /// Invalidate compile outputs of files that depend on the given path.
    ///
    /// Unlike `remove()`, this works even when the dependency file was never
    /// loaded into the host but reverse-dependency edges were registered.
    pub fn invalidate_dependents_of(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        self.smart_invalidate_dependents(&canonical, &[], &[]);
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            // Read aliases and dependencies from compile_cache before removing.
            let (aliases, deps) = {
                let cc = self.compile_cache.get(&canonical)?;
                (cc.aliases.clone(), cc.dependencies.clone())
            };

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &deps {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            // Invalidate compile_cache slots for dependents.
            for owner in &dependents {
                if let Some(mut cc) = self.compile_cache.get_mut(owner) {
                    cc.compile_slots.clear();
                    cc.cached_resolved_meta.clear();
                }
            }

            self.ws().notify_delete(&canonical);
            self.compile_cache.remove(&canonical);
            self.scheduler.remove(&canonical);

            self.bump_store_view_epoch();
            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let removed = {
                let mut files = write_lock(&self.files);
                files.remove(&canonical)
            }?;

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &removed.aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &removed.dependencies {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            if !dependents.is_empty() {
                let mut files = write_lock(&self.files);
                for owner in &dependents {
                    if let Some(file) = files.get_mut(owner) {
                        file.compile_slots.clear();
                        file.cached_resolved_meta.clear();
                    }
                }
            }

            self.ws().notify_delete(&canonical);

            self.bump_store_view_epoch();
            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }
    }

    /// Returns the list of virtual node kinds for a file.
    /// Returns an empty vec if the file doesn't exist.
    pub fn list_virtual_nodes(&self, canonical_or_alias: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return Vec::new();
                }
            }
            if let Some(snap) = self.scheduler.try_get_source(&canonical) {
                if let Some(hd) = snap.downcast_data::<HostSourceData>() {
                    return hd.parse.meta.virtual_nodes();
                }
            }
            Vec::new()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .get(&canonical)
                .map(|e| e.all_virtual_nodes())
                .unwrap_or_default()
        }
    }

    /// Provide caller-resolved import dependency resolution records.
    ///
    /// Called after `upsert()` when the caller resolves import specifiers
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    /// Each record maps a raw import specifier to its resolved canonical ID
    /// (or a list of candidate canonical IDs).
    ///
    /// Records are merged into the file's `dependency_resolutions` map (keyed by
    /// specifier). The flat `dependencies` set is updated in parallel for
    /// reverse-dependency tracking.
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: Vec<DependencyResolution>,
    ) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let parse_deps = self.parse_dependency_set_for_file(&canonical);

        // Build VFS exact resolutions for ALL relevant (phase, kind) contexts.
        let vfs_resolutions: Vec<verter_vfs::ExactResolution> = resolutions
            .iter()
            .flat_map(|r| {
                let resolved = r.resolved_canonical_id.as_ref().map(|id| {
                    let norm = canonicalize_id(id);
                    norm.into_owned()
                });
                let possible: Vec<String> = r
                    .possible_canonical_ids
                    .iter()
                    .map(|c| {
                        let norm = canonicalize_id(c);
                        norm.into_owned()
                    })
                    .collect();
                use verter_vfs::{ResolvePhase as P, ResolveRequestKind as K};
                [
                    (P::CodegenBlocker, K::EsmImport),
                    (P::CodegenBlocker, K::TypeImport),
                    (P::ProviderGraph, K::EsmImport),
                    (P::ProviderGraph, K::TypeImport),
                ]
                .into_iter()
                .map(move |(phase, kind)| verter_vfs::ExactResolution {
                    specifier: r.specifier.clone(),
                    phase,
                    kind,
                    resolved_canonical_id: resolved.clone(),
                    possible_canonical_ids: possible.clone(),
                })
            })
            .collect();

        // Normalize resolutions and persist direct import resolutions.
        let mut dep_resolutions = rustc_hash::FxHashMap::default();
        for mut res in resolutions {
            if let Some(ref mut id) = res.resolved_canonical_id {
                let norm = canonicalize_id(id);
                if norm != id.as_str() {
                    *id = norm.into_owned();
                }
            }
            for candidate in &mut res.possible_canonical_ids {
                let norm = canonicalize_id(candidate);
                if norm != candidate.as_str() {
                    *candidate = norm.into_owned();
                }
            }
            dep_resolutions.insert(res.specifier.clone(), res);
        }

        // Preserve already-discovered transitive macro-type deps; compilation
        // refreshes them, but direct import updates should not discard them.
        #[cfg(feature = "scheduler")]
        let old_transitive_deps = {
            let mut cc_ref = self.compile_cache.entry(canonical.clone()).or_default();
            let cc = cc_ref.value_mut();
            let old_deps = cc.dependencies.clone();
            let old_direct_deps = {
                let mut deps = parse_deps.clone();
                deps.extend(Self::resolved_dependency_targets(
                    &cc.dependency_resolutions,
                ));
                deps
            };
            cc.dependency_resolutions = dep_resolutions.clone();
            old_deps
                .difference(&old_direct_deps)
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        #[cfg(not(feature = "scheduler"))]
        let old_transitive_deps = {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                let old_deps = entry.dependencies.clone();
                let old_direct_deps = {
                    let mut deps = parse_deps.clone();
                    deps.extend(Self::resolved_dependency_targets(
                        &entry.dependency_resolutions,
                    ));
                    deps
                };
                entry.dependency_resolutions = dep_resolutions;
                old_deps
                    .difference(&old_direct_deps)
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            } else {
                std::collections::BTreeSet::new()
            }
        };

        self.sync_transitive_macro_type_dependencies(&canonical, &old_transitive_deps);

        // Sync exact resolutions to workspace.
        self.ws().set_exact_resolutions(&canonical, vfs_resolutions);
    }

    /// Returns all known canonical file IDs and their file kinds.
    pub fn list_files(&self) -> Vec<(String, FileKind)> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            self.scheduler
                .node_ids()
                .into_iter()
                .filter_map(|id| {
                    if let Some(cc) = self.compile_cache.get(&id) {
                        if cc.evicted {
                            return None;
                        }
                    }
                    let snap = self.scheduler.try_get_source(&id)?;
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    Some((id, hd.file_kind))
                })
                .collect()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .iter()
                .map(|(id, entry)| (id.clone(), entry.file_kind))
                .collect()
        }
    }

    pub(crate) fn raw_template_analysis_for_file(
        &self,
        canonical: &str,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let source_snap = self.scheduler.try_get_source(canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            drop(source_snap);
            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut snapshot = {
                let files = read_lock(&self.files);
                let entry = files.get(canonical)?;
                if entry.file_kind != FileKind::VueSfc {
                    return None;
                }
                Self::build_snapshot_from_entry(entry)
            };
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }
    }

    #[cfg(feature = "scheduler")]
    fn compute_override_template_analysis(
        &self,
        canonical: &str,
        profile_hash: u64,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        let override_with_parse = {
            let cc = self.compile_cache.get(canonical)?;
            cc.content_overrides.get(&profile_hash)?.clone()
        };

        self.build_template_analysis(
            canonical,
            &override_with_parse.source,
            override_with_parse.cached_parse.clone(),
            &override_with_parse.parse.src_blocks,
            &override_with_parse.parse.external_requests,
            &override_with_parse.parse.script_analysis.imports,
            &override_with_parse.parse.script_analysis.macros,
            &override_with_parse.parse.script_analysis.bindings,
        )
    }

    /// Returns cross-component CSS variable flow for a given variable name.
    ///
    /// Scans all files in the host to find where the variable is defined (in `<style>`),
    /// referenced via `var()` (in `<style>`), set via `:style` bindings (in `<template>`),
    /// and manipulated via DOM APIs (in `<script>`).
    ///
    /// When `profile` is provided, override-aware style/template/script state is
    /// used for that compile profile. `None` keeps the read profileless/raw.
    pub fn css_var_flow(
        &self,
        var_name: &str,
        profile: Option<&CompileProfile>,
    ) -> verter_analysis::CssVarFlow {
        #[cfg(feature = "scheduler")]
        let profile_hash = profile.map(compile_profile_hash);
        #[cfg(not(feature = "scheduler"))]
        let _ = profile;

        #[cfg(feature = "scheduler")]
        let canonical_ids: Vec<String> = self
            .scheduler
            .node_ids()
            .into_iter()
            .filter(|id| self.compile_cache.get(id).is_none_or(|cc| !cc.evicted))
            .collect();

        #[cfg(not(feature = "scheduler"))]
        let canonical_ids: Vec<String> = {
            let files = read_lock(&self.files);
            files.keys().cloned().collect()
        };

        let mut flow = verter_analysis::CssVarFlow {
            name: var_name.to_string(),
            ..Default::default()
        };

        for canonical_id in canonical_ids {
            let path: std::sync::Arc<std::path::Path> =
                std::sync::Arc::from(std::path::Path::new(canonical_id.as_str()));

            #[cfg(feature = "scheduler")]
            let style_analyses = self
                .effective_style_analyses(&canonical_id, profile_hash)
                .unwrap_or_default();
            #[cfg(not(feature = "scheduler"))]
            let style_analyses = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| entry.style_analyses.as_ref().clone())
                    .unwrap_or_default()
            };

            // Check style blocks for definitions and var() references
            for style in &style_analyses {
                if let Some(ref css) = style.css {
                    let has_def = css.custom_properties.iter().any(|p| p.name == var_name);
                    if has_def {
                        flow.style_definitions.push(std::sync::Arc::clone(&path));
                    }

                    let has_ref = css.var_usages.iter().any(|u| u.reference.name == var_name);
                    if has_ref {
                        flow.style_var_usages.push(std::sync::Arc::clone(&path));
                    }
                }
            }

            // Check template for :style CSS variable bindings
            #[cfg(feature = "scheduler")]
            let template_analysis = if let Some(profile_hash) = profile_hash {
                self.compile_cache
                    .get(&canonical_id)
                    .and_then(|cc| {
                        if cc.content_overrides.contains_key(&profile_hash) {
                            cc.compile_slots
                                .get(&profile_hash)
                                .and_then(|slot| slot.template_analysis.clone())
                                .map(Arc::new)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        self.compute_override_template_analysis(&canonical_id, profile_hash)
                    })
                    .or_else(|| self.raw_template_analysis_for_file(&canonical_id))
            } else {
                self.raw_template_analysis_for_file(&canonical_id)
            };
            #[cfg(not(feature = "scheduler"))]
            let template_analysis = self.raw_template_analysis_for_file(&canonical_id);

            if let Some(ref tmpl) = template_analysis {
                if tmpl.css_var_names.iter().any(|n| n == var_name) {
                    flow.template_definitions.push(std::sync::Arc::clone(&path));
                }
            }

            // Check script for DOM API CSS variable manipulations
            #[cfg(feature = "scheduler")]
            let script_has_manipulation = self
                .effective_file_state(&canonical_id, profile_hash)
                .map(|efs| {
                    efs.script_analysis
                        .css_var_manipulations
                        .iter()
                        .any(|m| m.var_name == var_name)
                })
                .unwrap_or(false);
            #[cfg(not(feature = "scheduler"))]
            let script_has_manipulation = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| {
                        entry
                            .script_analysis
                            .css_var_manipulations
                            .iter()
                            .any(|m| m.var_name == var_name)
                    })
                    .unwrap_or(false)
            };

            if script_has_manipulation {
                flow.script_manipulations.push(std::sync::Arc::clone(&path));
            }
        }

        flow
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) — spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `FileEntry.export_signatures` — spans are
    /// file-absolute.
    ///
    /// Returns `None` if the file doesn't exist or the name isn't exported.
    pub fn get_export_span(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let source_snap = self.scheduler.try_get_source(&canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(&canonical)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            Self::find_export_span(
                file_kind,
                &ad.script_analysis,
                &ad.export_signatures,
                binding_name,
            )
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            Self::find_export_span(
                entry.file_kind,
                &entry.script_analysis,
                &entry.export_signatures,
                binding_name,
            )
        }
    }

    /// Shared logic for finding an export span from analysis data.
    fn find_export_span(
        file_kind: FileKind,
        script_analysis: &verter_analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_analysis::ExportSignature],
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        if file_kind == FileKind::VueSfc {
            if let Some(binding) = script_analysis
                .bindings
                .iter()
                .find(|b| b.name == binding_name)
            {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return Some((binding.span.start, binding.span.end));
                }
            }
            for mac in &script_analysis.macros {
                if mac.binding_name.as_deref() == Some(binding_name)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return Some((mac.span.start, mac.span.end));
                }
            }
            if binding_name == "default" {
                if let Some(first_binding) = script_analysis.bindings.first() {
                    if first_binding.span.start > 0 || first_binding.span.end > 0 {
                        return Some((first_binding.span.start, first_binding.span.end));
                    }
                }
                if let Some(first_macro) = script_analysis.macros.first() {
                    if first_macro.span.start > 0 || first_macro.span.end > 0 {
                        return Some((first_macro.span.start, first_macro.span.end));
                    }
                }
                return Some((0, 0));
            }
            return None;
        }

        if let Some(sig) = export_signatures.iter().find(|s| s.name == binding_name) {
            if sig.span.start > 0 || sig.span.end > 0 {
                return Some((sig.span.start, sig.span.end));
            }
        }

        None
    }

    /// Follow re-exports to find the ultimate definition span.
    ///
    /// For a re-export like `export { default as Popup } from './Popup.vue'`,
    /// this follows the chain to find where `Popup` is actually defined.
    /// Returns `(canonical_id, start, end)` of the final definition.
    ///
    /// Uses cycle detection (visited set keyed on `(canonical_id, binding_name)`)
    /// instead of a depth counter. For local exports (no re-export), returns the
    /// span in the same file.
    pub fn get_export_span_follow_reexports(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(String, u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }
        let mut visited = rustc_hash::FxHashSet::default();
        self.follow_reexport_chain(&canonical, binding_name, &mut visited)
    }

    /// Internal recursive helper for following re-export chains.
    /// Uses a visited set keyed on `(canonical_id, binding_name)` to detect cycles.
    fn follow_reexport_chain(
        &self,
        canonical_id: &str,
        binding_name: &str,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<(String, u32, u32)> {
        if !visited.insert((canonical_id.to_string(), binding_name.to_string())) {
            return None;
        }

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            let source_snap = self.scheduler.try_get_source(canonical_id)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(canonical_id)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            if file_kind == crate::FileKind::VueSfc {
                return Self::find_export_span(
                    file_kind,
                    &ad.script_analysis,
                    &ad.export_signatures,
                    binding_name,
                )
                .map(|(start, end)| (canonical_id.to_string(), start, end));
            }

            if let Some(sig) = ad.export_signatures.iter().find(|s| s.name == binding_name) {
                if let (Some(ref source), Some(ref local_name)) =
                    (&sig.reexport_source, &sig.reexport_local)
                {
                    let resolved_target = resolve_reexport_target(self, canonical_id, source, sig);
                    if let Some(target_canonical) = resolved_target {
                        return self.follow_reexport_chain(&target_canonical, local_name, visited);
                    }
                    return None;
                }

                if sig.span.start > 0 || sig.span.end > 0 {
                    return Some((canonical_id.to_string(), sig.span.start, sig.span.end));
                }
            }

            None
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let (file_kind, export_signatures) = {
                let files = read_lock(&self.files);
                let entry = files.get(canonical_id)?;
                (entry.file_kind, entry.export_signatures.clone())
            };

            if file_kind == crate::FileKind::VueSfc {
                let files = read_lock(&self.files);
                let entry = files.get(canonical_id)?;
                return Self::find_export_span(
                    entry.file_kind,
                    &entry.script_analysis,
                    &entry.export_signatures,
                    binding_name,
                )
                .map(|(start, end)| (canonical_id.to_string(), start, end));
            }

            if let Some(sig) = export_signatures.iter().find(|s| s.name == binding_name) {
                if let (Some(ref source), Some(ref local_name)) =
                    (&sig.reexport_source, &sig.reexport_local)
                {
                    let resolved_target = resolve_reexport_target(self, canonical_id, source, sig);
                    if let Some(target_canonical) = resolved_target {
                        return self.follow_reexport_chain(&target_canonical, local_name, visited);
                    }
                    return None;
                }

                if sig.span.start > 0 || sig.span.end > 0 {
                    return Some((canonical_id.to_string(), sig.span.start, sig.span.end));
                }
            }

            None
        }
    }

    /// Resolve an import specifier to its canonical ID using the host's file map,
    /// alias map, and parent's resolved dependencies.
    ///
    /// Returns `None` if the import cannot be resolved to a known file
    /// (e.g., bare specifiers like `vue` or unregistered files).
    pub fn resolve_import(&self, parent_canonical_id: &str, import_source: &str) -> Option<String> {
        let canonical_parent = self.resolve_alias_or_canonical(parent_canonical_id);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical_parent) {
            if cc.evicted {
                return None;
            }
        }
        let ctx = verter_vfs::ResolutionContext {
            phase: verter_vfs::ResolvePhase::CodegenBlocker,
            kind: verter_vfs::ResolveRequestKind::EsmImport,
        };
        self.resolve_via_vfs(&canonical_parent, import_source, ctx)
    }

    /// Returns all exports of a file, following re-export chains to their ultimate source.
    ///
    /// For barrel files like `export { default as Button } from './Button.vue'`, this
    /// resolves through the chain to return the ultimate source file and name. For
    /// `export * from './module'`, it recursively resolves the target file's exports.
    ///
    /// Uses cycle detection to prevent infinite loops in circular re-exports.
    pub fn resolve_exports(&self, canonical_or_alias: &str) -> Vec<ResolvedExport> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return Vec::new();
            }
        }
        let mut visiting = rustc_hash::FxHashSet::default();
        self.collect_resolved_exports(&canonical, &mut visiting)
    }

    /// Recursively collect resolved exports from a file, following re-export chains.
    fn collect_resolved_exports(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Vec<ResolvedExport> {
        if !visiting.insert(canonical_id.to_string()) {
            return Vec::new();
        }

        let Some((file_kind, export_signatures)) =
            self.export_surface_for_reexport_resolution(canonical_id)
        else {
            visiting.remove(canonical_id);
            return Vec::new();
        };

        let mut results = Vec::new();

        let has_default_signature = export_signatures.iter().any(|sig| sig.name == "default");
        if file_kind == crate::FileKind::VueSfc && !has_default_signature {
            results.push(ResolvedExport {
                name: "default".to_string(),
                is_type: false,
                source_canonical_id: None,
                source_name: "default".to_string(),
            });
        }

        for sig in &export_signatures {
            if sig.name == "*" {
                if let Some(ref source) = sig.reexport_source {
                    let resolved_target = resolve_reexport_target(self, canonical_id, source, sig);
                    if let Some(target) = resolved_target {
                        let nested = self.collect_resolved_exports(&target, visiting);
                        for mut export in nested {
                            if export.source_canonical_id.is_none() {
                                export.source_canonical_id = Some(target.clone());
                            }
                            results.push(export);
                        }
                    }
                }
                continue;
            }

            if let (Some(ref source), Some(ref local_name)) =
                (&sig.reexport_source, &sig.reexport_local)
            {
                let resolved_target = resolve_reexport_target(self, canonical_id, source, sig);
                if let Some(target) = resolved_target {
                    let resolved = self.resolve_single_export(&target, local_name, visiting);
                    let (src_id, src_name) = match resolved {
                        Some((cid, n)) => (Some(cid), n),
                        None => (Some(target.clone()), local_name.clone()),
                    };
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: src_id,
                        source_name: src_name,
                    });
                } else {
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: None,
                        source_name: local_name.clone(),
                    });
                }
            } else {
                results.push(ResolvedExport {
                    name: sig.name.clone(),
                    is_type: sig.is_type,
                    source_canonical_id: None,
                    source_name: sig.name.clone(),
                });
            }
        }

        visiting.remove(canonical_id);
        results
    }

    /// Follow a re-export chain for a single named export.
    /// Returns (ultimate_canonical_id, ultimate_name) or None if unresolvable.
    fn resolve_single_export(
        &self,
        canonical_id: &str,
        name: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<(String, String)> {
        let (file_kind, export_signatures) =
            self.export_surface_for_reexport_resolution(canonical_id)?;

        if file_kind == crate::FileKind::VueSfc {
            if name == "default" {
                return Some((canonical_id.to_string(), name.to_string()));
            }
            if export_signatures.iter().any(|sig| sig.name == name) {
                return Some((canonical_id.to_string(), name.to_string()));
            }
            return None;
        }

        let sig = export_signatures.iter().find(|s| s.name == name)?;

        if let (Some(ref source), Some(ref local)) = (&sig.reexport_source, &sig.reexport_local) {
            if visiting.contains(canonical_id) {
                return Some((canonical_id.to_string(), name.to_string()));
            }
            visiting.insert(canonical_id.to_string());
            let target = resolve_reexport_target(self, canonical_id, source, sig);
            visiting.remove(canonical_id);

            if let Some(target_id) = target {
                self.resolve_single_export(&target_id, local, visiting)
                    .or(Some((target_id, local.clone())))
            } else {
                Some((canonical_id.to_string(), name.to_string()))
            }
        } else {
            Some((canonical_id.to_string(), name.to_string()))
        }
    }

    fn export_surface_for_reexport_resolution(
        &self,
        canonical_id: &str,
    ) -> Option<(crate::FileKind, Vec<verter_analysis::ExportSignature>)> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            if let Some(source_snap) = self.scheduler.try_get_source(canonical_id) {
                let hd = source_snap.downcast_data::<HostSourceData>()?;
                let file_kind = hd.file_kind;
                drop(source_snap);

                if let Some(sigs) = self.scheduler.try_get_analysis(canonical_id).and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| ad.export_signatures.clone())
                }) {
                    return Some((file_kind, sigs));
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical_id) {
                return Some((entry.file_kind, entry.export_signatures.clone()));
            }
        }

        let source = self
            .get_source(canonical_id)
            .or_else(|| self.ws().read_file(canonical_id))?;
        let file_kind = if canonical_id.ends_with(".vue") {
            crate::FileKind::VueSfc
        } else {
            crate::FileKind::NonSfc
        };
        let export_signatures = match file_kind {
            crate::FileKind::VueSfc => {
                crate::parse::parse_vue_snapshot(
                    canonical_id,
                    &source,
                    verter_analysis::AnalysisScope::NONE,
                )
                .0
                .export_signatures
            }
            crate::FileKind::NonSfc => {
                crate::parse::parse_non_sfc_snapshot(canonical_id, &source).export_signatures
            }
        };

        Some((file_kind, export_signatures))
    }
}

/// Check whether a type body has a top-level Intersection containing Ref nodes,
/// indicating `interface Foo extends ImportedType, Omit<Bar, ...> { ... }`.
/// When true, the flattened OXC resolved body may be incomplete (missing inherited
/// members), so the EvalEnv should load full dependency sources instead.
fn body_has_structural_intersection_refs(body: &verter_analysis::type_expr::TypeExpr) -> bool {
    use verter_analysis::type_expr::TypeExpr;

    match body {
        TypeExpr::Intersection(parts) => parts
            .iter()
            .any(|part| matches!(part, TypeExpr::Ref { .. })),
        _ => false,
    }
}

fn choose_preferred_imported_type_body(
    resolved_body: Option<verter_analysis::type_expr::TypeExpr>,
    resolved_decl_body: Option<verter_analysis::type_expr::TypeExpr>,
) -> Option<verter_analysis::type_expr::TypeExpr> {
    match (resolved_body, resolved_decl_body) {
        (Some(left), Some(right)) => {
            let left_empty_object = is_empty_object_surface(&left);
            let right_empty_object = is_empty_object_surface(&right);
            if left_empty_object != right_empty_object {
                return Some(if left_empty_object { right } else { left });
            }

            let left_surface_props = extracted_surface_property_count(&left);
            let right_surface_props = extracted_surface_property_count(&right);
            if let (Some(left_count), Some(right_count)) = (left_surface_props, right_surface_props)
            {
                if left_count != right_count {
                    return Some(if left_count > right_count {
                        left
                    } else {
                        right
                    });
                }
            }

            let left_nested = contains_nested_resolution_targets(&left);
            let right_nested = contains_nested_resolution_targets(&right);
            if left_nested != right_nested {
                return Some(if left_nested { right } else { left });
            }

            let left_non_object = has_non_object_top_level_surface(&left);
            let right_non_object = has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            if imported_type_body_specificity_score(&right)
                > imported_type_body_specificity_score(&left)
            {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(body), None) | (None, Some(body)) => Some(body),
        (None, None) => None,
    }
}

fn should_attempt_owner_env_resolution(
    decl: &verter_analysis::type_eval::TypeDeclInfo,
    resolved_body: Option<&verter_analysis::type_expr::TypeExpr>,
) -> bool {
    let Some(resolved_body) = resolved_body else {
        return true;
    };

    if is_empty_object_surface(resolved_body) && !is_empty_object_surface(&decl.body) {
        return true;
    }

    if has_non_object_top_level_surface(resolved_body) {
        return true;
    }

    if contains_nested_resolution_targets(resolved_body) {
        return true;
    }

    if contains_nested_resolution_targets(&decl.body) {
        return true;
    }

    if !has_non_object_top_level_surface(&decl.body) {
        return false;
    }

    count_top_level_properties(resolved_body) <= count_top_level_properties(&decl.body)
}

fn has_non_object_top_level_surface(expr: &verter_analysis::type_expr::TypeExpr) -> bool {
    use verter_analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => has_non_object_top_level_surface(inner),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            types.iter().any(has_non_object_top_level_surface)
                || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
        }
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Object(_) => false,
        _ => false,
    }
}

fn is_empty_object_surface(expr: &verter_analysis::type_expr::TypeExpr) -> bool {
    use verter_analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => is_empty_object_surface(inner),
        TypeExpr::Object(obj) => obj.properties.is_empty(),
        _ => false,
    }
}

fn contains_nested_resolution_targets(expr: &verter_analysis::type_expr::TypeExpr) -> bool {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => false,
        TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => contains_nested_resolution_targets(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| contains_nested_resolution_targets(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => contains_nested_resolution_targets(&prop.ty),
            ObjectMember::Method(method) => {
                contains_nested_resolution_targets_in_function(&method.function)
            }
            ObjectMember::IndexSignature(sig) => {
                contains_nested_resolution_targets(&sig.key_type)
                    || contains_nested_resolution_targets(&sig.value_type)
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                contains_nested_resolution_targets_in_function(func)
            }
        }),
        TypeExpr::Function(func) => contains_nested_resolution_targets_in_function(func),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Infer { .. } => false,
    }
}

fn contains_nested_resolution_targets_in_function(
    func: &verter_analysis::type_expr::FunctionExpr,
) -> bool {
    func.parameters
        .iter()
        .any(|param| contains_nested_resolution_targets(&param.ty))
        || func
            .return_type
            .as_deref()
            .is_some_and(contains_nested_resolution_targets)
        || func.type_parameters.iter().any(|param| {
            param
                .constraint
                .as_deref()
                .is_some_and(contains_nested_resolution_targets)
                || param
                    .default
                    .as_deref()
                    .is_some_and(contains_nested_resolution_targets)
        })
}

fn count_top_level_properties(expr: &verter_analysis::type_expr::TypeExpr) -> usize {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Parenthesized(inner) => count_top_level_properties(inner),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            types.iter().map(count_top_level_properties).sum()
        }
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter(|member| matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_)))
            .count(),
        _ => 0,
    }
}

fn extracted_surface_property_count(expr: &verter_analysis::type_expr::TypeExpr) -> Option<usize> {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Parenthesized(inner) => extracted_surface_property_count(inner),
        TypeExpr::Object(obj) => Some(
            obj.properties
                .iter()
                .filter(|member| {
                    matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_))
                })
                .count(),
        ),
        TypeExpr::Intersection(types) => {
            let mut total = 0usize;
            let mut saw_surface = false;
            for ty in types {
                let count = extracted_surface_property_count(ty)?;
                total += count;
                saw_surface = true;
            }
            saw_surface.then_some(total)
        }
        _ => None,
    }
}

const SPECIFICITY_UNKNOWN: usize = 0;
const SPECIFICITY_TYPEOF: usize = 4;
const SPECIFICITY_TERMINAL: usize = 8;
const SPECIFICITY_REF_BASE: usize = 16;
const SPECIFICITY_TEMPLATE_LITERAL_BASE: usize = 20;
const SPECIFICITY_WRAPPER_BASE: usize = 24;
const SPECIFICITY_INDEXED_ACCESS_BASE: usize = 28;
const SPECIFICITY_MAPPED_BASE: usize = 32;
const SPECIFICITY_TUPLE_BASE: usize = 40;
const SPECIFICITY_FUNCTION_BASE: usize = 48;
const SPECIFICITY_UNION_BASE: usize = 56;
const SPECIFICITY_INTERSECTION_BASE: usize = 64;
const SPECIFICITY_OBJECT_BASE: usize = 96;
const SPECIFICITY_OBJECT_PROPERTY: usize = 12;
const SPECIFICITY_INDEX_SIGNATURE: usize = 6;
const SPECIFICITY_CALL_LIKE_MEMBER: usize = 10;

/// Prefer bodies that expose more immediately usable structure for owner-env alias
/// injection.
///
/// Ordering invariant:
/// - concrete object surfaces outrank every other form
/// - intersections/unions outrank functions and wrappers because they still
///   expose aggregate structure
/// - functions/wrappers outrank opaque refs
/// - opaque refs outrank `typeof`
/// - unknown remains at the bottom
fn imported_type_body_specificity_score(expr: &verter_analysis::type_expr::TypeExpr) -> usize {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Unknown { .. } => SPECIFICITY_UNKNOWN,
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => SPECIFICITY_TERMINAL,
        TypeExpr::TypeOf(_) => SPECIFICITY_TYPEOF,
        TypeExpr::Ref { type_arguments, .. } => {
            SPECIFICITY_REF_BASE
                + type_arguments
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => {
            SPECIFICITY_WRAPPER_BASE + imported_type_body_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => {
            SPECIFICITY_TUPLE_BASE
                + elements
                    .iter()
                    .map(|element| imported_type_body_specificity_score(&element.ty))
                    .sum::<usize>()
        }
        TypeExpr::Union(types) => {
            SPECIFICITY_UNION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Intersection(types) => {
            SPECIFICITY_INTERSECTION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Object(obj) => {
            SPECIFICITY_OBJECT_BASE
                + obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(prop) => {
                            SPECIFICITY_OBJECT_PROPERTY
                                + imported_type_body_specificity_score(&prop.ty)
                        }
                        ObjectMember::IndexSignature(sig) => {
                            SPECIFICITY_INDEX_SIGNATURE
                                + imported_type_body_specificity_score(&sig.key_type)
                                + imported_type_body_specificity_score(&sig.value_type)
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            SPECIFICITY_CALL_LIKE_MEMBER + imported_function_specificity_score(func)
                        }
                        ObjectMember::Method(method) => {
                            SPECIFICITY_CALL_LIKE_MEMBER
                                + imported_function_specificity_score(&method.function)
                        }
                    })
                    .sum::<usize>()
        }
        TypeExpr::Function(func) => {
            SPECIFICITY_FUNCTION_BASE + imported_function_specificity_score(func)
        }
        TypeExpr::IndexedAccess { object, index } => {
            SPECIFICITY_INDEXED_ACCESS_BASE
                + imported_type_body_specificity_score(object)
                + imported_type_body_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            SPECIFICITY_WRAPPER_BASE
                + imported_type_body_specificity_score(check)
                + imported_type_body_specificity_score(extends)
                + imported_type_body_specificity_score(true_type)
                + imported_type_body_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            SPECIFICITY_MAPPED_BASE
                + imported_type_body_specificity_score(source)
                + imported_type_body_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            SPECIFICITY_TEMPLATE_LITERAL_BASE
                + expressions
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Infer { .. } => SPECIFICITY_TYPEOF,
    }
}

fn imported_function_specificity_score(func: &verter_analysis::type_expr::FunctionExpr) -> usize {
    let params = func
        .parameters
        .iter()
        .map(|param| imported_type_body_specificity_score(&param.ty))
        .sum::<usize>();
    let ret = func
        .return_type
        .as_deref()
        .map(imported_type_body_specificity_score)
        .unwrap_or_default();
    let generics = func
        .type_parameters
        .iter()
        .map(|param| {
            param
                .constraint
                .as_deref()
                .map(imported_type_body_specificity_score)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        })
        .sum::<usize>();
    params + ret + generics
}

fn collect_required_owner_import_names(
    snapshot: &FileAnalysisSnapshot,
    owner_eval_source: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let started = component_meta_debug_enabled().then(Instant::now);
    let mut required = rustc_hash::FxHashSet::default();
    if owner_eval_source.is_empty() {
        return required;
    }

    if component_meta_debug_enabled() {
        component_meta_debug(format!(
            "collect_required_imports:start macros={} bindings={} source_len={} type_symbols={} value_symbols={}",
            snapshot.macros.len(),
            snapshot.bindings.len(),
            owner_eval_source.len(),
            owner_env.type_symbols.len(),
            owner_env.value_symbols.len(),
        ));
    }
    let type_bindings = rustc_hash::FxHashMap::default();
    let mut active_locals = rustc_hash::FxHashSet::default();
    let macro_type_params =
        verter_analysis::type_eval_build::collect_define_macro_type_params(owner_eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;
    let imported_binding_names: rustc_hash::FxHashSet<&str> = snapshot
        .imports
        .iter()
        .flat_map(|import| import.bindings.iter().map(|binding| binding.name.as_str()))
        .collect();
    let binding_type_annotations: rustc_hash::FxHashMap<&str, &str> = snapshot
        .bindings
        .iter()
        .filter_map(|binding| {
            binding
                .type_annotation
                .as_deref()
                .map(|type_ann| (binding.name.as_str(), type_ann))
        })
        .collect();

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        // Prefer the owner-local surface walk. It can follow local aliases and
        // lazy indexed access without dragging in every imported generic arg
        // behind the macro root. Only fall back to shared macro deps when the
        // local macro analyzer captured no root type references.
        if mac.is_type_based {
            let is_define_slots = mac.kind == verter_analysis::AnalyzedMacroKind::DefineSlots;
            let macro_type_expr = match mac.kind {
                verter_analysis::AnalyzedMacroKind::DefineProps => {
                    let expr = macro_type_params.define_props.get(define_props_index);
                    define_props_index += 1;
                    expr
                }
                verter_analysis::AnalyzedMacroKind::DefineEmits => {
                    let expr = macro_type_params.define_emits.get(define_emits_index);
                    define_emits_index += 1;
                    expr
                }
                verter_analysis::AnalyzedMacroKind::DefineSlots => {
                    let expr = macro_type_params.define_slots.get(define_slots_index);
                    define_slots_index += 1;
                    expr
                }
                _ => None,
            };
            if let Some(expr) = macro_type_expr {
                if !expr.is_unknown() {
                    if is_define_slots {
                        collect_slot_eval_import_names_from_expr(
                            expr,
                            owner_env,
                            &type_bindings,
                            &mut active_locals,
                            &mut required,
                        );
                    } else {
                        collect_surface_eval_import_names_from_expr(
                            expr,
                            owner_env,
                            &type_bindings,
                            &mut active_locals,
                            &mut required,
                        );
                    }
                }
            }
            for type_reference in &mac.type_references {
                if is_define_slots {
                    collect_required_slot_import_names_for_symbol(
                        type_reference,
                        owner_env,
                        &type_bindings,
                        &imported_binding_names,
                        &mut active_locals,
                        &mut required,
                    );
                } else {
                    collect_required_import_names_for_symbol(
                        type_reference,
                        owner_env,
                        &type_bindings,
                        &imported_binding_names,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
            if mac.type_references.is_empty() {
                for dep in snapshot
                    .macro_type_deps
                    .iter()
                    .filter(|dep| dep.macro_index == macro_index)
                {
                    if imported_binding_names.contains(dep.type_name.as_str()) {
                        required.insert(dep.type_name.clone());
                    }
                }
            }
        }

        for field in &mac.prop_fields {
            if let Some(type_ann) = field.type_annotation.as_deref() {
                let expr = verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
                if !expr.is_unknown() {
                    collect_surface_eval_import_names_from_expr(
                        &expr,
                        owner_env,
                        &type_bindings,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
        }

        for field in &mac.emit_fields {
            if let Some(payload) = field.payload_type.as_deref() {
                let expr = verter_analysis::type_expr_lower::parse_type_annotation(payload);
                if !expr.is_unknown() {
                    collect_surface_eval_import_names_from_expr(
                        &expr,
                        owner_env,
                        &type_bindings,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
        }

        if mac.kind != verter_analysis::AnalyzedMacroKind::DefineSlots {
            for slot in &mac.slot_fields {
                for binding in &slot.bindings {
                    if let Some(type_ann) = binding.type_annotation.as_deref() {
                        let expr =
                            verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
                        if !expr.is_unknown() {
                            collect_surface_eval_import_names_from_expr(
                                &expr,
                                owner_env,
                                &type_bindings,
                                &mut active_locals,
                                &mut required,
                            );
                        }
                    }
                }
            }
        }

        for field in &mac.expose_fields {
            let Some(type_ann) = binding_type_annotations.get(field.name.as_str()) else {
                continue;
            };
            let expr = verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
            if expr.is_unknown() {
                continue;
            }
            collect_surface_eval_import_names_from_expr(
                &expr,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
    }

    if component_meta_debug_enabled() {
        component_meta_debug(format!(
            "collect_required_imports:end required_count={} required=[{}] total_took={:?}",
            required.len(),
            required.iter().cloned().collect::<Vec<_>>().join(", "),
            started.map(|start| start.elapsed()).unwrap_or_default(),
        ));
    }
    required
}

fn collect_required_import_names_for_type_decl(
    decl: &verter_analysis::type_eval::TypeDeclInfo,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let mut required = rustc_hash::FxHashSet::default();
    let mut active_locals = rustc_hash::FxHashSet::default();
    let mut type_bindings = rustc_hash::FxHashMap::default();

    for param in &decl.type_parameters {
        type_bindings.insert(
            param.name.clone(),
            verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
        );
        if let Some(constraint) = param.constraint.as_deref() {
            collect_surface_eval_import_names_from_expr(
                constraint,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
        if let Some(default) = param.default.as_deref() {
            collect_surface_eval_import_names_from_expr(
                default,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
    }

    collect_surface_eval_import_names_from_expr(
        &decl.body,
        owner_env,
        &type_bindings,
        &mut active_locals,
        &mut required,
    );
    required
}

fn collect_required_import_names_for_symbol(
    symbol: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    imported_binding_names: &rustc_hash::FxHashSet<&str>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    if owner_env.type_symbols.contains_key(symbol) || type_bindings.contains_key(symbol) {
        collect_surface_eval_import_names_from_expr(
            &verter_analysis::type_expr::TypeExpr::named(symbol),
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
        return;
    }

    if let Some((root, _)) = symbol.split_once('.') {
        if imported_binding_names.contains(root) {
            required.insert(symbol.to_string());
            return;
        }
    }

    if imported_binding_names.contains(symbol) {
        required.insert(symbol.to_string());
    }
}

fn collect_required_slot_import_names_for_symbol(
    symbol: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    imported_binding_names: &rustc_hash::FxHashSet<&str>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    if owner_env.type_symbols.contains_key(symbol) || type_bindings.contains_key(symbol) {
        collect_slot_eval_import_names_from_expr(
            &verter_analysis::type_expr::TypeExpr::named(symbol),
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
        return;
    }

    if let Some((root, _)) = symbol.split_once('.') {
        if imported_binding_names.contains(root) {
            required.insert(symbol.to_string());
            return;
        }
    }

    if imported_binding_names.contains(symbol) {
        required.insert(symbol.to_string());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotImportWalkMode {
    Surface,
    KeySpace,
    Structural,
}

fn slot_import_guard(prefix: &str, mode: SlotImportWalkMode, name: &str) -> String {
    let mode = match mode {
        SlotImportWalkMode::Surface => "surface",
        SlotImportWalkMode::KeySpace => "key",
        SlotImportWalkMode::Structural => "struct",
    };
    format!("$slot-{prefix}-{mode}:{name}")
}

fn slot_member_walk_mode(mode: SlotImportWalkMode) -> SlotImportWalkMode {
    match mode {
        SlotImportWalkMode::Structural => SlotImportWalkMode::Structural,
        SlotImportWalkMode::Surface | SlotImportWalkMode::KeySpace => SlotImportWalkMode::KeySpace,
    }
}

fn collect_slot_eval_import_names_from_expr(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    collect_slot_eval_import_names_from_expr_with_mode(
        expr,
        owner_env,
        type_bindings,
        active_locals,
        required,
        SlotImportWalkMode::Surface,
    );
}

fn collect_slot_eval_import_names_from_expr_with_mode(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_) => {}
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                collect_slot_eval_import_names_from_expr_with_mode(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_slot_eval_import_names_from_expr_with_mode(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
        TypeExpr::KeyOf(element) => collect_slot_eval_import_names_from_expr_with_mode(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
            SlotImportWalkMode::KeySpace,
        ),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements {
                collect_slot_eval_import_names_from_expr_with_mode(
                    &element.ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match (mode, member) {
                    (
                        SlotImportWalkMode::Surface | SlotImportWalkMode::KeySpace,
                        ObjectMember::IndexSignature(idx),
                    ) => collect_slot_eval_import_names_from_expr_with_mode(
                        &idx.key_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::KeySpace,
                    ),
                    (SlotImportWalkMode::Structural, ObjectMember::Property(prop)) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &prop.ty,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                    }
                    (SlotImportWalkMode::Structural, ObjectMember::IndexSignature(idx)) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &idx.key_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &idx.value_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                    }
                    (
                        SlotImportWalkMode::Structural,
                        ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func),
                    ) => collect_slot_eval_import_names_from_function_structural(
                        func,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    (SlotImportWalkMode::Structural, ObjectMember::Method(method)) => {
                        collect_slot_eval_import_names_from_function_structural(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Function(func) => {
            if mode == SlotImportWalkMode::Structural {
                collect_slot_eval_import_names_from_function_structural(
                    func,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = slot_import_guard("type", mode, name);
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_slot_eval_import_names_from_expr_with_mode(
                    bound,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_slot_eval_import_names_from_expr_with_mode(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&decl_guard);
                return;
            }

            required.insert(name.clone());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        arg,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        mode,
                    );
                }
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            let member_mode = slot_member_walk_mode(mode);
            if let TypeExpr::Literal(LiteralValue::String(key)) = index.as_ref() {
                collect_slot_eval_import_names_for_member(
                    object,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_slot_eval_import_names_from_expr_with_mode(
                check,
                owner_env,
                type_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                extends,
                owner_env,
                type_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                true_type,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                false_type,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            if mode == SlotImportWalkMode::Structural {
                collect_slot_eval_import_names_from_expr_with_mode(
                    source,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::Structural,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    value,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::Structural,
                );
                if let Some(name_type) = name_type.as_deref() {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        name_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::Structural,
                    );
                }
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    source,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::KeySpace,
                );
                if let Some(name_type) = name_type.as_deref() {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        name_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::KeySpace,
                    );
                }
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            let nested_mode = if mode == SlotImportWalkMode::Structural {
                SlotImportWalkMode::Structural
            } else {
                SlotImportWalkMode::KeySpace
            };
            for expr in expressions {
                collect_slot_eval_import_names_from_expr_with_mode(
                    expr,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    nested_mode,
                );
            }
        }
    }
}

fn collect_slot_eval_import_names_from_function_structural(
    func: &verter_analysis::type_expr::FunctionExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    let mut local_bindings = type_bindings.clone();
    for param in &func.type_parameters {
        local_bindings.insert(
            param.name.clone(),
            verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
        );
        if let Some(constraint) = param.constraint.as_deref() {
            collect_slot_eval_import_names_from_expr_with_mode(
                constraint,
                owner_env,
                &local_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
        }
        if let Some(default) = param.default.as_deref() {
            collect_slot_eval_import_names_from_expr_with_mode(
                default,
                owner_env,
                &local_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
        }
    }

    for param in &func.parameters {
        collect_slot_eval_import_names_from_expr_with_mode(
            &param.ty,
            owner_env,
            &local_bindings,
            active_locals,
            required,
            SlotImportWalkMode::Structural,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_slot_eval_import_names_from_expr_with_mode(
            return_type,
            owner_env,
            &local_bindings,
            active_locals,
            required,
            SlotImportWalkMode::Structural,
        );
    }
}

fn collect_slot_eval_import_names_for_member(
    object: &verter_analysis::type_expr::TypeExpr,
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match object {
        TypeExpr::Object(obj) => {
            if let Some(member) = obj.properties.iter().find(|member| match member {
                ObjectMember::Property(prop) => prop.name == key,
                ObjectMember::Method(method) => method.name == key,
                _ => false,
            }) {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &prop.ty,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            mode,
                        );
                    }
                    ObjectMember::Method(method) if mode == SlotImportWalkMode::Structural => {
                        collect_slot_eval_import_names_from_function_structural(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = slot_import_guard("type", mode, name);
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_slot_eval_import_names_for_member(
                    bound,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_slot_eval_import_names_for_member(
                    &decl.body,
                    key,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&decl_guard);
                return;
            }

            required.insert(name.clone());
            collect_slot_eval_import_names_for_builtin_member(
                name,
                type_arguments,
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        TypeExpr::Parenthesized(inner) => collect_slot_eval_import_names_for_member(
            inner,
            key,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for ty in types {
                collect_slot_eval_import_names_for_member(
                    ty,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(LiteralValue::String(inner_key)) = index.as_ref() {
                collect_slot_eval_import_names_for_member(
                    object,
                    inner_key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        _ => collect_slot_eval_import_names_from_expr_with_mode(
            object,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
    }
}

fn collect_slot_eval_import_names_for_builtin_member(
    name: &str,
    type_arguments: &[verter_analysis::type_expr::TypeExpr],
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    match name {
        "Partial" | "Required" | "Readonly" if type_arguments.len() == 1 => {
            collect_slot_eval_import_names_for_member(
                &type_arguments[0],
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if keys.contains(key) {
                collect_slot_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if !keys.contains(key) {
                collect_slot_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        _ => {}
    }
}

fn collect_requested_binding_names(
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    snapshot
        .macros
        .iter()
        .flat_map(|mac| mac.expose_fields.iter().map(|field| field.name.clone()))
        .collect()
}

fn required_type_alias_names_for_import_binding(
    local_binding_name: &str,
    is_namespace: bool,
    required_import_names: &rustc_hash::FxHashSet<String>,
) -> Vec<String> {
    if is_namespace {
        let prefix = format!("{local_binding_name}.");
        return required_import_names
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect();
    }

    required_import_names
        .contains(local_binding_name)
        .then(|| vec![local_binding_name.to_string()])
        .unwrap_or_default()
}

fn imported_member_name_for_type_alias(
    local_binding_name: &str,
    imported_name: Option<&str>,
    is_namespace: bool,
    required_alias_name: &str,
) -> Option<String> {
    if is_namespace {
        let prefix = format!("{local_binding_name}.");
        return required_alias_name
            .strip_prefix(&prefix)
            .map(str::to_string)
            .filter(|name| !name.is_empty());
    }

    Some(imported_name.unwrap_or(local_binding_name).to_string())
}

fn collect_surface_eval_import_names_from_expr(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => {}
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                collect_surface_eval_import_names_from_expr(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_surface_eval_import_names_from_expr(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements {
                collect_surface_eval_import_names_from_expr(
                    &element.ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => collect_surface_eval_import_names_from_expr(
                        &prop.ty,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    ObjectMember::IndexSignature(idx) => {
                        collect_surface_eval_import_names_from_expr(
                            &idx.key_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                        collect_surface_eval_import_names_from_expr(
                            &idx.value_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_surface_eval_import_names_from_function(
                            func,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_surface_eval_import_names_from_function(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                }
            }
        }
        TypeExpr::Function(func) => collect_surface_eval_import_names_from_function(
            func,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = format!("$type:{name}");
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_surface_eval_import_names_from_expr(
                    bound,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                if !active_locals.insert(name.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_surface_eval_import_names_from_expr(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(name);
                return;
            }

            required.insert(name.clone());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments {
                    collect_surface_eval_import_names_from_expr(
                        arg,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    );
                }
            }
        }
        TypeExpr::TypeOf(_) => {}
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(verter_analysis::type_expr::LiteralValue::String(key)) =
                index.as_ref()
            {
                collect_surface_eval_import_names_for_member(
                    object,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            } else {
                collect_surface_eval_import_names_from_expr(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                collect_surface_eval_import_names_from_expr(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            for ty in [check, extends, true_type, false_type] {
                collect_surface_eval_import_names_from_expr(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_surface_eval_import_names_from_expr(
                source,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            collect_surface_eval_import_names_from_expr(
                value,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_surface_eval_import_names_from_expr(
                    name_type,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for expr in expressions {
                collect_surface_eval_import_names_from_expr(
                    expr,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
    }
}

fn collect_surface_eval_import_names_for_member(
    object: &verter_analysis::type_expr::TypeExpr,
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match object {
        TypeExpr::Object(obj) => {
            if let Some(member) = obj.properties.iter().find(|member| match member {
                ObjectMember::Property(prop) => prop.name == key,
                ObjectMember::Method(method) => method.name == key,
                _ => false,
            }) {
                match member {
                    ObjectMember::Property(prop) => collect_surface_eval_import_names_from_expr(
                        &prop.ty,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    ObjectMember::Method(method) => {
                        collect_surface_eval_import_names_from_function(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        )
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = format!("$type:{name}");
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_surface_eval_import_names_for_member(
                    bound,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                if !active_locals.insert(name.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_surface_eval_import_names_for_member(
                    &decl.body,
                    key,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(name);
                return;
            }

            required.insert(name.clone());
            collect_surface_eval_import_names_for_builtin_member(
                name,
                type_arguments,
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
        }
        TypeExpr::Parenthesized(inner) => collect_surface_eval_import_names_for_member(
            inner,
            key,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for ty in types {
                collect_surface_eval_import_names_for_member(
                    ty,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(LiteralValue::String(inner_key)) = index.as_ref() {
                collect_surface_eval_import_names_for_member(
                    object,
                    inner_key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            } else {
                collect_surface_eval_import_names_from_expr(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        _ => collect_surface_eval_import_names_from_expr(
            object,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
    }
}

fn collect_surface_eval_import_names_for_builtin_member(
    name: &str,
    type_arguments: &[verter_analysis::type_expr::TypeExpr],
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    match name {
        "Partial" | "Required" | "Readonly" if type_arguments.len() == 1 => {
            collect_surface_eval_import_names_for_member(
                &type_arguments[0],
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if keys.contains(key) {
                collect_surface_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if !keys.contains(key) {
                collect_surface_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        _ => {}
    }
}

fn collect_string_literal_keys(
    expr: &verter_analysis::type_expr::TypeExpr,
) -> rustc_hash::FxHashSet<String> {
    use verter_analysis::type_expr::{LiteralValue, TypeExpr};

    let mut keys = rustc_hash::FxHashSet::default();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            keys.insert(value.clone());
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                keys.extend(collect_string_literal_keys(ty));
            }
        }
        TypeExpr::Parenthesized(inner) => {
            keys.extend(collect_string_literal_keys(inner));
        }
        _ => {}
    }
    keys
}

fn collect_surface_eval_import_names_from_function(
    func: &verter_analysis::type_expr::FunctionExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    for param in &func.parameters {
        collect_surface_eval_import_names_from_expr(
            &param.ty,
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
    }
}

fn should_recurse_surface_type_arguments(name: &str) -> bool {
    matches!(
        name,
        "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Record"
            | "Extract"
            | "Exclude"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited"
    )
}

fn resolve_reexport_target(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    sig: &verter_analysis::ExportSignature,
) -> Option<String> {
    if sig.is_type {
        host.resolve_type_dependency_canonical(canonical_id, source)
    } else {
        let ctx = verter_vfs::ResolutionContext {
            phase: verter_vfs::ResolvePhase::ProviderGraph,
            kind: verter_vfs::ResolveRequestKind::EsmImport,
        };
        host.resolve_via_vfs(canonical_id, source, ctx)
    }
}

#[derive(Debug, Clone, Default)]
struct ResolvedConsumedBindings {
    bindings: verter_analysis::component_meta::ConsumedRootBindings,
    partial_reasons: Vec<verter_analysis::component_meta::PartialBranchReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DynamicRootCandidate {
    NativeTag {
        tag: String,
    },
    ComponentImport {
        component_name: String,
        import_source: String,
    },
}

#[derive(Debug, Clone, Default)]
struct KnownSpreadKeys {
    attrs: std::collections::BTreeSet<String>,
    listeners: std::collections::BTreeSet<String>,
    exact: bool,
}

#[allow(clippy::too_many_arguments)]
fn push_native_candidate_branch(
    host: &VerterHost,
    tag: &str,
    branch_key: String,
    condition_text: Option<String>,
    consumed: &verter_analysis::component_meta::ConsumedRootBindings,
    parent_partial_reasons: &[verter_analysis::component_meta::PartialBranchReason],
    declared_prop_names: &rustc_hash::FxHashSet<String>,
    declared_event_names: &rustc_hash::FxHashSet<String>,
    declared_listener_aliases: &rustc_hash::FxHashSet<String>,
    fallthrough_branches: &mut Vec<verter_analysis::component_meta::FallthroughBranch>,
    any_partial: &mut bool,
) {
    use verter_analysis::component_meta::*;

    let intrinsic_members = host.intrinsic_members_for_tag(tag);

    let mut inherited_props = Vec::new();
    let mut inherited_events = Vec::new();

    for member in &intrinsic_members {
        match member.kind {
            verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => {
                if declared_prop_names.contains(member.name.as_str()) {
                    continue;
                }
                if consumed.attrs.iter().any(|attr| attr == &member.name) {
                    continue;
                }
                inherited_props.push(FallthroughPropEntry {
                    name: member.name.clone(),
                    type_expr: member.type_expr.clone(),
                    raw_type: None,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
            verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                if declared_event_names.contains(member.name.as_str())
                    || declared_listener_aliases.contains(member.name.as_str())
                {
                    continue;
                }
                if consumed
                    .listeners
                    .iter()
                    .any(|listener| listener == &member.name)
                {
                    continue;
                }
                inherited_events.push(FallthroughEventEntry {
                    name: member.name.clone(),
                    payload: member.type_expr.clone(),
                    raw_signature: None,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
        }
    }

    inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
    inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

    let status = if parent_partial_reasons.is_empty() {
        BranchStatus::Resolved
    } else {
        *any_partial = true;
        BranchStatus::PartiallyUnresolved {
            reasons: parent_partial_reasons.to_vec(),
        }
    };

    fallthrough_branches.push(FallthroughBranch {
        branch_key,
        condition_text,
        props: inherited_props,
        events: inherited_events,
        root_chain: vec![ResolvedRootStep::NativeTag {
            tag: tag.to_string(),
        }],
        status,
    });
}

#[allow(clippy::too_many_arguments)]
fn append_component_candidate_branches(
    host: &VerterHost,
    canonical_id: &str,
    component_name: &str,
    import_source: &str,
    branch_key: String,
    condition_text: Option<String>,
    consumed: &verter_analysis::component_meta::ConsumedRootBindings,
    parent_partial_reasons: &[verter_analysis::component_meta::PartialBranchReason],
    child_prop_overrides: Option<
        &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    >,
    declared_prop_names: &rustc_hash::FxHashSet<String>,
    declared_event_names: &rustc_hash::FxHashSet<String>,
    declared_listener_aliases: &rustc_hash::FxHashSet<String>,
    fallthrough_branches: &mut Vec<verter_analysis::component_meta::FallthroughBranch>,
    any_partial: &mut bool,
    any_unresolved: &mut bool,
    fact_versions: &mut Vec<verter_resolver::FactVersionRef>,
    visiting: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::component_meta::*;

    let child_canonical = host.resolve_loaded_dependency_canonical(
        canonical_id,
        import_source,
        verter_vfs::ResolveRequestKind::EsmImport,
    );

    let Some(child_id) = child_canonical else {
        *any_unresolved = true;
        fallthrough_branches.push(FallthroughBranch {
            branch_key,
            condition_text,
            props: Vec::new(),
            events: Vec::new(),
            root_chain: vec![ResolvedRootStep::Unresolved {
                tag: component_name.to_string(),
                reason: UnresolvedBranchReason::UnresolvedChildImport {
                    import_source: Some(import_source.to_string()),
                },
            }],
            status: BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::UnresolvedChildImport {
                    import_source: Some(import_source.to_string()),
                },
            },
        });
        return;
    };

    extend_unique_fact_versions(
        fact_versions,
        host.current_dependency_fact_versions(&child_id, &std::collections::BTreeSet::new()),
    );

    // Fallthrough inheritance depends on Vue root reachability facts. When the
    // imported child resolves to a non-SFC entrypoint (package declarations,
    // TS helpers, runtime JS), recursing cannot produce a stable inherited
    // surface and only drags the query through external graphs.
    if !child_id.ends_with(".vue") {
        *any_unresolved = true;
        fallthrough_branches.push(FallthroughBranch {
            branch_key,
            condition_text,
            props: Vec::new(),
            events: Vec::new(),
            root_chain: vec![ResolvedRootStep::Component {
                canonical_id: child_id,
                component_name: component_name.to_string(),
            }],
            status: BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::ChildResolutionFailed,
            },
        });
        return;
    }

    let Some(child_resolution) = host.resolve_fallthrough_surface_internal_with_overrides(
        &child_id,
        child_prop_overrides,
        visiting,
    ) else {
        *any_unresolved = true;
        fallthrough_branches.push(FallthroughBranch {
            branch_key,
            condition_text,
            props: Vec::new(),
            events: Vec::new(),
            root_chain: vec![ResolvedRootStep::Component {
                canonical_id: child_id.clone(),
                component_name: component_name.to_string(),
            }],
            status: BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::ChildResolutionFailed,
            },
        });
        return;
    };

    extend_unique_fact_versions(
        fact_versions,
        child_resolution.fact_versions.iter().cloned(),
    );

    match &child_resolution.fallthrough_surface {
        FallthroughSurface::None { .. } => {
            let mut inherited_props = Vec::new();
            let mut inherited_events = Vec::new();

            for prop in &child_resolution.accepted_props {
                if declared_prop_names.contains(&prop.name) {
                    continue;
                }
                if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                    continue;
                }
                inherited_props.push(FallthroughPropEntry {
                    name: prop.name.clone(),
                    type_expr: prop.type_expr.clone(),
                    raw_type: prop.raw_type.clone(),
                    sources: vec![InheritedSource::Component {
                        canonical_id: child_id.clone(),
                    }],
                });
            }

            for event in &child_resolution.accepted_events {
                if declared_event_names.contains(&event.name)
                    || declared_listener_aliases.contains(&event.name)
                {
                    continue;
                }
                if consumed
                    .listeners
                    .iter()
                    .any(|listener| listener == &event.name)
                {
                    continue;
                }
                inherited_events.push(FallthroughEventEntry {
                    name: event.name.clone(),
                    payload: event.payload.clone(),
                    raw_signature: event.raw_signature.clone(),
                    sources: vec![InheritedSource::Component {
                        canonical_id: child_id.clone(),
                    }],
                });
            }

            inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
            inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

            let status = if parent_partial_reasons.is_empty() {
                BranchStatus::Resolved
            } else {
                *any_partial = true;
                BranchStatus::PartiallyUnresolved {
                    reasons: parent_partial_reasons.to_vec(),
                }
            };

            fallthrough_branches.push(FallthroughBranch {
                branch_key,
                condition_text,
                props: inherited_props,
                events: inherited_events,
                root_chain: vec![ResolvedRootStep::Component {
                    canonical_id: child_id,
                    component_name: component_name.to_string(),
                }],
                status,
            });
        }
        FallthroughSurface::Branches {
            branches: child_branches,
        } => {
            let child_declared_props: Vec<_> = child_resolution
                .accepted_props
                .iter()
                .filter(|prop| matches!(prop.provenance, MemberProvenance::Declared))
                .collect();
            let child_declared_events: Vec<_> = child_resolution
                .accepted_events
                .iter()
                .filter(|event| matches!(event.provenance, MemberProvenance::Declared))
                .collect();

            for child_branch in child_branches {
                let composed_key = format!("{}.{}", branch_key, child_branch.branch_key);

                let mut inherited_props = Vec::new();
                let mut inherited_events = Vec::new();

                for prop in &child_declared_props {
                    if declared_prop_names.contains(&prop.name) {
                        continue;
                    }
                    if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                        continue;
                    }
                    inherited_props.push(FallthroughPropEntry {
                        name: prop.name.clone(),
                        type_expr: prop.type_expr.clone(),
                        raw_type: prop.raw_type.clone(),
                        sources: vec![InheritedSource::Component {
                            canonical_id: child_id.clone(),
                        }],
                    });
                }

                for prop in &child_branch.props {
                    if declared_prop_names.contains(&prop.name) {
                        continue;
                    }
                    if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                        continue;
                    }
                    inherited_props.push(prop.clone());
                }

                for event in &child_declared_events {
                    if declared_event_names.contains(&event.name)
                        || declared_listener_aliases.contains(&event.name)
                    {
                        continue;
                    }
                    if consumed
                        .listeners
                        .iter()
                        .any(|listener| listener == &event.name)
                    {
                        continue;
                    }
                    inherited_events.push(FallthroughEventEntry {
                        name: event.name.clone(),
                        payload: event.payload.clone(),
                        raw_signature: event.raw_signature.clone(),
                        sources: vec![InheritedSource::Component {
                            canonical_id: child_id.clone(),
                        }],
                    });
                }

                for event in &child_branch.events {
                    if declared_event_names.contains(&event.name)
                        || declared_listener_aliases.contains(&event.name)
                    {
                        continue;
                    }
                    if consumed
                        .listeners
                        .iter()
                        .any(|listener| listener == &event.name)
                    {
                        continue;
                    }
                    inherited_events.push(event.clone());
                }

                inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
                inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

                let mut root_chain = vec![ResolvedRootStep::Component {
                    canonical_id: child_id.clone(),
                    component_name: component_name.to_string(),
                }];
                root_chain.extend(child_branch.root_chain.clone());

                let status = match &child_branch.status {
                    BranchStatus::Resolved => {
                        if parent_partial_reasons.is_empty() {
                            BranchStatus::Resolved
                        } else {
                            *any_partial = true;
                            BranchStatus::PartiallyUnresolved {
                                reasons: parent_partial_reasons.to_vec(),
                            }
                        }
                    }
                    BranchStatus::PartiallyUnresolved { reasons } => {
                        *any_partial = true;
                        let mut combined = reasons.clone();
                        combined.extend(parent_partial_reasons.iter().cloned());
                        combined.sort();
                        combined.dedup();
                        BranchStatus::PartiallyUnresolved { reasons: combined }
                    }
                    BranchStatus::Unresolved { reason } => {
                        if !parent_partial_reasons.is_empty() {
                            *any_partial = true;
                        }
                        *any_unresolved = true;
                        BranchStatus::Unresolved {
                            reason: reason.clone(),
                        }
                    }
                };

                fallthrough_branches.push(FallthroughBranch {
                    branch_key: composed_key,
                    condition_text: condition_text.clone(),
                    props: inherited_props,
                    events: inherited_events,
                    root_chain,
                    status,
                });
            }
        }
    }
}

fn extend_unique_fact_versions<I>(
    fact_versions: &mut Vec<verter_resolver::FactVersionRef>,
    new_facts: I,
) where
    I: IntoIterator<Item = verter_resolver::FactVersionRef>,
{
    let mut seen: rustc_hash::FxHashSet<verter_resolver::FactVersionRef> =
        fact_versions.iter().cloned().collect();
    for fact in new_facts {
        if seen.insert(fact.clone()) {
            fact_versions.push(fact);
        }
    }
}

fn fallthrough_cache_key(
    canonical_id: &str,
    generic_root_propagation: bool,
    prop_type_overrides: Option<
        &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    >,
) -> verter_resolver::FallthroughNodeKey {
    verter_resolver::FallthroughNodeKey {
        canonical_component_id: canonical_id.to_string(),
        node_kind: verter_resolver::FallthroughNodeKind::BranchUnionMerge,
        override_fingerprint: prop_type_overrides
            .map(hash_prop_type_overrides)
            .unwrap_or_default(),
        behavior_flags: u32::from(generic_root_propagation),
        branch_selector: None,
    }
}

fn hash_prop_type_overrides(
    overrides: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut pairs: Vec<_> = overrides.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));

    let mut hasher = rustc_hash::FxHasher::default();
    for (name, ty) in pairs {
        name.hash(&mut hasher);
        ty.hash(&mut hasher);
    }
    hasher.finish()
}

fn inject_prop_type_overrides(
    env: &mut verter_analysis::type_eval::EvalEnv,
    overrides: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
) {
    for (name, ty) in overrides {
        env.add_value(verter_analysis::type_eval::ValueDeclInfo {
            name: name.clone(),
            declaration_id: 0,
            kind: verter_analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(ty.clone()),
            function_signature: None,
            object_shape: None,
        });
    }
}

fn resolve_usage_prop_type(
    prop: &verter_analysis::template::TemplatePropUsage,
    eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
) -> Option<verter_analysis::type_expr::TypeExpr> {
    use verter_analysis::type_expr::TypeExpr;

    if prop.from_spread {
        return None;
    }

    if !prop.is_bound {
        return match &prop.expression {
            Some(expression) => Some(TypeExpr::string_literal(expression.clone())),
            None => Some(TypeExpr::boolean_literal(true)),
        };
    }

    if let Some(expression) = &prop.expression {
        if let Some(env) = eval_env.as_mut() {
            if let Some(ty) =
                verter_analysis::type_eval_build::evaluate_value_expression(expression, env)
            {
                return Some(ty);
            }
        }

        if let Some(ty) = verter_analysis::type_eval_build::parse_value_expression_type(expression)
        {
            return Some(ty);
        }
    }

    if prop.is_shorthand {
        if let Some(env) = eval_env.as_mut() {
            if let Some(ty) =
                verter_analysis::type_eval_build::evaluate_value_expression(&prop.name, env)
            {
                return Some(ty);
            }
        }

        if let Some(ty) = verter_analysis::type_eval_build::parse_value_expression_type(&prop.name)
        {
            return Some(ty);
        }
    }

    None
}

fn merge_type_expr(
    existing: &mut verter_analysis::type_expr::TypeExpr,
    incoming: &verter_analysis::type_expr::TypeExpr,
) {
    use verter_analysis::type_expr::TypeExpr;

    if existing == incoming {
        return;
    }

    match existing {
        TypeExpr::Union(types) => {
            if !types.iter().any(|t| t == incoming) {
                types.push(incoming.clone());
            }
        }
        _ => {
            *existing = TypeExpr::union(vec![existing.clone(), incoming.clone()]);
        }
    }
}

fn merge_inherited_sources(
    existing: &mut Vec<verter_analysis::component_meta::InheritedSource>,
    incoming: &[verter_analysis::component_meta::InheritedSource],
) {
    existing.extend(incoming.iter().cloned());
    existing.sort();
    existing.dedup();
}

fn push_partial_reason(
    reasons: &mut Vec<verter_analysis::component_meta::PartialBranchReason>,
    reason: verter_analysis::component_meta::PartialBranchReason,
) {
    if !reasons.iter().any(|existing| existing == &reason) {
        reasons.push(reason);
    }
}

fn normalize_public_spread_key(
    key: &str,
    attrs: &mut std::collections::BTreeSet<String>,
    listeners: &mut std::collections::BTreeSet<String>,
) {
    if key == "class" || key == "style" {
        return;
    }
    if let Some(event_name) = verter_analysis::html_intrinsics::on_prop_to_event_name(key) {
        listeners.insert(event_name.to_string());
    } else {
        attrs.insert(key.to_string());
    }
}

fn known_spread_keys_from_object(
    object: &verter_analysis::type_expr::ObjectExpr,
) -> KnownSpreadKeys {
    let mut result = KnownSpreadKeys {
        exact: true,
        ..KnownSpreadKeys::default()
    };

    for member in &object.properties {
        match member {
            verter_analysis::type_expr::ObjectMember::Property(prop) => {
                normalize_public_spread_key(&prop.name, &mut result.attrs, &mut result.listeners)
            }
            verter_analysis::type_expr::ObjectMember::Method(method) => {
                normalize_public_spread_key(&method.name, &mut result.attrs, &mut result.listeners)
            }
            verter_analysis::type_expr::ObjectMember::IndexSignature(_)
            | verter_analysis::type_expr::ObjectMember::CallSignature(_)
            | verter_analysis::type_expr::ObjectMember::ConstructSignature(_) => {
                result.exact = false;
            }
        }
    }

    result
}

fn intersect_known_spread_keys(
    mut left: KnownSpreadKeys,
    right: KnownSpreadKeys,
) -> KnownSpreadKeys {
    left.attrs = left.attrs.intersection(&right.attrs).cloned().collect();
    left.listeners = left
        .listeners
        .intersection(&right.listeners)
        .cloned()
        .collect();
    left.exact &= right.exact;
    left
}

fn known_spread_keys_from_type_expr(
    ty: &verter_analysis::type_expr::TypeExpr,
) -> Option<KnownSpreadKeys> {
    use verter_analysis::type_expr::TypeExpr;

    match ty {
        TypeExpr::Object(obj) => Some(known_spread_keys_from_object(obj)),
        TypeExpr::Parenthesized(inner) => known_spread_keys_from_type_expr(inner),
        TypeExpr::Intersection(types) => {
            let mut result = KnownSpreadKeys {
                exact: true,
                ..KnownSpreadKeys::default()
            };
            let mut saw_any = false;
            for part in types {
                let Some(summary) = known_spread_keys_from_type_expr(part) else {
                    result.exact = false;
                    continue;
                };
                saw_any = true;
                result.attrs.extend(summary.attrs);
                result.listeners.extend(summary.listeners);
                result.exact &= summary.exact;
            }
            saw_any.then_some(result)
        }
        TypeExpr::Union(types) => {
            let mut iter = types.iter();
            let first = known_spread_keys_from_type_expr(iter.next()?)?;
            let mut result = first.clone();
            let mut exact_same_keys = first.exact;
            for ty in iter {
                let Some(summary) = known_spread_keys_from_type_expr(ty) else {
                    result.exact = false;
                    return Some(result);
                };
                exact_same_keys &= summary.exact
                    && summary.attrs == result.attrs
                    && summary.listeners == result.listeners;
                result = intersect_known_spread_keys(result, summary);
            }
            result.exact = exact_same_keys;
            Some(result)
        }
        _ => None,
    }
}

fn collect_dynamic_root_candidates_from_type(
    ty: &verter_analysis::type_expr::TypeExpr,
    snapshot: &FileAnalysisSnapshot,
) -> Vec<DynamicRootCandidate> {
    use verter_analysis::type_expr::{LiteralValue, TypeExpr};

    match ty {
        TypeExpr::Literal(LiteralValue::String(tag)) => {
            vec![DynamicRootCandidate::NativeTag { tag: tag.clone() }]
        }
        TypeExpr::Union(types) => types
            .iter()
            .flat_map(|branch| collect_dynamic_root_candidates_from_type(branch, snapshot))
            .collect(),
        TypeExpr::Parenthesized(inner) => {
            collect_dynamic_root_candidates_from_type(inner, snapshot)
        }
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => snapshot
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
            .find_map(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| !binding.is_type_only && binding.name == value_ref.path[0])
                    .map(|_| DynamicRootCandidate::ComponentImport {
                        component_name: value_ref.path[0].clone(),
                        import_source: import.source.clone(),
                    })
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
fn component_meta_resolved_macros(
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[crate::meta_resolve::ResolvedMacroMeta],
) -> Vec<verter_analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .filter(|resolved| {
            snapshot
                .macros
                .get(resolved.macro_index)
                .is_none_or(|mac| !raw_macro_surface_is_authoritative(mac))
        })
        .map(
            |resolved| verter_analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
}

fn raw_macro_surface_is_authoritative(mac: &verter_analysis::AnalyzedMacro) -> bool {
    match mac.kind {
        verter_analysis::AnalyzedMacroKind::DefineProps
        | verter_analysis::AnalyzedMacroKind::WithDefaults
        | verter_analysis::AnalyzedMacroKind::DefineModel => !mac.prop_fields.is_empty(),
        // Local emit parsing is often only a partial surface for type aliases
        // that intersect with imported helpers. Keep resolved emit members so
        // imported events can still merge in.
        verter_analysis::AnalyzedMacroKind::DefineEmits => false,
        verter_analysis::AnalyzedMacroKind::DefineSlots => !mac.slot_fields.is_empty(),
        verter_analysis::AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        verter_analysis::AnalyzedMacroKind::DefineOptions => false,
    }
}

fn component_meta_type_registry(
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_analysis::component_meta::ResolvedTypeAnalysis> {
    let mut seen = rustc_hash::FxHashSet::default();
    let mut registry = Vec::new();

    for entry in resolved_type_registry {
        if seen.insert(entry.name.clone()) {
            registry.push(entry.clone());
        }
    }

    registry
}

/// Build a `ComponentMetaAnalysis` from a resolved-meta state.
/// Shared by `get_component_meta` and `get_component_meta_with_resolution`.
fn extract_component_meta_from_inputs(
    host: &VerterHost,
    canonical_or_alias: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[verter_analysis::component_meta::ResolvedMacroInput],
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
    evaluated_types: Option<&verter_analysis::type_expand::ExpandedComponentTypes>,
    include_fallthrough: bool,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let started = component_meta_debug_enabled().then(Instant::now);
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let input = verter_analysis::component_meta::ComponentMetaInput {
        macros: &snapshot.macros,
        bindings: &snapshot.bindings,
        imports: &snapshot.imports,
        template: snapshot.template.as_deref(),
        options_api: snapshot.options_api.as_ref(),
        analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
            snapshot.script_flags,
        ),
        styles: &snapshot.styles,
        vue_api_calls: &snapshot.vue_api_calls,
        store_usages: &snapshot.store_usages,
        resolved_macros,
        resolved_type_registry,
        evaluated_types,
        file_path: &canonical,
    };
    let mut meta = verter_analysis::component_meta::extract_component_meta(input);

    if include_fallthrough {
        if let Some(resolution) = host.resolve_fallthrough_surface(&canonical) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
    }

    if let Some(started) = started {
        component_meta_debug(format!(
            "extract_component_meta owner={} include_fallthrough={} took {:?}",
            canonical,
            include_fallthrough,
            started.elapsed(),
        ));
    }

    meta
}

fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let resolved_macros =
        component_meta_resolved_macros(&resolved.snapshot, &resolved.resolved_macros);
    let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
    extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
        include_fallthrough,
    )
}

pub(crate) fn extract_slot_info_from_type_text(
    type_text: Option<&str>,
) -> (
    Vec<verter_analysis::AnalyzedSlotFieldBinding>,
    Option<String>,
) {
    let Some(text) = type_text else {
        return (Vec::new(), None);
    };

    // Extract return type: text after `=>` (arrow) or after closing `):`  (method).
    let return_type = if let Some(arrow_pos) = text.find("=>") {
        let ret = text[arrow_pos + 2..].trim();
        if !ret.is_empty() {
            Some(ret.to_string())
        } else {
            None
        }
    } else if let Some(colon_pos) = text.rfind("):") {
        let ret = text[colon_pos + 2..].trim();
        if !ret.is_empty() {
            Some(ret.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Extract bindings from the parameter object type.
    let Some(obj_start) = text.find('{') else {
        return (Vec::new(), return_type);
    };
    let mut depth = 0;
    let mut obj_end = obj_start;
    for (i, ch) in text[obj_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    obj_end = obj_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return (Vec::new(), return_type);
    }

    let obj_text = &text[obj_start..obj_end];

    // Parse the object literal as a type using verter_core's resolver.
    let alloc = oxc_allocator::Allocator::new();
    let resolved = verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
        "_Bindings",
        &format!("export interface _Bindings {obj_text}"),
        &alloc,
    );

    let Some(resolved) = resolved else {
        return (Vec::new(), return_type);
    };

    let bindings = resolved
        .props
        .iter()
        .filter_map(|p| {
            let name = p.key_name.as_ref()?.clone();
            Some(verter_analysis::AnalyzedSlotFieldBinding {
                name,
                type_annotation: p.type_text.clone(),
                span: verter_span::Span::default(),
            })
        })
        .collect();

    (bindings, return_type)
}

/// Convert `ResolvedElements` props to a structured `TypeExpr::Object`
/// using the pre-resolved `type_text` for each member.
pub(crate) fn resolved_elements_to_type_expr_via_type_text(
    resolved: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
) -> verter_analysis::type_expr::TypeExpr {
    use verter_analysis::type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

    let properties = resolved
        .props
        .iter()
        .map(|prop| {
            let ty = prop
                .type_text
                .as_deref()
                .map(verter_analysis::type_expr_lower::parse_type_annotation)
                .unwrap_or(TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                });
            ObjectMember::Property(ObjectProperty {
                name: prop
                    .key_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                ty,
                optional: prop.optional,
                readonly: false,
            })
        })
        .collect();

    TypeExpr::Object(ObjectExpr { properties })
}

#[cfg(test)]
#[path = "host_manage_tests.rs"]
mod tests;
