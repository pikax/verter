//! Arena-free per-file `FunctionProgramIndex`: the structural inventory of
//! every served authored function position in one parsed file.
//!
//! The index is a SHALLOW structural product built once per parsed file
//! version from the retained parse snapshot: exact function identities and
//! body locators, binding/reference inventory, return sites, writes and
//! evaluation effects, the control-region skeleton, exact direct local call
//! targets, and a whole-function `flow_body_stable_hash`. It borrows no OXC
//! node and lowers no type tree — lowering of one demanded function into
//! typed IR happens later, per function, over these locators.
//!
//! Hash rules (`flow_body_stable_hash`): the fold preserves observable
//! property / destructuring / computed keys, operators, literals, calls,
//! writes, control structure, authored type annotations (return,
//! type-parameter, and EVERY parameter annotation), parameter default
//! initializers, and type-affecting JSDoc (`@param` / `@returns` /
//! `@return` / `@type` payloads). Only binding/reference identifier
//! positions are alpha-normalized — a local rename that preserves
//! structure keeps the hash; a property key, free name, literal, operator,
//! control, parameter-annotation, or default-initializer edit changes it.

use std::sync::Arc;

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, CallExpression, Class, Expression, Function,
    MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, VariableDeclaration,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::facts::{
    FlowFunctionReturnIdentity, FunctionReturnSource, ProgramExpressionIdentity,
};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};
use verter_type_expr::span_origins::DeclContributorAnchor;

use crate::analysis::top_level_owners::TopLevelOwnerTable;
use crate::analysis::types::Hash16;
use crate::facts::SymbolSpace;

#[cfg(test)]
#[path = "function_program_tests.rs"]
mod function_program_tests;

/// The declaration a served function position belongs to (content-free;
/// the owner discriminates script-block owners, the name is the registered
/// merged-symbol name — namespaces qualify `Ns.Name` exactly like the eval
/// env registration).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionDeclarationRef {
    /// Lexical top-level owner of the contributing statement.
    pub owner: verter_type_expr::TopLevelOwnerId,
    /// Registered merged-symbol name.
    pub name: Arc<str>,
    /// Type-space vs value-space discriminator (functions are value-space;
    /// the discriminator keeps the ref shape aligned with slot identity).
    pub space: SymbolSpace,
}

/// One ordinal step from a contributing top-level statement down to the
/// function node. Named positions / small ordinals only — never a byte
/// span, never a lowered type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionDescentStep {
    /// The statement IS the function declaration.
    FunctionDeclaration,
    /// The init of the variable declarator at `declarator_ordinal` (the
    /// init is the arrow / function expression).
    VariableInitializer { declarator_ordinal: u32 },
    /// The class member at `member_ordinal` (`ClassBody.body` index).
    ClassMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside the current
    /// initializer object expression.
    ObjectMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside an
    /// `export default { … }` object expression.
    ExportDefaultObjectMember { member_ordinal: u32 },
    /// The statement at `statement_ordinal` inside a namespace block.
    NamespaceMember { statement_ordinal: u32 },
    /// The statement at `statement_ordinal` inside the enclosing
    /// function's body (a hoisted nested function declaration).
    BodyStatement { statement_ordinal: u32 },
    /// The argument at `arg_ordinal` of the enclosing body's
    /// `call_ordinal`-th call site (source order) — a callback position.
    CallArgument { call_ordinal: u32, arg_ordinal: u32 },
    /// The CALLEE of the enclosing body's `call_ordinal`-th call site
    /// (source order) when that callee is itself a function / arrow
    /// expression — an immediately-invoked function expression.
    CallCallee { call_ordinal: u32 },
}

/// Arena-free locator for one function's body inside the retained parse
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBodyLocator {
    /// The contributing top-level statement.
    pub contributor: DeclContributorAnchor,
    /// Ordinal descent from the contributing statement to the function node.
    pub descent: Arc<[FunctionDescentStep]>,
}

/// The full program identity of one served function position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionProgramKey {
    /// The owning declaration.
    pub declaration: FunctionDeclarationRef,
    /// Which authored position of the declaration this callable occupies.
    pub part: FunctionPartIdentity,
    /// Signature ordinal inside an overload group, in source order (the
    /// trailing implementation is the last ordinal). Zero outside overload
    /// groups.
    pub overload_ordinal: u32,
}

/// One formal parameter's binding fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParamRecord {
    /// The binding name (`None` for a destructured parameter).
    pub name: Option<Arc<str>>,
    /// Whether the parameter is optional (`?`).
    pub optional: bool,
    /// Whether this is the rest parameter.
    pub rest: bool,
    /// Whether the parameter carries an authored TS type annotation.
    pub has_ts_annotation: bool,
}

/// The kind of one local binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionBindingKind {
    /// A formal parameter.
    Param,
    /// A `const` declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator.
    Var,
    /// A nested function declaration's name.
    NestedFunction,
}

/// One local binding (parameter, variable declarator, nested function name).
///
/// The frame's binding list is the frame's FULL source-order inventory: no
/// name deduplication and no reordering, so two same-name bindings in
/// different lexical scopes of one frame stay distinct entries at distinct
/// slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBindingRecord {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind.
    pub kind: FunctionBindingKind,
    /// The binding's span.
    pub span: verter_span::Span,
    /// The span of the lexical scope the binding is visible in: the
    /// innermost enclosing block-like region for a `const` / `let` /
    /// nested function declaration, the whole frame for a parameter or a
    /// `var`. A reference resolves to the same-name binding whose scope
    /// CONTAINS the reference and is innermost among those.
    pub scope_span: verter_span::Span,
}

/// One identifier reference in the current function body (nested function
/// bodies excluded — their references resolve in their own frames).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReferenceRecord {
    /// The referenced name.
    pub name: Arc<str>,
    /// The reference span.
    pub span: verter_span::Span,
}

/// One `return` site of the current function, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReturnSite {
    /// Source-order ordinal among the function's return sites.
    pub ordinal: u32,
    /// Whether the site carries an argument expression (bare `return;`
    /// contributes `undefined`).
    pub has_argument: bool,
    /// The return statement's span.
    pub span: verter_span::Span,
}

/// The authored literal shape of one call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionCallArgLiteralMode {
    /// An ordinary (widened) argument position.
    Widened,
    /// A fresh literal argument position (string / number / boolean /
    /// template / object / array literal).
    Literal,
}

/// One argument of an indexed call site, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionCallArgRecord {
    /// The argument expression's program point.
    pub point: u32,
    /// Whether the argument is a spread element (`...xs`).
    pub spread: bool,
    /// The authored literal shape of the argument.
    pub literal_mode: FunctionCallArgLiteralMode,
    /// Whether the argument is a function / arrow expression (a callback
    /// position — its return is a served function position).
    pub is_function_value: bool,
    /// Exact return carrier when this argument is an indexed callback value.
    pub function_return_source: Option<FunctionReturnSource>,
}

/// One indexed call site in the current function body: the program point,
/// the callee carrier, the exact same-file target, and the per-argument
/// facts. This is the unified call record every call-shaped consumer
/// reads — never a raw-string reparse, never a synthesized call type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionCallSiteRecord {
    /// The call expression's span (the program point's offset identity).
    pub span: verter_span::Span,
    /// The callee shape.
    pub callee: FunctionEffectCallee,
    /// The exact same-file served function this site calls, when the
    /// callee is a bare identifier the enclosing frame's lexical scope
    /// binds to an indexed position (direct same-slot recursion
    /// included). `None` for every other callee shape, and for a site
    /// indexed outside a function frame (a top-level indexed expression
    /// has no frame-local lexical scope to resolve against).
    pub target: Option<FunctionProgramKey>,
    /// The ordered argument facts.
    pub args: Arc<[FunctionCallArgRecord]>,
}

/// How one indexed expression supplies its value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProgramExpressionSource {
    /// A call-free value expression, lowered lazily from the retained AST.
    Value,
    /// A direct semantic call/construct record.
    SemanticCall {
        kind: ProgramExpressionCallKind,
        site: FunctionCallSiteRecord,
    },
    /// An indexed callback/function value's exact return carrier.
    FunctionReturn(FunctionReturnSource),
    /// A call-bearing compound outside the indexed expression domain.
    UnsupportedCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramExpressionCallKind {
    Call,
    Construct,
}

/// One declaration/callback expression indexed by content-free program point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramExpressionRecord {
    pub point: ProgramExpressionIdentity,
    pub span: verter_span::Span,
    pub locator: FunctionBodyLocator,
    pub source: ProgramExpressionSource,
}

/// One captured binding's content-free identity: the frame that DECLARES
/// the binding plus the binding's stable source-order slot in that frame's
/// full binding inventory, alongside the binding's name and kind. The
/// `(defining_function, binding_slot)` pair is the identity — it separates
/// two same-name binders in different frames AND two same-name binders in
/// different lexical scopes of one frame, neither of which a name (or a
/// per-capture-list ordinal) can distinguish. NEVER a node id, a type, a
/// content hash, or a span — capture types rehydrate from indexed binding /
/// reaching-definition facts under the final type substitution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowBindingIdentity {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind in the DEFINING frame.
    pub kind: FunctionBindingKind,
    /// The frame whose binding inventory declares this binding.
    pub defining_function: FunctionProgramKey,
    /// The binding's source-order slot in that frame's binding inventory.
    pub binding_slot: u32,
}

/// The content-free capture environment of a nested function position:
/// capture binding identities (and their deterministic source order)
/// only. Until non-empty narrowing lands, a capture whose type cannot be
/// reconstructed from the indexed binding / reaching-definition facts is
/// a typed ReturnOnly, never guessed or separately keyed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct CanonicalCaptureIdentity(pub Arc<[FlowBindingIdentity]>);

/// The callee shape of one evaluation-effect call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionEffectCallee {
    /// A bare identifier callee (`g()`).
    Identifier(Arc<str>),
    /// A static member path (`a.b.c()`).
    StaticMember(Arc<[Arc<str>]>),
    /// Any other callee shape (computed, call-result, `this`-rooted).
    Other,
}

/// One evaluation-effect call site in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionEffectRecord {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee shape.
    pub callee: FunctionEffectCallee,
}

/// One write site (assignment or update) in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionWriteRecord {
    /// The write expression's span.
    pub span: verter_span::Span,
}

/// The control-region kind of one skeleton region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionControlKind {
    /// A block statement.
    Block,
    /// An `if` statement (consequent / alternate arms nest as regions).
    If,
    /// A loop (`for` / `for-in` / `for-of` / `while` / `do-while`).
    Loop,
    /// A `switch` statement.
    Switch,
    /// A `try` statement.
    Try,
    /// A labeled statement.
    Labeled,
}

/// One control-region skeleton entry (the current function's statement
/// tree, nested function bodies excluded).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionControlRegion {
    /// The region kind.
    pub kind: FunctionControlKind,
    /// Whether the region's statement subtree contains a `return` of the
    /// current function (drives return-transparency: return-free loop /
    /// labeled constructs are fall-through transparent; return-bearing
    /// loop / labeled regions, and every switch / try, are unsupported).
    pub has_return: bool,
    /// The region statement's span.
    pub span: verter_span::Span,
}

/// One exact direct local call: the callee is a bare identifier bound to a
/// function in the same index (a same-file, syntactically exact target —
/// direct same-slot recursion included).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionDirectCall {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee's program identity.
    pub target: FunctionProgramKey,
}

/// One parameter of an indexed function's OWN type-parameter clause.
///
/// Purely syntactic: a name, whether the parameter authored a DEFAULT,
/// and the smallest formal-parameter ordinal whose authored type
/// annotation names it. The default's TYPE is deliberately absent —
/// this index is a shallow declaration fact, never a body lowering — so
/// a caller that needs the default's meaning demands it through the
/// shared lazy body service, and pays for it only on the clauses that
/// have one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionProgramTypeParam {
    /// The type-parameter name.
    pub name: Arc<str>,
    /// Whether the parameter authored a default (`<T = D>`).
    pub has_default: bool,
    /// The SMALLEST formal-parameter ordinal whose authored type
    /// annotation references this name, or `None` when no parameter
    /// type mentions it at all.
    ///
    /// This is the caller's inference oracle, and the ONLY fact a
    /// caller needs to apply TypeScript's actual default rule: a
    /// declared default resolves the parameter only when inference
    /// produced NO candidate, and inference can produce a candidate
    /// only from an argument the call actually supplies at an ordinal
    /// whose parameter type names the parameter. `f<T = number>(x:
    /// string)` therefore takes its default even at an
    /// argument-bearing call, and `f<T = number>(a: string, b?: T)`
    /// takes it at `f("a")`.
    ///
    /// A REST parameter occupies its own ordinal and covers every
    /// later one, so the same `ordinal < argument_count` test holds:
    /// `f<T = number>(...xs: T[])` has occurrence ordinal 0, which no
    /// zero-argument call supplies.
    ///
    /// Shadowing-aware: a nested function / constructor type inside a
    /// parameter annotation that RE-DECLARES the name owns its own
    /// subtree, so `f<T = number>(cb: <T>(y: T) => T)` records `None`
    /// for the outer `T` — which is what the checker answers.
    pub first_parameter_occurrence: Option<u32>,
}

