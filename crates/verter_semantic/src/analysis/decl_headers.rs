//! Shallow declaration-header index — the parse-time symbol inventory.
//!
//! [`build_decl_header_index`] walks a program's top-level statements ONCE
//! and records, per declared symbol: its name, kind, declaration/name
//! spans, type-parameter headers, direct syntactic member headers, and the
//! source-order locators of every contributing top-level statement. It
//! performs NO type lowering — no `lower_ts_type`, no body walks — so an
//! `IndexedReady` publish that builds only this index lowers zero
//! declaration bodies.
//!
//! The index mirrors the NAME REGISTRATION of
//! [`crate::analysis::type_eval_build::build_eval_env`] exactly (file-scope
//! type/value tables, namespace-qualified names, default-export aliasing,
//! augmentation-scope inventories, JSDoc `@typedef` names under TS-decl
//! precedence): a name is in this index if and only if the whole-env walk
//! would register it. The lazy declaration-body service uses the recorded
//! statement locators to lower exactly a demanded symbol's contributing
//! statements through the shared
//! [`crate::analysis::type_eval_build::lower_top_level_statement`] arms.

use oxc_ast::ast::{
    Class, ClassElement, Declaration, ExportDefaultDeclarationKind, Expression,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSEnumDeclaration, TSInterfaceDeclaration, TSModuleBlock, TSModuleDeclaration,
    TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature, TSType, TSTypeAliasDeclaration,
    TSTypeParameterDeclaration, VariableDeclarationKind, VariableDeclarator,
};
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;
use verter_span::Span;
use verter_type_expr_oxc::property_key_name;

use crate::analysis::type_eval::{AugmentationScopeKind, TypeDeclKind, ValueDeclKind};

/// Direct syntactic member-header kind (a shallow shape fact, not a body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberHeaderKind {
    Property,
    Method,
}

/// One direct syntactic member header: the member's name plus the
/// header-level flags the declaration states syntactically. No member
/// VALUE type is recorded — that is body data, lowered on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberHeader {
    pub name: String,
    pub kind: MemberHeaderKind,
    pub optional: bool,
    pub readonly: bool,
}

/// One type-parameter header: the parameter name plus the source locators
/// of its constraint / default clauses (the clauses themselves lower with
/// the body, on demand).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParamHeader {
    pub name: String,
    pub constraint_span: Option<Span>,
    pub default_span: Option<Span>,
}

/// Header record for one declared TYPE symbol (file scope or augmentation
/// scope): everything later stages need to ADDRESS the declaration without
/// lowering its body.
#[derive(Debug, Clone)]
pub struct TypeDeclHeader {
    pub kind: TypeDeclKind,
    /// Full declaration span of the LAST contributor (the last-wins
    /// representative, matching `TypeDeclGroup::primary`).
    pub span: Span,
    /// Name-identifier span of the last contributor.
    pub name_span: Span,
    /// Type-parameter headers, unioned across contributors in first-seen
    /// order (matching the lowered group's parameter-union rule).
    pub type_params: Vec<TypeParamHeader>,
    /// Direct syntactic member headers, unioned across contributors in
    /// first-seen order (matching `TypeDeclBody::lookup_object`'s
    /// own-member projection: heritage members are NOT included).
    pub member_headers: Vec<MemberHeader>,
    /// Source-order top-level statement indices of every contributing
    /// statement (deduplicated; one statement can contribute several
    /// same-name declarations).
    pub contributors: Vec<u32>,
    /// `true` when the name exists ONLY as a JSDoc `@typedef` (no TS
    /// declaration claimed it — TS-decl precedence applied at build).
    pub from_jsdoc_typedef: bool,
}

/// Header record for one declared VALUE symbol.
#[derive(Debug, Clone)]
pub struct ValueDeclHeader {
    pub kind: ValueDeclKind,
    /// Full declaration span of the last contributor.
    pub span: Span,
    /// Name-identifier span of the last contributor.
    pub name_span: Span,
    /// Direct syntactic member headers of an object-literal initializer
    /// (`const x = { a, b }`), in first-seen order. Empty for
    /// non-object-literal values.
    pub object_member_headers: Vec<MemberHeader>,
    /// Source-order top-level statement indices of every contributing
    /// statement (deduplicated).
    pub contributors: Vec<u32>,
}

