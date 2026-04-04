//! `TypeSolverHost` implementation for `verter_session`.
//!
//! Bridges the solver's prepared declaration queries to the host-owned
//! `ImportedDependencyCacheEntry` caches.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::TypeDeclKind;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_semantic::analysis::type_solver::host::{
    RequestStatus, ResolvedRootIdentity, SolverProjection, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::host_manage::component_meta_trace_event;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Import binding: maps a local import name to its resolved target.
#[derive(Debug, Clone)]
struct ImportBinding {
    canonical_id: String,
    exported_name: String,
}

fn member_projection_debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
            || std::env::var_os("VERTER_SOLVER_DEBUG").is_some()
    })
}

fn member_projection_debug(message: impl AsRef<str>) {
    if member_projection_debug_enabled() {
        eprintln!("[verter-solver] {}", message.as_ref());
    }
}

fn projection_expr_summary(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(name) => format!("Primitive({name:?})"),
        TypeExpr::Literal(lit) => format!("Literal({lit:?})"),
        TypeExpr::Union(types) => format!("Union({} members)", types.len()),
        TypeExpr::Intersection(types) => format!("Intersection({} members)", types.len()),
        TypeExpr::Array { .. } => "Array".to_string(),
        TypeExpr::Tuple { elements, .. } => format!("Tuple({} elements)", elements.len()),
        TypeExpr::Object(obj) => format!("Object({} members)", obj.properties.len()),
        TypeExpr::Function(_) => "Function".to_string(),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                format!("Ref({name})")
            } else {
                format!("Ref({name}<{} args>)", type_arguments.len())
            }
        }
        TypeExpr::TypeParameter(param) => format!("TypeParameter({})", param.name),
        TypeExpr::KeyOf(_) => "KeyOf".to_string(),
        TypeExpr::TypeOf(value) => format!("TypeOf({})", value.path.join(".")),
        TypeExpr::IndexedAccess { .. } => "IndexedAccess".to_string(),
        TypeExpr::Conditional { .. } => "Conditional".to_string(),
        TypeExpr::Mapped { parameter, .. } => format!("Mapped({parameter})"),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            format!("TemplateLiteral({} exprs)", expressions.len())
        }
        TypeExpr::Infer { name } => format!("Infer({name})"),
        TypeExpr::Rest(_) => "Rest".to_string(),
        TypeExpr::Parenthesized(inner) => {
            format!("Parenthesized({})", projection_expr_summary(inner))
        }
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            if type_arguments.is_empty() {
                format!("RecursiveRef({name})")
            } else {
                format!("RecursiveRef({name}<{} args>)", type_arguments.len())
            }
        }
        TypeExpr::Unknown { raw } => {
            let preview: String = raw.chars().take(40).collect();
            format!("Unknown({preview})")
        }
    }
}

fn transparent_alias_ref(expr: &TypeExpr) -> Option<(&str, &[TypeExpr])> {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => Some((name.as_ref(), type_arguments.as_ref())),
        TypeExpr::Parenthesized(inner) => transparent_alias_ref(inner),
        _ => None,
    }
}

fn push_unique_projection_context(
    contexts: &mut Vec<Arc<PreparedTypeDecl>>,
    prepared: &Arc<PreparedTypeDecl>,
) {
    if contexts.iter().any(|existing| {
        existing.root_identity.canonical_id == prepared.root_identity.canonical_id
            && existing.root_identity.symbol_name == prepared.root_identity.symbol_name
    }) {
        return;
    }
    contexts.push(Arc::clone(prepared));
}

