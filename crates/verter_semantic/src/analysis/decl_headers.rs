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
use verter_session_query::declarations::header_index::{
    DeclHeaderContributor, DeclHeaderIndex, EnumDeclHeader, JsdocTypedefHeader, MemberHeader,
    NamespaceBlockRecord, TypeDeclHeader, TypeParamHeader, ValueDeclHeader,
};
use verter_session_query::declarations::headers::EnumMemberPosition;

use oxc_ast::ast::{
    Class, ClassElement, Comment, Declaration, ExportDefaultDeclarationKind, Expression,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, PropertyKey, Statement,
    TSEnumDeclaration, TSExternalModuleDeclaration, TSInterfaceDeclaration, TSModuleBlock,
    TSNamespaceDeclaration, TSNamespaceDeclarationBody, TSSignature, TSType,
    TSTypeAliasDeclaration, TSTypeName, TSTypeParameterDeclaration, VariableDeclarationKind,
    VariableDeclarator,
};
use oxc_span::GetSpan;
use rustc_hash::FxHashSet;
use verter_span::Span;
use verter_type_expr::facts::VueIgnoredHeritageFact;
use verter_type_expr::span_origins::DeclContributorAnchor;
use verter_type_expr::{DeclBindingKey, ObjectMethodKind};
use verter_type_expr_oxc::lower_property_key;

use verter_parser::utils::oxc::script::route_inventory::statements_have_export_declarations;
use verter_session_query::analysis::top_level_owners::{
    DeclMap, TopLevelOwnerTable, TopLevelStatementOwner,
};
use verter_session_query::declarations::{AugmentationScopeKind, TypeDeclKind, ValueDeclKind};

use crate::analysis::namespace_walk::{
    for_each_namespace, NamespaceVisitor, Nesting, QualifiedPath,
};

#[path = "decl_headers_augmentation.rs"]
mod augmentation;
use augmentation::index_augmentation_block;

#[derive(Debug, Clone, Copy)]
struct HeaderStatementContext<'a> {
    anchor: DeclContributorAnchor,
    vue_ignore_attachment_starts: &'a FxHashSet<u32>,
    source: &'a str,
    /// The statement belongs to a declaration file, where every
    /// declaration is ambient.
    declaration_file: bool,
}

impl<'a> HeaderStatementContext<'a> {
    fn new(
        statement_index: usize,
        owner: TopLevelStatementOwner,
        vue_ignore_attachment_starts: &'a FxHashSet<u32>,
        source: &'a str,
        declaration_file: bool,
    ) -> Option<Self> {
        Some(Self {
            anchor: DeclContributorAnchor {
                contributor_index: u32::try_from(statement_index).ok()?,
                owner: owner.owner,
                owner_local_ordinal: owner.owner_local_ordinal,
            },
            vue_ignore_attachment_starts,
            source,
            declaration_file,
        })
    }

    fn key(self, name: &str) -> DeclBindingKey {
        DeclBindingKey::new(self.anchor.owner, name)
    }
}

/// The block a namespace body declares, past a dotted name's segments
/// (`namespace A.B.C { … }`).
fn namespace_block<'s, 'a>(mut body: &'s TSNamespaceDeclarationBody<'a>) -> &'s TSModuleBlock<'a> {
    loop {
        match body {
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => body = &inner.body,
            TSNamespaceDeclarationBody::TSModuleBlock(block) => return block,
        }
    }
}

/// Whether a module block is instantiated ([`NamespaceBlockRecord::instantiated`]).
/// A namespace nested in the block costs no native level: the blocks still
/// to search are an explicit stack.
pub(crate) fn module_block_instantiated(block: &TSModuleBlock<'_>) -> bool {
    let mut pending = vec![block.body.iter()];
    while let Some(statements) = pending.last_mut() {
        match statements.next() {
            None => {
                pending.pop();
            }
            Some(statement) => match statement_instantiates(statement) {
                Instantiates::Yes => return true,
                Instantiates::No => {}
                Instantiates::Block(block) => pending.push(block.body.iter()),
            },
        }
    }
    false
}

/// Whether one statement makes its block instantiated, or the nested
/// block that decides it.
enum Instantiates<'s, 'a> {
    Yes,
    No,
    /// A nested namespace or module: instantiated when its block is.
    Block(&'s TSModuleBlock<'a>),
}

/// An external module declaration (`declare module "x"`) is instantiated
/// when its block is: a bodiless one is not.
fn external_module_instantiates<'s, 'a>(
    module: &'s TSExternalModuleDeclaration<'a>,
) -> Instantiates<'s, 'a> {
    match module.body.as_ref() {
        Some(block) => Instantiates::Block(block),
        None => Instantiates::No,
    }
}