/// Header record for one `enum` declaration. Kept in its OWN table:
/// the eval-env walk does not register enums as value symbols, so enum
/// headers must not feed the value-symbol inventory — they exist for
/// header-level facts (member presence) only.
#[derive(Debug, Clone)]
pub struct EnumDeclHeader {
    pub span: Span,
    pub name_span: Span,
    pub member_names: Vec<String>,
    pub contributors: Vec<u32>,
}

/// The shallow declaration-header index for one parsed program.
///
/// Augmentation tables are NESTED maps (`scope → name → header`) so a
/// scoped lookup needs no allocated tuple key.
#[derive(Debug, Clone, Default)]
pub struct DeclHeaderIndex {
    pub type_headers: FxHashMap<String, TypeDeclHeader>,
    pub value_headers: FxHashMap<String, ValueDeclHeader>,
    pub enum_headers: FxHashMap<String, EnumDeclHeader>,
    pub augmentation_type_headers:
        FxHashMap<AugmentationScopeKind, FxHashMap<String, TypeDeclHeader>>,
    pub augmentation_value_headers:
        FxHashMap<AugmentationScopeKind, FxHashMap<String, ValueDeclHeader>>,
}

impl DeclHeaderIndex {
    /// Look up a file-scope type header.
    pub fn type_header(&self, name: &str) -> Option<&TypeDeclHeader> {
        self.type_headers.get(name)
    }

    /// Look up a file-scope value header.
    pub fn value_header(&self, name: &str) -> Option<&ValueDeclHeader> {
        self.value_headers.get(name)
    }

    /// Look up an augmentation-scoped type header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_type_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&TypeDeclHeader> {
        self.augmentation_type_headers.get(scope)?.get(name)
    }

    /// Look up an augmentation-scoped value header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_value_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&ValueDeclHeader> {
        self.augmentation_value_headers.get(scope)?.get(name)
    }
}

