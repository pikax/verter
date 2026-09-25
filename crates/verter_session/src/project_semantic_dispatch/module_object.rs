//! The checker's module object.
//!
//! A module read as a value — through `import * as NS`, an
//! `import NS = require("…")` of a module without `export =`, or
//! `typeof import("…")` — is one object whose properties are the module's
//! exported values: its own exports, and every name an `export * from`
//! re-exports (except `default`, and never over a name the module already
//! exports). A namespace declaration read as a value is the same object over
//! the namespace's exported values and the nested namespaces that declare a
//! value, and an ambient `declare module "…"` block is the module a file
//! would be. Each property is the `typeof` of the declaration the export
//! names, read through the one `TypeOf` rail; a type-only export has no value
//! and is no property.
//!
//! A module assigned with `export = X` is X itself. A namespace import of
//! it applies TypeScript 7's interop when X can be called or constructed:
//! the object carries X's own properties and `default`, X itself.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::build::AmbientModuleBlock;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    NodeScopeId, QueryResult, SemanticContextId, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue, SignatureKind as GraphSignatureKind, SurfaceMember,
    SurfaceView, ValueRootKey,
};
use crate::semantic_query_memo::ObservedGraphSelfRoot;

/// A module object: its interned node and the file versions whose
/// declarations chose its properties.
pub(super) type ModuleObject = (SemanticNodeId, Vec<ObservedGraphSelfRoot>);

