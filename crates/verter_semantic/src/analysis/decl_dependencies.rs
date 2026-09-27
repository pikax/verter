//! Demand-scoped declaration dependency facts.
//!
//! Declaration names, contributor identity, and body dependency classification
//! are semantic lowering responsibilities. This module walks only the demanded
//! OXC statement/type payload and emits owned, span-free structural paths; it
//! performs no name or cross-file resolution.

use std::collections::BTreeSet;
use std::sync::Arc;

use oxc_ast::ast::*;
use verter_type_expr::facts::TypeDependencyPathFact;
use verter_type_expr::{DeclBindingKey, TopLevelOwnerId};

/// Owner-qualified declaration path. The lexical root and namespace members
/// remain separate structural segments; consumers never split dotted text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct DeclarationPath {
    pub root: DeclBindingKey,
    members: Arc<[String]>,
}

impl<'de> serde::Deserialize<'de> for DeclarationPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Wire {
            root: DeclBindingKey,
            members: Vec<String>,
        }

        let wire = <Wire as serde::Deserialize>::deserialize(deserializer)?;
        if wire.root.name.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "declaration root name must not be empty or whitespace-only",
            ));
        }
        if wire.members.iter().any(|member| member.trim().is_empty()) {
            return Err(serde::de::Error::custom(
                "declaration member path must not contain empty or whitespace-only segments",
            ));
        }
        Ok(Self {
            root: wire.root,
            members: Arc::from(wire.members.into_boxed_slice()),
        })
    }
}

