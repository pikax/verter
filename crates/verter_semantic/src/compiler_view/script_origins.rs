//! Where a component's top-level names come from: module imports, namespace
//! imports, and immutable aliases of either.
//!
//! The view records provenance only. It reads the script analysis of the
//! companion `<script>` and then of the `<script setup>`, which share one
//! module scope, and never walks a program itself:
//!
//! 1. the value imports, hoisted, so every alias below may use them; each
//!    keeps its source specifier and imported name;
//! 2. the top-level bindings in source order: `const b = a` where `a` is
//!    already proven, and `const { a, b: c } = ns` where `ns` is a proven
//!    namespace;
//! 3. every other recorded binding, which is [`ResolvedOrigin::Local`].
//!
//! A name declared more than once at the top level is never proven: which
//! declaration wins is the parser's error to report, not a fact to guess.
//! The rules follow `@vue/compiler-sfc`'s `walkDeclaration` and `isCallOf`
//! over `userImports`: only syntax proves an origin, and a mutable binding
//! never does.
//!
//! A name the analysis does not record as a top-level binding (an exported
//! declaration, an enum, a namespace, `import x = require(..)`) is
//! [`ResolvedOrigin::Undeclared`]: it proves nothing.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot;
use verter_session_query::analysis::types::{
    AnalyzedBinding, AnalyzedBindingKind, AnalyzedImportBinding, BindingInitializer,
    ImportBindingKind,
};

/// The last step of the proof that a name is a module export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginStep {
    /// `import { a } from 'm'` or `import { a as b } from 'm'`.
    NamedImport,
    /// `ns.a` where `ns` is a namespace import of `m`.
    NamespaceMember,
    /// `const { a } = ns`.
    NamespaceDestructure,
    /// `const b = a` where `a` is itself proven.
    ConstAlias,
}

/// What a top-level name, or a callee, is proven to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedOrigin<'v> {
    /// The export `imported` of module `source`.
    Export {
        source: &'v str,
        imported: &'v str,
        step: OriginStep,
    },
    /// The namespace object of module `source`.
    Namespace { source: &'v str },
    /// Declared at the top level and not proven to be an import or an alias
    /// of one: a function, a class, a `let` or `var`, or any other `const`.
    Local,
    /// Not recorded as a top-level name.
    Undeclared,
}

/// A call's callee, as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalleeRef<'a> {
    Name(&'a str),
    Member { object: &'a str, property: &'a str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Const,
    Let,
    Var,
}

/// How a top-level variable is declared and, when its whole initializer is
/// a call, what it calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitializerFact<'v> {
    pub kind: DeclarationKind,
    pub callee: Option<CalleeRef<'v>>,
}

/// The origin facts of one component's script blocks.
#[derive(Debug, Clone, Default)]
pub struct ScriptOriginView {
    names: FxHashMap<Box<str>, NameFacts>,
}

#[derive(Debug, Clone)]
struct NameFacts {
    origin: StoredOrigin,
    initializer: Option<StoredInitializer>,
}

#[derive(Debug, Clone)]
enum StoredOrigin {
    Export {
        source: Box<str>,
        imported: Box<str>,
        step: OriginStep,
    },
    Namespace {
        source: Box<str>,
    },
    Local,
}

impl StoredOrigin {
    fn resolve(&self) -> ResolvedOrigin<'_> {
        match self {
            StoredOrigin::Export {
                source,
                imported,
                step,
            } => ResolvedOrigin::Export {
                source,
                imported,
                step: *step,
            },
            StoredOrigin::Namespace { source } => ResolvedOrigin::Namespace { source },
            StoredOrigin::Local => ResolvedOrigin::Local,
        }
    }

    /// The origin of `const alias = <this name>`, when this name is proven.
    fn aliased(&self) -> Option<StoredOrigin> {
        match self {
            StoredOrigin::Export {
                source, imported, ..
            } => Some(StoredOrigin::Export {
                source: source.clone(),
                imported: imported.clone(),
                step: OriginStep::ConstAlias,
            }),
            StoredOrigin::Namespace { source } => Some(StoredOrigin::Namespace {
                source: source.clone(),
            }),
            StoredOrigin::Local => None,
        }
    }
}