impl DeclHeaderIndex {
    /// Synthesize a header index FROM an already-built [`EvalEnv`] — the
    /// env-seeded construction mirror (test fixtures and other
    /// already-built-env callers). Names/kinds/params/member names come
    /// from the env's groups; statement locators are empty (a seeded
    /// index never drives selective statement lowering — its memo is
    /// pre-filled).
    pub fn from_eval_env(env: &crate::analysis::type_eval::EvalEnv) -> Self {
        use crate::analysis::type_eval::{TypeDeclGroup, ValueDeclGroup};

        fn type_header_from_group(group: &TypeDeclGroup) -> TypeDeclHeader {
            let primary = group.primary();
            let mut type_params: Vec<TypeParamHeader> = Vec::new();
            for decl in group.contributors() {
                for param in &decl.type_parameters {
                    if !type_params.iter().any(|p| p.name == param.name) {
                        type_params.push(TypeParamHeader {
                            name: param.name.clone(),
                            constraint_span: None,
                            default_span: None,
                        });
                    }
                }
            }
            let member_headers = group
                .merged_body()
                .merged_member_names()
                .into_iter()
                .map(|name| MemberHeader {
                    name,
                    kind: MemberHeaderKind::Property,
                    optional: false,
                    readonly: false,
                })
                .collect();
            TypeDeclHeader {
                kind: primary.kind,
                span: Span::default(),
                name_span: Span::default(),
                type_params,
                member_headers,
                contributors: Vec::new(),
                from_jsdoc_typedef: false,
            }
        }

        fn value_header_from_group(group: &ValueDeclGroup) -> ValueDeclHeader {
            let primary = group.primary();
            let object_member_headers = primary
                .object_shape
                .as_ref()
                .map(|shape| {
                    shape
                        .properties
                        .iter()
                        .filter_map(|member| {
                            let name = match member {
                                verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                                verter_type_expr::ObjectMember::Method(m) => Some(m.name.clone()),
                                _ => None,
                            }?;
                            Some(MemberHeader {
                                name,
                                kind: MemberHeaderKind::Property,
                                optional: false,
                                readonly: false,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            ValueDeclHeader {
                kind: primary.kind,
                span: Span::default(),
                name_span: Span::default(),
                object_member_headers,
                contributors: Vec::new(),
            }
        }

        let mut index = DeclHeaderIndex::default();
        for (name, group) in &env.type_symbols {
            index
                .type_headers
                .insert(name.clone(), type_header_from_group(group));
        }
        for (name, group) in &env.value_symbols {
            index
                .value_headers
                .insert(name.clone(), value_header_from_group(group));
        }
        for ((scope, name), group) in &env.augmentation_scopes {
            index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default()
                .insert(name.clone(), type_header_from_group(group));
        }
        for ((scope, name), group) in &env.augmentation_value_scopes {
            index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default()
                .insert(name.clone(), value_header_from_group(group));
        }
        index
    }
}

/// Build the shallow declaration-header index for `program`. Walks every
/// top-level statement once; lowers NO declaration body.
pub fn build_decl_header_index(program: &Program<'_>, source: &str) -> DeclHeaderIndex {
    let mut index = DeclHeaderIndex::default();

    for (stmt_index, stmt) in program.body.iter().enumerate() {
        let stmt_index = u32::try_from(stmt_index).unwrap_or(u32::MAX);
        index_top_level_statement(stmt, stmt_index, &mut index);
    }

    // JSDoc `@typedef {T} Name` names — TS-decl precedence: a name a TS
    // declaration already claimed is skipped (mirrors
    // `register_jsdoc_typedefs`). A typedef has no statement locator; its
    // body lowers from the program's comments on demand.
    for name in crate::analysis::jsdoc::collect_jsdoc_typedef_names(&program.comments, source) {
        if index.type_headers.contains_key(&name) {
            continue;
        }
        index.type_headers.insert(
            name,
            TypeDeclHeader {
                kind: TypeDeclKind::Alias,
                span: Span::default(),
                name_span: Span::default(),
                type_params: Vec::new(),
                member_headers: Vec::new(),
                contributors: Vec::new(),
                from_jsdoc_typedef: true,
            },
        );
    }

    index
}

/// Mirror of `lower_top_level_statement`'s name registration, headers only.
fn index_top_level_statement(stmt: &Statement<'_>, stmt_index: u32, index: &mut DeclHeaderIndex) {
    match stmt {
        Statement::TSTypeAliasDeclaration(decl) => {
            index_type_alias(
                decl,
                decl.id.name.as_str(),
                stmt_index,
                &mut index.type_headers,
            );
        }
        Statement::TSInterfaceDeclaration(decl) => {
            index_interface(
                decl,
                decl.id.name.as_str(),
                stmt_index,
                &mut index.type_headers,
            );
        }
        Statement::TSModuleDeclaration(module) => {
            index_module_declaration(module, stmt_index, index, None);
        }
        Statement::TSGlobalDeclaration(global) => {
            index_augmentation_block(
                &global.body,
                stmt_index,
                index,
                &AugmentationScopeKind::Global,
            );
        }
        Statement::ClassDeclaration(decl) => {
            index_class(decl, stmt_index, index);
        }
        Statement::FunctionDeclaration(func) => {
            index_function(func, stmt_index, &mut index.value_headers);
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                index_variable(decl, var_decl.kind, stmt_index, &mut index.value_headers);
            }
        }
        Statement::TSEnumDeclaration(enum_decl) => {
            index_enum(enum_decl, stmt_index, &mut index.enum_headers);
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_declaration(decl, stmt_index, index);
            }
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                index_function(func, stmt_index, &mut index.value_headers);
            }
            ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                index_class(cls, stmt_index, index);
                // Mirror `alias_default_export_type_symbol`: the declared
                // class type also answers under the `default` export name.
                if let Some(id) = &cls.id {
                    alias_default_type_header(index, id.name.as_str(), stmt_index);
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                index_interface(
                    iface,
                    iface.id.name.as_str(),
                    stmt_index,
                    &mut index.type_headers,
                );
                alias_default_type_header(index, iface.id.name.as_str(), stmt_index);
            }
            other => {
                if let Some(expr) = other.as_expression() {
                    // Mirrors `extract_default_expression`: a `default`
                    // value symbol of kind `Const`, with object-literal
                    // member headers when the expression is one.
                    let entry = index
                        .value_headers
                        .entry("default".to_string())
                        .or_insert_with(|| ValueDeclHeader {
                            kind: ValueDeclKind::Const,
                            span: export.span.into(),
                            name_span: export.span.into(),
                            object_member_headers: object_literal_member_headers(expr),
                            contributors: Vec::new(),
                        });
                    push_contributor(&mut entry.contributors, stmt_index);
                }
            }
        },
        _ => {}
    }
}

/// Mirror of `extract_from_declaration` (the `export <decl>` wrapper arms).
fn index_declaration(decl: &Declaration<'_>, stmt_index: u32, index: &mut DeclHeaderIndex) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            index_type_alias(
                alias,
                alias.id.name.as_str(),
                stmt_index,
                &mut index.type_headers,
            );
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            index_interface(
                iface,
                iface.id.name.as_str(),
                stmt_index,
                &mut index.type_headers,
            );
        }
        Declaration::TSModuleDeclaration(module) => {
            index_module_declaration(module, stmt_index, index, None);
        }
        Declaration::TSGlobalDeclaration(global) => {
            index_augmentation_block(
                &global.body,
                stmt_index,
                index,
                &AugmentationScopeKind::Global,
            );
        }
        Declaration::ClassDeclaration(cls) => {
            index_class(cls, stmt_index, index);
        }
        Declaration::FunctionDeclaration(func) => {
            index_function(func, stmt_index, &mut index.value_headers);
        }
        Declaration::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                index_variable(d, var_decl.kind, stmt_index, &mut index.value_headers);
            }
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            index_enum(enum_decl, stmt_index, &mut index.enum_headers);
        }
        _ => {}
    }
}