impl DeclarationPath {
    #[must_use]
    pub fn new<I, S>(root: DeclBindingKey, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            root,
            members: Arc::from(
                members
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    #[must_use]
    pub fn root(root: DeclBindingKey) -> Self {
        Self::new(root, std::iter::empty::<String>())
    }

    #[must_use]
    pub fn from_dependency(owner: TopLevelOwnerId, path: &TypeDependencyPathFact) -> Self {
        Self::new(
            DeclBindingKey::new(owner, path.root()),
            path.member_path().iter().cloned(),
        )
    }

    #[must_use]
    pub fn member_path(&self) -> &[String] {
        &self.members
    }

    /// Canonical declaration key used by the current lowering environment.
    /// Namespace segments are joined from their structural representation;
    /// consumers never recover structure by splitting dotted text.
    #[must_use]
    pub fn qualified_key(&self) -> DeclBindingKey {
        if self.members.is_empty() {
            return self.root.clone();
        }
        let capacity = self.root.name.len()
            + self
                .members
                .iter()
                .map(|member| member.len() + 1)
                .sum::<usize>();
        let mut name = String::with_capacity(capacity);
        name.push_str(&self.root.name);
        for member in self.members.iter() {
            name.push('.');
            name.push_str(member);
        }
        DeclBindingKey::new(self.root.owner, name)
    }

    #[must_use]
    pub fn appended(&self, member: impl Into<String>) -> Self {
        let mut members = self.members.to_vec();
        members.push(member.into());
        Self::new(self.root.clone(), members)
    }
}

pub use verter_type_expr_oxc::UnsupportedValuePositionKind;

/// The reference-name pair one declaration's BODY contributes: the plain
/// dependency names plus the structural subset. This is the per-statement
/// demand product the lazy declaration-body path consumes — computed for
/// exactly the demanded declaration, never for every symbol in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclDependencyNames {
    /// Semantic-owned segment identities. Consumers classify the local binding
    /// from `segments[0]` and preserve the remaining member path separately;
    /// no downstream string splitting is permitted.
    pub dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub structural_dependency_paths: BTreeSet<TypeDependencyPathFact>,
    /// Complete declaration-carrier dependencies, including return types,
    /// generic bounds/defaults, constructors, and static members.
    pub declaration_carrier_paths: BTreeSet<TypeDependencyPathFact>,
    /// Runtime value roles carried by declaration syntax.
    pub value_query_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_position_paths: BTreeSet<TypeDependencyPathFact>,
    pub unsupported_value_positions: BTreeSet<UnsupportedValuePositionKind>,
}

impl From<verter_type_expr_oxc::TypeDependencyFacts> for DeclDependencyNames {
    fn from(facts: verter_type_expr_oxc::TypeDependencyFacts) -> Self {
        Self {
            dependency_paths: facts.dependency_paths,
            structural_dependency_paths: facts.structural_dependency_paths,
            declaration_carrier_paths: facts.declaration_carrier_paths,
            value_query_paths: facts.value_query_paths,
            value_position_paths: facts.value_position_paths,
            unsupported_value_positions: facts.unsupported_value_positions,
        }
    }
}

/// Collect the `(declared name, body reference names)` pairs ONE top-level
/// statement contributes, keyed under exactly the env-symbol names the
/// LOWERER (`lower_top_level_statement`) registers — the names the lazy
/// declaration-body memo iterates. This is NOT the analyzer's keying: the
/// default-exported class / interface arms emit deps under BOTH the
/// declared name AND the `default` alias (mirroring
/// `alias_default_export_type_symbol`), whereas the analyzer keys a
/// default export under `"default"` only. Covers type aliases, interfaces,
/// named classes, their `export` wrappers, identifier-namespace inner
/// declarations (under qualified `Ns.Name` keys), and the default export.
/// A statement that declares no type-space symbol yields an empty vector.
pub fn collect_statement_dependency_names(
    stmt: &Statement<'_>,
    owner: TopLevelOwnerId,
) -> Vec<(DeclarationPath, DeclDependencyNames)> {
    match stmt {
        Statement::TSTypeAliasDeclaration(type_alias) => vec![(
            DeclarationPath::root(DeclBindingKey::new(owner, type_alias.id.name.as_str())),
            type_alias_dependency_names(type_alias),
        )],
        Statement::TSInterfaceDeclaration(interface) => vec![(
            DeclarationPath::root(DeclBindingKey::new(owner, interface.id.name.as_str())),
            interface_dependency_names(interface),
        )],
        Statement::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| {
                vec![(
                    DeclarationPath::root(DeclBindingKey::new(owner, id.name.as_str())),
                    class_dependency_names(class_decl),
                )]
            })
            .unwrap_or_default(),
        Statement::TSNamespaceDeclaration(module) => {
            // An identifier namespace registers its inner type declarations
            // under qualified `Ns.Name` keys (matching `lower_top_level_
            // statement`'s `extract_module_declaration`); a string-literal
            // ambient module is an augmentation scope whose dep edges ride
            // on the per-contributor `FileWholeHash` rail — nothing to
            // collect here.
            let mut out = Vec::new();
            collect_module_dependencies(module, owner, &mut out);
            out
        }
        Statement::ExportDeclaration(export) => {
            collect_declaration_dependencies(&export.declaration, owner)
        }
        Statement::ExportDefaultDeclaration(export_default) => match &export_default.declaration {
            // The default class lowers under BOTH its declared name and the
            // `default` alias (see `alias_default_export_type_symbol`), so
            // emit the heritage deps under both keys — an anonymous default
            // class has no declared name, only `default`.
            ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                let deps = class_dependency_names(class_decl);
                match &class_decl.id {
                    Some(id) => vec![
                        (
                            DeclarationPath::root(DeclBindingKey::new(owner, id.name.as_str())),
                            deps.clone(),
                        ),
                        (
                            DeclarationPath::root(DeclBindingKey::new(owner, "default")),
                            deps,
                        ),
                    ],
                    None => vec![(
                        DeclarationPath::root(DeclBindingKey::new(owner, "default")),
                        deps,
                    )],
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                let deps = interface_dependency_names(interface);
                vec![
                    (
                        DeclarationPath::root(DeclBindingKey::new(
                            owner,
                            interface.id.name.as_str(),
                        )),
                        deps.clone(),
                    ),
                    (
                        DeclarationPath::root(DeclBindingKey::new(owner, "default")),
                        deps,
                    ),
                ]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Collect `(qualified name, body reference names)` pairs for an
/// identifier namespace's inner type declarations, mirroring
/// `extract_module_declaration` / `extract_namespaced_statement` so the
/// dep-record keys match the `Ns.Name` keys the env walk lowers under.
/// A string-literal ambient module (augmentation scope) contributes
/// nothing here. The namespace and every namespace nested in it are walked
/// from an explicit stack
/// ([`crate::analysis::namespace_walk::walk_namespaces`]).
fn collect_module_dependencies(
    module: &TSNamespaceDeclaration<'_>,
    owner: TopLevelOwnerId,
    out: &mut Vec<(DeclarationPath, DeclDependencyNames)>,
) {
    crate::analysis::namespace_walk::walk_namespaces(
        module,
        &mut ModuleDependencies {
            owner,
            members: Vec::new(),
            out,
        },
    );
}

/// [`collect_module_dependencies`]'s walk: the namespace path it is in,
/// the root's name first, grown and cut back as it enters and leaves a
/// namespace.
struct ModuleDependencies<'o> {
    owner: TopLevelOwnerId,
    members: Vec<String>,
    out: &'o mut Vec<(DeclarationPath, DeclDependencyNames)>,
}

impl ModuleDependencies<'_> {
    /// The path of the namespace member `name`.
    fn member(&self, name: &str) -> DeclarationPath {
        let (root, members) = self
            .members
            .split_first()
            .expect("a member sits in a namespace");
        DeclarationPath::new(
            DeclBindingKey::new(self.owner, root.as_str()),
            members
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(name)),
        )
    }
}

impl<'s, 'a> crate::analysis::namespace_walk::NamespaceVisitor<'s, 'a> for ModuleDependencies<'_> {
    type Frame = ();