fn file_root(
    canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> ValueRootKey {
    ValueRootKey {
        scope: crate::semantic_query::ScopeId {
            canonical_id: Arc::from(canonical),
            owner,
            local_scope: None,
            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(owner),
        },
        name: Arc::from(name),
    }
}

fn push_root(roots: &mut Vec<ObservedGraphSelfRoot>, root: ObservedGraphSelfRoot) {
    if !roots.contains(&root) {
        roots.push(root);
    }
}

impl ProjectSemanticDispatch<'_> {
    /// The module object the value root names when read whole, if it names
    /// one: a namespace import (or an import assignment of a module without
    /// `export =`), or a namespace this file declares — in its own scope or
    /// in one of its `declare module` blocks.
    pub(super) fn module_object_of_root(
        &self,
        value_root: &ValueRootKey,
        shallow: &crate::resolver_core::ShallowFileState,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<ModuleObject> {
        use crate::resolver_core::shallow_file_state::LexicalValueBinding;
        let canonical = value_root.scope.canonical_id.as_ref();
        let owner = value_root.scope.owner;
        match shallow.visible_value_binding(owner, value_root.name.as_ref()) {
            Some(LexicalValueBinding::Import(target)) if target.is_namespace => {
                let interop = !target.is_import_equals();
                match self
                    .ctx
                    .resolve_type_dependency_canonical(canonical, &target.source_specifier)
                {
                    Some(dependency) => match self.export_assignment_root(&dependency) {
                        Some((assigned, _)) => {
                            let hash = self
                                .ctx
                                .ensure_indexed_ready_serve(&dependency)?
                                .indexed
                                .whole_hash;
                            let mut object =
                                self.assigned_module_object(&assigned, None, interop, context)?;
                            push_root(&mut object.1, (Arc::from(dependency.as_str()), hash));
                            Some(object)
                        }
                        None => self.file_module_object(&dependency, context),
                    },
                    None => {
                        let blocks = self.ambient_module_blocks(&target.source_specifier);
                        match blocks
                            .iter()
                            .find(|block| block.export_assignment.is_some())
                        {
                            Some(block) => {
                                let assigned =
                                    block.root(Arc::clone(block.export_assignment.as_ref()?));
                                self.assigned_module_object(
                                    &assigned,
                                    Some(&block.scope),
                                    interop,
                                    context,
                                )
                            }
                            None => self.ambient_module_object(&blocks, context),
                        }
                    }
                }
            }
            Some(_) => None,
            None => {
                let headers = shallow.decl_bodies().header_index();
                let name = value_root.name.as_ref();
                let scope = if headers
                    .namespace_blocks
                    .iter()
                    .any(|block| block.owner == owner && block.qualified_name == name)
                {
                    None
                } else {
                    // A namespace a `declare module` block declares.
                    let (scope, _) = headers
                        .augmentation_namespace_blocks
                        .iter()
                        .find(|(_, block)| block.owner == owner && block.qualified_name == name)?;
                    Some(scope.clone())
                };
                self.namespace_object(canonical, owner, scope.as_ref(), name, context)
            }
        }
    }

    /// The object `typeof import("…")` of a file module without `export =`,
    /// and of a namespace import of it.
    pub(super) fn file_module_object(
        &self,
        module: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<ModuleObject> {
        let indexed = self.ctx.ensure_indexed_ready_serve(module)?.indexed;
        let mut roots: Vec<ObservedGraphSelfRoot> = vec![(Arc::from(module), indexed.whole_hash)];
        let mut names: Vec<String> = Vec::new();
        let mut visited: FxHashSet<String> = FxHashSet::default();
        visited.insert(module.to_owned());
        self.collect_module_export_names(module, true, &mut names, &mut visited, &mut roots);
        names.sort();
        names.dedup();
        let mut members = Vec::with_capacity(names.len());
        for name in names {
            let (resolved, route_facts) = self
                .ctx
                .resolve_imported_type_root_with_facts(module, &name);
            self.ctx.observe_borrowed_signature(&route_facts);
            let root = match resolved {
                Some(identity) => file_root(
                    identity.canonical_id.as_ref(),
                    identity.owner,
                    identity.symbol_name.as_ref(),
                ),
                None => file_root(
                    module,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    &name,
                ),
            };
            members.push((Arc::from(name.as_str()), root));
        }
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(module),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        Some((self.value_object(members, scope, module, context), roots))
    }

    /// The export names of `module`: its own exports (`default` among them
    /// only when `own`), then the names each `export * from` target exports.
    fn collect_module_export_names(
        &self,
        module: &str,
        own: bool,
        names: &mut Vec<String>,
        visited: &mut FxHashSet<String>,
        roots: &mut Vec<ObservedGraphSelfRoot>,
    ) {
        use crate::resolver_core::shallow_file_state::ExportTarget;
        let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(module)
            .map(|serve| serve.indexed)
        else {
            return;
        };
        push_root(roots, (Arc::from(module), indexed.whole_hash));
        let shallow = &indexed.shallow_state;
        for (name, target) in &shallow.exports {
            let type_only = matches!(target, ExportTarget::Reexport { is_type: true, .. });
            if type_only || (!own && name == "default") {
                continue;
            }
            names.push(name.clone());
        }
        let ordinary = verter_type_expr::TopLevelOwnerId::ordinary_file();
        for wildcard in &shallow.wildcard_reexports {
            if wildcard.owner != ordinary {
                continue;
            }
            let Some(source) = self
                .ctx
                .resolve_type_dependency_canonical(module, &wildcard.source_specifier)
            else {
                continue;
            };
            if visited.insert(source.clone()) {
                self.collect_module_export_names(&source, false, names, visited, roots);
            }
        }
    }

    /// The object an ambient module's `declare module` blocks declare: the
    /// values of each block (the first block declaring a name names it),
    /// and `default` the block's `export default` declaration.
    fn ambient_module_object(
        &self,
        blocks: &[AmbientModuleBlock],
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<ModuleObject> {
        let first = blocks.first()?;
        let mut roots: Vec<ObservedGraphSelfRoot> = Vec::new();
        let mut members: Vec<(Arc<str>, ValueRootKey)> = Vec::new();
        let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
        for block in blocks {
            let Some(indexed) = self
                .ctx
                .ensure_indexed_ready_serve(block.canonical.as_ref())
                .map(|serve| serve.indexed)
            else {
                continue;
            };
            push_root(
                &mut roots,
                (Arc::clone(&block.canonical), indexed.whole_hash),
            );
            let shallow = &indexed.shallow_state;
            let headers = shallow.decl_bodies().header_index();
            for name in headers.namespace_value_members(Some(&block.scope), block.owner, None) {
                let name: Arc<str> = Arc::from(name);
                // `export default function f` exports `f` as `default` only.
                if block.default_export.as_deref() == Some(name.as_ref())
                    || shallow.visible_value_binding(block.owner, &name).is_some()
                    || !seen.insert(Arc::clone(&name))
                {
                    continue;
                }
                members.push((Arc::clone(&name), block.root(name)));
            }
            if let Some(default) = block.default_export.as_ref() {
                if seen.insert(Arc::from("default")) {
                    members.push((Arc::from("default"), block.root(Arc::clone(default))));
                }
            }
        }
        members.sort_by(|left, right| left.0.cmp(&right.0));
        let hash = self
            .ctx
            .ensure_indexed_ready_serve(first.canonical.as_ref())?
            .indexed
            .whole_hash;
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&first.canonical),
            owner: first.owner,
            whole_hash: hash,
            local_scope: None,
        };
        Some((
            self.value_object(members, scope, first.canonical.as_ref(), context),
            roots,
        ))
    }

    /// The object of the namespace `name` declared in `canonical` (in its
    /// own scope, or in the `declare module` block `scope`); `None` when it
    /// declares no value.
    fn namespace_object(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        scope: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
        name: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<ModuleObject> {
        let indexed = self.ctx.ensure_indexed_ready_serve(canonical)?.indexed;
        let headers = indexed.shallow_state.decl_bodies().header_index();
        let names = headers.namespace_value_members(scope, owner, Some(name));
        if names.is_empty() {
            return None;
        }
        let members = names
            .into_iter()
            .map(|member| {
                let qualified = format!("{name}.{member}");
                (
                    Arc::from(member.as_str()),
                    file_root(canonical, owner, &qualified),
                )
            })
            .collect();
        let node_scope = NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            owner,
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        Some((
            self.value_object(members, node_scope, canonical, context),
            vec![(Arc::from(canonical), indexed.whole_hash)],
        ))
    }

    /// The value a module assigned with `export = X` is: X itself — or,
    /// through a namespace import (`interop`) when X can be called or
    /// constructed, the object of X's own properties (the namespace merged
    /// into X among them) and `default`, X.
    fn assigned_module_object(
        &self,
        assigned: &ValueRootKey,
        scope: Option<&verter_semantic::analysis::type_eval::AugmentationScopeKind>,
        interop: bool,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<ModuleObject> {
        let canonical = assigned.scope.canonical_id.as_ref();
        let indexed = self.ctx.ensure_indexed_ready_serve(canonical)?.indexed;
        let roots = vec![(Arc::clone(&assigned.scope.canonical_id), indexed.whole_hash)];
        let whole = match self
            .execute_read(self.typeof_key_for(assigned.clone(), context))
            .value
        {
            QueryResult::Value(node) => node,
            _ => return None,
        };
        if !interop || !self.has_call_or_construct_signatures(whole)? {
            return Some((whole, roots));
        }
        let mut members: Vec<SurfaceMember> = Vec::new();
        if let Some(SemanticNodeData::Object(surface)) = self.graph().node_data(whole).as_deref() {
            members.extend(surface.positive_members().iter().cloned());
        }
        let headers = indexed.shallow_state.decl_bodies().header_index();
        let merged: Vec<(Arc<str>, ValueRootKey)> = headers
            .namespace_value_members(scope, assigned.scope.owner, Some(assigned.name.as_ref()))
            .into_iter()
            .map(|member| {
                let qualified = format!("{}.{member}", assigned.name);
                (
                    Arc::from(member.as_str()),
                    file_root(canonical, assigned.scope.owner, &qualified),
                )
            })
            .collect();
        let mut named = self.typeof_members(merged, context);
        named.push((Arc::from("default"), whole));
        for (name, node) in named {
            let taken = members
                .iter()
                .any(|member| member.key.as_string() == Some(name.as_ref()));
            if !taken {
                members.push(Self::module_member(name, node, canonical));
            }
        }
        members.sort_by(|left, right| left.key.as_string().cmp(&right.key.as_string()));
        let node_scope = NodeScopeId::File {
            canonical_id: Arc::clone(&assigned.scope.canonical_id),
            owner: assigned.scope.owner,
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let object = self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView::from_members(members, None)),
            node_scope,
        );
        Some((object, roots))
    }

    /// Whether `node` has a call or a construct signature
    /// (`SignaturesOfType`); `None` when that does not settle.
    fn has_call_or_construct_signatures(&self, node: SemanticNodeId) -> Option<bool> {
        for kind in [GraphSignatureKind::Call, GraphSignatureKind::Construct] {
            match self
                .execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
                    subject: node,
                    kind,
                    context: SemanticContextId::production(),
                })
                .value
            {
                QueryResult::Value(SemanticQueryValue::SignatureSet(set)) => {
                    if !set.nodes.is_empty() {
                        return Some(true);
                    }
                }
                _ => return None,
            }
        }
        Some(false)
    }

    /// Each named root's `typeof`; a root with no value (a type-only
    /// export) is no member.
    fn typeof_members(
        &self,
        members: Vec<(Arc<str>, ValueRootKey)>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Vec<(Arc<str>, SemanticNodeId)> {
        members
            .into_iter()
            .filter_map(|(name, root)| {
                let node = match self.execute_read(self.typeof_key_for(root, context)).value {
                    QueryResult::Value(node) => node,
                    _ => return None,
                };
                (!matches!(
                    self.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Opaque(_)) | None
                ))
                .then_some((name, node))
            })
            .collect()
    }

    /// The object whose properties are the named roots' values, in name
    /// order.
    fn value_object(
        &self,
        members: Vec<(Arc<str>, ValueRootKey)>,
        scope: NodeScopeId,
        origin: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let members = self
            .typeof_members(members, context)
            .into_iter()
            .map(|(name, node)| Self::module_member(name, node, origin))
            .collect();
        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView::from_members(members, None)),
            scope,
        )
    }

    fn module_member(name: Arc<str>, value: SemanticNodeId, origin: &str) -> SurfaceMember {
        SurfaceMember {
            key: crate::semantic_query::AuthoredPropertyKey::String(name),
            value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            // A module's property is never an object literal's member.
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            // It has no one declaration site; its declaration lives in the
            // module.
            spans: verter_type_expr::MemberSpans::default(),
            declaration_origin: Some(Arc::from(origin)),
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        }
    }
}