/// Index one `enum` declaration's HEADER facts (member names + statement
/// locator). The eval-env walk has no enum arm — enums never enter
/// `value_symbols` — so enums live in their own header table for
/// header-level member-presence facts only; no body lowering.
///
/// A MERGED enum (`enum E { A }` then `enum E { B }`, legal TS declaration
/// merging) UNIONS every same-name declaration's members into the existing
/// header in source order — dropping a later declaration's members would
/// under-state the member surface and under-invalidate a warm consumer.
/// Member names dedup defensively so a malformed repeated variant is not
/// double-counted. The representative spans are the FIRST contributor's
/// (enum spans are not consumed downstream; only the member-name union and
/// the contributor locators feed the parse-stable skeleton and facts).
fn index_enum(
    enum_decl: &TSEnumDeclaration<'_>,
    stmt_index: u32,
    table: &mut FxHashMap<String, EnumDeclHeader>,
) {
    let entry = table
        .entry(enum_decl.id.name.to_string())
        .or_insert_with(|| EnumDeclHeader {
            span: enum_decl.span.into(),
            name_span: enum_decl.id.span.into(),
            member_names: Vec::new(),
            contributors: Vec::new(),
        });
    for member in &enum_decl.body.members {
        let name = member.id.static_name().to_string();
        if !entry.member_names.iter().any(|existing| existing == &name) {
            entry.member_names.push(name);
        }
    }
    push_contributor(&mut entry.contributors, stmt_index);
}

/// Mirror of `extract_module_declaration`: a string-literal module name is
/// an ambient augmentation scope; an identifier name is a namespace whose
/// inner type declarations register under qualified `Ns.Name` names.
fn index_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    stmt_index: u32,
    index: &mut DeclHeaderIndex,
    prefix: Option<&str>,
) {
    if let TSModuleDeclarationName::StringLiteral(spec) = &decl.id {
        if let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = decl.body.as_ref() {
            let scope = AugmentationScopeKind::Module(spec.value.to_string());
            index_augmentation_block(block, stmt_index, index, &scope);
        }
        return;
    }

    let module_name = match &decl.id {
        TSModuleDeclarationName::Identifier(id) => match prefix {
            Some(prefix) => format!("{prefix}.{}", id.name),
            None => id.name.to_string(),
        },
        TSModuleDeclarationName::StringLiteral(_) => return,
    };
    let Some(body) = decl.body.as_ref() else {
        return;
    };

    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            index_module_declaration(inner, stmt_index, index, Some(module_name.as_str()));
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                index_namespaced_statement(stmt, stmt_index, index, module_name.as_str());
            }
        }
    }
}