#[derive(Debug, Clone)]
struct StoredInitializer {
    kind: DeclarationKind,
    callee: Option<StoredCallee>,
}

#[derive(Debug, Clone)]
enum StoredCallee {
    Name(Box<str>),
    Member {
        object: Box<str>,
        property: Box<str>,
    },
}

impl StoredCallee {
    fn borrowed(&self) -> CalleeRef<'_> {
        match self {
            StoredCallee::Name(name) => CalleeRef::Name(name),
            StoredCallee::Member { object, property } => CalleeRef::Member { object, property },
        }
    }
}

impl ScriptOriginView {
    /// Build from the script analysis of the component's `<script setup>`
    /// and companion `<script>`. An alias is followed only when it is
    /// declared before its use, in source order.
    pub fn build(
        setup: Option<&ScriptAnalysisSnapshot>,
        companion: Option<&ScriptAnalysisSnapshot>,
    ) -> Self {
        let snapshots: Vec<&ScriptAnalysisSnapshot> =
            [companion, setup].into_iter().flatten().collect();

        // The same import written in both script blocks is one declaration:
        // the compiled component keeps one copy of it.
        let mut imports = Vec::new();
        let mut distinct_imports = FxHashSet::default();
        for import in snapshots.iter().flat_map(|snapshot| &snapshot.imports) {
            for binding in import
                .bindings
                .iter()
                .filter(|binding| !binding.is_type_only)
            {
                let key = (
                    import.source.as_str(),
                    binding.name.as_str(),
                    binding.imported_name.as_deref(),
                );
                if distinct_imports.insert(key) {
                    imports.push((import.source.as_str(), binding));
                }
            }
        }
        let bindings = || snapshots.iter().flat_map(|snapshot| &snapshot.bindings);

        let mut declared = FxHashSet::default();
        let mut ambiguous = FxHashSet::default();
        let names = imports
            .iter()
            .map(|(_, binding)| binding.name.as_str())
            .chain(bindings().map(|binding| binding.name.as_str()));
        for name in names {
            if !declared.insert(name) {
                ambiguous.insert(name);
            }
        }

        let mut builder = Builder {
            names: FxHashMap::default(),
            ambiguous,
        };
        for (source, binding) in &imports {
            builder.record_import(source, binding);
        }
        for binding in bindings() {
            builder.record_binding(binding);
        }
        Self {
            names: builder.names,
        }
    }

    pub fn origin_of(&self, local: &str) -> ResolvedOrigin<'_> {
        self.names
            .get(local)
            .map_or(ResolvedOrigin::Undeclared, |facts| facts.origin.resolve())
    }

    /// The origin of a callee: a bare name resolves like [`Self::origin_of`];
    /// `ns.member` is the export `member` of the module when `ns` is a
    /// proven namespace, and otherwise the origin of `ns` itself is not an
    /// export proof, so it resolves to [`ResolvedOrigin::Local`] when `ns`
    /// is declared and [`ResolvedOrigin::Undeclared`] when it is not.
    pub fn callee_origin<'v>(&'v self, callee: CalleeRef<'v>) -> ResolvedOrigin<'v> {
        match callee {
            CalleeRef::Name(name) => self.origin_of(name),
            CalleeRef::Member { object, property } => match self.origin_of(object) {
                ResolvedOrigin::Namespace { source } => ResolvedOrigin::Export {
                    source,
                    imported: property,
                    step: OriginStep::NamespaceMember,
                },
                ResolvedOrigin::Undeclared => ResolvedOrigin::Undeclared,
                ResolvedOrigin::Export { .. } | ResolvedOrigin::Local => ResolvedOrigin::Local,
            },
        }
    }

    pub fn initializer_of(&self, local: &str) -> Option<InitializerFact<'_>> {
        let initializer = self.names.get(local)?.initializer.as_ref()?;
        Some(InitializerFact {
            kind: initializer.kind,
            callee: initializer.callee.as_ref().map(StoredCallee::borrowed),
        })
    }

    /// Number of top-level names recorded.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