/// One served function position: identity, body locator, structural
/// inventory, and the whole-function stable hash.
///
/// `#[non_exhaustive]`, so no crate but this one can CONSTRUCT one —
/// neither by struct literal nor by `Clone` + functional update. An
/// entry is a STATEMENT about an authored function this file's discovery
/// walk found; a consumer that fabricates one is stating something no
/// walk observed, and the flow substrate's callee rail reads a clause off
/// exactly this record. Fields stay public for READING: the value is a
/// shallow structural fact, and the hazard is minting one, not reading
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FunctionProgramEntry {
    /// The program identity.
    pub key: FunctionProgramKey,
    /// The authored function node's span.
    pub span: verter_span::Span,
    /// Arena-free body locator into the retained snapshot.
    pub locator: FunctionBodyLocator,
    /// Formal parameter facts (source order).
    pub params: Arc<[FunctionParamRecord]>,
    /// Local bindings (parameters, declarators, nested function names).
    pub bindings: Arc<[FunctionBindingRecord]>,
    /// Identifier references in the current function body.
    pub references: Arc<[FunctionReferenceRecord]>,
    /// Return sites in source order.
    pub return_sites: Arc<[FunctionReturnSite]>,
    /// Write sites (assignments / updates).
    pub writes: Arc<[FunctionWriteRecord]>,
    /// Evaluation-effect call sites.
    pub effects: Arc<[FunctionEffectRecord]>,
    /// Indexed call sites: program point, callee carrier, exact same-file
    /// target, and per-argument facts — the unified call record every
    /// call-shaped consumer reads.
    pub call_sites: Arc<[FunctionCallSiteRecord]>,
    /// Control-region skeleton.
    pub control: Arc<[FunctionControlRegion]>,
    /// Exact direct local call targets.
    pub direct_calls: Arc<[FunctionDirectCall]>,
    /// This function's OWN type-parameter clause, in declaration order.
    ///
    /// A shallow syntactic FACT, not a lowering: the names are what a
    /// CALLER needs to instantiate the callee's clause, and the caller
    /// cannot read them off the callee's declared or body-derived return
    /// (a parameter that never bound interns as a deferred name
    /// reference, indistinguishable from an unrelated free name).
    ///
    /// This index answers for every position it INDEXES, which is what a
    /// direct-call target is by construction, and which the value
    /// registry is not: a namespace-scoped function has no prepared
    /// declaration at all. It is NOT an inventory of every declared
    /// signature — an overload group is indexed once, at its
    /// implementation, so a caller reaching a VISIBLE overload's clause
    /// through here would be reading the implementation's.
    pub type_parameters: Arc<[FunctionProgramTypeParam]>,
    /// The enclosing function position for a NESTED served position
    /// (a hoisted nested function declaration or a call-argument
    /// function value); `None` for a top-level position.
    pub lexical_parent: Option<Box<FunctionProgramKey>>,
    /// The authored binding name of a HOISTED NESTED FUNCTION DECLARATION
    /// (`function inner() { … }` inside another body). `None` for every
    /// other position — a top-level position, a callback value, an
    /// initializer arrow. It is the lexical name a bare-identifier call in
    /// the parent frame binds to.
    pub nested_declaration_name: Option<Arc<str>>,
    /// The content-free capture environment (empty for a top-level
    /// position).
    pub captures: CanonicalCaptureIdentity,
    /// The whole-function stable hash (structural content only — the
    /// parser / language / parse-env identity folds in at the artifact
    /// boundary).
    pub flow_body_stable_hash: Hash16,
    /// The EXACT byte hash of the function's own source text.
    ///
    /// [`Self::flow_body_stable_hash`] is an AST fold that
    /// alpha-normalizes binding and reference identifiers and sees no
    /// whitespace, which is exactly what makes it a good SHARING key —
    /// and exactly what makes it unusable on its own as the key of an
    /// artifact carrying SOURCE POSITIONS. Two contents that fold alike
    /// (`const aa = 1` vs `const aaaa = 1`) place every position inside
    /// the body differently, including positions measured relative to
    /// the function's own start.
    ///
    /// This is the axis that makes such an artifact genuinely
    /// content-addressed. It is deliberately per-FUNCTION rather than
    /// per-file: an edit to a sibling function changes neither this hash
    /// nor any anchor-relative position, so the untouched function's
    /// artifacts stay warm — which is the whole point of not keying on
    /// the file's content hash.
    ///
    /// `None` when the recorded function span does not lie within the
    /// source that produced this entry — a typed MISS, not a hash. It was
    /// a `unwrap_or_default()` over an out-of-range slice, which hashed
    /// the EMPTY string: every entry whose span fell out of range then
    /// shared one constant, collapsing exactly the axis this field exists
    /// to be. A consumer that cannot address the body's own bytes must
    /// not build a content-addressed key at all.
    pub flow_body_exact_hash: Option<Hash16>,
}

/// A LOOKUP-PROVEN entry: what THIS index answered when asked for one
/// specific function position.
///
/// The field is private and there is no public constructor, so the type
/// IS the witness: it cannot be forged, and it cannot be manufactured
/// from an entry obtained any other way — including a legitimately
/// obtained entry belonging to a DIFFERENT callee.
///
/// That second case is the one that mattered. While the flow rail's
/// clause reader took a bare `&FunctionProgramEntry`, two defeats
/// compiled. The first was a struct literal assembled out of nothing
/// (now separately impossible: [`FunctionProgramEntry`] is
/// `#[non_exhaustive]`). The second, and the realistic one, was an index
/// MISS falling back to `index.entries.first()` — a real entry, for the
/// wrong function, handed to a reader whose doc claimed the reference
/// itself was proof of a successful lookup. Both are closed by
/// construction now: this index hands out no entry except through a
/// KEYED lookup, and what it hands out is this witness.
#[derive(Debug, Clone, Copy)]
pub struct FunctionProgramMatch<'a> {
    entry: &'a FunctionProgramEntry,
}

impl<'a> FunctionProgramMatch<'a> {
    /// The matched entry's structural record.
    #[must_use]
    pub fn entry(self) -> &'a FunctionProgramEntry {
        self.entry
    }

    /// The position this lookup matched — the entry's own identity, so a
    /// caller can cross-check what it asked for against what it got.
    #[must_use]
    pub fn key(self) -> &'a FunctionProgramKey {
        &self.entry.key
    }
}

/// The per-file function program index.
///
/// `entries` is PRIVATE and there is no positional accessor: every way
/// out of this index is a lookup that NAMES the position it wants
/// ([`Self::get`], [`Self::value_function`], [`Self::matches_named`]),
/// and each returns a [`FunctionProgramMatch`]. `index.entries.first()`
/// — the shape a callee-lookup miss actually fell back to — does not
/// exist to be written, and neither does any spelling that reaches an
/// entry without naming its function first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionProgramIndex {
    /// Every served function position, in source order.
    entries: Arc<[FunctionProgramEntry]>,
    /// Indexed declaration/callback expressions, in source order.
    expressions: Arc<[ProgramExpressionRecord]>,
}

impl FunctionProgramIndex {
    /// The entry for `key`, when the position is served by this file.
    #[must_use]
    pub fn get(&self, key: &FunctionProgramKey) -> Option<FunctionProgramMatch<'_>> {
        self.entries
            .iter()
            .find(|entry| &entry.key == key)
            .map(|entry| FunctionProgramMatch { entry })
    }

    /// The entry for a value-space function declaration / initializer of
    /// `name` at `overload_ordinal`, when present.
    #[must_use]
    pub fn value_function(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        part: &FunctionPartIdentity,
        overload_ordinal: u32,
    ) -> Option<FunctionProgramMatch<'_>> {
        self.entries
            .iter()
            .find(|entry| {
                entry.key.declaration.owner == owner
                    && entry.key.declaration.name.as_ref() == name
                    && entry.key.declaration.space == SymbolSpace::Value
                    && &entry.key.part == part
                    && entry.key.overload_ordinal == overload_ordinal
            })
            .map(|entry| FunctionProgramMatch { entry })
    }

    /// Every served position DECLARED under `name`, in source order —
    /// the keyed lookup for a declaration whose part / overload ordinal
    /// the caller does not know up front (a class's members, an overload
    /// group's contributors).
    pub fn matches_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = FunctionProgramMatch<'a>> + 'a {
        self.entries
            .iter()
            .filter(move |entry| entry.key.declaration.name.as_ref() == name)
            .map(|entry| FunctionProgramMatch { entry })
    }

    /// How many function positions this file serves.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this file serves no function position.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// This index with every entry's `flow_body_stable_hash` re-folded
    /// through `mix`.
    ///
    /// The artifact boundary mixes the parser / language / parse-env
    /// identity into the semantic walk's body-content hash. It is
    /// expressed as a fold HERE rather than as a rebuild at the consumer
    /// because rebuilding needs to construct entries, and constructing an
    /// entry outside this module is exactly what must stay impossible.
    #[must_use]
    pub fn map_stable_hashes(&self, mix: impl Fn(&Hash16) -> Hash16) -> Self {
        Self {
            entries: Arc::from(
                self.entries
                    .iter()
                    .map(|entry| {
                        let mut folded = entry.clone();
                        folded.flow_body_stable_hash = mix(&entry.flow_body_stable_hash);
                        folded
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            expressions: Arc::clone(&self.expressions),
        }
    }

    /// Indexed expression at the exact content-free program point.
    #[must_use]
    pub fn expression(
        &self,
        point: &ProgramExpressionIdentity,
    ) -> Option<&ProgramExpressionRecord> {
        self.expressions
            .iter()
            .find(|record| &record.point == point)
    }
}

// ---------------------------------------------------------------------------
// Discovery walk
// ---------------------------------------------------------------------------

struct DiscoveryCtx<'a> {
    canonical_id: Arc<str>,
    source: &'a str,
    owners: &'a TopLevelOwnerTable,
    entries: Vec<FunctionProgramEntry>,
    expressions: Vec<ProgramExpressionRecord>,
    /// Source-order ordinal counter for nested served positions (hoisted
    /// nested function declarations and call-argument function values)
    /// across the file.
    next_nested_ordinal: u32,
}

impl<'a> DiscoveryCtx<'a> {
    fn anchor(&self, contributor_index: usize) -> Option<DeclContributorAnchor> {
        let owner = self.owners.statements().get(contributor_index)?;
        Some(DeclContributorAnchor {
            contributor_index: u32::try_from(contributor_index).ok()?,
            owner: owner.owner,
            owner_local_ordinal: owner.owner_local_ordinal,
        })
    }

    fn push(&mut self, entry: FunctionProgramEntry) {
        self.entries.push(entry);
    }
}

/// Build the per-file function program index from one retained parse
/// snapshot. One structural walk; no lowering, no type resolution.
pub fn build_function_program_index(
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
    owners: &TopLevelOwnerTable,
    canonical_id: Arc<str>,
) -> FunctionProgramIndex {
    let mut ctx = DiscoveryCtx {
        canonical_id,
        source,
        owners,
        entries: Vec::new(),
        expressions: Vec::new(),
        next_nested_ordinal: 0,
    };
    let mut overload_tracker = OverloadTracker::default();
    for (contributor_index, stmt) in program.body.iter().enumerate() {
        discover_statement(
            stmt,
            contributor_index,
            None,
            &mut overload_tracker,
            &mut ctx,
        );
    }
    resolve_captures(&mut ctx.entries);
    resolve_call_site_targets(&mut ctx.entries);
    resolve_direct_calls(&mut ctx.entries);
    link_callback_return_sources(&ctx.canonical_id, &mut ctx.entries, &mut ctx.expressions);
    ctx.expressions.sort_by_key(|record| record.span.start);
    FunctionProgramIndex {
        entries: Arc::from(ctx.entries.into_boxed_slice()),
        expressions: Arc::from(ctx.expressions.into_boxed_slice()),
    }
}

/// Resolve exact direct local call targets after discovery: a bare
/// identifier callee whose name binds a served function in the same index
/// (same file, same namespace qualification) targets the highest-ordinal
/// entry for that name — the trailing implementation of its overload
/// group. Computed callees, member calls, and unresolved names are never
/// direct calls.
fn resolve_direct_calls(entries: &mut [FunctionProgramEntry]) {
    let candidates: Vec<(Arc<str>, FunctionPartIdentity, u32, FunctionProgramKey)> = entries
        .iter()
        .map(|entry| {
            (
                Arc::clone(&entry.key.declaration.name),
                entry.key.part.clone(),
                entry.key.overload_ordinal,
                entry.key.clone(),
            )
        })
        .collect();
    for entry in entries.iter_mut() {
        let caller_ns = entry
            .key
            .declaration
            .name
            .rsplit_once('.')
            .map(|(ns, _)| ns.to_string());
        let mut direct = Vec::new();
        for effect in entry.effects.iter() {
            let FunctionEffectCallee::Identifier(callee) = &effect.callee else {
                continue;
            };
            // Lexical preference: the namespace-qualified binding
            // (`N.callee`) shadows the file-global one, exactly like
            // scoped name resolution — never the globally-highest overload
            // ordinal across both spellings.
            let best_for = |spelling: &str| {
                candidates
                    .iter()
                    .filter(|(name, part, _, _)| {
                        name.as_ref() == spelling
                            && matches!(
                                part,
                                FunctionPartIdentity::DeclarationBody
                                    | FunctionPartIdentity::Initializer
                            )
                    })
                    .max_by_key(|(_, _, ordinal, _)| *ordinal)
                    .map(|(_, _, _, key)| key.clone())
            };
            let target = caller_ns
                .as_ref()
                .and_then(|ns| best_for(&format!("{ns}.{callee}")))
                .or_else(|| best_for(callee));
            if let Some(target) = target {
                direct.push(FunctionDirectCall {
                    span: effect.span,
                    target,
                });
            }
        }
        entry.direct_calls = Arc::from(direct.into_boxed_slice());
    }
}

/// Whether `scope` lexically contains `site`.
fn scope_contains(scope: verter_span::Span, site: verter_span::Span) -> bool {
    scope.start <= site.start && site.end <= scope.end
}

/// Whether `inner` is strictly narrower than (or equally narrow as, but
/// later than) `outer` — the innermost-wins tiebreak of lexical lookup.
fn scope_is_at_least_as_inner(inner: verter_span::Span, outer: verter_span::Span) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

/// Compute every nested position's content-free capture identities: the
/// referenced names that bind in an enclosing frame, resolved LEXICALLY —
/// innermost enclosing frame first, and within a frame the innermost
/// same-name binding whose scope contains the capturing position. Each
/// distinct captured BINDING is recorded once, in first-reference source
/// order; identity is the `(defining frame, binding slot)` pair, so two
/// same-name binders never collapse. A name binding in NO enclosing frame
/// is not a capture (a free/global reference).
fn resolve_captures(entries: &mut [FunctionProgramEntry]) {
    // Snapshot the frame bindings + parents up front (no borrow conflicts).
    // The binding inventories are shared, not copied, and one key -> position
    // index resolves the whole parent chain by lookup. Duplicate keys keep
    // the FIRST position, matching source order.
    let frame_bindings: Vec<Arc<[FunctionBindingRecord]>> = entries
        .iter()
        .map(|entry| Arc::clone(&entry.bindings))
        .collect();
    let frame_keys: Vec<FunctionProgramKey> =
        entries.iter().map(|entry| entry.key.clone()).collect();
    let parents: Vec<Option<FunctionProgramKey>> = entries
        .iter()
        .map(|entry| entry.lexical_parent.as_deref().cloned())
        .collect();
    let mut position_of: rustc_hash::FxHashMap<FunctionProgramKey, usize> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(entries.len(), Default::default());
    for (position, entry) in entries.iter().enumerate() {
        position_of.entry(entry.key.clone()).or_insert(position);
    }
    for index in 0..entries.len() {
        let Some(parent) = parents[index].clone() else {
            continue;
        };
        // The enclosing frame chain, innermost first.
        let mut chain: Vec<usize> = Vec::new();
        let mut current = Some(parent);
        while let Some(key) = current {
            let Some(position) = position_of.get(&key).copied() else {
                break;
            };
            chain.push(position);
            current = parents[position].clone();
        }
        let site = entries[index].span;
        let mut captures: Vec<FlowBindingIdentity> = Vec::new();
        let mut seen: Vec<(usize, u32)> = Vec::new();
        for reference in entries[index].references.iter() {
            let Some((frame, slot)) =
                resolve_lexical_binding(&chain, &frame_bindings, &reference.name, site)
            else {
                continue;
            };
            if seen.contains(&(frame, slot)) {
                continue;
            }
            seen.push((frame, slot));
            captures.push(FlowBindingIdentity {
                name: Arc::clone(&reference.name),
                kind: frame_bindings[frame][slot as usize].kind,
                defining_function: frame_keys[frame].clone(),
                binding_slot: slot,
            });
        }
        entries[index].captures = CanonicalCaptureIdentity(Arc::from(captures.into_boxed_slice()));
    }
}

/// Resolve one referenced name against the enclosing frame chain: the
/// first (innermost) frame that binds it in a scope containing `site`
/// wins, and within that frame the innermost such binding wins (a later
/// binding wins over an earlier one of the same scope). Returns the
/// `(frame position, binding slot)` pair.
fn resolve_lexical_binding(
    chain: &[usize],
    frame_bindings: &[Arc<[FunctionBindingRecord]>],
    name: &Arc<str>,
    site: verter_span::Span,
) -> Option<(usize, u32)> {
    for &frame in chain {
        let bindings = &frame_bindings[frame];
        let mut best: Option<(u32, verter_span::Span)> = None;
        for (slot, binding) in bindings.iter().enumerate() {
            if &binding.name != name || !scope_contains(binding.scope_span, site) {
                continue;
            }
            let slot = u32::try_from(slot).unwrap_or(u32::MAX);
            best = match best {
                Some((_, best_scope))
                    if !scope_is_at_least_as_inner(binding.scope_span, best_scope) =>
                {
                    best
                }
                _ => Some((slot, binding.scope_span)),
            };
        }
        if let Some((slot, _)) = best {
            return Some((frame, slot));
        }
    }
    None
}

/// Walk every call expression inside one function body's statement list
/// in source order. Nested function frames (function / arrow / class
/// bodies) are NOT entered — a nested function's own call sites are
/// addressed through its own position. Discovery and the locator deref
/// share THIS walk, so a call site's `call_ordinal` means the same thing
/// on both sides BY CONSTRUCTION (no ordering drift, ever).
pub fn for_each_call_expression<'a>(
    statements: &'a [Statement<'a>],
    fire: impl FnMut(&'a CallExpression<'a>),
) {
    for_each_call_expression_root(CallExpressionWalkRoot::Statements(statements), fire);
}

