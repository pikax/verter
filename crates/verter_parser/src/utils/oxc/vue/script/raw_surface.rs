//! Parse-time `RawSourceSurface` raw-fact capture for the TS7
//! `TypeExpr`-projection oracle harness (`docs/arch/u0-oracle-harness-design.md`
//! §Q2, design item G).
//!
//! When a canonical file is parsed, OXC's `lower_ts_type` SILENTLY ERASES a
//! closed set of identity-bearing constructs (`oxc/lib.rs:99,126,171,223,427,921`):
//! a computed / `symbol` / `unique symbol` member key, an accessor, declared
//! member visibility, the `abstract` ctor flag, type-parameter `const`/variance
//! modifiers, a `this` type/param, `as const` provenance, the multi-signature
//! overload SET, and the optional/labelled/`| undefined` tuple distinction. The
//! oracle's source-side admission gate must REJECT a capture whose real fixture
//! source carries any of these — but it can only do so if the facts survive the
//! parse. `RawSourceSurface` is that retained inventory: captured during the
//! file's INITIAL PARSE (while the transient OXC arena is live), as OWNED
//! `Send + Sync` data, alongside the already-lowered shallow body.
//!
//! This module is PURE and PARSE-derived — it walks the OXC declaration AST and
//! records raw facts. It performs NO type resolution and runs NO query-time
//! dispatch, so it is NOT a second resolver. The capture is INFALLIBLE: an
//! unanticipated construct simply contributes no fact (the source-side allowlist
//! default-rejects on the lowered body regardless), so the walk never panics on
//! an arbitrary real-world AST.
//!
//! The capture is keyed by `(name, symbol_space)` within a file; the file's
//! canonical id (`decl_canonical`) is stamped by the file-aware storage layer
//! that owns the `(canonical, name, symbol_space)` identity.

use oxc_ast::ast::{
    BindingPattern, Class, ClassElement, Declaration, Function, MethodDefinitionKind, PropertyKey,
    Statement, TSAccessibility, TSEnumDeclaration, TSInterfaceDeclaration, TSSignature,
    TSTupleElement, TSType, TSTypeAliasDeclaration, TSTypeName, TSTypeOperatorOperator,
    TSTypeParameterDeclaration, VariableDeclaration,
};
use oxc_ast::ast::{Expression, TSTypeQuery};

use verter_type_expr::{MemberVisibility, TypeExpr};

// ===========================================================================
// Source-side raw-fact data model (design item G — the closed §Q2 fact set).
//
// `RawSourceSurface` retains EXACTLY the pre-lowering admission facts the OXC
// lowering would erase (each the catch-target of a §Q2 REJECT row), and NOTHING
// that survives lowering losslessly (those are read from the lowered body).
// ===========================================================================

/// The symbol space a declaration contributes to. Type declarations
/// (interface / type-alias / class / enum) live in `Type`; value declarations
/// (`const`/`let`/`var`, `function`) live in `Value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolSpace {
    Type,
    Value,
}

/// The RAW key form of an object/class member, before OXC drops a non-static
/// key (`property_key_name` returns `None` for any non-static key —
/// `verter_type_expr_oxc/src/lib.rs:921` — so the member is silently elided at
/// `oxc/lib.rs:99`). A computed / `symbol` / `unique symbol` key must be visible
/// as such, not silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawKey {
    /// `name: T` — a static (identifier / string / numeric) key.
    Static(String),
    /// `[expr]: T` — a computed key.
    Computed,
    /// A `symbol`-keyed member.
    SymbolKeyed,
    /// A `unique symbol`-keyed member.
    UniqueSymbolKeyed,
}

/// The kind of an object/class member, before OXC collapses an accessor to a
/// plain property (an accessor is not even an `ObjectMember` variant —
/// `verter_type_expr/src/lib.rs:426`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMemberKind {
    Property,
    IndexSignature,
    Getter,
    Setter,
    Method,
    CallSignature,
    ConstructSignature,
}

/// A type parameter's RAW modifiers, before lowering drops them (`TypeParam` has
/// no `const` modifier and no variance field — `lib.rs:1018`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TypeParamModifiers {
    /// The `const` modifier on a `const T` type parameter.
    pub is_const: bool,
    /// The `in` (contravariant) variance annotation.
    pub variance_in: bool,
    /// The `out` (covariant) variance annotation.
    pub variance_out: bool,
}