fn build_type_param_bindings(
    prepared: &PreparedTypeDecl,
    args: &[TypeExpr],
) -> FxHashMap<String, TypeExpr> {
    prepared
        .type_parameters
        .iter()
        .zip(args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect()
}

fn substitute_type_expr(expr: &TypeExpr, bindings: &FxHashMap<String, TypeExpr>) -> TypeExpr {
    fn substitute(
        expr: &TypeExpr,
        bindings: &FxHashMap<String, TypeExpr>,
        shadowed: &mut Vec<String>,
    ) -> TypeExpr {
        let is_shadowed =
            |name: &str, shadowed: &[String]| shadowed.iter().any(|item| item == name);

        match expr {
            TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => expr.clone(),
            TypeExpr::TypeParameter(param)
                if !is_shadowed(&param.name, shadowed) && bindings.contains_key(&param.name) =>
            {
                bindings.get(&param.name).cloned().unwrap_or_else(|| expr.clone())
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty()
                && !is_shadowed(name.as_ref(), shadowed)
                && bindings.contains_key(name.as_ref()) =>
            {
                bindings
                    .get(name.as_ref())
                    .cloned()
                    .unwrap_or_else(|| expr.clone())
            }
            TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
                types
                    .iter()
                    .map(|ty| substitute(ty, bindings, shadowed))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
                types
                    .iter()
                    .map(|ty| substitute(ty, bindings, shadowed))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(substitute(element, bindings, shadowed)),
                readonly: *readonly,
            },
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: Arc::from(
                    elements
                        .iter()
                        .map(|element| verter_semantic::analysis::type_expr::TupleElement {
                            ty: substitute(&element.ty, bindings, shadowed),
                            optional: element.optional,
                            rest: element.rest,
                            label: element.label.clone(),
                        })
                        .collect::<Vec<_>>(),
                ),
                readonly: *readonly,
            },
            TypeExpr::Object(obj) => TypeExpr::Object(Arc::new(verter_semantic::analysis::type_expr::ObjectExpr {
                properties: obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        verter_semantic::analysis::type_expr::ObjectMember::Property(prop) => {
                            verter_semantic::analysis::type_expr::ObjectMember::Property(
                                verter_semantic::analysis::type_expr::ObjectProperty {
                                    name: prop.name.clone(),
                                    ty: substitute(&prop.ty, bindings, shadowed),
                                    optional: prop.optional,
                                    readonly: prop.readonly,
                                },
                            )
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(sig) => {
                            verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(
                                verter_semantic::analysis::type_expr::IndexSignature {
                                    key_name: sig.key_name.clone(),
                                    key_type: substitute(&sig.key_type, bindings, shadowed),
                                    value_type: substitute(&sig.value_type, bindings, shadowed),
                                    readonly: sig.readonly,
                                },
                            )
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::CallSignature(func) => {
                            verter_semantic::analysis::type_expr::ObjectMember::CallSignature(
                                substitute_function_expr(func, bindings, shadowed),
                            )
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(func) => {
                            verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                                substitute_function_expr(func, bindings, shadowed),
                            )
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                            verter_semantic::analysis::type_expr::ObjectMember::Method(
                                verter_semantic::analysis::type_expr::MethodSignature {
                                    name: method.name.clone(),
                                    function: substitute_function_expr(
                                        &method.function,
                                        bindings,
                                        shadowed,
                                    ),
                                    optional: method.optional,
                                },
                            )
                        }
                    })
                    .collect(),
            })),
            TypeExpr::Function(func) => {
                TypeExpr::Function(Arc::new(substitute_function_expr(func, bindings, shadowed)))
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => TypeExpr::Ref {
                name: Arc::clone(name),
                type_arguments: Arc::from(
                    type_arguments
                        .iter()
                        .map(|arg| substitute(arg, bindings, shadowed))
                        .collect::<Vec<_>>(),
                ),
            },
            TypeExpr::TypeParameter(_) => expr.clone(),
            TypeExpr::KeyOf(inner) => {
                TypeExpr::KeyOf(Arc::new(substitute(inner, bindings, shadowed)))
            }
            TypeExpr::TypeOf(value_ref) => TypeExpr::TypeOf(value_ref.clone()),
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(substitute(object, bindings, shadowed)),
                index: Arc::new(substitute(index, bindings, shadowed)),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(substitute(check, bindings, shadowed)),
                extends: Arc::new(substitute(extends, bindings, shadowed)),
                true_type: Arc::new(substitute(true_type, bindings, shadowed)),
                false_type: Arc::new(substitute(false_type, bindings, shadowed)),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => {
                shadowed.push(parameter.clone());
                let mapped = TypeExpr::Mapped {
                    parameter: parameter.clone(),
                    source: Arc::new(substitute(source, bindings, shadowed)),
                    value: Arc::new(substitute(value, bindings, shadowed)),
                    optional: *optional,
                    readonly: *readonly,
                    name_type: name_type
                        .as_ref()
                        .map(|name_type| Arc::new(substitute(name_type, bindings, shadowed))),
                };
                shadowed.pop();
                mapped
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: Arc::from(
                    expressions
                        .iter()
                        .map(|expr| substitute(expr, bindings, shadowed))
                        .collect::<Vec<_>>(),
                ),
            },
            TypeExpr::Infer { .. } => expr.clone(),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(substitute(inner, bindings, shadowed))),
            TypeExpr::Parenthesized(inner) => {
                TypeExpr::Parenthesized(Arc::new(substitute(inner, bindings, shadowed)))
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => TypeExpr::RecursiveRef {
                name: Arc::clone(name),
                type_arguments: Arc::from(
                    type_arguments
                        .iter()
                        .map(|arg| substitute(arg, bindings, shadowed))
                        .collect::<Vec<_>>(),
                ),
                conditional_context: Arc::clone(conditional_context),
            },
        }
    }

    fn substitute_function_expr(
        func: &verter_semantic::analysis::type_expr::FunctionExpr,
        bindings: &FxHashMap<String, TypeExpr>,
        shadowed: &mut Vec<String>,
    ) -> verter_semantic::analysis::type_expr::FunctionExpr {
        let base_len = shadowed.len();
        for param in &func.type_parameters {
            shadowed.push(param.name.clone());
        }
        let substituted = verter_semantic::analysis::type_expr::FunctionExpr {
            parameters: func
                .parameters
                .iter()
                .map(
                    |param| verter_semantic::analysis::type_expr::FunctionParam {
                        name: param.name.clone(),
                        ty: substitute(&param.ty, bindings, shadowed),
                        optional: param.optional,
                        rest: param.rest,
                    },
                )
                .collect(),
            return_type: func
                .return_type
                .as_ref()
                .map(|ret| Arc::new(substitute(ret, bindings, shadowed))),
            type_parameters: func
                .type_parameters
                .iter()
                .map(|param| verter_semantic::analysis::type_expr::TypeParam {
                    name: param.name.clone(),
                    constraint: param
                        .constraint
                        .as_ref()
                        .map(|constraint| Arc::new(substitute(constraint, bindings, shadowed))),
                    default: param
                        .default
                        .as_ref()
                        .map(|default| Arc::new(substitute(default, bindings, shadowed))),
                })
                .collect(),
        };
        shadowed.truncate(base_len);
        substituted
    }

    substitute(expr, bindings, &mut Vec::new())
}

/// Host-backed `TypeSolverHost` that resolves from:
/// 1. Declaration-scoped same-file prepared declarations
/// 2. Import bindings (local name → canonical_id + exported name)
/// 3. Host's `ImportedDependencyCacheEntry` prepared decl caches (cross-file)
pub struct SessionSolverHost<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Canonical file scope for declaration-scoped solving.
    scope_canonical_id: Option<String>,
    /// Same-file type names visible in the active declaration scope.
    scope_type_names: FxHashSet<String>,
    /// Same-file value names visible in the active declaration scope.
    scope_value_names: FxHashSet<String>,
    /// Script-setup generic bindings visible in the active declaration scope.
    scope_type_bindings: FxHashMap<String, Arc<PreparedTypeDecl>>,
    /// Import bindings: local name → (canonical_id, exported_name).
    /// Built from the owner file's `AnalyzedImport` entries.
    import_bindings: FxHashMap<String, ImportBinding>,
}

