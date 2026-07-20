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
    Class, ClassElement, Comment, Declaration, ExportDefaultDeclarationKind, Expression,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSEnumDeclaration, TSInterfaceDeclaration, TSModuleBlock, TSModuleDeclaration,
    TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature, TSType, TSTypeAliasDeclaration,
    TSTypeParameterDeclaration, VariableDeclarationKind, VariableDeclarator,
};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;
use verter_type_expr::facts::VueIgnoredHeritageFact;
use verter_type_expr::span_origins::DeclContributorAnchor;
use verter_type_expr::{DeclBindingKey, TopLevelOwnerId};
use verter_type_expr_oxc::property_key_name;

use crate::analysis::top_level_owners::{DeclMap, TopLevelOwnerTable, TopLevelStatementOwner};
use crate::analysis::type_eval::{AugmentationScopeKind, TypeDeclKind, ValueDeclKind};

#[path = "decl_headers_augmentation.rs"]
mod augmentation;
use augmentation::index_augmentation_block;

#[derive(Debug, Clone, Copy)]
struct HeaderStatementContext<'a> {
    anchor: DeclContributorAnchor,
    vue_ignore_attachment_starts: &'a FxHashSet<u32>,
}

impl<'a> HeaderStatementContext<'a> {
    fn new(
        statement_index: usize,
        owner: TopLevelStatementOwner,
        vue_ignore_attachment_starts: &'a FxHashSet<u32>,
    ) -> Option<Self> {
        Some(Self {
            anchor: DeclContributorAnchor {
                contributor_index: u32::try_from(statement_index).ok()?,
                owner: owner.owner,
                owner_local_ordinal: owner.owner_local_ordinal,
            },
            vue_ignore_attachment_starts,
        })
    }

    fn key(self, name: &str) -> DeclBindingKey {
        DeclBindingKey::new(self.anchor.owner, name)
    }
}

/// Exact authored contributor record retained by the shallow header index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclHeaderContributor {
    pub anchor: DeclContributorAnchor,
    pub declaration_span: Span,
    pub name_span: Span,
}

/// Exact parser-authored locator for a JSDoc typedef declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsdocTypedefHeader {
    pub owner: TopLevelOwnerId,
    pub attached_to: u32,
    pub comment_span: Span,
    pub name_span: Span,
    pub statement_index: Option<u32>,
    pub owner_local_ordinal: Option<u32>,
}

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
    /// first-seen order (matching `TypeDeclGroup::merged_member_header_facts`'
    /// own-member inventory: heritage members are NOT included).
    pub member_headers: Vec<MemberHeader>,
    /// Source-order top-level statement indices of every contributing
    /// statement (deduplicated; one statement can contribute several
    /// same-name declarations).
    pub contributors: Vec<DeclHeaderContributor>,
    /// Vue runtime-only heritage suppression, addressed against the exact
    /// lowered contributor/heritage-arm shape. This is parser-authored
    /// comment meaning captured once at indexing; consumers never rescan
    /// source text or comments.
    pub vue_ignored_heritage: Vec<VueIgnoredHeritageFact>,
    /// `true` when the name exists ONLY as a JSDoc `@typedef` (no TS
    /// declaration claimed it — TS-decl precedence applied at build).
    pub from_jsdoc_typedef: bool,
    /// Exact comment identity for a JSDoc-only typedef header.
    pub jsdoc_typedef: Option<JsdocTypedefHeader>,
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
    pub contributors: Vec<DeclHeaderContributor>,
}

/// Header record for one `enum` declaration. The dedicated table carries
/// the member NAMES + statement locators for the member-presence facts rail
/// (each variant is a `MemberKind::EnumMember`). The enum is ALSO registered
/// as a dual-space type + value header (see [`index_enum`]) so it resolves
/// through the shared demand path; this table is the member-name authority,
/// not a sign that enums are absent from the value/type inventory.
#[derive(Debug, Clone)]
pub struct EnumDeclHeader {
    pub span: Span,
    pub name_span: Span,
    pub member_names: Vec<String>,
    pub contributors: Vec<DeclHeaderContributor>,
}