fn statement_instantiates<'s, 'a>(statement: &'s Statement<'a>) -> Instantiates<'s, 'a> {
    let yes = |instantiated: bool| {
        if instantiated {
            Instantiates::Yes
        } else {
            Instantiates::No
        }
    };
    match statement {
        Statement::TSInterfaceDeclaration(_) | Statement::TSTypeAliasDeclaration(_) => yes(false),
        Statement::TSEnumDeclaration(enum_decl) => yes(!enum_decl.r#const),
        Statement::TSNamespaceDeclaration(module) => {
            Instantiates::Block(namespace_block(&module.body))
        }
        Statement::TSExternalModuleDeclaration(module) => external_module_instantiates(module),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::TSInterfaceDeclaration(_) | Declaration::TSTypeAliasDeclaration(_) => {
                yes(false)
            }
            Declaration::TSEnumDeclaration(enum_decl) => yes(!enum_decl.r#const),
            Declaration::TSNamespaceDeclaration(module) => {
                Instantiates::Block(namespace_block(&module.body))
            }
            Declaration::TSExternalModuleDeclaration(module) => {
                external_module_instantiates(module)
            }
            _ => yes(true),
        },
        Statement::ExportNamedDeclaration(_) | Statement::ExportFromDeclaration(_) => yes(false),
        _ => yes(true),
    }
}

/// Whether one statement of a namespace body makes the body instantiated,
/// the namespaces nested in it aside (their own walk answers for them).
fn statement_instantiates_here(statement: &Statement<'_>) -> bool {
    match statement_instantiates(statement) {
        Instantiates::Yes => true,
        Instantiates::No => false,
        Instantiates::Block(block) => module_block_instantiated(block),
    }
}

/// Build the shallow declaration-header index for `program`. Walks every
/// top-level statement once; lowers NO declaration body.
pub fn build_decl_header_index(program: &Program<'_>, source: &str) -> DeclHeaderIndex {
    let owners = TopLevelOwnerTable::ordinary_file(program.body.len());
    build_decl_header_index_with_owners(program, source, &owners)
}

/// Build the shallow declaration index under an explicit validated lexical
/// owner mapping.
pub fn build_decl_header_index_with_owners(
    program: &Program<'_>,
    source: &str,
    owners: &TopLevelOwnerTable,
) -> DeclHeaderIndex {
    assert_eq!(
        owners.len(),
        program.body.len(),
        "validated owner table must cover the indexed program exactly"
    );
    let mut index = DeclHeaderIndex::default();
    let vue_ignore_attachment_starts =
        collect_vue_ignore_attachment_starts(&program.comments, source);

    for (stmt_index, stmt) in program.body.iter().enumerate() {
        let Some(ctx) = HeaderStatementContext::new(
            stmt_index,
            owners.statement(stmt_index),
            &vue_ignore_attachment_starts,
            source,
            program.source_type.is_typescript_definition(),
        ) else {
            break;
        };
        index_top_level_statement(stmt, ctx, &mut index);
    }

    // JSDoc typedefs are keyed by their parser-authored attachment owner. An
    // unattached carrier comment must fall inside an explicit owner region;
    // ambiguous/unowned comments are skipped rather than guessed.
    for typedef in
        crate::analysis::jsdoc::collect_jsdoc_typedef_name_records(&program.comments, source)
    {
        let Some(attached_owner) = owners.resolve_comment_owner(
            typedef.attached_to,
            typedef.comment_span,
            program.body.iter().map(|statement| statement.span().start),
        ) else {
            continue;
        };
        let key = DeclBindingKey::new(attached_owner.owner, typedef.name.as_str());
        if index.type_headers.contains_key(&key) {
            continue;
        }
        let jsdoc_typedef = JsdocTypedefHeader {
            owner: attached_owner.owner,
            attached_to: typedef.attached_to,
            comment_span: typedef.comment_span,
            name_span: typedef.name_span,
            statement_index: attached_owner.statement_index,
            owner_local_ordinal: attached_owner.owner_local_ordinal,
        };
        index.type_headers.insert(
            key,
            TypeDeclHeader {
                kind: TypeDeclKind::Alias,
                span: typedef.comment_span,
                name_span: typedef.name_span,
                type_params: Vec::new(),
                member_headers: Vec::new(),
                contributors: Vec::new(),
                vue_ignored_heritage: Vec::new(),
                from_jsdoc_typedef: true,
                jsdoc_typedef: Some(jsdoc_typedef),
            },
        );
    }

    index
}

fn collect_vue_ignore_attachment_starts(comments: &[Comment], source: &str) -> FxHashSet<u32> {
    comments
        .iter()
        .filter(|comment| comment.is_block())
        .filter_map(|comment| {
            let content = comment.content_span();
            let content = source.get(content.start as usize..content.end as usize)?;
            contains_exact_vue_ignore_directive(content).then_some(comment.attached_to)
        })
        .collect()
}

fn contains_exact_vue_ignore_directive(content: &str) -> bool {
    const DIRECTIVE: &[u8] = b"@vue-ignore";

    content
        .as_bytes()
        .windows(DIRECTIVE.len())
        .enumerate()
        .any(|(start, candidate)| {
            if candidate != DIRECTIVE {
                return false;
            }
            let bytes = content.as_bytes();
            let before = start.checked_sub(1).and_then(|index| bytes.get(index));
            let after = bytes.get(start + DIRECTIVE.len());
            !before.is_some_and(|byte| is_vue_directive_token_byte(*byte))
                && !after.is_some_and(|byte| is_vue_directive_token_byte(*byte))
        })
}

const fn is_vue_directive_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'$' | b'@')
}