/// Mirror of `extract_namespaced_statement`: only type aliases, interfaces
/// and nested modules register (namespaced VALUE declarations are not
/// extracted by the env walk).
fn index_namespaced_statement(
    stmt: &Statement<'_>,
    stmt_index: u32,
    index: &mut DeclHeaderIndex,
    namespace: &str,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            index_type_alias(alias, name.as_str(), stmt_index, &mut index.type_headers);
        }
        Statement::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            index_interface(iface, name.as_str(), stmt_index, &mut index.type_headers);
        }
        Statement::TSModuleDeclaration(module) => {
            index_module_declaration(module, stmt_index, index, Some(namespace));
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_namespaced_declaration(decl, stmt_index, index, namespace);
            }
        }
        _ => {}
    }
}

fn index_namespaced_declaration(
    decl: &Declaration<'_>,
    stmt_index: u32,
    index: &mut DeclHeaderIndex,
    namespace: &str,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            index_type_alias(alias, name.as_str(), stmt_index, &mut index.type_headers);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            index_interface(iface, name.as_str(), stmt_index, &mut index.type_headers);
        }
        Declaration::TSModuleDeclaration(module) => {
            index_module_declaration(module, stmt_index, index, Some(namespace));
        }
        _ => {}
    }
}

/// Mirror of `extract_augmentation_block` + `extract_augmentation_declaration`
/// + `retain_value_statement_into_augmentation`.
///
/// Inner interfaces / type-aliases register under the TYPE augmentation
/// scope; inner value statements (`const`/`let`/`var`, `function`,
/// `class`) register under the VALUE augmentation scope. An inner class
/// registers ONLY its value side (the env walk drops the throwaway type
/// side).
fn index_augmentation_block(
    block: &TSModuleBlock<'_>,
    stmt_index: u32,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
) {
    for stmt in &block.body {
        match stmt {
            Statement::TSInterfaceDeclaration(iface) => {
                let scoped = index
                    .augmentation_type_headers
                    .entry(scope.clone())
                    .or_default();
                index_interface(iface, iface.id.name.as_str(), stmt_index, scoped);
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let scoped = index
                    .augmentation_type_headers
                    .entry(scope.clone())
                    .or_default();
                index_type_alias(alias, alias.id.name.as_str(), stmt_index, scoped);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = export.declaration.as_ref() {
                    match decl {
                        Declaration::TSInterfaceDeclaration(iface) => {
                            let scoped = index
                                .augmentation_type_headers
                                .entry(scope.clone())
                                .or_default();
                            index_interface(iface, iface.id.name.as_str(), stmt_index, scoped);
                        }
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let scoped = index
                                .augmentation_type_headers
                                .entry(scope.clone())
                                .or_default();
                            index_type_alias(alias, alias.id.name.as_str(), stmt_index, scoped);
                        }
                        Declaration::VariableDeclaration(var_decl) => {
                            let scoped = index
                                .augmentation_value_headers
                                .entry(scope.clone())
                                .or_default();
                            for d in &var_decl.declarations {
                                index_variable(d, var_decl.kind, stmt_index, scoped);
                            }
                        }
                        Declaration::FunctionDeclaration(func) => {
                            let scoped = index
                                .augmentation_value_headers
                                .entry(scope.clone())
                                .or_default();
                            index_function(func, stmt_index, scoped);
                        }
                        Declaration::ClassDeclaration(cls) => {
                            index_augmentation_class_value(cls, stmt_index, index, scope);
                        }
                        _ => {}
                    }
                }
            }
            Statement::VariableDeclaration(var_decl) => {
                let scoped = index
                    .augmentation_value_headers
                    .entry(scope.clone())
                    .or_default();
                for d in &var_decl.declarations {
                    index_variable(d, var_decl.kind, stmt_index, scoped);
                }
            }
            Statement::FunctionDeclaration(func) => {
                let scoped = index
                    .augmentation_value_headers
                    .entry(scope.clone())
                    .or_default();
                index_function(func, stmt_index, scoped);
            }
            Statement::ClassDeclaration(cls) => {
                index_augmentation_class_value(cls, stmt_index, index, scope);
            }
            _ => {}
        }
    }
}