    fn enter(
        &mut self,
        decl: &'s TSNamespaceDeclaration<'a>,
        _parent: Option<&()>,
        _nesting: crate::analysis::namespace_walk::Nesting,
    ) {
        self.members.push(decl.id.name.to_string());
    }

    fn statement(&mut self, _frame: &mut (), statement: &'s Statement<'a>) {
        let entry = match statement {
            Statement::TSTypeAliasDeclaration(alias) => Some((
                self.member(alias.id.name.as_str()),
                type_alias_dependency_names(alias),
            )),
            Statement::TSInterfaceDeclaration(interface) => Some((
                self.member(interface.id.name.as_str()),
                interface_dependency_names(interface),
            )),
            Statement::ClassDeclaration(class) => class
                .id
                .as_ref()
                .map(|id| (self.member(id.name.as_str()), class_dependency_names(class))),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::TSTypeAliasDeclaration(alias) => Some((
                    self.member(alias.id.name.as_str()),
                    type_alias_dependency_names(alias),
                )),
                Declaration::TSInterfaceDeclaration(interface) => Some((
                    self.member(interface.id.name.as_str()),
                    interface_dependency_names(interface),
                )),
                Declaration::ClassDeclaration(class) => class
                    .id
                    .as_ref()
                    .map(|id| (self.member(id.name.as_str()), class_dependency_names(class))),
                _ => None,
            },
            _ => None,
        };
        self.out.extend(entry);
    }

    fn exit(&mut self, _frame: (), _parent: Option<&mut ()>) {
        self.members.pop();
    }
}

fn collect_declaration_dependencies(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
) -> Vec<(DeclarationPath, DeclDependencyNames)> {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => vec![(
            DeclarationPath::root(DeclBindingKey::new(owner, type_alias.id.name.as_str())),
            type_alias_dependency_names(type_alias),
        )],
        Declaration::TSInterfaceDeclaration(interface) => vec![(
            DeclarationPath::root(DeclBindingKey::new(owner, interface.id.name.as_str())),
            interface_dependency_names(interface),
        )],
        Declaration::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| {
                vec![(
                    DeclarationPath::root(DeclBindingKey::new(owner, id.name.as_str())),
                    class_dependency_names(class_decl),
                )]
            })
            .unwrap_or_default(),
        Declaration::TSNamespaceDeclaration(module) => {
            // `export namespace N { … }` — collect its inner type
            // declarations under qualified `N.Name` keys.
            let mut out = Vec::new();
            collect_module_dependencies(module, owner, &mut out);
            out
        }
        _ => Vec::new(),
    }
}

fn type_alias_dependency_names(declaration: &TSTypeAliasDeclaration<'_>) -> DeclDependencyNames {
    verter_type_expr_oxc::collect_type_alias_dependency_facts(declaration).into()
}

/// Collect typed dependency facts for a standalone authored type payload.
///
/// This is also the JSDoc synthetic-alias bridge: callers invoke it while the
/// wrapper OXC arena is alive, before type lowering erases qualified segments.
#[must_use]
pub fn collect_type_dependency_facts(ts_type: &TSType<'_>) -> DeclDependencyNames {
    verter_type_expr_oxc::collect_type_dependency_facts(ts_type).into()
}

fn interface_dependency_names(declaration: &TSInterfaceDeclaration<'_>) -> DeclDependencyNames {
    verter_type_expr_oxc::collect_interface_dependency_facts(declaration).into()
}

fn class_dependency_names(declaration: &Class<'_>) -> DeclDependencyNames {
    verter_type_expr_oxc::collect_class_dependency_facts(declaration).into()
}