impl TypeParamModifiers {
    /// Whether ANY non-default modifier is present (the reject trigger).
    pub fn is_present(&self) -> bool {
        self.is_const || self.variance_in || self.variance_out
    }
}

/// The RAW shape of a tuple element, before TS/lowering collapse the
/// optional-element vs `| undefined` distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TupleElementShape {
    /// `[A, B]` — a plain, required, unlabeled element (the ONLY admissible
    /// shape).
    Plain,
    /// `[A, B?]` — an optional element.
    Optional,
    /// `[label: A]` — a labelled element.
    Labelled,
    /// `[...A]` — a rest element.
    Rest,
}

/// The declaration kind of a contributor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawDeclKind {
    TypeAlias,
    Interface,
    Enum,
    Class,
    Function,
    Variable,
}

/// A single `unique symbol` type-operator occurrence (opaque — its presence is
/// the reject trigger).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueSymbolOp;

/// One raw signature in an overload group (opaque — the GROUP's arity is the
/// reject trigger).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverloadSignature;

/// A `typeof`/`ReturnType`/`Parameters` next-hop locator. `reference_canonical`
/// is left empty at parse time (the file context is unknown to the pure walk)
/// and is resolved by the live transitive walk that re-enters the shared
/// resolver for each hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitiveReferent {
    pub reference_canonical: String,
    pub reference_name: String,
}

/// The retained parse-time raw-fact record for ONE contributor declaration. The
/// closed set of pre-lowering admission facts §Q2 enumerates — captured at the
/// file's initial parse, before lowering erases them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSourceSurface {
    /// Leading-slash file id of THIS contributor. Empty at capture; stamped by
    /// the file-aware storage layer.
    pub decl_canonical: String,
    /// The declaration kind.
    pub decl_kind: RawDeclKind,
    /// Per object/class member, the RAW key form.
    pub raw_member_keys: Vec<RawKey>,
    /// Per member, its raw kind (so an accessor is visible as Getter/Setter).
    pub member_kinds: Vec<RawMemberKind>,
    /// Per member, the DECLARED visibility modifier (before `oxc/lib.rs:427`
    /// stamps it public).
    pub member_visibility: Vec<MemberVisibility>,
    /// Each `unique symbol` type-operator occurrence (before `oxc/lib.rs:171`
    /// lowers it straight through).
    pub unique_symbol_ops: Vec<UniqueSymbolOp>,
    /// Whether a constructor type / class carries `abstract` (before
    /// `oxc/lib.rs:126` ignores it).
    pub abstract_ctor: bool,
    /// Per type parameter, the `const` flag + `in`/`out` variance.
    pub type_param_modifiers: Vec<TypeParamModifiers>,
    /// Whether the decl uses a `this` type or a `this` parameter (erased to
    /// `Ref("this")` / unrepresentable).
    pub this_type_or_param: bool,
    /// For a value / `typeof` referent, the `as const` provenance (collapsed by
    /// lowering). `None` when not a value referent.
    pub value_const_assertion: Option<bool>,
    /// The ORDERED raw signature group as written; `len >= 2` is an overload
    /// SET (the multi-signature group a hover summary would collapse).
    pub overload_signatures: Vec<OverloadSignature>,
    /// For a utility-type application, the RAW referent identifier(s) as
    /// written (inspection only — the transitive walk uses
    /// `transitive_referents`).
    pub utility_referent_names: Vec<String>,
    /// Per tuple element, the optional / labelled / rest presence.
    pub tuple_element_shape: Vec<TupleElementShape>,
    /// `typeof`/`ReturnType`/`Parameters` next hops for the transitive walk.
    pub transitive_referents: Vec<TransitiveReferent>,
}

impl RawSourceSurface {
    fn new(decl_kind: RawDeclKind) -> Self {
        RawSourceSurface {
            decl_canonical: String::new(),
            decl_kind,
            raw_member_keys: Vec::new(),
            member_kinds: Vec::new(),
            member_visibility: Vec::new(),
            unique_symbol_ops: Vec::new(),
            abstract_ctor: false,
            type_param_modifiers: Vec::new(),
            this_type_or_param: false,
            value_const_assertion: None,
            overload_signatures: Vec::new(),
            utility_referent_names: Vec::new(),
            tuple_element_shape: Vec::new(),
            transitive_referents: Vec::new(),
        }
    }
}