/// An ambient-augmentation class contributes its VALUE side only (mirrors
/// `move_value_symbols_into_augmentation`, which drops the type side).
fn index_augmentation_class_value(
    cls: &Class<'_>,
    stmt_index: u32,
    index: &mut DeclHeaderIndex,
    scope: &AugmentationScopeKind,
) {
    let Some(id) = &cls.id else {
        return;
    };
    let scoped = index
        .augmentation_value_headers
        .entry(scope.clone())
        .or_default();
    let entry = scoped
        .entry(id.name.to_string())
        .or_insert_with(|| ValueDeclHeader {
            kind: ValueDeclKind::Class,
            span: cls.span.into(),
            name_span: id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = ValueDeclKind::Class;
    entry.span = cls.span.into();
    entry.name_span = id.span.into();
    push_contributor(&mut entry.contributors, stmt_index);
}

// ───────────────────────────────────────────────────────────────────────
// Per-declaration header builders
// ───────────────────────────────────────────────────────────────────────

fn index_type_alias(
    decl: &TSTypeAliasDeclaration<'_>,
    name: &str,
    stmt_index: u32,
    table: &mut FxHashMap<String, TypeDeclHeader>,
) {
    let params = type_param_headers(decl.type_parameters.as_deref());
    let members = alias_body_member_headers(&decl.type_annotation);
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Alias,
        decl.span.into(),
        decl.id.span.into(),
        params,
        members,
        stmt_index,
    );
}

fn index_interface(
    decl: &TSInterfaceDeclaration<'_>,
    name: &str,
    stmt_index: u32,
    table: &mut FxHashMap<String, TypeDeclHeader>,
) {
    let params = type_param_headers(decl.type_parameters.as_deref());
    let mut members = Vec::new();
    for sig in &decl.body.body {
        if let Some(header) = interface_member_header(sig) {
            members.push(header);
        }
    }
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Interface,
        decl.span.into(),
        decl.id.span.into(),
        params,
        members,
        stmt_index,
    );
}