impl<'a> SessionSolverHost<'a> {
    pub fn new(host: &'a VerterHost, store_view: Option<&'a HostStoreView>) -> Self {
        Self {
            host,
            store_view,
            scope_canonical_id: None,
            scope_type_names: FxHashSet::default(),
            scope_value_names: FxHashSet::default(),
            scope_type_bindings: FxHashMap::default(),
            import_bindings: FxHashMap::default(),
        }
    }

    /// Create a solver host scoped to one declaration file's cached shallow
    /// state.
    ///
    /// Reads same-file symbol names and import targets from the host-owned
    /// `ShallowFileState` for the declaration file. This keeps declaration-
    /// scoped solving on the prepared/cache-backed path instead of rebuilding
    /// any owner-local eval state.
    pub fn with_declaration_scope(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        declaration_canonical_id: &str,
    ) -> Self {
        let mut import_bindings = FxHashMap::default();
        let mut scope_type_names = FxHashSet::default();
        let mut scope_value_names = FxHashSet::default();
        let mut scope_type_bindings = FxHashMap::default();
        let dependency_resolutions = host
            .dependency_resolutions_for_eval_in_view(declaration_canonical_id, store_view)
            .unwrap_or_default();

        if let Some(state) = host.shallow_file_state_in_view(declaration_canonical_id, store_view) {
            scope_type_names.extend(state.symbols.keys().cloned());
            scope_value_names.extend(state.value_symbols.keys().cloned());
            for (local_name, (source_specifier, imported_name)) in state.import_targets.iter() {
                let resolved_id = dependency_resolutions
                    .get(source_specifier)
                    .and_then(|resolution| {
                        resolution
                            .effective_target()
                            .map(str::to_string)
                            .or_else(|| resolution.resolved_canonical_id.clone())
                    })
                    .or_else(|| {
                        host.resolve_type_dependency_canonical_shallow_in_view(
                            declaration_canonical_id,
                            source_specifier,
                            store_view,
                        )
                    });
                if let Some(resolved_id) = resolved_id {
                    import_bindings.insert(
                        local_name.clone(),
                        ImportBinding {
                            canonical_id: resolved_id,
                            exported_name: imported_name.clone(),
                        },
                    );
                }
            }

            if let Some((raw_source, cached_parse, _)) =
                host.current_eval_state_in_view(declaration_canonical_id, store_view)
            {
                for param in VerterHost::sfc_script_setup_type_params(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ) {
                    let mut prepared = PreparedTypeDecl::new(
                        ResolvedRootIdentity::new(declaration_canonical_id, &param.name),
                        TypeDeclKind::Alias,
                        TypeExpr::type_parameter(param.clone()),
                    );
                    for local_name in state.symbols.keys() {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(declaration_canonical_id, local_name),
                        );
                    }
                    for local_name in state.value_symbols.keys() {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(declaration_canonical_id, local_name),
                        );
                    }
                    for (local_name, binding) in &import_bindings {
                        prepared.name_resolution.insert(
                            local_name.clone(),
                            ResolvedRootIdentity::new(
                                &binding.canonical_id,
                                &binding.exported_name,
                            ),
                        );
                    }
                    scope_type_names.insert(param.name.clone());
                    scope_type_bindings.insert(param.name.clone(), Arc::new(prepared));
                }
            }
        }
        Self {
            host,
            store_view,
            scope_canonical_id: Some(declaration_canonical_id.to_string()),
            scope_type_names,
            scope_value_names,
            scope_type_bindings,
            import_bindings,
        }
    }
}

