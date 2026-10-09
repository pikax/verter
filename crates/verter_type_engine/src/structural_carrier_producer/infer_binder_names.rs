//! Producer-private lexical binder context and canonical conditional-`infer`
//! syntax identities.

use std::{cell::Cell, sync::Arc};

use rustc_hash::FxHashMap;

use crate::semantic_query::{
    InferBinderFactory, MacroOwnBodyStamp, MergeRoleStamp, SemanticNodeId,
};

pub(super) use crate::semantic_query::infer_binder_names::{
    collect_extends_infer_declarations, InferSyntaxPathStep,
};

/// One lexical frame of type-parameter and conditional-`infer` bindings.
#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct BinderScope {
    names: FxHashMap<Arc<str>, SemanticNodeId>,
    infer_declarations: FxHashMap<Arc<str>, SemanticNodeId>,
}

impl BinderScope {
    /// Test-support binding of a type-parameter name to its binder node.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn bind_for_tests(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.bind(name, node);
    }

    /// Bind a syntactic type-parameter name to its interned binder node.
    pub(super) fn bind(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.names.insert(name, node);
    }

    /// Predeclare a literal conditional-`infer` declaration.
    pub(super) fn bind_infer_declaration(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.infer_declarations.insert(name, node);
    }

    fn lookup(&self, name: &str) -> Option<SemanticNodeId> {
        self.names.get(name).copied()
    }

    fn lookup_infer_declaration(&self, name: &str) -> Option<SemanticNodeId> {
        self.infer_declarations.get(name).copied()
    }
}

/// Syntactic binder and provenance inputs to structural lowering.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct StructuralLowerContext<'a> {
    pub(super) binders: &'a [BinderScope],
    pub(super) merge_role: MergeRoleStamp,
    pub(super) macro_own_body: MacroOwnBodyStamp,
    mapper_ordinals: Option<&'a Cell<u16>>,
    pub(super) infer_binders: Option<&'a InferBinderFactory>,
    pub(super) infer_source: Option<&'a verter_type_expr::locators::AuthoredBodyLocator>,
}

impl<'a> StructuralLowerContext<'a> {
    /// Test-support construction of a root context with neutral provenance.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn new_for_tests(binders: &'a [BinderScope]) -> Self {
        Self::new(binders)
    }

    /// Construct a root context with neutral provenance.
    pub(super) fn new(binders: &'a [BinderScope]) -> Self {
        Self {
            binders,
            merge_role: MergeRoleStamp::NEUTRAL,
            macro_own_body: MacroOwnBodyStamp::NEUTRAL,
            mapper_ordinals: None,
            infer_binders: None,
            infer_source: None,
        }
    }

    pub(super) fn with_mapper_ordinals(mut self, ordinals: &'a Cell<u16>) -> Self {
        self.mapper_ordinals = Some(ordinals);
        self
    }

    pub(super) fn with_infer_binders(mut self, infer_binders: &'a InferBinderFactory) -> Self {
        self.infer_binders = Some(infer_binders);
        self
    }

    pub(super) fn with_infer_source(
        mut self,
        source: &'a verter_type_expr::locators::AuthoredBodyLocator,
    ) -> Self {
        self.infer_source = Some(source);
        self
    }

    pub(super) fn next_mapper_ordinal(&self) -> u16 {
        match self.mapper_ordinals {
            Some(cell) => {
                let next = cell.get();
                cell.set(next.saturating_add(1));
                next
            }
            None => 0,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_merge_role(mut self, merge_role: MergeRoleStamp) -> Self {
        self.merge_role = merge_role;
        self
    }

    pub(super) fn with_macro_own_body(mut self, macro_own_body: MacroOwnBodyStamp) -> Self {
        self.macro_own_body = macro_own_body;
        self
    }

    pub(super) fn with_binders<'b>(&self, binders: &'b [BinderScope]) -> StructuralLowerContext<'b>
    where
        'a: 'b,
    {
        StructuralLowerContext {
            binders,
            merge_role: self.merge_role,
            macro_own_body: self.macro_own_body,
            mapper_ordinals: self.mapper_ordinals,
            infer_binders: self.infer_binders,
            infer_source: self.infer_source,
        }
    }

    pub(super) fn structural_provenance(&self) -> Self {
        Self {
            binders: self.binders,
            merge_role: self.merge_role,
            macro_own_body: MacroOwnBodyStamp::NEUTRAL,
            mapper_ordinals: self.mapper_ordinals,
            infer_binders: self.infer_binders,
            infer_source: self.infer_source,
        }
    }

    /// Test entry to the innermost-first binder lookup.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn lookup_binder_for_tests(&self, name: &str) -> Option<SemanticNodeId> {
        self.lookup_binder(name)
    }

    pub(super) fn lookup_binder(&self, name: &str) -> Option<SemanticNodeId> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup(name))
    }

    pub(super) fn lookup_infer_declaration(&self, name: &str) -> Option<SemanticNodeId> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup_infer_declaration(name))
    }
}