/// Mirror of `lower_top_level_statement`'s name registration, headers only.
fn index_top_level_statement(
    stmt: &Statement<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    // Mirror of `collect_hoisted_vars`: a `var` inside a nested block
    // belongs to the top level.
    crate::analysis::type_eval_build::for_each_hoisted_var(stmt, &mut |declarator, _| {
        index_variable_in(declarator, VariableDeclarationKind::Var, ctx, index, None);
    });
    match stmt {
        Statement::TSTypeAliasDeclaration(decl) => {
            index_type_alias(decl, decl.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::TSInterfaceDeclaration(decl) => {
            index_interface(decl, decl.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::TSNamespaceDeclaration(module) => {
            index_module_declaration(module, ctx, index, None, ctx.declaration_file);
        }
        Statement::TSExternalModuleDeclaration(module) => {
            index_external_module_declaration(module, ctx, index);
        }
        Statement::TSGlobalDeclaration(global) => {
            index_augmentation_block(&global.body, ctx, index, &AugmentationScopeKind::Global);
        }
        Statement::ClassDeclaration(decl) => {
            index_class(decl, ctx, index);
        }
        Statement::FunctionDeclaration(func) => {
            index_function(func, ctx, &mut index.value_headers);
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                index_variable_in(decl, var_decl.kind, ctx, index, None);
            }
        }
        Statement::TSEnumDeclaration(enum_decl) => {
            index_enum(
                enum_decl,
                enum_decl.id.name.as_str(),
                ctx,
                ctx.declaration_file,
                index,
            );
        }
        Statement::ExportDeclaration(export) => {
            let decl = &export.declaration;
            index_declaration(decl, ctx, index);
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                index_function(func, ctx, &mut index.value_headers);
            }
            ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                index_class(cls, ctx, index);
                // Mirror `alias_default_export_type_symbol`: the declared
                // class type also answers under the `default` export name.
                if let Some(id) = &cls.id {
                    alias_default_type_header(index, id.name.as_str(), ctx);
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                index_interface(iface, iface.id.name.as_str(), ctx, &mut index.type_headers);
                alias_default_type_header(index, iface.id.name.as_str(), ctx);
            }
            other => {
                if let Some(expr) = other.as_expression() {
                    // Mirrors `extract_default_expression`: a `default`
                    // value symbol of kind `Const`, with object-literal
                    // member headers when the expression is one.
                    let entry = index
                        .value_headers
                        .entry(ctx.key("default"))
                        .or_insert_with(|| ValueDeclHeader {
                            kind: ValueDeclKind::Const,
                            span: verter_span::Span::new(export.span.start, export.span.end),
                            name_span: verter_span::Span::new(export.span.start, export.span.end),
                            object_member_headers: object_literal_member_headers(expr, ctx.source),
                            contributors: Vec::new(),
                        });
                    push_contributor(
                        &mut entry.contributors,
                        ctx,
                        verter_span::Span::new(export.span.start, export.span.end),
                        verter_span::Span::new(export.span.start, export.span.end),
                    );
                }
            }
        },
        _ => {}
    }
}

/// Mirror of `extract_from_declaration` (the `export <decl>` wrapper arms).
fn index_declaration(
    decl: &Declaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            index_type_alias(alias, alias.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            index_interface(iface, iface.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Declaration::TSNamespaceDeclaration(module) => {
            index_module_declaration(module, ctx, index, None, ctx.declaration_file);
        }
        Declaration::TSExternalModuleDeclaration(module) => {
            index_external_module_declaration(module, ctx, index);
        }
        Declaration::TSGlobalDeclaration(global) => {
            index_augmentation_block(&global.body, ctx, index, &AugmentationScopeKind::Global);
        }
        Declaration::ClassDeclaration(cls) => {
            index_class(cls, ctx, index);
        }
        Declaration::FunctionDeclaration(func) => {
            index_function(func, ctx, &mut index.value_headers);
        }
        Declaration::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                index_variable_in(d, var_decl.kind, ctx, index, None);
            }
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            index_enum(
                enum_decl,
                enum_decl.id.name.as_str(),
                ctx,
                ctx.declaration_file,
                index,
            );
        }
        _ => {}
    }
}

/// Index one `enum` declaration's HEADER facts: the member-name inventory
/// (in the dedicated `enum_headers` table — the member-presence authority)
/// plus the dual-space resolution locators (an `enum` is both a type and a
/// value, registered below). No body lowering here — the eval-env walk's
/// enum arm lowers the bodies (the ordered member inventory — a folded literal
/// or degraded primitive per member — and the projected-type union) lazily on
/// demand.
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
    name: &str,
    ctx: HeaderStatementContext<'_>,
    ambient: bool,
    index: &mut DeclHeaderIndex,
) {
    let entry = index
        .enum_headers
        .entry(ctx.key(name))
        .or_insert_with(|| EnumDeclHeader {
            span: verter_span::Span::new(enum_decl.span.start, enum_decl.span.end),
            name_span: verter_span::Span::new(enum_decl.id.span.start, enum_decl.id.span.end),
            member_names: Vec::new(),
            member_positions: Vec::new(),
            contributors: Vec::new(),
        });
    let ambient = ambient || enum_decl.declare;
    let mut seen: FxHashSet<String> = entry.member_names.iter().cloned().collect();
    for member in &enum_decl.body.members {
        let member_name = member.id.static_name().to_string();
        if seen.insert(member_name.clone()) {
            entry.member_names.push(member_name);
            entry.member_positions.push(EnumMemberPosition {
                start: member.span.start,
                ambient,
            });
        }
    }
    push_contributor(
        &mut entry.contributors,
        ctx,
        verter_span::Span::new(enum_decl.span.start, enum_decl.span.end),
        verter_span::Span::new(enum_decl.id.span.start, enum_decl.id.span.end),
    );

    // Dual-space RESOLUTION headers (mirrors `index_class`): an `enum` is
    // BOTH a type (its projected-type union) and a value (its `typeof`
    // object), so it must be reachable through the shared type/value demand
    // path like any other dual-space symbol — not invisible in a side table.
    // The bodies lower lazily through the eval-env enum arm on demand; these
    // headers carry only the locator + kind, with NO members (member names
    // live on `enum_headers` for the member-presence facts rail; the
    // value-space `enum_members` inventory — a folded literal or degraded
    // primitive per member — and the type-space projected-type union are
    // produced at lowering). The type side registers as the `Alias` it
    // structurally is
    // (there is no dedicated enum `TypeDeclKind`).
    upsert_type_header(
        &mut index.type_headers,
        name,
        TypeDeclKind::Alias,
        verter_span::Span::new(enum_decl.span.start, enum_decl.span.end),
        verter_span::Span::new(enum_decl.id.span.start, enum_decl.id.span.end),
        Vec::new(),
        Vec::new(),
        &[],
        ctx,
    );
    let value_entry = index
        .value_headers
        .entry(ctx.key(name))
        .or_insert_with(|| ValueDeclHeader {
            kind: ValueDeclKind::Enum,
            span: verter_span::Span::new(enum_decl.span.start, enum_decl.span.end),
            name_span: verter_span::Span::new(enum_decl.id.span.start, enum_decl.id.span.end),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    value_entry.kind = ValueDeclKind::Enum;
    push_contributor(
        &mut value_entry.contributors,
        ctx,
        verter_span::Span::new(enum_decl.span.start, enum_decl.span.end),
        verter_span::Span::new(enum_decl.id.span.start, enum_decl.id.span.end),
    );
}

/// Mirror of `extract_module_declaration`: a string-literal module name is
/// an ambient augmentation scope; an identifier name is a namespace whose
/// inner type declarations register under qualified `Ns.Name` names.
fn index_external_module_declaration(
    decl: &TSExternalModuleDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    if let Some(block) = decl.body.as_ref() {
        let scope = AugmentationScopeKind::Module(decl.id.value.to_string());
        index_augmentation_block(block, ctx, index, &scope);
    }
}

/// The identifier-named half of [`index_external_module_declaration`]: the
/// namespace and every namespace nested in it, walked from an explicit
/// stack ([`for_each_namespace`]).
fn index_module_declaration(
    decl: &TSNamespaceDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    prefix: Option<&str>,
    ambient: bool,
) {
    for_each_namespace(
        decl,
        &mut HeaderNamespaces {
            ctx,
            index,
            path: QualifiedPath::under(prefix),
            ambient,
        },
    );
}

/// [`index_module_declaration`]'s walk.
struct HeaderNamespaces<'i, 'c> {
    ctx: HeaderStatementContext<'c>,
    index: &'i mut DeclHeaderIndex,
    /// The qualified name of the namespace being walked.
    path: QualifiedPath,
    /// Whether the root namespace is in an ambient context.
    ambient: bool,
}

/// One namespace being indexed.
struct HeaderNamespace {
    /// Its block's record in [`DeclHeaderIndex::namespace_blocks`].
    record: usize,
    /// The qualified path's length before it was entered.
    enclosing: usize,
    ambient: bool,
    implicit_export: bool,
    /// Whether what it declares so far instantiates it.
    instantiated: bool,
}

impl<'s, 'a> NamespaceVisitor<'s, 'a> for HeaderNamespaces<'_, '_> {
    type Frame = HeaderNamespace;

    /// Record the namespace BLOCK itself (EMPTY blocks included — a block
    /// with zero members is still a named lexical scope) at block entry, so
    /// the scope inventory is complete even when no member registers. Its
    /// instantiation is settled when it is left.
    fn enter(
        &mut self,
        decl: &'s TSNamespaceDeclaration<'a>,
        parent: Option<&HeaderNamespace>,
        nesting: Nesting,
    ) -> HeaderNamespace {
        let ambient = parent.map_or(self.ambient, |parent| parent.ambient);
        let enclosing = self.path.enter(decl.id.name.as_str());
        // A namespace a non-exporting body declares without `export` is
        // private to it.
        if nesting == (Nesting::Statement { exported: false })
            && parent.is_some_and(|parent| !parent.implicit_export)
        {
            self.index
                .namespace_private_members
                .insert(self.ctx.key(self.path.name()));
        }
        let ambient = ambient || decl.declare;
        let record = self.index.namespace_blocks.len();
        self.index.namespace_blocks.push(NamespaceBlockRecord {
            owner: self.ctx.anchor.owner,
            qualified_name: self.path.name().to_owned(),
            span: verter_span::Span::new(decl.span.start, decl.span.end),
            instantiated: false,
        });
        let implicit_export = match &decl.body {
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => false,
            TSNamespaceDeclarationBody::TSModuleBlock(block) => {
                ambient && !statements_have_export_declarations(&block.body)
            }
        };
        HeaderNamespace {
            record,
            enclosing,
            ambient,
            implicit_export,
            instantiated: false,
        }
    }

    fn statement(&mut self, frame: &mut HeaderNamespace, statement: &'s Statement<'a>) {
        frame.instantiated |= statement_instantiates_here(statement);
        let block = self.index.namespace_blocks[frame.record].span;
        index_namespaced_statement(
            statement,
            self.ctx,
            self.index,
            self.path.name(),
            NamespaceBody {
                implicit_export: frame.implicit_export,
                ambient: frame.ambient,
                block,
            },
        );
    }

    fn exit(&mut self, frame: HeaderNamespace, parent: Option<&mut HeaderNamespace>) {
        self.path.leave(frame.enclosing);
        self.index.namespace_blocks[frame.record].instantiated = frame.instantiated;
        if let Some(parent) = parent {
            parent.instantiated |= frame.instantiated;
        }
    }
}

/// The namespace body a statement is indexed in.
#[derive(Clone, Copy)]
struct NamespaceBody {
    /// The body exports every member, written `export` or not (an ambient
    /// namespace body without an export declaration).
    implicit_export: bool,
    /// The body is in an ambient context.
    ambient: bool,
    /// The block's span.
    block: Span,
}

/// Record the private namespace value member `name` declared in `body`.
fn index_private_value(
    index: &mut DeclHeaderIndex,
    ctx: HeaderStatementContext<'_>,
    name: &str,
    body: NamespaceBody,
) {
    if body.implicit_export {
        return;
    }
    index.namespace_private_members.insert(ctx.key(name));
    index
        .namespace_private_value_blocks
        .insert(ctx.key(name), body.block);
}

/// Mirror of `collect_namespaced_statement`: type aliases, interfaces and
/// nested modules register under their qualified `Ns.Name`, and so do
/// values: an exported `const`/`let`/`var`/`function` (routed via the
/// `ExportNamedDeclaration` path to `index_namespaced_declaration`) is a
/// member such as `N.VERSION`; a non-exported `const hidden = …` is a
/// private member, named only by a reference inside its block — except in
/// an export context (an ambient namespace body without an export
/// declaration), which exports every member.
fn index_namespaced_statement(
    stmt: &Statement<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: &str,
    body: NamespaceBody,
) {
    let NamespaceBody {
        implicit_export,
        ambient,
        ..
    } = body;
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            index_type_alias(alias, name.as_str(), ctx, &mut index.type_headers);
            if !implicit_export {
                index.namespace_private_members.insert(ctx.key(&name));
            }
        }
        Statement::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            index_interface(iface, name.as_str(), ctx, &mut index.type_headers);
            if !implicit_export {
                index.namespace_private_members.insert(ctx.key(&name));
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                let name = format!("{namespace}.{}", identifier.name);
                index_named_class(class, &name, ctx, index);
                index_private_value(index, ctx, &name, body);
            }
        }
        Statement::TSEnumDeclaration(enum_decl) => {
            let name = format!("{namespace}.{}", enum_decl.id.name);
            index_enum(enum_decl, &name, ctx, ambient, index);
            index_private_value(index, ctx, &name, body);
        }
        // A nested namespace is a frame of [`index_module_declaration`]'s
        // walk.
        Statement::TSNamespaceDeclaration(_) => {}
        Statement::TSExternalModuleDeclaration(module) => {
            index_external_module_declaration(module, ctx, index);
        }
        // Export-only: a DIRECT (non-exported) value declaration is private to
        // a non-ambient namespace body and is NOT indexed. The exported path
        // below (`export const VERSION = …` → `index_namespaced_declaration`)
        // registers a qualified value member.
        Statement::ExportDeclaration(export) => {
            let decl = &export.declaration;
            index_namespaced_declaration(decl, ctx, index, namespace, ambient);
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                index_variable_in(decl, var_decl.kind, ctx, index, Some(namespace));
                for name in decl.id.get_binding_identifiers() {
                    index_private_value(index, ctx, &format!("{namespace}.{}", name.name), body);
                }
            }
        }
        Statement::FunctionDeclaration(func) => {
            index_function_in(func, ctx, &mut index.value_headers, Some(namespace));
            if let Some(id) = &func.id {
                index_private_value(index, ctx, &format!("{namespace}.{}", id.name), body);
            }
        }
        _ => {}
    }
}