/// Mirror of `extract_class`'s NAME registration: a named class declares a
/// type symbol (instance members) AND a value symbol (constructor shape +
/// static members). An anonymous class declares nothing.
fn index_class(decl: &Class<'_>, stmt_index: u32, index: &mut DeclHeaderIndex) {
    let Some(id) = &decl.id else {
        return;
    };
    let name = id.name.as_str();

    let params = type_param_headers(decl.type_parameters.as_deref());
    let mut instance_members = Vec::new();
    let mut static_members = Vec::new();
    for element in &decl.body.body {
        match element {
            ClassElement::PropertyDefinition(prop) => {
                if let Some(prop_name) = property_key_name(&prop.key) {
                    let header = MemberHeader {
                        name: prop_name,
                        kind: MemberHeaderKind::Property,
                        optional: prop.optional,
                        readonly: prop.readonly,
                    };
                    if prop.r#static {
                        static_members.push(header);
                    } else {
                        instance_members.push(header);
                    }
                }
            }
            ClassElement::MethodDefinition(method) => {
                if method.kind == MethodDefinitionKind::Constructor {
                    continue;
                }
                if let Some(method_name) = property_key_name(&method.key) {
                    let header = MemberHeader {
                        name: method_name,
                        kind: MemberHeaderKind::Method,
                        optional: method.optional,
                        readonly: false,
                    };
                    if method.r#static {
                        static_members.push(header);
                    } else {
                        instance_members.push(header);
                    }
                }
            }
            _ => {}
        }
    }

    upsert_type_header(
        &mut index.type_headers,
        name,
        TypeDeclKind::Class,
        decl.span.into(),
        id.span.into(),
        params,
        instance_members,
        stmt_index,
    );

    let entry = index
        .value_headers
        .entry(name.to_string())
        .or_insert_with(|| ValueDeclHeader {
            kind: ValueDeclKind::Class,
            span: decl.span.into(),
            name_span: id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = ValueDeclKind::Class;
    entry.span = decl.span.into();
    entry.name_span = id.span.into();
    for header in static_members {
        if !entry
            .object_member_headers
            .iter()
            .any(|existing| existing.name == header.name)
        {
            entry.object_member_headers.push(header);
        }
    }
    push_contributor(&mut entry.contributors, stmt_index);
}

fn index_function(
    func: &oxc_ast::ast::Function<'_>,
    stmt_index: u32,
    table: &mut FxHashMap<String, ValueDeclHeader>,
) {
    let Some(id) = &func.id else {
        return;
    };
    let kind = if func.r#async {
        ValueDeclKind::AsyncFunction
    } else {
        ValueDeclKind::Function
    };
    let entry = table
        .entry(id.name.to_string())
        .or_insert_with(|| ValueDeclHeader {
            kind,
            span: func.span.into(),
            name_span: id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    // Last contributor wins for the representative kind/spans (matching
    // `ValueDeclGroup::primary`).
    entry.kind = kind;
    entry.span = func.span.into();
    entry.name_span = id.span.into();
    push_contributor(&mut entry.contributors, stmt_index);
}

fn index_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    stmt_index: u32,
    table: &mut FxHashMap<String, ValueDeclHeader>,
) {
    let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &decl.id else {
        return;
    };
    let var_kind = match kind {
        VariableDeclarationKind::Const
        | VariableDeclarationKind::Using
        | VariableDeclarationKind::AwaitUsing => ValueDeclKind::Const,
        VariableDeclarationKind::Let => ValueDeclKind::Let,
        VariableDeclarationKind::Var => ValueDeclKind::Var,
    };
    let members = decl
        .init
        .as_ref()
        .map(|init| object_literal_member_headers(init))
        .unwrap_or_default();
    let entry = table
        .entry(id.name.to_string())
        .or_insert_with(|| ValueDeclHeader {
            kind: var_kind,
            span: decl.span.into(),
            name_span: id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = var_kind;
    entry.span = decl.span.into();
    entry.name_span = id.span.into();
    for header in members {
        if !entry
            .object_member_headers
            .iter()
            .any(|existing| existing.name == header.name)
        {
            entry.object_member_headers.push(header);
        }
    }
    push_contributor(&mut entry.contributors, stmt_index);
}

/// Mirror of `alias_default_export_type_symbol`: clone the declared-name
/// header under `default` (no-op when `default` already exists or the
/// declared name produced no type header).
fn alias_default_type_header(index: &mut DeclHeaderIndex, declared_name: &str, stmt_index: u32) {
    if index.type_headers.contains_key("default") {
        return;
    }
    let Some(declared) = index.type_headers.get(declared_name) else {
        return;
    };
    let mut aliased = declared.clone();
    aliased.contributors = vec![stmt_index];
    index.type_headers.insert("default".to_string(), aliased);
}

#[allow(clippy::too_many_arguments)]
fn upsert_type_header(
    table: &mut FxHashMap<String, TypeDeclHeader>,
    name: &str,
    kind: TypeDeclKind,
    span: Span,
    name_span: Span,
    params: Vec<TypeParamHeader>,
    members: Vec<MemberHeader>,
    stmt_index: u32,
) {
    let entry = table
        .entry(name.to_string())
        .or_insert_with(|| TypeDeclHeader {
            kind,
            span,
            name_span,
            type_params: Vec::new(),
            member_headers: Vec::new(),
            contributors: Vec::new(),
            from_jsdoc_typedef: false,
        });
    // Last contributor wins for the representative kind/spans (matching
    // `TypeDeclGroup::primary`); params and members UNION across
    // contributors in first-seen order (matching the lowered group's
    // parameter-union and `lookup_object`'s first-seen member rules).
    entry.kind = kind;
    entry.span = span;
    entry.name_span = name_span;
    entry.from_jsdoc_typedef = false;
    for param in params {
        if !entry.type_params.iter().any(|p| p.name == param.name) {
            entry.type_params.push(param);
        }
    }
    for member in members {
        if !entry
            .member_headers
            .iter()
            .any(|existing| existing.name == member.name)
        {
            entry.member_headers.push(member);
        }
    }
    push_contributor(&mut entry.contributors, stmt_index);
}

fn push_contributor(contributors: &mut Vec<u32>, stmt_index: u32) {
    if contributors.last() != Some(&stmt_index) {
        contributors.push(stmt_index);
    }
}

fn type_param_headers(decl: Option<&TSTypeParameterDeclaration<'_>>) -> Vec<TypeParamHeader> {
    let Some(decl) = decl else {
        return Vec::new();
    };
    decl.params
        .iter()
        .map(|param| TypeParamHeader {
            name: param.name.name.to_string(),
            constraint_span: param.constraint.as_ref().map(|c| c.span().into()),
            default_span: param.default.as_ref().map(|d| d.span().into()),
        })
        .collect()
}

/// Direct syntactic member headers of a type-alias body: a `TSTypeLiteral`
/// contributes its named members; intersection / parenthesized arms are
/// descended (mirroring the lowered body's `lookup_object` own-member
/// projection). Every other body shape has no direct syntactic members.
fn alias_body_member_headers(ty: &TSType<'_>) -> Vec<MemberHeader> {
    let mut out = Vec::new();
    collect_alias_member_headers(ty, &mut out);
    out
}

fn collect_alias_member_headers(ty: &TSType<'_>, out: &mut Vec<MemberHeader>) {
    match ty {
        TSType::TSTypeLiteral(literal) => {
            for sig in &literal.members {
                if let Some(header) = interface_member_header(sig) {
                    if !out.iter().any(|existing| existing.name == header.name) {
                        out.push(header);
                    }
                }
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for part in &intersection.types {
                collect_alias_member_headers(part, out);
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_alias_member_headers(&paren.type_annotation, out);
        }
        _ => {}
    }
}

fn interface_member_header(sig: &TSSignature<'_>) -> Option<MemberHeader> {
    match sig {
        TSSignature::TSPropertySignature(prop) => Some(MemberHeader {
            name: property_key_name(&prop.key)?,
            kind: MemberHeaderKind::Property,
            optional: prop.optional,
            readonly: prop.readonly,
        }),
        TSSignature::TSMethodSignature(method) => Some(MemberHeader {
            name: property_key_name(&method.key)?,
            kind: MemberHeaderKind::Method,
            optional: method.optional,
            readonly: false,
        }),
        _ => None,
    }
}

/// Direct member headers of an object-literal initializer, seen through
/// `as` / `satisfies` / parenthesized wrappers (mirroring
/// `extract_initializer_object_shape`).
fn object_literal_member_headers(expr: &Expression<'_>) -> Vec<MemberHeader> {
    match expr {
        Expression::ObjectExpression(obj) => object_expression_member_headers(obj),
        Expression::TSAsExpression(ts_as) => object_literal_member_headers(&ts_as.expression),
        Expression::TSSatisfiesExpression(sat) => object_literal_member_headers(&sat.expression),
        Expression::ParenthesizedExpression(paren) => {
            object_literal_member_headers(&paren.expression)
        }
        _ => Vec::new(),
    }
}

fn object_expression_member_headers(obj: &ObjectExpression<'_>) -> Vec<MemberHeader> {
    let mut out: Vec<MemberHeader> = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let Some(name) = property_key_name(&p.key) {
                // Mirror `push_object_property_with_override`: a duplicate
                // key's LAST occurrence wins.
                out.retain(|existing| existing.name != name);
                out.push(MemberHeader {
                    name,
                    kind: MemberHeaderKind::Property,
                    optional: false,
                    readonly: false,
                });
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "decl_headers_tests.rs"]
mod decl_headers_tests;