/// Walk every call expression inside one expression in the same source
/// order and with the same nested-frame boundary as
/// [`for_each_call_expression`].
pub fn for_each_call_expression_in_expression<'a>(
    expression: &'a Expression<'a>,
    fire: impl FnMut(&'a CallExpression<'a>),
) {
    for_each_call_expression_root(CallExpressionWalkRoot::Expression(expression), fire);
}

enum CallExpressionWalkRoot<'a> {
    Statements(&'a [Statement<'a>]),
    Expression(&'a Expression<'a>),
}

fn for_each_call_expression_root<'a>(
    root: CallExpressionWalkRoot<'a>,
    mut fire: impl FnMut(&'a CallExpression<'a>),
) {
    fn walk_statements<'a>(
        statements: &'a [Statement<'a>],
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        for stmt in statements {
            walk_statement(stmt, fire);
        }
    }

    fn walk_statement<'a>(stmt: &'a Statement<'a>, fire: &mut impl FnMut(&'a CallExpression<'a>)) {
        match stmt {
            Statement::ExpressionStatement(expr) => walk_expr(&expr.expression, fire),
            Statement::BlockStatement(block) => walk_statements(&block.body, fire),
            Statement::IfStatement(if_stmt) => {
                walk_expr(&if_stmt.test, fire);
                walk_statement(&if_stmt.consequent, fire);
                if let Some(alternate) = &if_stmt.alternate {
                    walk_statement(alternate, fire);
                }
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    walk_for_init(init, fire);
                }
                if let Some(test) = &for_stmt.test {
                    walk_expr(test, fire);
                }
                if let Some(update) = &for_stmt.update {
                    walk_expr(update, fire);
                }
                walk_statement(&for_stmt.body, fire);
            }
            Statement::ForInStatement(for_stmt) => {
                walk_expr(&for_stmt.right, fire);
                walk_statement(&for_stmt.body, fire);
            }
            Statement::ForOfStatement(for_stmt) => {
                walk_expr(&for_stmt.right, fire);
                walk_statement(&for_stmt.body, fire);
            }
            Statement::WhileStatement(while_stmt) => {
                walk_expr(&while_stmt.test, fire);
                walk_statement(&while_stmt.body, fire);
            }
            Statement::DoWhileStatement(do_stmt) => {
                walk_statement(&do_stmt.body, fire);
                walk_expr(&do_stmt.test, fire);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(argument) = &ret.argument {
                    walk_expr(argument, fire);
                }
            }
            Statement::SwitchStatement(switch) => {
                walk_expr(&switch.discriminant, fire);
                for case in &switch.cases {
                    if let Some(test) = &case.test {
                        walk_expr(test, fire);
                    }
                    walk_statements(&case.consequent, fire);
                }
            }
            Statement::TryStatement(try_stmt) => {
                walk_statements(&try_stmt.block.body, fire);
                if let Some(handler) = &try_stmt.handler {
                    walk_statements(&handler.body.body, fire);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    walk_statements(&finalizer.body, fire);
                }
            }
            Statement::LabeledStatement(labeled) => walk_statement(&labeled.body, fire),
            Statement::ThrowStatement(throw) => walk_expr(&throw.argument, fire),
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        walk_expr(init, fire);
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(oxc_ast::ast::Declaration::VariableDeclaration(decl)) =
                    export.declaration.as_ref()
                {
                    for declarator in &decl.declarations {
                        if let Some(init) = &declarator.init {
                            walk_expr(init, fire);
                        }
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                if let Some(expression) = export.declaration.as_expression() {
                    walk_expr(expression, fire);
                }
            }
            // Nested frames (function / class bodies) and type-space
            // declarations carry no call sites of THIS frame.
            _ => {}
        }
    }

    fn walk_for_init<'a>(
        init: &'a oxc_ast::ast::ForStatementInit<'a>,
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        match init {
            oxc_ast::ast::ForStatementInit::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        walk_expr(init, fire);
                    }
                }
            }
            other => walk_expr(other.as_expression().unwrap(), fire),
        }
    }

    fn walk_simple_assignment_target<'a>(
        target: &'a oxc_ast::ast::SimpleAssignmentTarget<'a>,
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        match target {
            oxc_ast::ast::SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                walk_expr(&member.object, fire);
                walk_expr(&member.expression, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member) => {
                walk_expr(&member.object, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                walk_expr(&member.object, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::TSAsExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::TSSatisfiesExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::TSNonNullExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::TSTypeAssertion(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(_) => {}
        }
    }

    fn walk_assignment_target<'a>(
        target: &'a oxc_ast::ast::AssignmentTarget<'a>,
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        match target {
            oxc_ast::ast::AssignmentTarget::TSAsExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::AssignmentTarget::TSSatisfiesExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::AssignmentTarget::TSNonNullExpression(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::AssignmentTarget::TSTypeAssertion(ts) => {
                walk_expr(&ts.expression, fire);
            }
            oxc_ast::ast::AssignmentTarget::ComputedMemberExpression(member) => {
                walk_expr(&member.object, fire);
                walk_expr(&member.expression, fire);
            }
            oxc_ast::ast::AssignmentTarget::StaticMemberExpression(member) => {
                walk_expr(&member.object, fire);
            }
            oxc_ast::ast::AssignmentTarget::PrivateFieldExpression(member) => {
                walk_expr(&member.object, fire);
            }
            oxc_ast::ast::AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    walk_assignment_target_maybe_default(element, fire);
                }
                if let Some(rest) = &array.rest {
                    walk_assignment_target(&rest.target, fire);
                }
            }
            oxc_ast::ast::AssignmentTarget::ObjectAssignmentTarget(object) => {
                for property in &object.properties {
                    match property {
                        oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                            identifier,
                        ) => {
                            if let Some(init) = &identifier.init {
                                walk_expr(init, fire);
                            }
                        }
                        oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(
                            property,
                        ) => {
                            walk_assignment_target_maybe_default(&property.binding, fire);
                        }
                    }
                }
                if let Some(rest) = &object.rest {
                    walk_assignment_target(&rest.target, fire);
                }
            }
            oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(_) => {}
        }
    }

    fn walk_assignment_target_maybe_default<'a>(
        target: &'a oxc_ast::ast::AssignmentTargetMaybeDefault<'a>,
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        match target {
            oxc_ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(
                with_default,
            ) => {
                walk_assignment_target(&with_default.binding, fire);
                walk_expr(&with_default.init, fire);
            }
            other => walk_assignment_target(other.to_assignment_target(), fire),
        }
    }

    fn walk_argument<'a>(
        argument: &'a oxc_ast::ast::Argument<'a>,
        fire: &mut impl FnMut(&'a CallExpression<'a>),
    ) {
        match argument {
            oxc_ast::ast::Argument::SpreadElement(spread) => walk_expr(&spread.argument, fire),
            other => walk_expr(other.to_expression(), fire),
        }
    }

    fn walk_expr<'a>(expr: &'a Expression<'a>, fire: &mut impl FnMut(&'a CallExpression<'a>)) {
        match expr {
            Expression::CallExpression(call) => {
                fire(call);
                walk_expr(&call.callee, fire);
                for argument in &call.arguments {
                    walk_argument(argument, fire);
                }
            }
            Expression::NewExpression(new_expr) => {
                walk_expr(&new_expr.callee, fire);
                for argument in &new_expr.arguments {
                    walk_argument(argument, fire);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                            walk_expr(&spread.argument, fire);
                        }
                        other => walk_expr(other.to_expression(), fire),
                    }
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            if let Some(key) = property.key.as_expression() {
                                walk_expr(key, fire);
                            }
                            walk_expr(&property.value, fire);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            walk_expr(&spread.argument, fire);
                        }
                    }
                }
            }
            Expression::AssignmentExpression(assignment) => {
                walk_assignment_target(&assignment.left, fire);
                walk_expr(&assignment.right, fire);
            }
            Expression::AwaitExpression(await_expr) => walk_expr(&await_expr.argument, fire),
            Expression::UnaryExpression(unary) => walk_expr(&unary.argument, fire),
            Expression::UpdateExpression(update) => {
                walk_simple_assignment_target(&update.argument, fire);
            }
            Expression::BinaryExpression(binary) => {
                walk_expr(&binary.left, fire);
                walk_expr(&binary.right, fire);
            }
            Expression::LogicalExpression(logical) => {
                walk_expr(&logical.left, fire);
                walk_expr(&logical.right, fire);
            }
            Expression::ConditionalExpression(conditional) => {
                walk_expr(&conditional.test, fire);
                walk_expr(&conditional.consequent, fire);
                walk_expr(&conditional.alternate, fire);
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::CallExpression(call) => {
                    fire(call);
                    walk_expr(&call.callee, fire);
                    for argument in &call.arguments {
                        walk_argument(argument, fire);
                    }
                }
                oxc_ast::ast::ChainElement::TSNonNullExpression(ts) => {
                    walk_expr(&ts.expression, fire);
                }
                oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
                    walk_expr(&member.object, fire);
                    walk_expr(&member.expression, fire);
                }
                oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
                    walk_expr(&member.object, fire);
                }
                oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
                    walk_expr(&member.object, fire);
                }
            },
            Expression::ParenthesizedExpression(paren) => walk_expr(&paren.expression, fire),
            Expression::SequenceExpression(sequence) => {
                for expression in &sequence.expressions {
                    walk_expr(expression, fire);
                }
            }
            Expression::TaggedTemplateExpression(tagged) => {
                walk_expr(&tagged.tag, fire);
                for expression in &tagged.quasi.expressions {
                    walk_expr(expression, fire);
                }
            }
            Expression::TemplateLiteral(template) => {
                for expression in &template.expressions {
                    walk_expr(expression, fire);
                }
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(argument) = &yield_expr.argument {
                    walk_expr(argument, fire);
                }
            }
            Expression::PrivateInExpression(private_in) => {
                walk_expr(&private_in.right, fire);
            }
            Expression::ComputedMemberExpression(member) => {
                walk_expr(&member.object, fire);
                walk_expr(&member.expression, fire);
            }
            Expression::StaticMemberExpression(member) => walk_expr(&member.object, fire),
            Expression::PrivateFieldExpression(member) => walk_expr(&member.object, fire),
            Expression::ImportExpression(import) => {
                walk_expr(&import.source, fire);
                if let Some(options) = &import.options {
                    walk_expr(options, fire);
                }
            }
            Expression::TSAsExpression(ts) => walk_expr(&ts.expression, fire),
            Expression::TSSatisfiesExpression(ts) => walk_expr(&ts.expression, fire),
            Expression::TSTypeAssertion(ts) => walk_expr(&ts.expression, fire),
            Expression::TSNonNullExpression(ts) => walk_expr(&ts.expression, fire),
            Expression::TSInstantiationExpression(ts) => walk_expr(&ts.expression, fire),
            Expression::V8IntrinsicExpression(intrinsic) => {
                for argument in &intrinsic.arguments {
                    walk_argument(argument, fire);
                }
            }
            // Nested frames (function / arrow / class bodies) and leaves
            // carry no call sites of THIS frame.
            _ => {}
        }
    }

    match root {
        CallExpressionWalkRoot::Statements(statements) => walk_statements(statements, &mut fire),
        CallExpressionWalkRoot::Expression(expression) => walk_expr(expression, &mut fire),
    }
}