/// One captured contributor surface, addressed by its `(name, symbol_space)` key
/// within the parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSurface {
    pub name: String,
    pub symbol_space: SymbolSpace,
    pub surface: RawSourceSurface,
}

// ===========================================================================
// The pure capture walk
// ===========================================================================

/// Capture the `RawSourceSurface`(s) a single top-level declaration contributes.
/// A `VariableDeclaration` can bind several names, so the result is a vector; a
/// declaration with no nameable binding (an anonymous default-export class, a
/// destructuring binding) contributes nothing.
///
/// INFALLIBLE: never panics on an arbitrary AST. An unanticipated construct
/// simply records no extra fact.
pub fn capture_declaration_surfaces(decl: &Declaration<'_>) -> Vec<CapturedSurface> {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            vec![capture_type_alias(alias)]
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            vec![capture_interface(interface)]
        }
        Declaration::ClassDeclaration(class) => capture_class(class).into_iter().collect(),
        Declaration::TSEnumDeclaration(enum_decl) => vec![capture_enum(enum_decl)],
        Declaration::FunctionDeclaration(func) => capture_function(func).into_iter().collect(),
        Declaration::VariableDeclaration(var_decl) => capture_variables(var_decl),
        _ => Vec::new(),
    }
}

/// Capture the `RawSourceSurface`(s) a top-level STATEMENT contributes — the
/// bare declaration statements (`type`/`interface`/`class`/`enum`/`function`/
/// `const`) plus the declaration carried by an `export <decl>`. This is the
/// program-walk entry the parse pass drives over every top-level statement.
pub fn capture_statement_surfaces(stmt: &Statement<'_>) -> Vec<CapturedSurface> {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => vec![capture_type_alias(alias)],
        Statement::TSInterfaceDeclaration(interface) => vec![capture_interface(interface)],
        Statement::ClassDeclaration(class) => capture_class(class).into_iter().collect(),
        Statement::TSEnumDeclaration(enum_decl) => vec![capture_enum(enum_decl)],
        Statement::FunctionDeclaration(func) => capture_function(func).into_iter().collect(),
        Statement::VariableDeclaration(var_decl) => capture_variables(var_decl),
        Statement::ExportNamedDeclaration(export) => export
            .declaration
            .as_ref()
            .map(capture_declaration_surfaces)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn capture_enum(enum_decl: &TSEnumDeclaration<'_>) -> CapturedSurface {
    CapturedSurface {
        name: enum_decl.id.name.to_string(),
        symbol_space: SymbolSpace::Type,
        surface: RawSourceSurface::new(RawDeclKind::Enum),
    }
}

/// Merge same-name `Function` contributions into one ordered overload group, in
/// source order. A file declaring `function f(a): X; function f(b): Y;` yields a
/// SINGLE `Value`-space `f` surface whose `overload_signatures.len() == 2` — the
/// arity the source-side allowlist rejects. Every non-function surface passes
/// through unchanged.
pub fn merge_overload_groups(captured: Vec<CapturedSurface>) -> Vec<CapturedSurface> {
    let mut out: Vec<CapturedSurface> = Vec::with_capacity(captured.len());
    for c in captured {
        let is_function = matches!(c.surface.decl_kind, RawDeclKind::Function);
        if is_function {
            if let Some(existing) = out.iter_mut().find(|e| {
                e.name == c.name
                    && e.symbol_space == c.symbol_space
                    && matches!(e.surface.decl_kind, RawDeclKind::Function)
            }) {
                existing.surface.overload_signatures.push(OverloadSignature);
                existing.surface.this_type_or_param |= c.surface.this_type_or_param;
                for m in c.surface.type_param_modifiers {
                    existing.surface.type_param_modifiers.push(m);
                }
                for r in c.surface.transitive_referents {
                    existing.surface.transitive_referents.push(r);
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn capture_type_alias(alias: &TSTypeAliasDeclaration<'_>) -> CapturedSurface {
    let mut surface = RawSourceSurface::new(RawDeclKind::TypeAlias);
    capture_type_params(alias.type_parameters.as_deref(), &mut surface);
    scan_ts_type(&alias.type_annotation, &mut surface, true);
    CapturedSurface {
        name: alias.id.name.to_string(),
        symbol_space: SymbolSpace::Type,
        surface,
    }
}

fn capture_interface(interface: &TSInterfaceDeclaration<'_>) -> CapturedSurface {
    let mut surface = RawSourceSurface::new(RawDeclKind::Interface);
    capture_type_params(interface.type_parameters.as_deref(), &mut surface);
    for member in &interface.body.body {
        capture_signature_member(member, &mut surface);
    }
    CapturedSurface {
        name: interface.id.name.to_string(),
        symbol_space: SymbolSpace::Type,
        surface,
    }
}

fn capture_class(class: &Class<'_>) -> Option<CapturedSurface> {
    let name = class.id.as_ref()?.name.to_string();
    let mut surface = RawSourceSurface::new(RawDeclKind::Class);
    surface.abstract_ctor = class.r#abstract;
    capture_type_params(class.type_parameters.as_deref(), &mut surface);
    for element in &class.body.body {
        capture_class_element(element, &mut surface);
    }
    Some(CapturedSurface {
        name,
        symbol_space: SymbolSpace::Type,
        surface,
    })
}

fn capture_function(func: &Function<'_>) -> Option<CapturedSurface> {
    let name = func.id.as_ref()?.name.to_string();
    let mut surface = RawSourceSurface::new(RawDeclKind::Function);
    // One signature per declaration; same-name functions merge into the
    // overload group by `merge_overload_groups`.
    surface.overload_signatures.push(OverloadSignature);
    capture_type_params(func.type_parameters.as_deref(), &mut surface);
    if func.this_param.is_some() {
        surface.this_type_or_param = true;
    }
    for param in &func.params.items {
        if let Some(ann) = &param.type_annotation {
            scan_ts_type(&ann.type_annotation, &mut surface, false);
        }
    }
    if let Some(ret) = &func.return_type {
        scan_ts_type(&ret.type_annotation, &mut surface, false);
    }
    Some(CapturedSurface {
        name,
        symbol_space: SymbolSpace::Value,
        surface,
    })
}

fn capture_variables(var_decl: &VariableDeclaration<'_>) -> Vec<CapturedSurface> {
    let mut out = Vec::new();
    for declarator in &var_decl.declarations {
        // Only a plain identifier binding has an addressable `(name, Value)`
        // identity; a destructuring binding contributes no single name.
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let mut surface = RawSourceSurface::new(RawDeclKind::Variable);
        // `as const` provenance — `Some(true)` when the initializer is asserted
        // `as const`, `Some(false)` for a plain value declaration, `None` only
        // for non-value decls.
        let is_as_const = declarator
            .init
            .as_ref()
            .map(expression_is_as_const)
            .unwrap_or(false);
        surface.value_const_assertion = Some(is_as_const);
        if let Some(ann) = &declarator.type_annotation {
            scan_ts_type(&ann.type_annotation, &mut surface, false);
        }
        out.push(CapturedSurface {
            name: id.name.to_string(),
            symbol_space: SymbolSpace::Value,
            surface,
        });
    }
    out
}

/// Whether an initializer expression is asserted `as const` (`expr as const`).
fn expression_is_as_const(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::TSAsExpression(as_expr) => ts_type_is_const_reference(&as_expr.type_annotation),
        _ => false,
    }
}

/// Whether a type annotation is the literal `const` reference of an `as const`.
fn ts_type_is_const_reference(ty: &TSType<'_>) -> bool {
    if let TSType::TSTypeReference(r) = ty {
        if let TSTypeName::IdentifierReference(id) = &r.type_name {
            return id.name == "const";
        }
    }
    false
}

fn capture_type_params(
    params: Option<&TSTypeParameterDeclaration<'_>>,
    surface: &mut RawSourceSurface,
) {
    let Some(params) = params else {
        return;
    };
    for param in &params.params {
        surface.type_param_modifiers.push(TypeParamModifiers {
            is_const: param.r#const,
            variance_in: param.r#in,
            variance_out: param.out,
        });
    }
}

/// Capture a single interface / type-literal signature member.
fn capture_signature_member(sig: &TSSignature<'_>, surface: &mut RawSourceSurface) {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            surface
                .raw_member_keys
                .push(raw_key_of(&prop.key, prop.computed));
            surface.member_kinds.push(RawMemberKind::Property);
            surface.member_visibility.push(MemberVisibility::Public);
            if let Some(ann) = &prop.type_annotation {
                scan_ts_type(&ann.type_annotation, surface, false);
            }
        }
        TSSignature::TSIndexSignature(idx) => {
            surface.member_kinds.push(RawMemberKind::IndexSignature);
            surface.member_visibility.push(MemberVisibility::Public);
            scan_ts_type(&idx.type_annotation.type_annotation, surface, false);
        }
        TSSignature::TSMethodSignature(m) => {
            surface.raw_member_keys.push(raw_key_of(&m.key, m.computed));
            surface.member_kinds.push(RawMemberKind::Method);
            surface.member_visibility.push(MemberVisibility::Public);
        }
        TSSignature::TSCallSignatureDeclaration(_) => {
            surface.member_kinds.push(RawMemberKind::CallSignature);
            surface.member_visibility.push(MemberVisibility::Public);
        }
        TSSignature::TSConstructSignatureDeclaration(_) => {
            surface.member_kinds.push(RawMemberKind::ConstructSignature);
            surface.member_visibility.push(MemberVisibility::Public);
        }
    }
}

/// Capture a single class body element.
fn capture_class_element(element: &ClassElement<'_>, surface: &mut RawSourceSurface) {
    match element {
        ClassElement::PropertyDefinition(prop) => {
            if prop.r#static {
                return;
            }
            surface
                .raw_member_keys
                .push(raw_key_of(&prop.key, prop.computed));
            surface.member_kinds.push(RawMemberKind::Property);
            surface
                .member_visibility
                .push(visibility_of(prop.accessibility));
            if let Some(ann) = &prop.type_annotation {
                scan_ts_type(&ann.type_annotation, surface, false);
            }
        }
        ClassElement::MethodDefinition(method) => {
            if method.r#static {
                return;
            }
            let kind = match method.kind {
                MethodDefinitionKind::Get => RawMemberKind::Getter,
                MethodDefinitionKind::Set => RawMemberKind::Setter,
                // A constructor / instance method is not a data property.
                MethodDefinitionKind::Constructor | MethodDefinitionKind::Method => {
                    RawMemberKind::Method
                }
            };
            surface
                .raw_member_keys
                .push(raw_key_of(&method.key, method.computed));
            surface.member_kinds.push(kind);
            surface
                .member_visibility
                .push(visibility_of(method.accessibility));
        }
        ClassElement::AccessorProperty(prop) => {
            if prop.r#static {
                return;
            }
            // An `accessor` field synthesises a getter/setter pair — not a
            // hover-representable data property.
            surface
                .raw_member_keys
                .push(raw_key_of(&prop.key, prop.computed));
            surface.member_kinds.push(RawMemberKind::Getter);
            surface
                .member_visibility
                .push(visibility_of(prop.accessibility));
        }
        ClassElement::TSIndexSignature(_) => {
            surface.member_kinds.push(RawMemberKind::IndexSignature);
            surface.member_visibility.push(MemberVisibility::Public);
        }
        ClassElement::StaticBlock(_) => {}
    }
}