impl TypeSolverHost for SessionSolverHost<'_> {
    fn resolve_prepared_type_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        if let Some(scope_canonical_id) = self.scope_canonical_id.as_deref() {
            if root_identity.canonical_id == scope_canonical_id {
                if let Some(bound) = self.scope_type_bindings.get(&root_identity.symbol_name) {
                    component_meta_trace_event!(
                        "solver_resolve_prepared_type_decl_result",
                        format!(
                            "root={}::{} source=scope_binding hit=true store_view={}",
                            root_identity.canonical_id,
                            root_identity.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(Arc::clone(bound));
                }
            }
        }

        // Resolve from the host-owned prepared decl cache.
        if let Some(prepared) = self.host.prepared_type_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        ) {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=direct_prepared hit=true store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return Some(prepared);
        }

        // Declaration-scoped name resolution and import bindings may point at
        // a shallow import target that is itself a barrel. Follow the cached
        // export route once here so the solver still reads the final prepared
        // declaration from host-owned cache state instead of stranding the
        // lookup on the barrel file.
        if root_identity.canonical_id.is_empty() {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=empty_canonical hit=false store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        let (final_canonical_id, final_symbol_name) = self.host.resolve_imported_type_root_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        );
        if final_canonical_id == root_identity.canonical_id
            && final_symbol_name == root_identity.symbol_name
        {
            component_meta_trace_event!(
                "solver_resolve_prepared_type_decl_result",
                format!(
                    "root={}::{} source=root_resolve_same hit=false store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        let resolved = self.host.prepared_type_decl_in_view(
            &final_canonical_id,
            &final_symbol_name,
            self.store_view,
        );
        component_meta_trace_event!(
            "solver_resolve_prepared_type_decl_result",
            format!(
                "root={}::{} source=root_resolve target={}::{} hit={} store_view={}",
                root_identity.canonical_id,
                root_identity.symbol_name,
                final_canonical_id,
                final_symbol_name,
                resolved.is_some(),
                self.store_view.is_some()
            ),
        );
        resolved
    }

    fn resolve_prepared_value_decl(
        &self,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedValueDecl>> {
        // Resolve from the host-owned prepared decl cache.
        if let Some(prepared) = self.host.prepared_value_decl_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        ) {
            return Some(prepared);
        }

        if root_identity.canonical_id.is_empty() {
            return None;
        }

        let target = self.host.resolve_value_export_target_in_view(
            &root_identity.canonical_id,
            &root_identity.symbol_name,
            self.store_view,
        )?;
        let final_canonical_id = target.canonical_id;
        let final_symbol_name = target.name;
        if final_canonical_id == root_identity.canonical_id
            && final_symbol_name == root_identity.symbol_name
        {
            return None;
        }

        self.host.prepared_value_decl_in_view(
            &final_canonical_id,
            &final_symbol_name,
            self.store_view,
        )
    }

    fn resolve_member_projection(
        &self,
        root_identity: &ResolvedRootIdentity,
        member: &str,
    ) -> Option<SolverProjection<TypeExpr>> {
        let Some(prepared) = self.resolve_prepared_type_decl(root_identity) else {
            member_projection_debug(format!(
                "member_projection miss root={}:{} member={} reason=prepared_decl_missing",
                root_identity.canonical_id, root_identity.symbol_name, member,
            ));
            component_meta_trace_event!(
                "solver_member_projection_result",
                format!(
                    "root={}::{} member={} source=prepared_decl_missing hit=false store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    member,
                    self.store_view.is_some()
                ),
            );
            return None;
        };
        // Direct member lookup
        if let Some(m) = prepared.member(member) {
            member_projection_debug(format!(
                "member_projection hit root={}:{} member={} expr={}",
                root_identity.canonical_id,
                root_identity.symbol_name,
                member,
                projection_expr_summary(&m.ty),
            ));
            component_meta_trace_event!(
                "solver_member_projection_result",
                format!(
                    "root={}::{} member={} source=direct_member hit=true store_view={}",
                    root_identity.canonical_id,
                    root_identity.symbol_name,
                    member,
                    self.store_view.is_some()
                ),
            );
            return Some(SolverProjection::exact_concrete(m.ty.clone()));
        }

        // Bounded transparent alias chase: follow Ref wrappers through prepared
        // data (up to 5 hops) to find the member. Generic alias hops substitute
        // their active type arguments into the projected member while preserving
        // the declaration contexts needed to resolve helper-local aliases and
        // caller-local type arguments.
        {
            let mut visited = rustc_hash::FxHashSet::default();
            visited.insert((
                root_identity.canonical_id.clone(),
                root_identity.symbol_name.clone(),
            ));
            let mut current_prepared = Arc::clone(&prepared);
            let mut current_bindings = FxHashMap::default();
            let mut projection_contexts = vec![Arc::clone(&prepared)];

            for _hop in 0..5 {
                let (ref_name, raw_type_arguments) =
                    match transparent_alias_ref(&current_prepared.body) {
                        Some((name, type_arguments)) => (name.to_string(), type_arguments.to_vec()),
                        _ => break,
                    };
                let effective_type_arguments = raw_type_arguments
                    .iter()
                    .map(|arg| substitute_type_expr(arg, &current_bindings))
                    .collect::<Vec<_>>();

                // Resolve through name_resolution of the current declaration
                let Some(next_root) = current_prepared.name_resolution.get(&ref_name) else {
                    break;
                };

                // Cycle protection
                if !visited.insert((
                    next_root.canonical_id.clone(),
                    next_root.symbol_name.clone(),
                )) {
                    break;
                }

                let Some(next_prepared) = self.host.prepared_type_decl_in_view(
                    &next_root.canonical_id,
                    &next_root.symbol_name,
                    self.store_view,
                ) else {
                    break;
                };
                push_unique_projection_context(&mut projection_contexts, &next_prepared);
                let next_bindings =
                    build_type_param_bindings(&next_prepared, &effective_type_arguments);

                // Check member on this hop
                if let Some(m) = next_prepared.member(member) {
                    let projected_ty = if next_bindings.is_empty() {
                        m.ty.clone()
                    } else {
                        substitute_type_expr(&m.ty, &next_bindings)
                    };
                    member_projection_debug(format!(
                        "member_projection hit (alias chase hop {}) root={}:{} via={}:{} member={} expr={}",
                        _hop + 1,
                        root_identity.canonical_id,
                        root_identity.symbol_name,
                        next_root.canonical_id,
                        next_root.symbol_name,
                        member,
                        projection_expr_summary(&projected_ty),
                    ));
                    component_meta_trace_event!(
                        "solver_member_projection_result",
                        format!(
                            "root={}::{} member={} source=alias_chase hop={} target={}::{} generic={} hit=true store_view={}",
                            root_identity.canonical_id,
                            root_identity.symbol_name,
                            member,
                            _hop + 1,
                            next_root.canonical_id,
                            next_root.symbol_name,
                            !effective_type_arguments.is_empty(),
                            self.store_view.is_some()
                        ),
                    );
                    return Some(
                        SolverProjection::exact_concrete(projected_ty)
                            .with_type_decl_contexts(projection_contexts),
                    );
                }

                // Continue chasing through the next declaration's body
                current_prepared = next_prepared;
                current_bindings = next_bindings;
            }
        }

        let mut available = prepared.member_index.keys().cloned().collect::<Vec<_>>();
        available.sort();
        member_projection_debug(format!(
            "member_projection miss root={}:{} member={} reason=member_missing available=[{}]",
            root_identity.canonical_id,
            root_identity.symbol_name,
            member,
            available.join(", "),
        ));
        component_meta_trace_event!(
            "solver_member_projection_result",
            format!(
                "root={}::{} member={} source=member_missing hit=false available=[{}] store_view={}",
                root_identity.canonical_id,
                root_identity.symbol_name,
                member,
                available.join(", "),
                self.store_view.is_some()
            ),
        );
        None
    }

    fn utility_source(&self, name: &str) -> UtilitySource {
        if self.scope_type_names.contains(name) || self.scope_type_bindings.contains_key(name) {
            return UtilitySource::Shadowed;
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn root_identity(&self, canonical_id: &str, symbol_name: &str) -> Option<ResolvedRootIdentity> {
        if let Some(scope_canonical_id) = self.scope_canonical_id.as_deref() {
            if self.scope_type_bindings.contains_key(symbol_name)
                || self.scope_type_names.contains(symbol_name)
                || self.scope_value_names.contains(symbol_name)
            {
                let resolved = ResolvedRootIdentity::new(scope_canonical_id, symbol_name);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=scope result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
        }

        // 2. If canonical_id is provided and non-empty, use it directly
        if !canonical_id.is_empty() {
            if self
                .host
                .prepared_type_decl_in_view(canonical_id, symbol_name, self.store_view)
                .is_some()
            {
                let resolved = ResolvedRootIdentity::new(canonical_id, symbol_name);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_type_decl result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            if self
                .host
                .prepared_value_decl_in_view(canonical_id, symbol_name, self.store_view)
                .is_some()
            {
                let resolved = ResolvedRootIdentity::new(canonical_id, symbol_name);
                component_meta_trace_event!(
                    "solver_root_identity_result",
                    format!(
                        "requested_canonical={} requested_symbol={} source=explicit_value_decl result={}::{} hit=true store_view={}",
                        canonical_id,
                        symbol_name,
                        resolved.canonical_id,
                        resolved.symbol_name,
                        self.store_view.is_some()
                    ),
                );
                return Some(resolved);
            }
            component_meta_trace_event!(
                "solver_root_identity_result",
                format!(
                    "requested_canonical={} requested_symbol={} source=explicit_canonical hit=false store_view={}",
                    canonical_id,
                    symbol_name,
                    self.store_view.is_some()
                ),
            );
            return None;
        }

        // 3. Check import bindings: local name → (canonical_id, exported_name).
        // This is the targeted resolution path for the owner file's direct imports.
        // It handles renamed and default imports where the local name differs
        // from the exported name.
        if let Some(binding) = self.import_bindings.get(symbol_name) {
            let resolved = ResolvedRootIdentity::new(&binding.canonical_id, &binding.exported_name);
            component_meta_trace_event!(
                "solver_root_identity_result",
                format!(
                    "requested_canonical={} requested_symbol={} source=import_binding result={}::{} hit=true store_view={}",
                    canonical_id,
                    symbol_name,
                    resolved.canonical_id,
                    resolved.symbol_name,
                    self.store_view.is_some()
                ),
            );
            return Some(resolved);
        }

        // 4. Handle namespace-qualified names: `Ns.Member` → split on first dot,
        // resolve prefix as namespace import, look up member in the target file.
        if let Some(dot_pos) = symbol_name.find('.') {
            let prefix = &symbol_name[..dot_pos];
            let member = &symbol_name[dot_pos + 1..];
            if let Some(binding) = self.import_bindings.get(prefix) {
                if self
                    .host
                    .prepared_type_decl_in_view(&binding.canonical_id, member, self.store_view)
                    .is_some()
                {
                    let resolved = ResolvedRootIdentity::new(&binding.canonical_id, member);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_prepared_type result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if self
                    .host
                    .prepared_value_decl_in_view(&binding.canonical_id, member, self.store_view)
                    .is_some()
                {
                    let resolved = ResolvedRootIdentity::new(&binding.canonical_id, member);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_prepared_value result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if let Some((canonical_id, exported_name)) =
                    self.host.resolve_named_type_export_target_in_view(
                        &binding.canonical_id,
                        member,
                        self.store_view,
                    )
                {
                    let resolved = ResolvedRootIdentity::new(&canonical_id, &exported_name);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_named_export result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
                if let Some(target) = self.host.resolve_value_export_target_in_view(
                    &binding.canonical_id,
                    member,
                    self.store_view,
                ) {
                    let resolved = ResolvedRootIdentity::new(&target.canonical_id, &target.name);
                    component_meta_trace_event!(
                        "solver_root_identity_result",
                        format!(
                            "requested_canonical={} requested_symbol={} source=namespace_value_export result={}::{} hit=true store_view={}",
                            canonical_id,
                            symbol_name,
                            resolved.canonical_id,
                            resolved.symbol_name,
                            self.store_view.is_some()
                        ),
                    );
                    return Some(resolved);
                }
            }
        }

        // Unresolved bare-name: the solver encountered a reference that is not
        // in the owner env, not at a known canonical_id, and not in the import
        // bindings. This is expected for transitive same-file deps inside
        // imported prepared decl bodies — the solver does not yet propagate
        // the defining file's canonical_id through resolution context.
        component_meta_trace_event!(
            "solver_root_identity_result",
            format!(
                "requested_canonical={} requested_symbol={} source=unresolved_bare_name hit=false store_view={}",
                canonical_id,
                symbol_name,
                self.store_view.is_some()
            ),
        );
        None
    }

    fn request_status(&self) -> RequestStatus {
        RequestStatus::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_semantic::analysis::type_solver::host::NoopSolverHost;
    use verter_semantic::analysis::Hash16;

    #[test]
    fn noop_host_returns_none() {
        let host = NoopSolverHost;
        let id = ResolvedRootIdentity::new("/t.ts", "T");
        assert!(host.resolve_prepared_type_decl(&id).is_none());
    }

    #[test]
    fn session_host_without_env() {
        let host = VerterHost::new_standalone(Default::default());
        let solver_host = SessionSolverHost::new(&host, None);
        let id = ResolvedRootIdentity::new("/t.ts", "T");
        assert!(solver_host.resolve_prepared_type_decl(&id).is_none());
    }

    #[test]
    fn declaration_scope_prefers_cached_prepared_decl_shape() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;

        let host = VerterHost::new_standalone(Default::default());
        let source = r#"
import type { Inner } from "./dep"
export interface Props { child: Inner }
"#;
        let allocator = oxc_allocator::Allocator::new();
        let analysis = Arc::new(analyze_external_type_source(source, &allocator));
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
        let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&analysis),
            Some(&env),
        ));
        let dep_edges = FxHashMap::from_iter([("./dep".to_string(), "/dep.ts".to_string())]);
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );

        host.imported_dependency_cache.lock().insert(
            "/decl.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/decl.ts".into(),
                raw_source: Arc::<str>::from(source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(analysis),
                shallow_file_state: Some(state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./dep".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./dep".to_string(),
                        resolved_canonical_id: Some("/dep.ts".to_string()),
                        possible_canonical_ids: vec!["/dep.ts".to_string()],
                    },
                )]),
            }),
        );

        let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");
        let id = ResolvedRootIdentity::new("/decl.ts", "Props");
        let decl = solver_host
            .resolve_prepared_type_decl(&id)
            .expect("declaration-scoped host should use cached prepared decls");
        assert_eq!(
            decl.name_resolution
                .get("Inner")
                .map(|identity| identity.canonical_id.as_str()),
            Some("/dep.ts"),
            "declaration-scoped solving should preserve cached name-resolution instead of rebuilding a local decl from EvalEnv",
        );
    }

    #[test]
    fn declaration_scope_root_identity_resolves_same_file_symbols_and_imports() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let source = r#"
import type { Theme } from "./theme"
export interface Props { theme: Theme }
export const defaults: Props = {} as Props
"#;
        let allocator = oxc_allocator::Allocator::new();
        let analysis = Arc::new(analyze_external_type_source(source, &allocator));
        let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
        let state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&analysis),
            Some(&env),
        ));
        let dep_edges = FxHashMap::from_iter([("./theme".to_string(), "/theme.ts".to_string())]);
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );
        let prepared_value_decls = crate::resolver_core::build_prepared_value_decl_cache(
            "/decl.ts",
            &state,
            Some(&dep_edges),
        );

        host.imported_dependency_cache.lock().insert(
            "/decl.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/decl.ts".into(),
                raw_source: Arc::<str>::from(source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(analysis),
                shallow_file_state: Some(state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls,
                dependency_resolutions: FxHashMap::from_iter([(
                    "./theme".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./theme".to_string(),
                        resolved_canonical_id: Some("/theme.ts".to_string()),
                        possible_canonical_ids: vec!["/theme.ts".to_string()],
                    },
                )]),
            }),
        );

        let solver_host = SessionSolverHost::with_declaration_scope(&host, None, "/decl.ts");

        let props = solver_host
            .root_identity("", "Props")
            .expect("same-file type should resolve in declaration scope");
        assert_eq!(props.canonical_id, "/decl.ts");

        let defaults = solver_host
            .root_identity("", "defaults")
            .expect("same-file value should resolve in declaration scope");
        assert_eq!(defaults.canonical_id, "/decl.ts");

        let theme = solver_host
            .root_identity("", "Theme")
            .expect("import binding should resolve from declaration scope");
        assert_eq!(theme.canonical_id, "/theme.ts");
        assert_eq!(theme.symbol_name, "Theme");
    }

    #[test]
    fn prepared_type_decl_lookup_routes_barrel_targets_before_cache_lookup() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let barrel_source = "export { Props } from './props'";
        let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
        let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&barrel_analysis),
            None,
        ));

        host.imported_dependency_cache.lock().insert(
            "/types/index.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/index.ts".into(),
                raw_source: Arc::<str>::from(barrel_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(barrel_analysis),
                shallow_file_state: Some(barrel_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(barrel_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./props".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./props".to_string(),
                        resolved_canonical_id: Some("/types/props.ts".to_string()),
                        possible_canonical_ids: vec!["/types/props.ts".to_string()],
                    },
                )]),
            }),
        );

        let props_source = "export interface Props { label: string }";
        let props_analysis = Arc::new(analyze_external_type_source(props_source, &allocator));
        let props_env = parse_and_build_env(props_source);
        let props_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&props_analysis),
            Some(&props_env),
        ));
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            "/types/props.ts",
            &props_state,
            None,
        );

        host.imported_dependency_cache.lock().insert(
            "/types/props.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/props.ts".into(),
                raw_source: Arc::<str>::from(props_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(props_analysis),
                shallow_file_state: Some(props_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(props_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::default(),
            }),
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prepared = solver_host
            .resolve_prepared_type_decl(&ResolvedRootIdentity::new("/types/index.ts", "Props"))
            .expect("barrel lookup should route to the defining prepared type decl");
        assert_eq!(prepared.root_identity.canonical_id, "/types/props.ts");
        assert_eq!(prepared.root_identity.symbol_name, "Props");
    }

    #[test]
    fn prepared_value_decl_lookup_routes_barrel_targets_before_cache_lookup() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let barrel_source = "export { theme } from './theme'";
        let barrel_analysis = Arc::new(analyze_external_type_source(barrel_source, &allocator));
        let barrel_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&barrel_analysis),
            None,
        ));

        host.imported_dependency_cache.lock().insert(
            "/theme/index.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/theme/index.ts".into(),
                raw_source: Arc::<str>::from(barrel_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(barrel_analysis),
                shallow_file_state: Some(barrel_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(barrel_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./theme".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./theme".to_string(),
                        resolved_canonical_id: Some("/theme/theme.ts".to_string()),
                        possible_canonical_ids: vec!["/theme/theme.ts".to_string()],
                    },
                )]),
            }),
        );

        let theme_source = "export const theme: { color: string } = { color: 'blue' }";
        let theme_analysis = Arc::new(analyze_external_type_source(theme_source, &allocator));
        let theme_env = parse_and_build_env(theme_source);
        let theme_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&theme_analysis),
            Some(&theme_env),
        ));
        let prepared_value_decls = crate::resolver_core::build_prepared_value_decl_cache(
            "/theme/theme.ts",
            &theme_state,
            None,
        );

        host.imported_dependency_cache.lock().insert(
            "/theme/theme.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/theme/theme.ts".into(),
                raw_source: Arc::<str>::from(theme_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(theme_analysis),
                shallow_file_state: Some(theme_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(theme_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: FxHashMap::default(),
                prepared_value_decls,
                dependency_resolutions: FxHashMap::default(),
            }),
        );

        let solver_host = SessionSolverHost::new(&host, None);
        let prepared = solver_host
            .resolve_prepared_value_decl(&ResolvedRootIdentity::new("/theme/index.ts", "theme"))
            .expect("barrel lookup should route to the defining prepared value decl");
        assert_eq!(prepared.root_identity.canonical_id, "/theme/theme.ts");
        assert_eq!(prepared.root_identity.symbol_name, "theme");
    }

    #[test]
    fn member_projection_chases_generic_alias_slots_through_helper_context() {
        use verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_source;
        use verter_semantic::analysis::type_eval_build::parse_and_build_env;
        use verter_semantic::analysis::type_expr::{
            LiteralValue, ObjectMember, PrimitiveName, TypeExpr,
        };
        use verter_semantic::analysis::type_solver::solve::solve_type_with_trace;
        use verter_semantic::analysis::Hash16;

        let host = VerterHost::new_standalone(Default::default());
        let allocator = oxc_allocator::Allocator::new();

        let config_source = r#"
export type Id<T> = {} & { [P in keyof T]: T[P] }
export type Theme = {
  slots: {
    item: string
  }
}
export type Noise = {
  boom: string
}
export type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<T['slots']>
export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}
"#;
        let config_analysis = Arc::new(analyze_external_type_source(config_source, &allocator));
        let config_env = parse_and_build_env(config_source);
        let config_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&config_analysis),
            Some(&config_env),
        ));
        let config_prepared = crate::resolver_core::build_prepared_type_decl_cache(
            "/types/config.ts",
            &config_state,
            None,
        );

        host.imported_dependency_cache.lock().insert(
            "/types/config.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/config.ts".into(),
                raw_source: Arc::<str>::from(config_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(config_analysis),
                shallow_file_state: Some(config_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(config_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: config_prepared,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::default(),
            }),
        );

        let consumer_source = r#"
import type { ComponentConfig, Theme } from './config'
export type CheckboxGroup = ComponentConfig<Theme>
"#;
        let consumer_analysis = Arc::new(analyze_external_type_source(consumer_source, &allocator));
        let consumer_env = parse_and_build_env(consumer_source);
        let consumer_state = Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
            Hash16::default(),
            Arc::clone(&consumer_analysis),
            Some(&consumer_env),
        ));
        let dep_edges =
            FxHashMap::from_iter([("./config".to_string(), "/types/config.ts".to_string())]);
        let consumer_prepared = crate::resolver_core::build_prepared_type_decl_cache(
            "/types/consumer.ts",
            &consumer_state,
            Some(&dep_edges),
        );

        host.imported_dependency_cache.lock().insert(
            "/types/consumer.ts".into(),
            Arc::new(crate::ImportedDependencyCacheEntry {
                workspace_generation: host.ws().content_generation(),
                whole_hash: Hash16::default(),
                resolved_canonical_id: "/types/consumer.ts".into(),
                raw_source: Arc::<str>::from(consumer_source),
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: Some(consumer_analysis),
                shallow_file_state: Some(consumer_state),
                snapshot: None,
                eval_source: Some(Arc::<str>::from(consumer_source)),
                required_owner_import_names: None,
                exported_required_import_names: FxHashMap::default(),
                resolved_type_roots: FxHashMap::default(),
                resolved_type_declarations: FxHashMap::default(),
                prepared_type_decls: consumer_prepared,
                prepared_value_decls: FxHashMap::default(),
                dependency_resolutions: FxHashMap::from_iter([(
                    "./config".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./config".to_string(),
                        resolved_canonical_id: Some("/types/config.ts".to_string()),
                        possible_canonical_ids: vec!["/types/config.ts".to_string()],
                    },
                )]),
            }),
        );

        let solver_host =
            SessionSolverHost::with_declaration_scope(&host, None, "/types/consumer.ts");
        let projection = solver_host
            .resolve_member_projection(
                &ResolvedRootIdentity::new("/types/consumer.ts", "CheckboxGroup"),
                "slots",
            )
            .expect("generic alias member projection should resolve slots");
        assert_eq!(
            projection.exactness,
            verter_semantic::analysis::type_solver::result::SolverExactness::ExactConcrete
        );

        let (solved, trace) = solve_type_with_trace(
            &TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("CheckboxGroup")),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
            },
            &solver_host,
        );

        let TypeExpr::Object(slots) = solved.value else {
            panic!("expected object slots projection, got {:?}", solved.value);
        };
        let item = slots
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(prop) if prop.name == "item" => Some(prop),
                _ => None,
            })
            .expect("slots projection should contain item");
        assert!(
            !item.optional,
            "fixture keeps the projected slot member required"
        );
        assert!(matches!(
            item.ty,
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        assert!(
            !trace.iter().any(|identity| {
                identity.canonical_id == "/types/config.ts" && identity.symbol_name == "Noise"
            }),
            "solving CheckboxGroup['slots'] should stay on-route and never visit Noise"
        );
    }
}