/// Resolve each indexed call site's exact same-file target after
/// discovery: a bare identifier callee whose name binds a served function
/// in the same index (same file, same namespace qualification) targets the
/// highest-ordinal entry for that name — the trailing implementation of
/// its overload group. Computed callees, member calls, and unresolved
/// names carry no target.
fn resolve_call_site_targets(entries: &mut [FunctionProgramEntry]) {
    let candidates: Vec<(Arc<str>, FunctionPartIdentity, u32, FunctionProgramKey)> = entries
        .iter()
        .map(|entry| {
            (
                Arc::clone(&entry.key.declaration.name),
                entry.key.part.clone(),
                entry.key.overload_ordinal,
                entry.key.clone(),
            )
        })
        .collect();
    // The hoisted nested function declarations each frame binds, keyed by
    // the enclosing frame. A declaration hoists over parameters, locals and
    // every file-level binding, so a bare-identifier call in the parent
    // frame binds HERE first.
    let nested_declarations: Vec<(FunctionProgramKey, Arc<str>, FunctionProgramKey)> = entries
        .iter()
        .filter_map(|entry| {
            let parent = entry.lexical_parent.as_deref()?.clone();
            let name = entry.nested_declaration_name.clone()?;
            Some((parent, name, entry.key.clone()))
        })
        .collect();
    for entry in entries.iter_mut() {
        let caller_ns = entry
            .key
            .declaration
            .name
            .rsplit_once('.')
            .map(|(ns, _)| ns.to_string());
        let mut sites = entry.call_sites.to_vec();
        for site in &mut sites {
            let FunctionEffectCallee::Identifier(callee) = &site.callee else {
                continue;
            };
            // Lexical preference: the namespace-qualified binding
            // (`N.callee`) shadows the file-global one, exactly like
            // scoped name resolution — never the globally-highest overload
            // ordinal across both spellings.
            let best_for = |spelling: &str| {
                candidates
                    .iter()
                    .filter(|(name, part, _, _)| {
                        name.as_ref() == spelling
                            && matches!(
                                part,
                                FunctionPartIdentity::DeclarationBody
                                    | FunctionPartIdentity::Initializer
                            )
                    })
                    .max_by_key(|(_, _, ordinal, _)| *ordinal)
                    .map(|(_, _, _, key)| key.clone())
            };
            let nested = nested_declarations.iter().find_map(|(parent, name, key)| {
                (parent == &entry.key && name.as_ref() == callee.as_ref()).then(|| key.clone())
            });
            site.target = nested.or_else(|| {
                caller_ns
                    .as_ref()
                    .and_then(|ns| best_for(&format!("{ns}.{callee}")))
                    .or_else(|| best_for(callee))
            });
        }
        entry.call_sites = Arc::from(sites.into_boxed_slice());
    }
}

fn link_callback_return_sources(
    canonical_id: &Arc<str>,
    entries: &mut [FunctionProgramEntry],
    expressions: &mut Vec<ProgramExpressionRecord>,
) {
    let callbacks: Vec<(
        u32,
        FunctionReturnSource,
        FunctionBodyLocator,
        verter_span::Span,
    )> = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.locator.descent.last(),
                Some(FunctionDescentStep::CallArgument { .. })
            )
        })
        .map(|entry| {
            let source = FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::clone(canonical_id),
                    owner: entry.key.declaration.owner,
                    symbol: Arc::clone(&entry.key.declaration.name),
                    space: LocatorSymbolSpace::Value,
                },
                function_part: entry.key.part.clone(),
                overload_ordinal: entry.key.overload_ordinal,
            });
            (entry.span.start, source, entry.locator.clone(), entry.span)
        })
        .collect();

    let link_args = |args: &mut Arc<[FunctionCallArgRecord]>| {
        let mut linked = args.to_vec();
        for argument in &mut linked {
            if let Some((_, source, _, _)) = callbacks
                .iter()
                .find(|(point, _, _, _)| *point == argument.point && argument.is_function_value)
            {
                argument.function_return_source = Some(source.clone());
            }
        }
        *args = Arc::from(linked.into_boxed_slice());
    };

    for entry in entries.iter_mut() {
        let mut sites = entry.call_sites.to_vec();
        for site in &mut sites {
            link_args(&mut site.args);
        }
        entry.call_sites = Arc::from(sites.into_boxed_slice());
    }
    for expression in expressions.iter_mut() {
        if let ProgramExpressionSource::SemanticCall { site, .. } = &mut expression.source {
            link_args(&mut site.args);
        }
    }
    expressions.extend(
        callbacks
            .into_iter()
            .map(|(offset, source, locator, span)| ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    offset,
                },
                span,
                locator,
                source: ProgramExpressionSource::FunctionReturn(source),
            }),
    );
}

/// Overload ordinals: consecutive per (name, member container) group in
/// source order, counting bodiless declarations (the trailing
/// implementation is the last ordinal).
#[derive(Default)]
struct OverloadTracker {
    function_counts: rustc_hash::FxHashMap<String, u32>,
}

impl OverloadTracker {
    fn next_function_ordinal(&mut self, name: &str) -> u32 {
        let count = self.function_counts.entry(name.to_string()).or_insert(0);
        let ordinal = *count;
        *count += 1;
        ordinal
    }
}

