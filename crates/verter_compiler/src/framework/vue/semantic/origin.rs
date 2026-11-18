//! Component-local proof that a name is a framework API.
//!
//! The evidence is the component's script origin view: literal imports from
//! the framework module, namespace imports and member access on them,
//! destructuring of a namespace, and `const` alias chains. This module only
//! says which of those origins are framework APIs. It walks no syntax: every
//! origin comes from [`ScriptOriginView`], every API spelling from
//! [`classify_vue_api`], and the framework-module test from
//! [`is_vue_source`].

use verter_semantic::analysis::{classify_vue_api, is_vue_source};
use verter_semantic::compiler_view::{CalleeRef, OriginStep, ResolvedOrigin, ScriptOriginView};
use verter_session_query::analysis::types::VueApiClassification;

/// A framework API whose call result has a known reactivity class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameworkApi {
    Ref,
    ShallowRef,
    CustomRef,
    ToRef,
    Computed,
    Reactive,
    ShallowReactive,
    Readonly,
    ShallowReadonly,
    ToRefs,
    UseTemplateRef,
    UseModel,
}

impl FrameworkApi {
    /// The API an export name spells, read from [`classify_vue_api`].
    fn of_export(name: &str) -> Option<Self> {
        Some(match classify_vue_api(name) {
            VueApiClassification::Ref => Self::Ref,
            VueApiClassification::ShallowRef => Self::ShallowRef,
            VueApiClassification::CustomRef => Self::CustomRef,
            VueApiClassification::ToRef => Self::ToRef,
            VueApiClassification::Computed => Self::Computed,
            VueApiClassification::Reactive => Self::Reactive,
            VueApiClassification::ShallowReactive => Self::ShallowReactive,
            VueApiClassification::Readonly => Self::Readonly,
            VueApiClassification::ShallowReadonly => Self::ShallowReadonly,
            VueApiClassification::ToRefs => Self::ToRefs,
            VueApiClassification::UseTemplateRef => Self::UseTemplateRef,
            VueApiClassification::UseModel => Self::UseModel,
            _ => return None,
        })
    }
}

/// How the proof was obtained. Sparse; produced only on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginEvidence {
    /// `import { ref } from 'vue'` or `import { ref as r } from 'vue'`.
    NamedImport,
    /// `import * as Vue from 'vue'` then `Vue.ref`.
    NamespaceMember,
    /// `const { ref } = Vue`.
    NamespaceDestructure,
    /// `const r = ref` where `ref` is itself proven.
    ConstAlias,
}

impl From<OriginStep> for OriginEvidence {
    fn from(step: OriginStep) -> Self {
        match step {
            OriginStep::NamedImport => Self::NamedImport,
            OriginStep::NamespaceMember => Self::NamespaceMember,
            OriginStep::NamespaceDestructure => Self::NamespaceDestructure,
            OriginStep::ConstAlias => Self::ConstAlias,
        }
    }
}

/// What a local name is proven to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkOrigin {
    /// Proven to be this framework API.
    Api(FrameworkApi, OriginEvidence),
    /// Proven to be the framework module namespace.
    Namespace,
    /// Declared locally with a framework API's spelling and not proven to
    /// be it: a user function, a parameter, a mutable alias.
    Shadowed,
    /// No evidence either way.
    Unknown,
}

/// The framework origins of one component's top-level names, answered on
/// demand from its script origin view.
#[derive(Debug, Clone, Default)]
pub struct FrameworkOriginTable {
    view: ScriptOriginView,
    runtime_module: Box<str>,
}

impl FrameworkOriginTable {
    /// What the top-level name `name` is proven to be.
    pub fn origin(&self, name: &str) -> FrameworkOrigin {
        self.classify(self.view.origin_of(name), Some(name))
    }

    /// What a call's callee is proven to be. A bare callee is a top-level
    /// name; a member callee is proven only through a framework namespace.
    pub fn callee_origin(&self, callee: CalleeRef<'_>) -> FrameworkOrigin {
        let local = match callee {
            CalleeRef::Name(name) => Some(name),
            CalleeRef::Member { .. } => None,
        };
        self.classify(self.view.callee_origin(callee), local)
    }

    /// The API a call's callee text resolves to: a bare name or a
    /// `namespace.member` access.
    pub fn callee_api(&self, callee: &str) -> Option<FrameworkApi> {
        let callee = match callee.split_once('.') {
            None => CalleeRef::Name(callee),
            Some((object, property)) if !property.contains('.') => {
                CalleeRef::Member { object, property }
            }
            Some(_) => return None,
        };
        match self.callee_origin(callee) {
            FrameworkOrigin::Api(api, _) => Some(api),
            _ => None,
        }
    }

    /// Where the callee of `binding`'s initializer comes from, when the
    /// initializer is a call: the module specifier and imported name of an
    /// import (followed through namespaces and `const` aliases), a local
    /// declaration, or nothing declared. Provenance only: no reactivity class
    /// is derived from it here.
    pub fn callee_provenance(&self, binding: &str) -> Option<ResolvedOrigin<'_>> {
        let callee = self.view.initializer_of(binding)?.callee?;
        Some(self.view.callee_origin(callee))
    }

    /// The script origin view the table answers from.
    pub fn view(&self) -> &ScriptOriginView {
        &self.view
    }

    /// Number of top-level names the view records.
    pub fn len(&self) -> usize {
        self.view.len()
    }

    pub fn is_empty(&self) -> bool {
        self.view.is_empty()
    }

    fn is_runtime_module(&self, source: &str) -> bool {
        source == &*self.runtime_module || is_vue_source(source)
    }

    /// Map a proven origin to a framework origin. `local` is the name the
    /// origin was asked for, when it is a top-level name: declared under an
    /// API's spelling and not proven to be that API, it is shadowed.
    fn classify(&self, origin: ResolvedOrigin<'_>, local: Option<&str>) -> FrameworkOrigin {
        match origin {
            ResolvedOrigin::Export {
                source,
                imported,
                step,
            } if self.is_runtime_module(source) => {
                if let Some(api) = FrameworkApi::of_export(imported) {
                    return FrameworkOrigin::Api(api, step.into());
                }
            }
            ResolvedOrigin::Namespace { source } if self.is_runtime_module(source) => {
                return FrameworkOrigin::Namespace;
            }
            ResolvedOrigin::Undeclared => return FrameworkOrigin::Unknown,
            ResolvedOrigin::Export { .. }
            | ResolvedOrigin::Namespace { .. }
            | ResolvedOrigin::Local => {}
        }
        if local.is_some_and(|name| FrameworkApi::of_export(name).is_some()) {
            FrameworkOrigin::Shadowed
        } else {
            FrameworkOrigin::Unknown
        }
    }
}

/// Build the table from the component's origin view.
/// `runtime_module` is the framework module specifier (`vue` by default);
/// Vue's own runtime entries count as it too.
pub fn build_origin_table(view: ScriptOriginView, runtime_module: &str) -> FrameworkOriginTable {
    FrameworkOriginTable {
        view,
        runtime_module: runtime_module.into(),
    }
}

#[cfg(test)]
#[path = "origin_tests.rs"]
mod origin_tests;