fn index_namespaced_declaration(
    decl: &Declaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: &str,
    ambient: bool,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            index_type_alias(alias, name.as_str(), ctx, &mut index.type_headers);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            index_interface(iface, name.as_str(), ctx, &mut index.type_headers);
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                let name = format!("{namespace}.{}", identifier.name);
                index_named_class(class, &name, ctx, index);
            }
        }
        // A nested namespace is a frame of [`index_module_declaration`]'s
        // walk.
        Declaration::TSNamespaceDeclaration(_) => {}
        Declaration::TSExternalModuleDeclaration(module) => {
            index_external_module_declaration(module, ctx, index);
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            let name = format!("{namespace}.{}", enum_decl.id.name);
            index_enum(enum_decl, &name, ctx, ambient, index);
        }
        Declaration::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                index_variable_in(decl, var_decl.kind, ctx, index, Some(namespace));
            }
        }
        Declaration::FunctionDeclaration(func) => {
            index_function_in(func, ctx, &mut index.value_headers, Some(namespace));
        }
        _ => {}
    }
}

// ───────────────────────────────────────────────────────────────────────
// Per-declaration header builders
// ───────────────────────────────────────────────────────────────────────

fn index_type_alias(
    decl: &TSTypeAliasDeclaration<'_>,
    name: &str,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<TypeDeclHeader>,
) {
    let params = type_param_headers(decl.type_parameters.as_deref());
    let members = alias_body_member_headers(&decl.type_annotation, ctx.source);
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Alias,
        verter_span::Span::new(decl.span.start, decl.span.end),
        verter_span::Span::new(decl.id.span.start, decl.id.span.end),
        params,
        members,
        &[],
        ctx,
    );
}