fn visibility_of(accessibility: Option<TSAccessibility>) -> MemberVisibility {
    match accessibility {
        Some(TSAccessibility::Private) => MemberVisibility::Private,
        Some(TSAccessibility::Protected) => MemberVisibility::Protected,
        _ => MemberVisibility::Public,
    }
}

/// The RAW key form of a member, recording a computed / `symbol` key as such so
/// the source-side allowlist (which admits ONLY `Static`) can reject it.
fn raw_key_of(key: &PropertyKey<'_>, computed: bool) -> RawKey {
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => RawKey::Static(id.name.to_string()),
        PropertyKey::StringLiteral(s) if !computed => RawKey::Static(s.value.to_string()),
        PropertyKey::NumericLiteral(n) if !computed => RawKey::Static(n.value.to_string()),
        // A computed key whose expression is a `Symbol.*` member access is a
        // symbol-keyed member; any other computed key is generically computed.
        _ => {
            if let PropertyKey::StaticMemberExpression(member) = key {
                if let Expression::Identifier(obj) = &member.object {
                    if obj.name == "Symbol" {
                        return RawKey::SymbolKeyed;
                    }
                }
            }
            RawKey::Computed
        }
    }
}

/// Recursively scan a `TSType` for the erased facts that live INSIDE a type
/// expression — `unique symbol` operators, tuple element shapes, `this` types,
/// and transitive `typeof` referents. `is_alias_root` distinguishes the alias
/// body's top-level type (which contributes object/tuple members directly) from
/// nested positions.
fn scan_ts_type(ts: &TSType<'_>, surface: &mut RawSourceSurface, is_alias_root: bool) {
    match ts {
        TSType::TSTypeOperatorType(op) => {
            if op.operator == TSTypeOperatorOperator::Unique {
                surface.unique_symbol_ops.push(UniqueSymbolOp);
            }
            scan_ts_type(&op.type_annotation, surface, false);
        }
        TSType::TSThisType(_) => {
            surface.this_type_or_param = true;
        }
        TSType::TSArrayType(arr) => scan_ts_type(&arr.element_type, surface, false),
        TSType::TSParenthesizedType(p) => scan_ts_type(&p.type_annotation, surface, is_alias_root),
        TSType::TSUnionType(u) => {
            for arm in &u.types {
                scan_ts_type(arm, surface, false);
            }
        }
        TSType::TSIntersectionType(i) => {
            for arm in &i.types {
                scan_ts_type(arm, surface, false);
            }
        }
        TSType::TSTupleType(tuple) => {
            for el in &tuple.element_types {
                let shape = match el {
                    TSTupleElement::TSOptionalType(_) => TupleElementShape::Optional,
                    TSTupleElement::TSNamedTupleMember(_) => TupleElementShape::Labelled,
                    TSTupleElement::TSRestType(_) => TupleElementShape::Rest,
                    _ => TupleElementShape::Plain,
                };
                surface.tuple_element_shape.push(shape);
                if let Some(inner) = el.as_ts_type() {
                    scan_ts_type(inner, surface, false);
                }
            }
        }
        TSType::TSTypeLiteral(lit) => {
            // Only the alias-root literal contributes the alias's OWN members;
            // a nested literal's members belong to that nested type, not to the
            // alias surface — but for admission soundness a nested callable /
            // computed key must still reject, so we record them too.
            let _ = is_alias_root;
            for member in &lit.members {
                capture_signature_member(member, surface);
            }
        }
        TSType::TSTypeQuery(query) => {
            if let Some(referent) = type_query_referent(query) {
                surface.transitive_referents.push(TransitiveReferent {
                    reference_canonical: String::new(),
                    reference_name: referent,
                });
            }
        }
        TSType::TSTypeReference(r) => {
            if let TSTypeName::IdentifierReference(id) = &r.type_name {
                let name = id.name.as_str();
                if matches!(name, "ReturnType" | "Parameters" | "InstanceType") {
                    surface.utility_referent_names.push(name.to_string());
                }
            }
            if let Some(args) = &r.type_arguments {
                for arg in &args.params {
                    scan_ts_type(arg, surface, false);
                }
            }
        }
        TSType::TSConstructorType(ctor) => {
            if ctor.r#abstract {
                surface.abstract_ctor = true;
            }
            scan_ts_type(&ctor.return_type.type_annotation, surface, false);
        }
        _ => {}
    }
}