fn discover_statement(
    stmt: &Statement<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            discover_function_declaration(
                func,
                contributor_index,
                namespace_prefix,
                overload_tracker,
                ctx,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            discover_variable_declaration(var_decl, contributor_index, namespace_prefix, ctx);
        }
        Statement::ClassDeclaration(class) => {
            discover_class(class, contributor_index, namespace_prefix, ctx);
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
                match decl {
                    oxc_ast::ast::Declaration::FunctionDeclaration(func) => {
                        discover_function_declaration(
                            func,
                            contributor_index,
                            namespace_prefix,
                            overload_tracker,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::VariableDeclaration(var_decl) => {
                        discover_variable_declaration(
                            var_decl,
                            contributor_index,
                            namespace_prefix,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::ClassDeclaration(class) => {
                        discover_class(class, contributor_index, namespace_prefix, ctx);
                    }
                    _ => {}
                }
            }
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                if let Some(id) = func.id.as_ref() {
                    discover_function_declaration_named(
                        func,
                        id.name.as_str(),
                        contributor_index,
                        namespace_prefix,
                        overload_tracker,
                        ctx,
                    );
                }
            }
            oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class)
                if class.id.is_some() =>
            {
                discover_class(class, contributor_index, namespace_prefix, ctx);
            }
            other => {
                let obj = match other.as_expression() {
                    Some(Expression::ObjectExpression(obj)) => obj,
                    _ => return,
                };
                // `export default { … }` object methods are served member
                // positions of the `default` declaration (the merged-symbol
                // name the value side registers).
                for (member_ordinal, prop) in obj.properties.iter().enumerate() {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        continue;
                    };
                    if !p.method && matches!(p.kind, oxc_ast::ast::PropertyKind::Init) {
                        continue;
                    }
                    if static_property_key_name(&p.key).is_none() {
                        continue;
                    }
                    let member_path: Arc<[u32]> = Arc::from(
                        vec![u32::try_from(member_ordinal).unwrap_or(u32::MAX)].into_boxed_slice(),
                    );
                    let descent = vec![FunctionDescentStep::ExportDefaultObjectMember {
                        member_ordinal: u32::try_from(member_ordinal).unwrap_or(u32::MAX),
                    }];
                    match &p.value {
                        Expression::FunctionExpression(func) => {
                            discover_function_inner(
                                func,
                                "default",
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent,
                                0,
                                ctx,
                            );
                        }
                        Expression::ArrowFunctionExpression(arrow) => {
                            discover_arrow_inner(
                                arrow,
                                "default",
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent,
                                ctx,
                            );
                        }
                        _ => {}
                    }
                }
            }
        },
        Statement::TSModuleDeclaration(module) => {
            // `declare module "specifier" { .. }` is an ambient augmentation,
            // not a file-scope function owner — never indexed here. Identifier
            // namespaces recurse with qualified names.
            if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id {
                let prefix = match namespace_prefix {
                    Some(prefix) => format!("{prefix}.{}", id.name),
                    None => id.name.to_string(),
                };
                if let Some(oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block)) =
                    module.body.as_ref()
                {
                    for (statement_ordinal, inner) in block.body.iter().enumerate() {
                        discover_namespaced_statement(
                            inner,
                            contributor_index,
                            statement_ordinal,
                            &prefix,
                            overload_tracker,
                            ctx,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn discover_namespaced_statement(
    stmt: &Statement<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    match stmt {
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
                match decl {
                    oxc_ast::ast::Declaration::FunctionDeclaration(func) => {
                        discover_namespaced_function(
                            func,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            overload_tracker,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::VariableDeclaration(var_decl) => {
                        discover_variable_declaration_ns(
                            var_decl,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            ctx,
                        );
                    }
                    oxc_ast::ast::Declaration::ClassDeclaration(class) => {
                        discover_class_ns(
                            class,
                            contributor_index,
                            statement_ordinal,
                            namespace,
                            ctx,
                        );
                    }
                    _ => {}
                }
            }
        }
        Statement::FunctionDeclaration(func) => {
            discover_namespaced_function(
                func,
                contributor_index,
                statement_ordinal,
                namespace,
                overload_tracker,
                ctx,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            discover_variable_declaration_ns(
                var_decl,
                contributor_index,
                statement_ordinal,
                namespace,
                ctx,
            );
        }
        Statement::ClassDeclaration(class) => {
            discover_class_ns(class, contributor_index, statement_ordinal, namespace, ctx);
        }
        Statement::TSModuleDeclaration(module) => {
            if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id {
                let prefix = format!("{namespace}.{}", id.name);
                if let Some(oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block)) =
                    module.body.as_ref()
                {
                    for (inner_ordinal, inner) in block.body.iter().enumerate() {
                        let _ = inner_ordinal;
                        discover_namespaced_statement(
                            inner,
                            contributor_index,
                            inner_ordinal,
                            &prefix,
                            overload_tracker,
                            ctx,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn discover_namespaced_function(
    func: &Function<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    if let Some(id) = func.id.as_ref() {
        let qualified = format!("{namespace}.{}", id.name);
        let overload_ordinal = overload_tracker.next_function_ordinal(&qualified);
        discover_function_inner(
            func,
            &qualified,
            FunctionPartIdentity::DeclarationBody,
            contributor_index,
            vec![
                FunctionDescentStep::NamespaceMember {
                    statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
                },
                FunctionDescentStep::FunctionDeclaration,
            ],
            overload_ordinal,
            ctx,
        );
    }
}

fn discover_function_declaration(
    func: &Function<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(id) = func.id.as_ref() else {
        return;
    };
    discover_function_declaration_named(
        func,
        id.name.as_str(),
        contributor_index,
        namespace_prefix,
        overload_tracker,
        ctx,
    );
}

fn discover_function_declaration_named(
    func: &Function<'_>,
    name: &str,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    overload_tracker: &mut OverloadTracker,
    ctx: &mut DiscoveryCtx<'_>,
) {
    if func.body.is_none() {
        // A bodiless overload declaration consumes its group ordinal but has
        // no body to serve.
        overload_tracker.next_function_ordinal(name);
        return;
    }
    let name = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{name}"),
        None => name.to_string(),
    };
    let overload_ordinal = overload_tracker.next_function_ordinal(&name);
    discover_function_inner(
        func,
        &name,
        FunctionPartIdentity::DeclarationBody,
        contributor_index,
        vec![FunctionDescentStep::FunctionDeclaration],
        overload_ordinal,
        ctx,
    );
}

fn discover_variable_declaration(
    var_decl: &VariableDeclaration<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    for (declarator_ordinal, declarator) in var_decl.declarations.iter().enumerate() {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let name = match namespace_prefix {
            Some(prefix) => format!("{prefix}.{}", id.name),
            None => id.name.to_string(),
        };
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        let declarator_ordinal = u32::try_from(declarator_ordinal).unwrap_or(u32::MAX);
        let base_descent = vec![FunctionDescentStep::VariableInitializer { declarator_ordinal }];
        if let Some(anchor) = ctx.anchor(contributor_index) {
            ctx.expressions.push(ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(&ctx.canonical_id),
                    offset: init.span().start,
                },
                span: init.span().into(),
                locator: FunctionBodyLocator {
                    contributor: anchor,
                    descent: Arc::from(base_descent.clone().into_boxed_slice()),
                },
                source: program_expression_source(init),
            });
            discover_top_level_call_arg_positions(init, &name, anchor, &base_descent, ctx);
        }
        let descent = |extra: FunctionDescentStep| {
            vec![
                FunctionDescentStep::VariableInitializer { declarator_ordinal },
                extra,
            ]
        };
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base_descent.clone(),
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base_descent,
                    0,
                    ctx,
                );
            }
            Expression::ObjectExpression(obj) => {
                for (member_ordinal, prop) in obj.properties.iter().enumerate() {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        continue;
                    };
                    if !p.method && matches!(p.kind, oxc_ast::ast::PropertyKind::Init) {
                        continue;
                    }
                    if static_property_key_name(&p.key).is_none() {
                        continue;
                    }
                    let member_path: Arc<[u32]> = Arc::from(
                        vec![u32::try_from(member_ordinal).unwrap_or(u32::MAX)].into_boxed_slice(),
                    );
                    match &p.value {
                        Expression::FunctionExpression(func) => {
                            discover_function_inner(
                                func,
                                &name,
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent(FunctionDescentStep::ObjectMember {
                                    member_ordinal: u32::try_from(member_ordinal)
                                        .unwrap_or(u32::MAX),
                                }),
                                0,
                                ctx,
                            );
                        }
                        Expression::ArrowFunctionExpression(arrow) => {
                            discover_arrow_inner(
                                arrow,
                                &name,
                                FunctionPartIdentity::Member { member_path },
                                contributor_index,
                                descent(FunctionDescentStep::ObjectMember {
                                    member_ordinal: u32::try_from(member_ordinal)
                                        .unwrap_or(u32::MAX),
                                }),
                                ctx,
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn unwrap_program_expression<'a>(mut expression: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        expression = match expression {
            Expression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
            Expression::TSAsExpression(assertion) => &assertion.expression,
            Expression::TSSatisfiesExpression(satisfies) => &satisfies.expression,
            Expression::TSNonNullExpression(non_null) => &non_null.expression,
            _ => return expression,
        };
    }
}

fn call_arg_record(argument: &oxc_ast::ast::Argument<'_>) -> FunctionCallArgRecord {
    let expression = argument.as_expression();
    FunctionCallArgRecord {
        point: expression
            .map(|expression| expression.span().start)
            .unwrap_or_else(|| argument.span().start),
        spread: matches!(argument, oxc_ast::ast::Argument::SpreadElement(_)),
        literal_mode: match expression.map(unwrap_program_expression) {
            Some(
                Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::TemplateLiteral(_)
                | Expression::ObjectExpression(_)
                | Expression::ArrayExpression(_),
            ) => FunctionCallArgLiteralMode::Literal,
            _ => FunctionCallArgLiteralMode::Widened,
        },
        is_function_value: matches!(
            expression.map(unwrap_program_expression),
            Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
        ),
        function_return_source: None,
    }
}

fn effect_callee(callee: &Expression<'_>) -> FunctionEffectCallee {
    match callee {
        Expression::Identifier(id) => FunctionEffectCallee::Identifier(Arc::from(id.name.as_str())),
        Expression::StaticMemberExpression(member) => {
            let mut path = Vec::new();
            if collect_static_member_path(member, &mut path) {
                FunctionEffectCallee::StaticMember(Arc::from(path.into_boxed_slice()))
            } else {
                FunctionEffectCallee::Other
            }
        }
        _ => FunctionEffectCallee::Other,
    }
}

fn call_site_record(call: &CallExpression<'_>) -> FunctionCallSiteRecord {
    FunctionCallSiteRecord {
        span: call.span.into(),
        callee: effect_callee(&call.callee),
        target: None,
        args: Arc::from(
            call.arguments
                .iter()
                .map(call_arg_record)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn new_site_record(call: &oxc_ast::ast::NewExpression<'_>) -> FunctionCallSiteRecord {
    FunctionCallSiteRecord {
        span: call.span.into(),
        callee: effect_callee(&call.callee),
        target: None,
        args: Arc::from(
            call.arguments
                .iter()
                .map(call_arg_record)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn program_expression_source(expression: &Expression<'_>) -> ProgramExpressionSource {
    match unwrap_program_expression(expression) {
        Expression::CallExpression(call) => ProgramExpressionSource::SemanticCall {
            kind: ProgramExpressionCallKind::Call,
            site: call_site_record(call),
        },
        Expression::NewExpression(call) => ProgramExpressionSource::SemanticCall {
            kind: ProgramExpressionCallKind::Construct,
            site: new_site_record(call),
        },
        expression if expression_has_call(expression) => ProgramExpressionSource::UnsupportedCall,
        _ => ProgramExpressionSource::Value,
    }
}

fn expression_has_call(expression: &Expression<'_>) -> bool {
    #[derive(Default)]
    struct Probe(bool);
    impl<'a> Visit<'a> for Probe {
        fn visit_call_expression(&mut self, _call: &CallExpression<'a>) {
            self.0 = true;
        }

        fn visit_new_expression(&mut self, _call: &oxc_ast::ast::NewExpression<'a>) {
            self.0 = true;
        }

        fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {}

        fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}
    }
    let mut probe = Probe::default();
    probe.visit_expression(expression);
    probe.0
}

fn discover_top_level_call_arg_positions(
    expression: &Expression<'_>,
    declaration_name: &str,
    contributor: DeclContributorAnchor,
    base_descent: &[FunctionDescentStep],
    ctx: &mut DiscoveryCtx<'_>,
) {
    let mut call_ordinal = 0usize;
    for_each_call_expression_in_expression(expression, |call| {
        let current_call_ordinal = call_ordinal;
        call_ordinal += 1;
        for (arg_ordinal, argument) in call.arguments.iter().enumerate() {
            let Some(expression) = argument.as_expression() else {
                continue;
            };
            let node = match unwrap_program_expression(expression) {
                Expression::ArrowFunctionExpression(arrow) => FunctionNode::Arrow(arrow),
                Expression::FunctionExpression(function) => FunctionNode::Function(function),
                _ => continue,
            };
            let ordinal = ctx.next_nested_ordinal;
            ctx.next_nested_ordinal += 1;
            let mut descent = base_descent.to_vec();
            descent.push(FunctionDescentStep::CallArgument {
                call_ordinal: u32::try_from(current_call_ordinal).unwrap_or(u32::MAX),
                arg_ordinal: u32::try_from(arg_ordinal).unwrap_or(u32::MAX),
            });
            discover_top_level_callable(
                node,
                expression.span().into(),
                declaration_name,
                contributor,
                descent,
                ordinal,
                ctx,
            );
        }
    });
}

fn discover_top_level_callable(
    node: FunctionNode<'_>,
    span: verter_span::Span,
    declaration_name: &str,
    contributor: DeclContributorAnchor,
    descent: Vec<FunctionDescentStep>,
    ordinal: u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let (params, statements) = match &node {
        FunctionNode::Function(function) => {
            let Some(body) = function.body.as_ref() else {
                return;
            };
            (formal_params(&function.params), &body.statements[..])
        }
        FunctionNode::Arrow(arrow) => (formal_params(&arrow.params), &arrow.body.statements[..]),
    };
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: contributor.owner,
            name: Arc::from(declaration_name),
            space: SymbolSpace::Value,
        },
        part: FunctionPartIdentity::Other { ordinal },
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor,
        descent: Arc::from(descent.into_boxed_slice()),
    };
    let entry = build_entry(
        ctx.source,
        key.clone(),
        locator.clone(),
        params,
        statements,
        span.start,
        node,
    );
    ctx.push(entry);
    discover_nested_positions(statements, &key, &locator, ctx);
}

fn discover_variable_declaration_ns(
    var_decl: &VariableDeclaration<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_>,
) {
    for (declarator_ordinal, declarator) in var_decl.declarations.iter().enumerate() {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let name = format!("{namespace}.{}", id.name);
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        let declarator_ordinal = u32::try_from(declarator_ordinal).unwrap_or(u32::MAX);
        let base = vec![
            FunctionDescentStep::NamespaceMember {
                statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
            },
            FunctionDescentStep::VariableInitializer { declarator_ordinal },
        ];
        if let Some(anchor) = ctx.anchor(contributor_index) {
            ctx.expressions.push(ProgramExpressionRecord {
                point: ProgramExpressionIdentity {
                    canonical_id: Arc::clone(&ctx.canonical_id),
                    offset: init.span().start,
                },
                span: init.span().into(),
                locator: FunctionBodyLocator {
                    contributor: anchor,
                    descent: Arc::from(base.clone().into_boxed_slice()),
                },
                source: program_expression_source(init),
            });
            discover_top_level_call_arg_positions(init, &name, anchor, &base, ctx);
        }
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                discover_arrow_inner(
                    arrow,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base.clone(),
                    ctx,
                );
            }
            Expression::FunctionExpression(func) => {
                discover_function_inner(
                    func,
                    &name,
                    FunctionPartIdentity::Initializer,
                    contributor_index,
                    base.clone(),
                    0,
                    ctx,
                );
            }
            _ => {}
        }
    }
}

fn discover_class(
    class: &Class<'_>,
    contributor_index: usize,
    namespace_prefix: Option<&str>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = match namespace_prefix {
        Some(prefix) => format!("{prefix}.{}", id.name),
        None => id.name.to_string(),
    };
    discover_class_members(class, &name, contributor_index, Vec::new(), ctx);
}

fn discover_class_ns(
    class: &Class<'_>,
    contributor_index: usize,
    statement_ordinal: usize,
    namespace: &str,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(id) = class.id.as_ref() else {
        return;
    };
    let name = format!("{namespace}.{}", id.name);
    discover_class_members(
        class,
        &name,
        contributor_index,
        vec![FunctionDescentStep::NamespaceMember {
            statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
        }],
        ctx,
    );
}

fn discover_class_members(
    class: &Class<'_>,
    name: &str,
    contributor_index: usize,
    base_descent: Vec<FunctionDescentStep>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let mut member_overloads: rustc_hash::FxHashMap<(String, bool), u32> =
        rustc_hash::FxHashMap::default();
    for (member_ordinal, element) in class.body.body.iter().enumerate() {
        let member_ordinal = u32::try_from(member_ordinal).unwrap_or(u32::MAX);
        match element {
            oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                if matches!(method.kind, MethodDefinitionKind::Constructor) {
                    continue;
                }
                let Some(member_name) = static_property_key_name(&method.key) else {
                    continue;
                };
                let overload = {
                    let count = member_overloads
                        .entry((member_name.clone(), method.r#static))
                        .or_insert(0);
                    let ordinal = *count;
                    *count += 1;
                    ordinal
                };
                if method.value.body.is_none() {
                    continue;
                }
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let mut descent = base_descent.clone();
                descent.push(FunctionDescentStep::ClassMember { member_ordinal });
                discover_function_inner(
                    &method.value,
                    name,
                    FunctionPartIdentity::Member { member_path },
                    contributor_index,
                    descent,
                    overload,
                    ctx,
                );
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(prop) => {
                let Some(_member_name) = static_property_key_name(&prop.key) else {
                    continue;
                };
                let member_path: Arc<[u32]> = Arc::from(vec![member_ordinal].into_boxed_slice());
                let mut descent = base_descent.clone();
                descent.push(FunctionDescentStep::ClassMember { member_ordinal });
                match prop.value.as_ref() {
                    Some(Expression::ArrowFunctionExpression(arrow)) => {
                        discover_arrow_inner(
                            arrow,
                            name,
                            FunctionPartIdentity::Member { member_path },
                            contributor_index,
                            descent,
                            ctx,
                        );
                    }
                    Some(Expression::FunctionExpression(func)) => {
                        discover_function_inner(
                            func,
                            name,
                            FunctionPartIdentity::Member { member_path },
                            contributor_index,
                            descent,
                            0,
                            ctx,
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn static_property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    crate::analysis::flow::static_property_key_text(key).map(str::to_owned)
}

fn discover_function_inner(
    func: &Function<'_>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: Vec<FunctionDescentStep>,
    overload_ordinal: u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let Some(body) = func.body.as_ref() else {
        return;
    };
    let params = formal_params(&func.params);
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: anchor.owner,
            name: Arc::from(name),
            space: SymbolSpace::Value,
        },
        part,
        overload_ordinal,
    };
    let locator = FunctionBodyLocator {
        contributor: anchor,
        descent: Arc::from(descent.into_boxed_slice()),
    };
    let entry = build_entry(
        ctx.source,
        key.clone(),
        locator.clone(),
        params,
        &body.statements,
        func.span.start,
        FunctionNode::Function(func),
    );
    ctx.push(entry);
    discover_nested_positions(&body.statements, &key, &locator, ctx);
}

fn discover_arrow_inner(
    arrow: &ArrowFunctionExpression<'_>,
    name: &str,
    part: FunctionPartIdentity,
    contributor_index: usize,
    descent: Vec<FunctionDescentStep>,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(anchor) = ctx.anchor(contributor_index) else {
        return;
    };
    let params = formal_params(&arrow.params);
    let key = FunctionProgramKey {
        declaration: FunctionDeclarationRef {
            owner: anchor.owner,
            name: Arc::from(name),
            space: SymbolSpace::Value,
        },
        part,
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor: anchor,
        descent: Arc::from(descent.into_boxed_slice()),
    };
    let entry = build_entry(
        ctx.source,
        key.clone(),
        locator.clone(),
        params,
        &arrow.body.statements,
        arrow.span.start,
        FunctionNode::Arrow(arrow),
    );
    ctx.push(entry);
    discover_nested_positions(&arrow.body.statements, &key, &locator, ctx);
}

/// Discover the nested served positions inside one function body:
/// hoisted nested function declarations (their own frames) and function /
/// arrow expressions in call-argument position (callback positions).
/// Each nested position gets an exact `FunctionProgramKey` (the parent's
/// declaration identity with part `Other { ordinal }` — source-order
/// among nested positions in the file), a body locator (the parent's
/// descent + the nested step), and its lexical parent. Nested positions
/// recurse: a callback's own body is discovered under the callback's key.
fn discover_nested_positions(
    statements: &[Statement<'_>],
    parent_key: &FunctionProgramKey,
    parent_locator: &FunctionBodyLocator,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let mut call_ordinal: u32 = 0;
    for (statement_ordinal, stmt) in statements.iter().enumerate() {
        if let Statement::FunctionDeclaration(func) = stmt {
            if func.id.is_some() {
                let ordinal = ctx.next_nested_ordinal;
                ctx.next_nested_ordinal += 1;
                let mut descent = parent_locator.descent.to_vec();
                descent.push(FunctionDescentStep::BodyStatement {
                    statement_ordinal: u32::try_from(statement_ordinal).unwrap_or(u32::MAX),
                });
                discover_nested_function(
                    func,
                    parent_key,
                    parent_locator.contributor,
                    descent,
                    ordinal,
                    ctx,
                );
            }
            continue;
        }
        discover_call_positions(stmt, parent_key, parent_locator, &mut call_ordinal, ctx);
    }
}

/// One hoisted nested function declaration, discovered under its lexical
/// parent's key.
fn discover_nested_function(
    func: &Function<'_>,
    parent_key: &FunctionProgramKey,
    contributor: DeclContributorAnchor,
    descent: Vec<FunctionDescentStep>,
    ordinal: u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let Some(body) = func.body.as_ref() else {
        return;
    };
    let params = formal_params(&func.params);
    let key = FunctionProgramKey {
        declaration: parent_key.declaration.clone(),
        part: FunctionPartIdentity::Other { ordinal },
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor,
        descent: Arc::from(descent.into_boxed_slice()),
    };
    let mut entry = build_entry(
        ctx.source,
        key.clone(),
        locator.clone(),
        params,
        &body.statements,
        func.span.start,
        FunctionNode::Function(func),
    );
    entry.lexical_parent = Some(Box::new(parent_key.clone()));
    entry.nested_declaration_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
    ctx.push(entry);
    discover_nested_positions(&body.statements, &key, &locator, ctx);
}

/// One function / arrow expression in call-argument position, discovered
/// under its lexical parent's key.
fn discover_nested_callable(
    node: FunctionNode<'_>,
    span: verter_span::Span,
    parent_key: &FunctionProgramKey,
    parent_locator: &FunctionBodyLocator,
    descent: Vec<FunctionDescentStep>,
    ordinal: u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    let (params, statements) = match &node {
        FunctionNode::Function(func) => {
            let Some(body) = func.body.as_ref() else {
                return;
            };
            (formal_params(&func.params), &body.statements[..])
        }
        FunctionNode::Arrow(arrow) => (formal_params(&arrow.params), &arrow.body.statements[..]),
    };
    let key = FunctionProgramKey {
        declaration: parent_key.declaration.clone(),
        part: FunctionPartIdentity::Other { ordinal },
        overload_ordinal: 0,
    };
    let locator = FunctionBodyLocator {
        contributor: parent_locator.contributor,
        descent: Arc::from(descent.into_boxed_slice()),
    };
    let mut entry = build_entry(
        ctx.source,
        key.clone(),
        locator.clone(),
        params,
        statements,
        span.start,
        node,
    );
    entry.lexical_parent = Some(Box::new(parent_key.clone()));
    ctx.push(entry);
    discover_nested_positions(statements, &key, &locator, ctx);
}

/// Walk one statement for the function values a call site binds — its
/// callee when that callee is an immediately-invoked function expression,
/// and every function / arrow ARGUMENT (callback positions) — without
/// entering nested frames (each nested frame discovers its own call
/// sites). The call ordinal comes from the ONE shared call-site walk
/// ([`for_each_call_expression`]) — the locator deref resolves ordinals
/// through the same walk.
fn discover_call_positions(
    stmt: &Statement<'_>,
    parent_key: &FunctionProgramKey,
    parent_locator: &FunctionBodyLocator,
    call_ordinal: &mut u32,
    ctx: &mut DiscoveryCtx<'_>,
) {
    for_each_call_expression(std::slice::from_ref(stmt), |call| {
        let ordinal = *call_ordinal;
        *call_ordinal += 1;
        if let Some(node) = match &call.callee {
            Expression::ArrowFunctionExpression(arrow) => Some(FunctionNode::Arrow(arrow)),
            Expression::FunctionExpression(func) => Some(FunctionNode::Function(func)),
            Expression::ParenthesizedExpression(paren) => match &paren.expression {
                Expression::ArrowFunctionExpression(arrow) => Some(FunctionNode::Arrow(arrow)),
                Expression::FunctionExpression(func) => Some(FunctionNode::Function(func)),
                _ => None,
            },
            _ => None,
        } {
            let nested_ordinal = ctx.next_nested_ordinal;
            ctx.next_nested_ordinal += 1;
            let mut descent = parent_locator.descent.to_vec();
            descent.push(FunctionDescentStep::CallCallee {
                call_ordinal: ordinal,
            });
            discover_nested_callable(
                node,
                call.callee.span().into(),
                parent_key,
                parent_locator,
                descent,
                nested_ordinal,
                ctx,
            );
        }
        for (arg_ordinal, argument) in call.arguments.iter().enumerate() {
            let Some(expression) = argument.as_expression() else {
                continue;
            };
            let node = match expression {
                Expression::ArrowFunctionExpression(arrow) => FunctionNode::Arrow(arrow),
                Expression::FunctionExpression(func) => FunctionNode::Function(func),
                _ => continue,
            };
            let nested_ordinal = ctx.next_nested_ordinal;
            ctx.next_nested_ordinal += 1;
            let mut descent = parent_locator.descent.to_vec();
            descent.push(FunctionDescentStep::CallArgument {
                call_ordinal: ordinal,
                arg_ordinal: u32::try_from(arg_ordinal).unwrap_or(u32::MAX),
            });
            discover_nested_callable(
                node,
                expression.span().into(),
                parent_key,
                parent_locator,
                descent,
                nested_ordinal,
                ctx,
            );
        }
    });
}

fn formal_params(params: &oxc_ast::ast::FormalParameters<'_>) -> Arc<[FunctionParamRecord]> {
    let mut out: Vec<FunctionParamRecord> = params
        .items
        .iter()
        .map(|param| FunctionParamRecord {
            name: match &param.pattern {
                BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                _ => None,
            },
            optional: param.optional,
            rest: false,
            has_ts_annotation: param.type_annotation.is_some(),
        })
        .collect();
    if let Some(rest) = params.rest.as_ref() {
        out.push(FunctionParamRecord {
            name: match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                _ => None,
            },
            optional: false,
            rest: true,
            has_ts_annotation: false,
        });
    }
    Arc::from(out.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// Type-parameter occurrence in the formal parameter list
// ---------------------------------------------------------------------------

/// For each type name referenced from a formal parameter's authored
/// annotation, the SMALLEST parameter ordinal that references it.
///
/// A caller's inference oracle: TypeScript infers a type argument only
/// from an argument the call actually supplies at a parameter position
/// whose type names the parameter, and falls back to the declared
/// default only when inference produced NO candidate. That is a purely
/// SYNTACTIC question about the callee's parameter list, so it is a
/// shallow index fact rather than a lowering.
#[derive(Debug, Default)]
struct TypeParamOccurrences {
    /// `(referenced name, smallest parameter ordinal)`.
    first: rustc_hash::FxHashMap<String, u32>,
}

impl TypeParamOccurrences {
    fn of(node: &FunctionNode<'_>) -> Self {
        let params = match node {
            FunctionNode::Function(func) => &func.params,
            FunctionNode::Arrow(arrow) => &arrow.params,
        };
        let mut out = Self::default();
        for (ordinal, param) in params.items.iter().enumerate() {
            if let Some(annotation) = param.type_annotation.as_ref() {
                out.collect(&annotation.type_annotation, ordinal as u32);
            }
        }
        if let Some(rest) = params.rest.as_ref() {
            if let Some(annotation) = rest.type_annotation.as_ref() {
                out.collect(&annotation.type_annotation, params.items.len() as u32);
            }
        }
        out
    }

    fn collect(&mut self, ty: &oxc_ast::ast::TSType<'_>, ordinal: u32) {
        let mut visitor = ReferencedTypeNames {
            found: Vec::new(),
            shadowed: Vec::new(),
        };
        visitor.visit_ts_type(ty);
        for name in visitor.found {
            self.first
                .entry(name)
                .and_modify(|existing| *existing = (*existing).min(ordinal))
                .or_insert(ordinal);
        }
    }

    fn first_ordinal(&self, name: &str) -> Option<u32> {
        self.first.get(name).copied()
    }
}

/// The HEAD names of every type reference inside one authored type
/// annotation, skipping any subtree a nested type-parameter clause
/// re-declares (a nested signature owns its own binders, so an outer
/// clause parameter is not referenced there).
struct ReferencedTypeNames {
    found: Vec<String>,
    /// The stack of names nested clauses currently shadow.
    shadowed: Vec<Vec<String>>,
}

impl ReferencedTypeNames {
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed
            .iter()
            .any(|frame| frame.iter().any(|shadow| shadow == name))
    }

    fn with_clause<R>(
        &mut self,
        declaration: Option<&oxc_ast::ast::TSTypeParameterDeclaration<'_>>,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let names: Vec<String> = declaration
            .map(|declaration| {
                declaration
                    .params
                    .iter()
                    .map(|param| param.name.name.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.shadowed.push(names);
        let out = body(self);
        self.shadowed.pop();
        out
    }
}

impl<'a> Visit<'a> for ReferencedTypeNames {
    fn visit_ts_type_reference(&mut self, reference: &oxc_ast::ast::TSTypeReference<'a>) {
        if let oxc_ast::ast::TSTypeName::IdentifierReference(id) = &reference.type_name {
            if !self.is_shadowed(id.name.as_str()) {
                self.found.push(id.name.as_str().to_string());
            }
        }
        walk::walk_ts_type_reference(self, reference);
    }

    fn visit_ts_function_type(&mut self, function: &oxc_ast::ast::TSFunctionType<'a>) {
        self.with_clause(function.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_function_type(visitor, function);
        });
    }

    fn visit_ts_constructor_type(&mut self, constructor: &oxc_ast::ast::TSConstructorType<'a>) {
        self.with_clause(constructor.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_constructor_type(visitor, constructor);
        });
    }

    fn visit_ts_method_signature(&mut self, signature: &oxc_ast::ast::TSMethodSignature<'a>) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_method_signature(visitor, signature);
        });
    }

    fn visit_ts_call_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSCallSignatureDeclaration<'a>,
    ) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_call_signature_declaration(visitor, signature);
        });
    }

    fn visit_ts_construct_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSConstructSignatureDeclaration<'a>,
    ) {
        self.with_clause(signature.type_parameters.as_deref(), |visitor| {
            walk::walk_ts_construct_signature_declaration(visitor, signature);
        });
    }
}

// ---------------------------------------------------------------------------
// Entry build: inventory + stable hash
// ---------------------------------------------------------------------------

fn build_entry(
    source: &str,
    key: FunctionProgramKey,
    locator: FunctionBodyLocator,
    params: Arc<[FunctionParamRecord]>,
    statements: &[Statement<'_>],
    function_start: u32,
    node: FunctionNode<'_>,
) -> FunctionProgramEntry {
    let function_end = match node {
        FunctionNode::Function(function) => function.span.end,
        FunctionNode::Arrow(arrow) => arrow.span.end,
    };
    let frame_span = verter_span::Span::new(function_start, function_end);
    let mut inventory = InventoryVisitor {
        frame_span,
        ..InventoryVisitor::default()
    };
    // Parameters bind first in source order, before any body statement.
    for param in params.iter() {
        if let Some(name) = param.name.as_ref() {
            inventory.bindings.push(FunctionBindingRecord {
                name: Arc::clone(name),
                kind: FunctionBindingKind::Param,
                span: verter_span::Span::new(0, 0),
                scope_span: frame_span,
            });
        }
    }
    for stmt in statements {
        inventory.visit_statement(stmt);
    }
    let InventoryVisitor {
        mut bindings,
        references,
        return_sites,
        writes,
        effects,
        control,
        control_stack: _,
        scope_stack: _,
        frame_span: _,
    } = inventory;
    // The indexed call sites come from the ONE shared call-site walk
    // (`for_each_call_expression`) — the same ordering the callback
    // locator ordinals and the deref use, by construction.
    let mut call_sites = Vec::new();
    for_each_call_expression(statements, |call| {
        call_sites.push(call_site_record(call));
    });
    bindings.dedup_by(|left, right| left.name == right.name && left.kind == right.kind);

    let parameter_occurrences = TypeParamOccurrences::of(&node);
    let type_parameters: Vec<FunctionProgramTypeParam> = node
        .type_parameters()
        .map(|declaration| {
            declaration
                .params
                .iter()
                .map(|param| {
                    let name = param.name.name.as_str();
                    FunctionProgramTypeParam {
                        name: Arc::from(name),
                        has_default: param.default.is_some(),
                        first_parameter_occurrence: parameter_occurrences.first_ordinal(name),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let function_span = node.span();
    let hash = crate::analysis::function_program_hash::hash_function_body(
        source,
        statements,
        &params,
        function_start,
        node,
    );
    // A span outside the source is a MISS, never the empty string's hash:
    // hashing `b""` gives every out-of-range entry the same constant and
    // silently retires the exact-content axis for all of them.
    let exact_hash = source
        .get(function_span.start as usize..function_span.end as usize)
        .map(|text| crate::analysis::types::hash_16(text.as_bytes()));

    FunctionProgramEntry {
        key,
        span: frame_span,
        locator,
        params,
        bindings: Arc::from(bindings.into_boxed_slice()),
        references: Arc::from(references.into_boxed_slice()),
        return_sites: Arc::from(return_sites.into_boxed_slice()),
        writes: Arc::from(writes.into_boxed_slice()),
        effects: Arc::from(effects.into_boxed_slice()),
        call_sites: Arc::from(call_sites.into_boxed_slice()),
        control: Arc::from(control.into_boxed_slice()),
        direct_calls: Arc::from(Vec::new().into_boxed_slice()),
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        lexical_parent: None,
        nested_declaration_name: None,
        captures: CanonicalCaptureIdentity::default(),
        flow_body_stable_hash: hash,
        flow_body_exact_hash: exact_hash,
    }
}

/// The out-of-index statement-list inventory — the SAME single inventory
/// walk the index uses (a control region's `has_return` is marked when
/// the walk meets a `return` of the current function; nested function
/// bodies are never entered). For consumers that lower a body outside the
/// index (e.g. a nested function value's body in the flow IR).
#[derive(Default)]
pub struct StatementListInventory {
    /// The control-region skeleton.
    pub control: Vec<FunctionControlRegion>,
    /// The hoisted nested function declaration names bound in this frame.
    pub nested_function_names: Vec<Arc<str>>,
    /// The function-scoped (`var` / `using`) declarator names bound in
    /// this frame — the bindings that OUTLIVE the statement list that
    /// declares them.
    pub var_names: Vec<Arc<str>>,
}

/// Inventory one statement list with the SAME single walk the index uses.
pub fn inventory_statement_list(statements: &[Statement<'_>]) -> StatementListInventory {
    let mut inventory = InventoryVisitor::default();
    for stmt in statements {
        inventory.visit_statement(stmt);
    }
    let mut nested_function_names = Vec::new();
    let mut var_names = Vec::new();
    for binding in inventory.bindings {
        match binding.kind {
            FunctionBindingKind::NestedFunction => nested_function_names.push(binding.name),
            FunctionBindingKind::Var => var_names.push(binding.name),
            _ => {}
        }
    }
    StatementListInventory {
        control: inventory.control,
        nested_function_names,
        var_names,
    }
}

/// The current-function inventory walker. Nested function / arrow / class
/// bodies are never entered (their contents belong to their own frames);
/// a nested function DECLARATION still binds its name in this frame. A
/// control region's `has_return` is computed by THIS SAME walk: a return
/// of the current function marks every enclosing region on the control
/// stack.
#[derive(Default)]
struct InventoryVisitor {
    bindings: Vec<FunctionBindingRecord>,
    references: Vec<FunctionReferenceRecord>,
    return_sites: Vec<FunctionReturnSite>,
    writes: Vec<FunctionWriteRecord>,
    effects: Vec<FunctionEffectRecord>,
    control: Vec<FunctionControlRegion>,
    /// Indices into `control` of the currently open regions (innermost
    /// last) — a `return` marks every one of them.
    control_stack: Vec<usize>,
    /// The spans of the currently open block-like scopes (innermost
    /// last). A block-scoped binding records the innermost one.
    scope_stack: Vec<verter_span::Span>,
    /// The whole frame's span — the scope of a parameter or a `var`, and
    /// the fallback when no block-like region is open.
    frame_span: verter_span::Span,
}

impl InventoryVisitor {
    /// The innermost open block-like scope, else the whole frame.
    fn block_scope(&self) -> verter_span::Span {
        self.scope_stack.last().copied().unwrap_or(self.frame_span)
    }
}

impl<'a> Visit<'a> for InventoryVisitor {
    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {
        // Nested function body: not this frame. (visit_function is only
        // reached for nested positions — the entry's own body is driven
        // statement-by-statement.)
    }

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &Class<'a>) {}

    fn visit_statement(&mut self, it: &Statement<'a>) {
        if let Statement::FunctionDeclaration(func) = it {
            if let Some(id) = func.id.as_ref() {
                let scope_span = self.block_scope();
                self.bindings.push(FunctionBindingRecord {
                    name: Arc::from(id.name.as_str()),
                    kind: FunctionBindingKind::NestedFunction,
                    span: id.span.into(),
                    scope_span,
                });
            }
            // Do not descend: the nested body is its own frame.
            return;
        }
        let kind = match it {
            Statement::BlockStatement(_) => Some(FunctionControlKind::Block),
            Statement::IfStatement(_) => Some(FunctionControlKind::If),
            Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::ForStatement(_)
            | Statement::WhileStatement(_) => Some(FunctionControlKind::Loop),
            Statement::SwitchStatement(_) => Some(FunctionControlKind::Switch),
            Statement::TryStatement(_) => Some(FunctionControlKind::Try),
            Statement::LabeledStatement(_) => Some(FunctionControlKind::Labeled),
            _ => None,
        };
        if let Some(kind) = kind {
            self.control.push(FunctionControlRegion {
                kind,
                has_return: false,
                span: it.span().into(),
            });
            self.control_stack.push(self.control.len() - 1);
            // Every one of these constructs also opens a lexical scope
            // for the `const` / `let` / nested function declarations it
            // contains.
            self.scope_stack.push(it.span().into());
        }
        walk::walk_statement(self, it);
        if kind.is_some() {
            self.control_stack.pop();
            self.scope_stack.pop();
        }
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        // `using` / `await using` are BLOCK-scoped resource declarations
        // (the `const` scoping rule plus disposal), never function-scoped
        // `var`s: classifying them as `var` makes them escape their block
        // through every hoisting rail.
        let kind = match it.kind {
            oxc_ast::ast::VariableDeclarationKind::Const
            | oxc_ast::ast::VariableDeclarationKind::Using
            | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => FunctionBindingKind::Const,
            oxc_ast::ast::VariableDeclarationKind::Let => FunctionBindingKind::Let,
            oxc_ast::ast::VariableDeclarationKind::Var => FunctionBindingKind::Var,
        };
        // `var` is function-scoped; `const` / `let` / `using` are scoped
        // to the innermost enclosing block-like region.
        let scope_span = if kind == FunctionBindingKind::Var {
            self.frame_span
        } else {
            self.block_scope()
        };
        for declarator in &it.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                self.bindings.push(FunctionBindingRecord {
                    name: Arc::from(id.name.as_str()),
                    kind,
                    span: id.span.into(),
                    scope_span,
                });
            }
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        // A `return` of the current function is contained by every
        // enclosing control region (drives return-transparency: a
        // return-free loop / labeled construct is fall-through
        // transparent; a return-bearing one is unsupported).
        for index in &self.control_stack {
            self.control[*index].has_return = true;
        }
        self.return_sites.push(FunctionReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            has_argument: it.argument.is_some(),
            span: it.span.into(),
        });
        walk::walk_return_statement(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.references.push(FunctionReferenceRecord {
            name: Arc::from(it.name.as_str()),
            span: it.span.into(),
        });
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: it.span.into(),
        });
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        self.writes.push(FunctionWriteRecord {
            span: it.span.into(),
        });
        walk::walk_update_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let callee = match &it.callee {
            Expression::Identifier(id) => {
                FunctionEffectCallee::Identifier(Arc::from(id.name.as_str()))
            }
            Expression::StaticMemberExpression(member) => {
                let mut path = Vec::new();
                if collect_static_member_path(member, &mut path) {
                    FunctionEffectCallee::StaticMember(Arc::from(path.into_boxed_slice()))
                } else {
                    FunctionEffectCallee::Other
                }
            }
            _ => FunctionEffectCallee::Other,
        };
        self.effects.push(FunctionEffectRecord {
            span: it.span.into(),
            callee,
        });
        walk::walk_call_expression(self, it);
    }
}

/// Collect a dotted member path from a static member expression chain.
/// `a.b.c` → `["a", "b", "c"]` (in order). Returns `false` for
/// non-identifier roots (`this`, calls, computed) — an unsupported callee
/// shape, not a path.
fn collect_static_member_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    path: &mut Vec<Arc<str>>,
) -> bool {
    let mut properties = Vec::new();
    let mut current = member;
    loop {
        properties.push(Arc::from(current.property.name.as_str()));
        match &current.object {
            Expression::Identifier(identifier) => {
                path.push(Arc::from(identifier.name.as_str()));
                break;
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => return false,
        }
    }
    properties.reverse();
    path.extend(properties);
    true
}

// ---------------------------------------------------------------------------
// Locator resolution against the retained snapshot
// ---------------------------------------------------------------------------

/// The function node a [`FunctionBodyLocator`] descent lands on — the ONE
/// authored-position view every per-function body product (skeleton
/// build, lazy body lowering) reads from the retained snapshot.
pub enum FunctionNode<'a> {
    /// A `function` declaration / expression or a class/object method.
    Function(&'a Function<'a>),
    /// An arrow function.
    Arrow(&'a ArrowFunctionExpression<'a>),
}

impl<'a> FunctionNode<'a> {
    /// The function's own source span.
    #[must_use]
    pub fn span(&self) -> oxc_span::Span {
        match self {
            Self::Function(func) => func.span,
            Self::Arrow(arrow) => arrow.span,
        }
    }

    /// The formal parameters.
    #[must_use]
    pub fn params(&self) -> &'a oxc_ast::ast::FormalParameters<'a> {
        match self {
            Self::Function(func) => &func.params,
            Self::Arrow(arrow) => &arrow.params,
        }
    }

    /// The function body (`None` for a bodiless overload signature).
    #[must_use]
    pub fn body(&self) -> Option<&'a oxc_ast::ast::FunctionBody<'a>> {
        match self {
            Self::Function(func) => func.body.as_deref(),
            Self::Arrow(arrow) => Some(&arrow.body),
        }
    }

    /// Whether this is an expression-bodied arrow (`(x) => x * 2`).
    #[must_use]
    pub fn is_expression_body(&self) -> bool {
        matches!(self, Self::Arrow(arrow) if arrow.expression)
    }

    /// The function's own type parameter clause, when authored.
    #[must_use]
    pub fn type_parameters(&self) -> Option<&oxc_ast::ast::TSTypeParameterDeclaration<'a>> {
        match self {
            Self::Function(func) => func.type_parameters.as_deref(),
            Self::Arrow(arrow) => arrow.type_parameters.as_deref(),
        }
    }

    /// The declared return-type annotation, when authored.
    #[must_use]
    pub fn return_type(&self) -> Option<&'a oxc_ast::ast::TSTypeAnnotation<'a>> {
        match self {
            Self::Function(func) => func.return_type.as_deref(),
            Self::Arrow(arrow) => arrow.return_type.as_deref(),
        }
    }
}

/// The declaration view of a statement, unwrapping the export wrappers the
/// locator descent does not record (the index discovers through
/// `export { … }` / `export default` transparently).
enum DeclRef<'a> {
    /// A function declaration.
    Function(&'a Function<'a>),
    /// A variable declaration.
    Variable(&'a VariableDeclaration<'a>),
    /// A class declaration.
    Class(&'a Class<'a>),
    /// A namespace (`TSModuleDeclaration`).
    Module(&'a oxc_ast::ast::TSModuleDeclaration<'a>),
    /// An `export default { … }` object expression.
    ExportDefaultObject(&'a oxc_ast::ast::ObjectExpression<'a>),
}

fn declaration_of<'a>(statement: &'a Statement<'a>) -> Option<DeclRef<'a>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};
    match statement {
        Statement::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
        Statement::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
        Statement::ClassDeclaration(class) => Some(DeclRef::Class(class)),
        Statement::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
            Declaration::FunctionDeclaration(func) => Some(DeclRef::Function(func)),
            Declaration::VariableDeclaration(decl) => Some(DeclRef::Variable(decl)),
            Declaration::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            Declaration::TSModuleDeclaration(module) => Some(DeclRef::Module(module)),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                Some(DeclRef::Function(func))
            }
            ExportDefaultDeclarationKind::ClassDeclaration(class) => Some(DeclRef::Class(class)),
            other => match other.as_expression() {
                Some(Expression::ObjectExpression(obj)) => Some(DeclRef::ExportDefaultObject(obj)),
                _ => None,
            },
        },
        _ => None,
    }
}

/// The function node behind an initializer expression: an arrow or a
/// function expression, nothing else.
pub fn function_from_expression<'a>(expression: &'a Expression<'a>) -> Option<FunctionNode<'a>> {
    match expression {
        Expression::FunctionExpression(func) => Some(FunctionNode::Function(func)),
        Expression::ArrowFunctionExpression(arrow) => Some(FunctionNode::Arrow(arrow)),
        _ => None,
    }
}

/// One resolved function position: the node, its bare-identifier self
/// name, and the type-parameter clause of the declaration ENCLOSING it.
pub struct ResolvedFunctionNode<'a> {
    /// The function node the locator addresses.
    pub node: FunctionNode<'a>,
    /// The function's bare-identifier SELF name, for direct-recursion
    /// detection. `None` for class members and object-literal members.
    pub self_name: Option<Arc<str>>,
    /// The type-parameter clause of the enclosing DECLARATION, when the
    /// function sits inside one that has binders of its own — today that
    /// is exactly a class member (`class C<T> { m(x: T) {} }`), whose
    /// binders are in scope throughout every member body. A namespace,
    /// a variable declarator, and an object literal declare no type
    /// parameters, so those descents carry `None`.
    pub enclosing_type_parameters: Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>>,
}

/// Resolve one function's locator against the retained snapshot: the
/// contributing top-level statement, then the ordinal descent. Also
/// derives the function's bare-identifier SELF name for direct-recursion
/// detection: a function declaration contributes its id, a variable
/// initializer its declarator binding; class members and object-literal
/// members have no bare-identifier self name. Any miss is a typed `None`.
pub fn resolve_function_node<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    locator: &FunctionBodyLocator,
) -> Option<ResolvedFunctionNode<'a>> {
    use oxc_ast::ast::{ClassElement, TSModuleDeclarationBody};
    let mut statement = program
        .body
        .get(locator.contributor.contributor_index as usize)?;
    // The body of the function resolved so far, when the descent has
    // stepped INSIDE it (a nested declaration, a callback argument, an
    // IIFE callee). `None` while the descent is still navigating
    // declarations from the contributor statement.
    let mut current_body: Option<&'a oxc_ast::ast::FunctionBody<'a>> = None;
    let mut steps = locator.descent.iter();
    loop {
        match steps.next()? {
            FunctionDescentStep::NamespaceMember { statement_ordinal } => {
                let DeclRef::Module(module) = declaration_of(statement)? else {
                    return None;
                };
                let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = module.body.as_ref()
                else {
                    return None;
                };
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            FunctionDescentStep::FunctionDeclaration => {
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                if steps.len() == 0 {
                    // Terminal step: the statement IS the function declaration.
                    let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                    return Some(ResolvedFunctionNode {
                        node: FunctionNode::Function(func),
                        self_name,
                        enclosing_type_parameters: None,
                    });
                }
                // Non-terminal: a nested position inside this function's body.
                current_body = func.body.as_deref();
            }
            FunctionDescentStep::VariableInitializer { declarator_ordinal } => {
                let DeclRef::Variable(var_decl) = declaration_of(statement)? else {
                    return None;
                };
                let declarator = var_decl.declarations.get(*declarator_ordinal as usize)?;
                let self_name = match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some(Arc::from(id.name.as_str())),
                    _ => None,
                };
                let init = declarator.init.as_ref()?;
                match steps.next() {
                    None => {
                        return Some(ResolvedFunctionNode {
                            node: function_from_expression(init)?,
                            self_name,
                            enclosing_type_parameters: None,
                        });
                    }
                    Some(FunctionDescentStep::ObjectMember { member_ordinal }) => {
                        let Expression::ObjectExpression(obj) = init else {
                            return None;
                        };
                        let prop = obj.properties.get(*member_ordinal as usize)?;
                        let ObjectPropertyKind::ObjectProperty(property) = prop else {
                            return None;
                        };
                        let node = function_from_expression(&property.value)?;
                        if steps.len() == 0 {
                            // Terminal step: the object-literal member inside
                            // the current initializer object expression.
                            // Object members have no bare-identifier self name.
                            return Some(ResolvedFunctionNode {
                                node,
                                self_name: None,
                                enclosing_type_parameters: None,
                            });
                        }
                        // Non-terminal: a nested position inside the member body.
                        current_body = node.body();
                    }
                    Some(_) => {
                        // Non-terminal: a nested position inside the
                        // initializer function's own body.
                        current_body = function_from_expression(init)?.body();
                    }
                }
            }
            FunctionDescentStep::ClassMember { member_ordinal } => {
                let DeclRef::Class(class) = declaration_of(statement)? else {
                    return None;
                };
                let element = class.body.body.get(*member_ordinal as usize)?;
                let node = match element {
                    ClassElement::MethodDefinition(method) => FunctionNode::Function(&method.value),
                    ClassElement::PropertyDefinition(property) => {
                        function_from_expression(property.value.as_ref()?)?
                    }
                    _ => return None,
                };
                if steps.len() == 0 {
                    // Terminal step: the class member at `member_ordinal`.
                    // Class members have no bare-identifier self name. The
                    // CLASS's own type-parameter clause binds throughout every
                    // member body, so it rides out with the node.
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name: None,
                        enclosing_type_parameters: class.type_parameters.as_deref(),
                    });
                }
                // Non-terminal: a nested position inside the member body.
                current_body = node.body();
            }
            FunctionDescentStep::ExportDefaultObjectMember { member_ordinal } => {
                let DeclRef::ExportDefaultObject(obj) = declaration_of(statement)? else {
                    return None;
                };
                let prop = obj.properties.get(*member_ordinal as usize)?;
                let ObjectPropertyKind::ObjectProperty(property) = prop else {
                    return None;
                };
                let node = function_from_expression(&property.value)?;
                if steps.len() == 0 {
                    // Terminal step: the object-literal method at
                    // `member_ordinal` inside the `export default { … }`
                    // object expression.
                    // Object members have no bare-identifier self name.
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name: None,
                        enclosing_type_parameters: None,
                    });
                }
                // Non-terminal: a nested position inside the member body.
                current_body = node.body();
            }
            FunctionDescentStep::ObjectMember { .. } => {
                // Only valid immediately after a VariableInitializer step
                // (handled there).
                return None;
            }
            FunctionDescentStep::BodyStatement { statement_ordinal } => {
                // The statement at `statement_ordinal` inside the enclosing
                // function's body — a hoisted nested function declaration.
                let body = current_body?;
                let Statement::FunctionDeclaration(func) =
                    body.statements.get(*statement_ordinal as usize)?
                else {
                    return None;
                };
                if steps.len() == 0 {
                    let self_name = func.id.as_ref().map(|id| Arc::from(id.name.as_str()));
                    return Some(ResolvedFunctionNode {
                        node: FunctionNode::Function(func),
                        self_name,
                        enclosing_type_parameters: None,
                    });
                }
                // Non-terminal: a nested position inside this declaration's body.
                current_body = func.body.as_deref();
            }
            FunctionDescentStep::CallArgument {
                call_ordinal,
                arg_ordinal,
            } => {
                // The argument at `arg_ordinal` of the enclosing body's
                // `call_ordinal`-th call site — a callback position.
                let body = current_body?;
                let call = nth_call_expression(&body.statements, *call_ordinal)?;
                let argument = call.arguments.get(*arg_ordinal as usize)?;
                let expression = argument.as_expression()?;
                let node = function_from_expression(unwrap_program_expression(expression))?;
                if steps.len() == 0 {
                    let self_name = match node {
                        FunctionNode::Function(func) => {
                            func.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                        }
                        FunctionNode::Arrow(_) => None,
                    };
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name,
                        enclosing_type_parameters: None,
                    });
                }
                // Non-terminal: a nested position inside the callback's body.
                current_body = node.body();
            }
            FunctionDescentStep::CallCallee { call_ordinal } => {
                // The CALLEE of the enclosing body's `call_ordinal`-th call
                // site — an immediately-invoked function expression.
                let body = current_body?;
                let call = nth_call_expression(&body.statements, *call_ordinal)?;
                let node = function_from_expression(unwrap_program_expression(&call.callee))?;
                if steps.len() == 0 {
                    let self_name = match node {
                        FunctionNode::Function(func) => {
                            func.id.as_ref().map(|id| Arc::from(id.name.as_str()))
                        }
                        FunctionNode::Arrow(_) => None,
                    };
                    return Some(ResolvedFunctionNode {
                        node,
                        self_name,
                        enclosing_type_parameters: None,
                    });
                }
                // Non-terminal: a nested position inside the callee's body.
                current_body = node.body();
            }
        }
    }
}