fn index_interface(
    decl: &TSInterfaceDeclaration<'_>,
    name: &str,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<TypeDeclHeader>,
) {
    let params = type_param_headers(decl.type_parameters.as_deref());
    let mut members = Vec::new();
    for sig in &decl.body.body {
        if let Some(header) = interface_member_header(sig, ctx.source) {
            members.push(header);
        }
    }
    let ignored_heritage_arms = vue_ignored_heritage_arms(decl, ctx);
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Interface,
        verter_span::Span::new(decl.span.start, decl.span.end),
        verter_span::Span::new(decl.id.span.start, decl.id.span.end),
        params,
        members,
        &ignored_heritage_arms,
        ctx,
    );
}

fn vue_ignored_heritage_arms(
    decl: &TSInterfaceDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
) -> Vec<u32> {
    let mut lowered_arm_ordinal = 0u32;
    let mut ignored = Vec::new();

    for heritage in &decl.extends {
        if !is_lowerable_heritage_type_name(&heritage.type_name) {
            continue;
        }
        if matches!(heritage.type_name, TSTypeName::IdentifierReference(_))
            && ctx
                .vue_ignore_attachment_starts
                .contains(&heritage.type_name.span().start)
        {
            ignored.push(lowered_arm_ordinal);
        }
        let Some(next) = lowered_arm_ordinal.checked_add(1) else {
            break;
        };
        lowered_arm_ordinal = next;
    }
    ignored
}

fn is_lowerable_heritage_type_name(name: &TSTypeName<'_>) -> bool {
    match name {
        TSTypeName::IdentifierReference(_) => true,
        TSTypeName::QualifiedName(qualified) => {
            let mut left = &qualified.left;
            loop {
                match left {
                    TSTypeName::IdentifierReference(_) => return true,
                    TSTypeName::QualifiedName(parent) => left = &parent.left,
                    TSTypeName::ThisExpression(_) => return false,
                }
            }
        }
        TSTypeName::ThisExpression(_) => false,
    }
}

/// Mirror of `extract_class`'s NAME registration: a named class declares a
/// type symbol (instance members) AND a value symbol (constructor shape +
/// static members). An anonymous class declares nothing.
fn index_class(decl: &Class<'_>, ctx: HeaderStatementContext<'_>, index: &mut DeclHeaderIndex) {
    let Some(id) = &decl.id else {
        return;
    };
    index_named_class(decl, id.name.as_str(), ctx, index);
}

/// The accessibility a class's FIRST authored constructor declares (the
/// checker reads the first construct signature's declaration); `None`
/// when the class declares no constructor.
fn constructor_visibility(decl: &Class<'_>) -> Option<verter_type_expr::MemberVisibility> {
    let constructor = decl.body.body.iter().find_map(|element| match element {
        ClassElement::MethodDefinition(method)
            if method.kind == oxc_ast::ast::MethodDefinitionKind::Constructor =>
        {
            Some(method)
        }
        _ => None,
    })?;
    Some(match constructor.accessibility {
        Some(oxc_ast::ast::TSAccessibility::Private) => verter_type_expr::MemberVisibility::Private,
        Some(oxc_ast::ast::TSAccessibility::Protected) => {
            verter_type_expr::MemberVisibility::Protected
        }
        Some(oxc_ast::ast::TSAccessibility::Public) | None => {
            verter_type_expr::MemberVisibility::Public
        }
    })
}

/// Classify one field of the class `name` once for every consumer (see
/// `class_field_value`) and index the synthetic value a field read through
/// one declares, mirroring `collect_named_class`.
fn index_class_field_value(
    class: &Class<'_>,
    name: &str,
    prop: &oxc_ast::ast::PropertyDefinition<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    let classified = crate::analysis::class_field_value::classify_class_field(
        std::sync::Arc::make_mut(&mut index.class_field_values),
        class,
        prop,
        ctx.source,
    );
    if let (Some(_), Some(field_name), Some(value)) = (
        classified,
        crate::analysis::type_eval_build::class_field_value_name(name, prop),
        prop.value.as_ref(),
    ) {
        let span: Span = verter_span::Span::new(value.span().start, value.span().end);
        let entry = index
            .value_headers
            .entry(ctx.key(&field_name))
            .or_insert_with(|| ValueDeclHeader {
                kind: if prop.readonly {
                    ValueDeclKind::Const
                } else {
                    ValueDeclKind::Let
                },
                span,
                name_span: span,
                object_member_headers: Vec::new(),
                contributors: Vec::new(),
            });
        push_contributor(&mut entry.contributors, ctx, span, span);
    }
}