/// The shallow declaration-header index for one parsed program.
///
/// Augmentation tables are NESTED maps (`scope → name → header`) so a
/// scoped lookup needs no allocated tuple key.
#[derive(Debug, Clone, Default)]
pub struct DeclHeaderIndex {
    pub type_headers: DeclMap<TypeDeclHeader>,
    pub value_headers: DeclMap<ValueDeclHeader>,
    pub enum_headers: DeclMap<EnumDeclHeader>,
    pub augmentation_type_headers: FxHashMap<AugmentationScopeKind, DeclMap<TypeDeclHeader>>,
    pub augmentation_value_headers: FxHashMap<AugmentationScopeKind, DeclMap<ValueDeclHeader>>,
}

impl DeclHeaderIndex {
    /// Look up a file-scope type header.
    pub fn type_header(&self, name: &str) -> Option<&TypeDeclHeader> {
        self.type_header_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn type_header_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&TypeDeclHeader> {
        self.type_headers.get(&DeclBindingKey::new(owner, name))
    }

    /// Look up a file-scope value header.
    pub fn value_header(&self, name: &str) -> Option<&ValueDeclHeader> {
        self.value_header_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn value_header_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&ValueDeclHeader> {
        self.value_headers.get(&DeclBindingKey::new(owner, name))
    }

    /// Look up an augmentation-scoped type header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_type_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&TypeDeclHeader> {
        self.augmentation_type_header_in(scope, TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn augmentation_type_header_in(
        &self,
        scope: &AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&TypeDeclHeader> {
        self.augmentation_type_headers
            .get(scope)?
            .get(&DeclBindingKey::new(owner, name))
    }

    /// Look up an augmentation-scoped value header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_value_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&ValueDeclHeader> {
        self.augmentation_value_header_in(scope, TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn augmentation_value_header_in(
        &self,
        scope: &AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&ValueDeclHeader> {
        self.augmentation_value_headers
            .get(scope)?
            .get(&DeclBindingKey::new(owner, name))
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
                for param in decl.type_parameters.params.iter() {
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
                .merged_member_header_facts()
                .into_iter()
                .map(|fact| MemberHeader {
                    name: fact.name,
                    kind: if fact.is_method {
                        MemberHeaderKind::Method
                    } else {
                        MemberHeaderKind::Property
                    },
                    optional: fact.optional,
                    readonly: fact.readonly,
                })
                .collect();
            TypeDeclHeader {
                kind: primary.kind,
                span: Span::default(),
                name_span: Span::default(),
                type_params,
                member_headers,
                contributors: Vec::new(),
                vue_ignored_heritage: Vec::new(),
                from_jsdoc_typedef: false,
                jsdoc_typedef: None,
            }
        }

        fn value_header_from_group(group: &ValueDeclGroup) -> ValueDeclHeader {
            let primary = group.primary();
            let object_member_headers = primary
                .object_shape
                .as_ref()
                .map(|shape| {
                    shape
                        .members
                        .iter()
                        .filter_map(|member| {
                            let name = match member {
                                verter_type_expr::facts::ObjectMemberFact::Property(p) => {
                                    Some(p.name.clone())
                                }
                                verter_type_expr::facts::ObjectMemberFact::Method(m) => {
                                    Some(m.name.clone())
                                }
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
            // An `enum` is a dual-space symbol: post-merge it is a VALUE
            // symbol (kind Enum) carrying its ordered member inventory. The
            // production `index_enum` path ALSO records the member NAMES in
            // the dedicated `enum_headers` table (the member-presence
            // authority), so this env-seeded mirror must too — else a seeded
            // `DeclHeaderIndex` UNDER-COUNTS `enum_symbol_names()` /
            // `enum_member_names()` and the parse-stable-hash enum-header fold
            // plus the enum `MemberPresence` fact emission go wrong for seeded
            // artifacts. The presence rail is the FULL member-NAME set — the
            // stored `EnumMemberNamesFact` inventory unioned across
            // contributors (`merged_enum_member_names_fact`), EVERY
            // statically-named member including unfoldable-VALUE ones — which
            // is the SUPERSET the value rail (`merged_enum_members`) filters;
            // both resolve names via the same `static_name` helper
            // `index_enum` uses, so the seeded mirror reconstructs
            // `index_enum`'s exact union (a value subset would drop
            // unfoldable-value and computed-name members). `Some` exactly when
            // a contributor is an enum. Locators/spans stay empty like every
            // other seeded header (a seeded index never drives selective
            // statement lowering — its memo is pre-filled).
            if let Some(names_fact) = group.merged_enum_member_names_fact() {
                index.enum_headers.insert(
                    name.clone(),
                    EnumDeclHeader {
                        span: Span::default(),
                        name_span: Span::default(),
                        member_names: names_fact.names.iter().cloned().collect(),
                        contributors: Vec::new(),
                    },
                );
            }
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
    match stmt {
        Statement::TSTypeAliasDeclaration(decl) => {
            index_type_alias(decl, decl.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::TSInterfaceDeclaration(decl) => {
            index_interface(decl, decl.id.name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::TSModuleDeclaration(module) => {
            index_module_declaration(module, ctx, index, None);
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
                index_variable(decl, var_decl.kind, ctx, &mut index.value_headers, None);
            }
        }
        Statement::TSEnumDeclaration(enum_decl) => {
            index_enum(enum_decl, ctx, index);
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_declaration(decl, ctx, index);
            }
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
                            span: export.span.into(),
                            name_span: export.span.into(),
                            object_member_headers: object_literal_member_headers(expr),
                            contributors: Vec::new(),
                        });
                    push_contributor(
                        &mut entry.contributors,
                        ctx,
                        export.span.into(),
                        export.span.into(),
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
        Declaration::TSModuleDeclaration(module) => {
            index_module_declaration(module, ctx, index, None);
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
                index_variable(d, var_decl.kind, ctx, &mut index.value_headers, None);
            }
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            index_enum(enum_decl, ctx, index);
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
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    let name = enum_decl.id.name.as_str();
    let entry = index
        .enum_headers
        .entry(ctx.key(name))
        .or_insert_with(|| EnumDeclHeader {
            span: enum_decl.span.into(),
            name_span: enum_decl.id.span.into(),
            member_names: Vec::new(),
            contributors: Vec::new(),
        });
    for member in &enum_decl.body.members {
        let member_name = member.id.static_name().to_string();
        if !entry
            .member_names
            .iter()
            .any(|existing| existing == &member_name)
        {
            entry.member_names.push(member_name);
        }
    }
    push_contributor(
        &mut entry.contributors,
        ctx,
        enum_decl.span.into(),
        enum_decl.id.span.into(),
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
        enum_decl.span.into(),
        enum_decl.id.span.into(),
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
            span: enum_decl.span.into(),
            name_span: enum_decl.id.span.into(),
            object_member_headers: Vec::new(),
            contributors: Vec::new(),
        });
    value_entry.kind = ValueDeclKind::Enum;
    push_contributor(
        &mut value_entry.contributors,
        ctx,
        enum_decl.span.into(),
        enum_decl.id.span.into(),
    );
}

/// Mirror of `extract_module_declaration`: a string-literal module name is
/// an ambient augmentation scope; an identifier name is a namespace whose
/// inner type declarations register under qualified `Ns.Name` names.
fn index_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    prefix: Option<&str>,
) {
    if let TSModuleDeclarationName::StringLiteral(spec) = &decl.id {
        if let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = decl.body.as_ref() {
            let scope = AugmentationScopeKind::Module(spec.value.to_string());
            index_augmentation_block(block, ctx, index, &scope);
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
            index_module_declaration(inner, ctx, index, Some(module_name.as_str()));
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                index_namespaced_statement(stmt, ctx, index, module_name.as_str());
            }
        }
    }
}

/// Mirror of `extract_namespaced_statement`: type aliases, interfaces and
/// nested modules register under their qualified `Ns.Name`. Namespace VALUE
/// indexing is EXPORT-ONLY — only an `export const`/`let`/`var` (routed via the
/// `ExportNamedDeclaration` path to `index_namespaced_declaration`) registers a
/// qualified value member such as `N.VERSION`; a non-exported `const hidden = …`
/// is private to the namespace body and is intentionally NOT indexed.
fn index_namespaced_statement(
    stmt: &Statement<'_>,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
    namespace: &str,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            let name = format!("{namespace}.{}", alias.id.name);
            index_type_alias(alias, name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::TSInterfaceDeclaration(iface) => {
            let name = format!("{namespace}.{}", iface.id.name);
            index_interface(iface, name.as_str(), ctx, &mut index.type_headers);
        }
        Statement::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                let name = format!("{namespace}.{}", identifier.name);
                index_named_class(class, &name, ctx, index);
            }
        }
        Statement::TSModuleDeclaration(module) => {
            index_module_declaration(module, ctx, index, Some(namespace));
        }
        // Export-only: a DIRECT (non-exported) `VariableDeclaration` is private
        // to the namespace body and is intentionally NOT indexed. Only the
        // exported path below (`export const VERSION = …` →
        // `index_namespaced_declaration`) registers a qualified value member.
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                index_namespaced_declaration(decl, ctx, index, namespace);
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
        Declaration::TSModuleDeclaration(module) => {
            index_module_declaration(module, ctx, index, Some(namespace));
        }
        Declaration::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                index_variable(
                    decl,
                    var_decl.kind,
                    ctx,
                    &mut index.value_headers,
                    Some(namespace),
                );
            }
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
    let members = alias_body_member_headers(&decl.type_annotation);
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Alias,
        decl.span.into(),
        decl.id.span.into(),
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
        if let Some(header) = interface_member_header(sig) {
            members.push(header);
        }
    }
    let ignored_heritage_arms = vue_ignored_heritage_arms(decl, ctx);
    upsert_type_header(
        table,
        name,
        TypeDeclKind::Interface,
        decl.span.into(),
        decl.id.span.into(),
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
        if !is_lowerable_heritage_expression(&heritage.expression) {
            continue;
        }
        if matches!(heritage.expression, Expression::Identifier(_))
            && ctx
                .vue_ignore_attachment_starts
                .contains(&heritage.expression.span().start)
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

fn is_lowerable_heritage_expression(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_) => true,
        Expression::StaticMemberExpression(member) => {
            let mut object = &member.object;
            loop {
                match object {
                    Expression::Identifier(_) => return true,
                    Expression::StaticMemberExpression(parent) => object = &parent.object,
                    _ => return false,
                }
            }
        }
        _ => false,
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

fn index_named_class(
    decl: &Class<'_>,
    name: &str,
    ctx: HeaderStatementContext<'_>,
    index: &mut DeclHeaderIndex,
) {
    let Some(id) = &decl.id else {
        return;
    };

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
        &[],
        ctx,
    );

    let entry = index
        .value_headers
        .entry(ctx.key(name))
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
    push_contributor(
        &mut entry.contributors,
        ctx,
        decl.span.into(),
        id.span.into(),
    );
}

fn index_function(
    func: &oxc_ast::ast::Function<'_>,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
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
        .entry(ctx.key(id.name.as_str()))
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
    push_contributor(
        &mut entry.contributors,
        ctx,
        func.span.into(),
        id.span.into(),
    );
}

fn index_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    ctx: HeaderStatementContext<'_>,
    table: &mut DeclMap<ValueDeclHeader>,
    namespace: Option<&str>,
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
    push_contributor(
        &mut entry.contributors,
        ctx,
        decl.span.into(),
        id.span.into(),
    );
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
            .any(|existing| existing.name == member.name)
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
    if contributors
        .last()
        .is_some_and(|entry| entry.anchor == ctx.anchor)
    {
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
            constraint_span: param.constraint.as_ref().map(|c| c.span().into()),
            default_span: param.default.as_ref().map(|d| d.span().into()),
        })
        .collect()
}

/// Direct syntactic member headers of a type-alias body: a `TSTypeLiteral`
/// contributes its named members; intersection / parenthesized arms are
/// descended (mirroring the lowered inventory's own-member header facts).
/// Every other body shape has no direct syntactic members.
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