/// The referent identifier of a `typeof X` query, when it is a plain identifier.
fn type_query_referent(query: &TSTypeQuery<'_>) -> Option<String> {
    use oxc_ast::ast::TSTypeQueryExprName;
    match &query.expr_name {
        TSTypeQueryExprName::IdentifierReference(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Whether a lowered body still carries a rejectable non-erased `TypeExpr`
/// variant (callable / conditional / mapped / template-literal / infer / keyof /
/// indexed-access / typeof / recursive-ref). Exposed for the source-side
/// admission gate, which reads the COMBINED `(raw facts, lowered body)` pair.
/// Returns the variant name when present, `None` when the body is clean of them.
pub fn lowered_body_rejectable_variant(body: &TypeExpr) -> Option<&'static str> {
    match body {
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => Some("callable"),
        TypeExpr::Conditional { .. } => Some("conditional"),
        TypeExpr::Mapped { .. } => Some("mapped"),
        TypeExpr::TemplateLiteral { .. } => Some("template-literal"),
        TypeExpr::Infer { .. } => Some("infer"),
        TypeExpr::KeyOf(_) => Some("keyof"),
        TypeExpr::IndexedAccess { .. } => Some("indexed-access"),
        TypeExpr::TypeOf(_) => Some("typeof"),
        TypeExpr::RecursiveRef { .. } => Some("recursive-ref"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "raw_surface_tests.rs"]
mod tests;