fn index_named_class(
    decl: &Class<'_>,
    name: &str,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    let Some(id) = &decl.id else {
        return;
    };
    if decl.r#abstract {
        index.abstract_classes.insert(ctx.key(name));
    }
    if let Some(visibility) = constructor_visibility(decl) {
        index
            .constructor_visibility
            .insert(ctx.key(name), visibility);
    }
    // A heritage EXPRESSION's synthetic value (see
    // `class_heritage_value_name`), mirroring `collect_named_class`.
    if let Some(heritage) = decl
        .heritage
        .as_ref()
        .map(|heritage| &heritage.expression)
        .filter(|heritage| {
            crate::analysis::type_eval_build::heritage_expression_name(heritage).is_none()
        })
    {
        let span: Span = verter_span::Span::new(heritage.span().start, heritage.span().end);
        let entry = index
            .value_headers
            .entry(ctx.key(&crate::analysis::type_eval_build::class_heritage_value_name(name)))
            .or_insert_with(|| ValueDeclHeader {
                kind: ValueDeclKind::Const,
                span,
                name_span: span,
                object_member_headers: Vec::new(),
                contributors: Vec::new(),
            });
        push_contributor(&mut entry.contributors, ctx, span, span);
    }

    let params = type_param_headers(decl.type_parameters.as_deref());
    let mut instance_members = Vec::new();
    let mut static_members = Vec::new();
    for element in &decl.body.body {
        match element {
            ClassElement::PropertyDefinition(prop) => {
                // A `#private` brand is not a type-level member: it never
                // lands on the instance or static surface.
                if matches!(prop.key, PropertyKey::PrivateIdentifier(_)) {
                    continue;
                }
                index_class_field_value(decl, name, prop, ctx, index);
                let header = MemberHeader {
                    key: lower_property_key(&prop.key, ctx.source),
                    method_kind: None,
                    has_implementation_body: false,
                    optional: prop.optional,
                    readonly: prop.readonly,
                };
                if prop.r#static {
                    static_members.push(header);
                } else {
                    instance_members.push(header);
                }
            }
            ClassElement::MethodDefinition(method) => {
                if method.kind == MethodDefinitionKind::Constructor {
                    continue;
                }
                if matches!(method.key, PropertyKey::PrivateIdentifier(_)) {
                    continue;
                }
                let header = MemberHeader {
                    key: lower_property_key(&method.key, ctx.source),
                    method_kind: Some(match method.kind {
                        MethodDefinitionKind::Get => ObjectMethodKind::Get,
                        MethodDefinitionKind::Set => ObjectMethodKind::Set,
                        MethodDefinitionKind::Method => ObjectMethodKind::Method,
                        MethodDefinitionKind::Constructor => unreachable!(),
                    }),
                    has_implementation_body: method.value.body.is_some(),
                    optional: method.optional,
                    readonly: false,
                };
                if method.r#static {
                    static_members.push(header);
                } else {
                    instance_members.push(header);
                }
            }
            _ => {}
        }
    }

    upsert_type_header(
        &mut index.type_headers,
        name,
        TypeDeclKind::Class,
        verter_span::Span::new(decl.span.start, decl.span.end),
        verter_span::Span::new(id.span.start, id.span.end),
        params,
        instance_members,
        &[],
        ctx,
    );

    let entry = index
        .value_headers
        .entry(ctx.key(name))
        .or_insert_with(|| ValueDeclHeader {
            kind: ValueDeclKind::Class,
            span: verter_span::Span::new(decl.span.start, decl.span.end),
            name_span: verter_span::Span::new(id.span.start, id.span.end),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = ValueDeclKind::Class;
    entry.span = verter_span::Span::new(decl.span.start, decl.span.end);
    entry.name_span = verter_span::Span::new(id.span.start, id.span.end);
    for header in static_members {
        if !entry
            .object_member_headers
            .iter()
            .any(|existing| existing.key == header.key)
        {
            entry.object_member_headers.push(header);
        }
    }
    push_contributor(
        &mut entry.contributors,
        ctx,
        verter_span::Span::new(decl.span.start, decl.span.end),
        verter_span::Span::new(id.span.start, id.span.end),
    );
}

fn index_function(
    func: &oxc_ast::ast::Function<'_>,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
) {
    index_function_in(func, ctx, table, None);
}

/// [`index_function`] for a function declared in `namespace`: indexed under
/// its QUALIFIED name `NS.f`, as a namespaced variable is.
fn index_function_in(
    func: &oxc_ast::ast::Function<'_>,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
    namespace: Option<&str>,
) {
    let Some(id) = &func.id else {
        return;
    };
    let kind = if func.r#async {
        ValueDeclKind::AsyncFunction
    } else {
        ValueDeclKind::Function
    };
    let key = match namespace {
        Some(ns) => format!("{ns}.{}", id.name),
        None => id.name.to_string(),
    };
    let entry = table
        .entry(ctx.key(&key))
        .or_insert_with(|| ValueDeclHeader {
            kind,
            span: verter_span::Span::new(func.span.start, func.span.end),
            name_span: verter_span::Span::new(id.span.start, id.span.end),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    // Last contributor wins for the representative kind/spans (matching
    // `ValueDeclGroup::primary`).
    entry.kind = kind;
    entry.span = verter_span::Span::new(func.span.start, func.span.end);
    entry.name_span = verter_span::Span::new(id.span.start, id.span.end);
    push_contributor(
        &mut entry.contributors,
        ctx,
        verter_span::Span::new(func.span.start, func.span.end),
        verter_span::Span::new(id.span.start, id.span.end),
    );
}

/// [`index_variable`] into `index`'s value headers, with the fields of a
/// class expression the declarator holds classified as a class
/// declaration's are (see `class_field_value`), so the class lowering and
/// the function-program discovery read that one answer.
fn index_variable_in(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: Option<&str>,
) {
    index_variable(decl, kind, ctx, &mut index.value_headers, namespace);
    if let Some(class) = decl
        .init
        .as_ref()
        .filter(|_| decl.type_annotation.is_none())
        .and_then(crate::analysis::type_eval_build::initializer_class_expression)
    {
        crate::analysis::class_field_value::classify_class_fields(
            std::sync::Arc::make_mut(&mut index.class_field_values),
            class,
            ctx.source,
        );
    }
}

fn index_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
    namespace: Option<&str>,
) {
    let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &decl.id else {
        index_destructured_variable(decl, kind, ctx, table, namespace);
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
        .map(|init| object_literal_member_headers(init, ctx.source))
        .unwrap_or_default();
    // A namespaced value member (`namespace NS { export const M = … }`) is
    // indexed under its QUALIFIED name `NS.M`, mirroring the qualified TYPE
    // member index (`NS.Point`), so `typeof NS.M` binds the value root.
    let key = match namespace {
        Some(ns) => format!("{ns}.{}", id.name),
        None => id.name.to_string(),
    };
    let entry = table
        .entry(ctx.key(&key))
        .or_insert_with(|| ValueDeclHeader {
            kind: var_kind,
            span: verter_span::Span::new(decl.span.start, decl.span.end),
            name_span: verter_span::Span::new(id.span.start, id.span.end),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    entry.kind = var_kind;
    entry.span = verter_span::Span::new(decl.span.start, decl.span.end);
    entry.name_span = verter_span::Span::new(id.span.start, id.span.end);
    for header in members {
        if !entry
            .object_member_headers
            .iter()
            .any(|existing| existing.key == header.key)
        {
            entry.object_member_headers.push(header);
        }
    }
    push_contributor(
        &mut entry.contributors,
        ctx,
        verter_span::Span::new(decl.span.start, decl.span.end),
        verter_span::Span::new(id.span.start, id.span.end),
    );
}

/// Index the element bindings of a DESTRUCTURING declarator — every
/// binding identifier a static, string or numeric key or an array position
/// names (the elements the value lowering types; a rest element and a
/// computed key declare no value header).
fn index_destructured_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
    namespace: Option<&str>,
) {
    use oxc_ast::ast::{BindingPattern, PropertyKey};
    fn leaves<'a>(
        pattern: &'a BindingPattern<'a>,
        out: &mut Vec<&'a oxc_ast::ast::BindingIdentifier<'a>>,
    ) {
        let element = |element: &'a BindingPattern<'a>, out: &mut Vec<_>| match element {
            BindingPattern::AssignmentPattern(assignment) => leaves(&assignment.left, out),
            other => leaves(other, out),
        };
        match pattern {
            BindingPattern::BindingIdentifier(id) => out.push(id),
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    let named = match &property.key {
                        PropertyKey::StaticIdentifier(_) => !property.computed,
                        PropertyKey::StringLiteral(_) | PropertyKey::NumericLiteral(_) => true,
                        _ => false,
                    };
                    if named {
                        element(&property.value, out);
                    }
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for item in array.elements.iter().flatten() {
                    element(item, out);
                }
            }
            BindingPattern::AssignmentPattern(_) => {}
        }
    }
    let var_kind = match kind {
        VariableDeclarationKind::Const
        | VariableDeclarationKind::Using
        | VariableDeclarationKind::AwaitUsing => ValueDeclKind::Const,
        VariableDeclarationKind::Let => ValueDeclKind::Let,
        VariableDeclarationKind::Var => ValueDeclKind::Var,
    };
    let mut ids = Vec::new();
    leaves(&decl.id, &mut ids);
    for id in ids {
        let key = match namespace {
            Some(ns) => format!("{ns}.{}", id.name),
            None => id.name.to_string(),
        };
        let entry = table
            .entry(ctx.key(&key))
            .or_insert_with(|| ValueDeclHeader {
                kind: var_kind,
                span: verter_span::Span::new(decl.span.start, decl.span.end),
                name_span: verter_span::Span::new(id.span.start, id.span.end),
                object_member_headers: Vec::new(),
                contributors: Vec::new(),
            });
        entry.kind = var_kind;
        entry.span = verter_span::Span::new(decl.span.start, decl.span.end);
        entry.name_span = verter_span::Span::new(id.span.start, id.span.end);
        push_contributor(
            &mut entry.contributors,
            ctx,
            verter_span::Span::new(decl.span.start, decl.span.end),
            verter_span::Span::new(id.span.start, id.span.end),
        );
    }
}

/// Mirror of `alias_default_export_type_symbol`: clone the declared-name
/// header under `default` (no-op when `default` already exists or the
/// declared name produced no type header).
fn alias_default_type_header(
    index: &mut DeclHeaderIndex,
    declared_name: &str,
    ctx: HeaderStatementContext<'_>,
) {
    let default_key = ctx.key("default");
    if index.type_headers.contains_key(&default_key) {
        return;
    }
    let Some(declared) = index.type_headers.get(&ctx.key(declared_name)) else {
        return;
    };
    let mut aliased = declared.clone();
    aliased
        .contributors
        .retain(|entry| entry.anchor == ctx.anchor);
    index.type_headers.insert(default_key, aliased);
}

#[allow(clippy::too_many_arguments)]
fn upsert_type_header(
    table: &mut DeclMap<TypeDeclHeader>,
    name: &str,
    kind: TypeDeclKind,
    span: Span,
    name_span: Span,
    params: Vec<TypeParamHeader>,
    members: Vec<MemberHeader>,
    ignored_heritage_arm_ordinals: &[u32],
    ctx: HeaderStatementContext<'_>,
) {
    let entry = table
        .entry(ctx.key(name))
        .or_insert_with(|| TypeDeclHeader {
            kind,
            span,
            name_span,
            type_params: Vec::new(),
            member_headers: Vec::new(),
            contributors: Vec::new(),
            vue_ignored_heritage: Vec::new(),
            from_jsdoc_typedef: false,
            jsdoc_typedef: None,
        });
    // Last contributor wins for the representative kind/spans (matching
    // `TypeDeclGroup::primary`); params and members UNION across
    // contributors in first-seen order (matching the lowered group's
    // parameter-union and `merged_member_header_facts`' first-seen member
    // rules).
    entry.kind = kind;
    entry.span = span;
    entry.name_span = name_span;
    entry.from_jsdoc_typedef = false;
    entry.jsdoc_typedef = None;
    for param in params {
        if !entry.type_params.iter().any(|p| p.name == param.name) {
            entry.type_params.push(param);
        }
    }
    for member in members {
        if !entry
            .member_headers
            .iter()
            .any(|existing| existing.key == member.key)
        {
            entry.member_headers.push(member);
        }
    }
    let contributor_ordinal = entry
        .contributors
        .iter()
        .position(|contributor| contributor.anchor == ctx.anchor)
        .unwrap_or(entry.contributors.len());
    if let Ok(contributor_ordinal) = u32::try_from(contributor_ordinal) {
        for &intersection_arm_ordinal in ignored_heritage_arm_ordinals {
            let fact = VueIgnoredHeritageFact {
                contributor_ordinal,
                intersection_arm_ordinal,
            };
            if !entry.vue_ignored_heritage.contains(&fact) {
                entry.vue_ignored_heritage.push(fact);
            }
        }
    }
    push_contributor(&mut entry.contributors, ctx, span, name_span);
}

fn push_contributor(
    contributors: &mut Vec<DeclHeaderContributor>,
    ctx: HeaderStatementContext<'_>,
    declaration_span: Span,
    name_span: Span,
) {
    // Dedup TRUE duplicates only (same statement anchor AND same
    // declaration span — one statement re-registering the SAME
    // declaration). Two DISTINCT declarations in one statement or block
    // (e.g. `interface A {} interface B {} interface A {}` inside one
    // `declare module "m"`) share the anchor but have distinct spans:
    // BOTH are recorded, at their authored positions. The lazy
    // body-lowering consumers dedup statement anchors at consumption
    // (`decl_body_memo`), so a same-anchor duplicate never lowers the
    // same statement twice.
    if contributors.last().is_some_and(|entry| {
        entry.anchor == ctx.anchor && entry.declaration_span == declaration_span
    }) {
        return;
    }
    contributors.push(DeclHeaderContributor {
        anchor: ctx.anchor,
        declaration_span,
        name_span,
    });
}

fn type_param_headers(decl: Option<&TSTypeParameterDeclaration<'_>>) -> Vec<TypeParamHeader> {
    let Some(decl) = decl else {
        return Vec::new();
    };
    decl.params
        .iter()
        .map(|param| TypeParamHeader {
            name: param.name.name.to_string(),
            constraint_span: param
                .constraint
                .as_ref()
                .map(|c| verter_span::Span::new(c.span().start, c.span().end)),
            default_span: param
                .default
                .as_ref()
                .map(|d| verter_span::Span::new(d.span().start, d.span().end)),
        })
        .collect()
}

/// Direct syntactic member headers of a type-alias body: a `TSTypeLiteral`
/// contributes its named members; intersection / parenthesized arms are
/// descended (mirroring the lowered inventory's own-member header facts).
/// Every other body shape has no direct syntactic members.
fn alias_body_member_headers(ty: &TSType<'_>, source: &str) -> Vec<MemberHeader> {
    let mut out = Vec::new();
    collect_alias_member_headers(ty, source, &mut out);
    out
}

fn collect_alias_member_headers(ty: &TSType<'_>, source: &str, out: &mut Vec<MemberHeader>) {
    match ty {
        TSType::TSTypeLiteral(literal) => {
            for sig in &literal.members {
                if let Some(header) = interface_member_header(sig, source) {
                    if !out.iter().any(|existing| existing.key == header.key) {
                        out.push(header);
                    }
                }
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for part in &intersection.types {
                collect_alias_member_headers(part, source, out);
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_alias_member_headers(&paren.type_annotation, source, out);
        }
        _ => {}
    }
}

fn interface_member_header(sig: &TSSignature<'_>, source: &str) -> Option<MemberHeader> {
    match sig {
        TSSignature::TSPropertySignature(prop) => Some(MemberHeader {
            key: lower_property_key(&prop.key, source),
            method_kind: None,
            has_implementation_body: false,
            optional: prop.optional,
            readonly: prop.readonly,
        }),
        TSSignature::TSMethodSignature(method) => Some(MemberHeader {
            key: lower_property_key(&method.key, source),
            method_kind: Some(ObjectMethodKind::Method),
            has_implementation_body: false,
            optional: method.optional,
            readonly: false,
        }),
        _ => None,
    }
}

/// Direct member headers of an object-literal initializer, seen through
/// `as` / `satisfies` / parenthesized wrappers (mirroring
/// `extract_initializer_object_shape`).
fn object_literal_member_headers(expr: &Expression<'_>, source: &str) -> Vec<MemberHeader> {
    match expr {
        Expression::ObjectExpression(obj) => object_expression_member_headers(obj, source),
        Expression::TSAsExpression(ts_as) => {
            object_literal_member_headers(&ts_as.expression, source)
        }
        Expression::TSSatisfiesExpression(sat) => {
            object_literal_member_headers(&sat.expression, source)
        }
        Expression::ParenthesizedExpression(paren) => {
            object_literal_member_headers(&paren.expression, source)
        }
        _ => Vec::new(),
    }
}

fn object_expression_member_headers(obj: &ObjectExpression<'_>, source: &str) -> Vec<MemberHeader> {
    let mut out: Vec<MemberHeader> = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key = lower_property_key(&p.key, source);
            // Mirror `push_object_property_with_override`: a duplicate
            // key's LAST occurrence wins.
            out.retain(|existing| existing.key != key);
            out.push(MemberHeader {
                key,
                method_kind: if p.method || !matches!(p.kind, oxc_ast::ast::PropertyKind::Init) {
                    Some(match p.kind {
                        oxc_ast::ast::PropertyKind::Get => ObjectMethodKind::Get,
                        oxc_ast::ast::PropertyKind::Set => ObjectMethodKind::Set,
                        oxc_ast::ast::PropertyKind::Init => ObjectMethodKind::Method,
                    })
                } else {
                    None
                },
                has_implementation_body: p.method
                    || !matches!(p.kind, oxc_ast::ast::PropertyKind::Init),
                optional: false,
                readonly: false,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "decl_headers_tests.rs"]
mod decl_headers_tests;