struct Builder<'s> {
    names: FxHashMap<Box<str>, NameFacts>,
    /// Names declared more than once at the top level.
    ambiguous: FxHashSet<&'s str>,
}

impl Builder<'_> {
    fn record_import(&mut self, source: &str, binding: &AnalyzedImportBinding) {
        let source = source.into();
        let origin = match (binding.kind, &binding.imported_name) {
            (ImportBindingKind::Namespace, _) => StoredOrigin::Namespace { source },
            (ImportBindingKind::Named | ImportBindingKind::Default, Some(imported)) => {
                StoredOrigin::Export {
                    source,
                    imported: imported.as_str().into(),
                    step: OriginStep::NamedImport,
                }
            }
            (ImportBindingKind::Named | ImportBindingKind::Default, None) => StoredOrigin::Local,
        };
        self.insert(&binding.name, origin, None);
    }

    fn record_binding(&mut self, binding: &AnalyzedBinding) {
        let kind = match binding.kind {
            AnalyzedBindingKind::Const => DeclarationKind::Const,
            AnalyzedBindingKind::Let => DeclarationKind::Let,
            AnalyzedBindingKind::Var => DeclarationKind::Var,
            AnalyzedBindingKind::Function
            | AnalyzedBindingKind::AsyncFunction
            | AnalyzedBindingKind::Class => {
                self.insert(&binding.name, StoredOrigin::Local, None);
                return;
            }
        };
        // A destructured name is a member of the call's result, not the
        // call's value, so only a whole-result binding records its callee.
        let callee = match &binding.initializer {
            Some(BindingInitializer::FunctionCall {
                callee,
                binds_whole_call_result: true,
                ..
            }) => Some(StoredCallee::Name(callee.as_str().into())),
            Some(BindingInitializer::MemberCall {
                object,
                property,
                binds_whole_call_result: true,
            }) => Some(StoredCallee::Member {
                object: object.as_str().into(),
                property: property.as_str().into(),
            }),
            _ => None,
        };
        let origin = match &binding.initializer {
            Some(BindingInitializer::Reference { name }) if kind == DeclarationKind::Const => {
                self.proven(name).and_then(StoredOrigin::aliased)
            }
            Some(BindingInitializer::MemberReference { object, property })
                if kind == DeclarationKind::Const =>
            {
                self.namespace_source(object)
                    .map(|source| StoredOrigin::Export {
                        source,
                        imported: property.as_str().into(),
                        step: OriginStep::NamespaceDestructure,
                    })
            }
            _ => None,
        };
        self.insert(
            &binding.name,
            origin.unwrap_or(StoredOrigin::Local),
            Some(StoredInitializer { kind, callee }),
        );
    }

    fn namespace_source(&self, name: &str) -> Option<Box<str>> {
        match self.proven(name)? {
            StoredOrigin::Namespace { source } => Some(source.clone()),
            StoredOrigin::Export { .. } | StoredOrigin::Local => None,
        }
    }

    fn proven(&self, name: &str) -> Option<&StoredOrigin> {
        if self.ambiguous.contains(name) {
            return None;
        }
        self.names.get(name).map(|facts| &facts.origin)
    }

    /// Record the facts of `name`'s first declaration. A name declared more
    /// than once is only [`StoredOrigin::Local`].
    fn insert(&mut self, name: &str, origin: StoredOrigin, initializer: Option<StoredInitializer>) {
        let facts = if self.ambiguous.contains(name) {
            NameFacts {
                origin: StoredOrigin::Local,
                initializer: None,
            }
        } else {
            NameFacts {
                origin,
                initializer,
            }
        };
        self.names.entry(name.into()).or_insert(facts);
    }
}

#[cfg(test)]
#[path = "script_origins_tests.rs"]
mod tests;