/// The `ordinal`-th call expression inside one expression, by the ONE
/// shared source-order walk (the expression-position mirror of
/// [`nth_call_expression`]).
fn nth_call_expression_in_expression<'a>(
    expression: &'a Expression<'a>,
    ordinal: usize,
) -> Option<&'a CallExpression<'a>> {
    let mut remaining = ordinal;
    let mut found = None;
    for_each_call_expression_in_expression(expression, |call| {
        if found.is_none() {
            if remaining == 0 {
                found = Some(call);
            } else {
                remaining -= 1;
            }
        }
    });
    found
}

/// Transient typed IR for one indexed declaration/callback expression:
/// the retained snapshot is re-read on demand and lowered through the ONE
/// indexed-expression lowering; no body `TypeExpr` is memo-owned.
///
/// A semantic-call record's per-argument served-return identities are
/// patched back onto the lowered arguments (and onto a function value's
/// `flow_return` slot) so the call executor can demand the exact served
/// position instead of re-deriving it.
pub fn build_indexed_program_expression_ir(
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
    record: &ProgramExpressionRecord,
) -> Option<verter_type_expr::IndexedValueExpression> {
    let mut statement = program
        .body
        .get(record.locator.contributor.contributor_index as usize)?;
    let mut current_body: Option<&oxc_ast::ast::FunctionBody<'_>> = None;
    let mut steps = record.locator.descent.iter().peekable();
    // Prefix steps navigate to the declaration OWNING the expression: a
    // namespace block or the enclosing function's body statement list.
    loop {
        match steps.peek() {
            Some(FunctionDescentStep::NamespaceMember { statement_ordinal }) => {
                steps.next();
                let DeclRef::Module(module) = declaration_of(statement)? else {
                    return None;
                };
                let Some(oxc_ast::ast::TSModuleDeclarationBody::TSModuleBlock(block)) =
                    module.body.as_ref()
                else {
                    return None;
                };
                statement = block.body.get(*statement_ordinal as usize)?;
            }
            Some(FunctionDescentStep::BodyStatement { statement_ordinal }) => {
                steps.next();
                let body = current_body?;
                statement = body.statements.get(*statement_ordinal as usize)?;
                let DeclRef::Function(func) = declaration_of(statement)? else {
                    return None;
                };
                current_body = func.body.as_deref();
            }
            _ => break,
        }
    }
    let expression = match steps.next()? {
        FunctionDescentStep::VariableInitializer { declarator_ordinal } => {
            let DeclRef::Variable(declaration) = declaration_of(statement)? else {
                return None;
            };
            let initializer = declaration
                .declarations
                .get(*declarator_ordinal as usize)?
                .init
                .as_ref()?;
            match steps.next() {
                None => initializer,
                Some(FunctionDescentStep::CallArgument {
                    call_ordinal,
                    arg_ordinal,
                }) if steps.len() == 0 => {
                    let call =
                        nth_call_expression_in_expression(initializer, *call_ordinal as usize)?;
                    call.arguments.get(*arg_ordinal as usize)?.as_expression()?
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let mut indexed =
        crate::analysis::type_eval_build::lower_indexed_value_expression(expression, source);
    if let (
        ProgramExpressionSource::SemanticCall { site, .. },
        verter_type_expr::IndexedValueExpression::Call(call),
    ) = (&record.source, &mut indexed)
    {
        for (argument, indexed_argument) in site.args.iter().zip(Arc::make_mut(&mut call.args)) {
            indexed_argument.function_return_source = argument.function_return_source.clone();
            if let (
                Some(verter_type_expr::facts::FunctionReturnSource::Flow(identity)),
                verter_type_expr::IndexedValueExpression::Value(
                    verter_type_expr::TypeExpr::Function(function),
                ),
            ) = (
                indexed_argument.function_return_source.as_ref(),
                &mut indexed_argument.expression,
            ) {
                Arc::make_mut(function).flow_return = Some(Box::new(identity.clone()));
            }
        }
    }
    Some(indexed)
}

/// The one authored call expression at `span`, addressed the way the
/// index addresses every call: through the served function entries and
/// the ONE shared source-order walk. A flow-selected call is inside a
/// served function by construction; a span nothing indexed is a typed
/// `None`, never a fabricated record.
pub fn call_expression_at<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    index: &FunctionProgramIndex,
    span: verter_span::Span,
) -> Option<&'a CallExpression<'a>> {
    for entry in index.entries.iter() {
        let Some(resolved) = resolve_function_node(program, &entry.locator) else {
            continue;
        };
        let Some(body) = resolved.node.body() else {
            continue;
        };
        let mut found = None;
        for_each_call_expression(&body.statements, |call| {
            if found.is_none() && verter_span::Span::from(call.span) == span {
                found = Some(call);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The `ordinal`-th call expression inside one function body's statement
/// list, by the ONE shared source-order walk discovery used to assign
/// `call_ordinal` (so a locator derefs to exactly the call it indexed).
fn nth_call_expression<'a>(
    statements: &'a [Statement<'a>],
    ordinal: u32,
) -> Option<&'a CallExpression<'a>> {
    let mut seen = 0u32;
    let mut found = None;
    for_each_call_expression(statements, |call| {
        if found.is_none() {
            if seen == ordinal {
                found = Some(call);
            }
            seen += 1;
        }
    });
    found
}
