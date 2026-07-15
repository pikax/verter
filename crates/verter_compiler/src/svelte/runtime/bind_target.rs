//! Structural classification of a two-way `bind:` directive's bound target
//! expression from the parsed OXC node (NOT a text scan).

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// The STRUCTURAL classification of a two-way `bind:` directive's bound target
/// expression, derived from the parsed OXC node — NOT a text scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindTargetKind {
    /// A bare identifier target (`bind:value={name}`) — a REASSIGNMENT of the
    /// bound binding.
    Identifier,
    /// A member target (`bind:value={o.x}` / `bind:value={a[i]}`) — a DEEP MUTATION
    /// of the bound binding's value.
    Member,
    /// A FUNCTION-PAIR target (`bind:value={get, set}`) — a two-element sequence
    /// expression supplying an explicit getter + setter. Official passes the two
    /// supplied expressions DIRECTLY to the `$.bind_*` helper (rewriting any signal
    /// read/write inside them), rather than synthesizing lvalue thunks from a
    /// reassignable target. The lvalue-root rules (`$state` vs plain vs prop) do NOT
    /// apply — the user owns the get/set, so the only structural requirement is the
    /// exactly-two-element sequence shape.
    FunctionPair,
}

/// The ONE owned, structurally-derived fact about a `bind:` target expression — the
/// SINGLE shared authority every bind consumer reads (classification, name collection,
/// setter planning, write attribution, and the official-reject gate's policy scan)
/// instead of RE-PARSING the same expression per consumer.
///
/// Computed ONCE in the single expression-analysis parse (carried on
/// [`AnalyzedExpr`](super::expr::AnalyzedExpr)) or on demand via [`BindTargetFact::from_source`]
/// (the official-reject gate, which runs before the analysis arena exists). The
/// structural fields are derived from the already-parsed OXC expression (no extra parse);
/// the [`function_pair`](Self::function_pair) plain-Svelte-JS slices come from ONE optional
/// `SourceType::mjs()` parse gated on [`is_sequence`](Self::is_sequence) — at most one stored
/// pair-parse per expression, never one per consumer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BindTargetFact {
    /// The structural lvalue kind (`Identifier` / `Member` / `FunctionPair`), or `None`
    /// for a NON-lvalue target (a literal, a call, a binary, or a non-two-element
    /// sequence).
    pub kind: Option<BindTargetKind>,
    /// Whether the transparent-paren-unwrapped target is a `SequenceExpression` of ANY
    /// arity. Distinct from `kind == Some(FunctionPair)` (which requires exactly two
    /// elements): a 3-element sequence is `is_sequence = true` but `kind = None`. This is
    /// the `bind:group` identifier/member-only POLICY violation signal — official rejects
    /// ANY sequence target on `bind:group`, not just the two-element shape.
    pub is_sequence: bool,
    /// Whether the target's would-be lvalue spine carries — ANYWHERE (the spine TOP, a
    /// member-OBJECT chain link, a computed-INDEX expression, OR any deeper sub-expression,
    /// possibly under parentheses) — a node that is NOT plain-Svelte-JS-faithful: a TS-ONLY
    /// operator (`name!` / `name as T` / `name satisfies T` / `<T>name` / `f<T>`), a call / new /
    /// tagged-template carrying TS type arguments (`g<a,b>(c)`), OR a TS-only node in a deeper
    /// sub-expression (a typed arrow / function param, a typed local). Detected by the wholesale
    /// shared `expression_contains_non_plain_svelte_js` scan (one authority for both bind lanes).
    /// A deferred official surface that fails closed (5c): official svelte@5.56.3
    /// PARSE-REJECTS plain-script TS in ANY template-expression position. The structural walk
    /// covers the member object spine + computed-index expressions, so a NON-ROOT TS node
    /// (`o!.x`, `a[x as T]`, `a[x!]`) is caught EXACTLY like a root wrapper (`name!` /
    /// `name as T`). A SEQUENCE (function-pair) target is NOT an lvalue spine — its TS
    /// rejection is owned by the plain-JS function-pair lane (`mjs` parse + strict delta
    /// scan), so this fact is `false` for it. The EXACT diagnostic-code parity
    /// (`expected_token`/`js_parse_error`) stays D-26 (the shared template-expression parse
    /// authority); this fact only drives the structural fail-closed.
    pub lvalue_contains_ts: bool,
    /// The ROOT identifier name of an identifier/member target (`v` for `v`, `o` for
    /// `o.x` / `a[i]`), or `None` when the chain bottoms out at a non-identifier (a call
    /// `f().x`, a literal, a sequence, a `this`-member).
    pub root_ident: Option<String>,
    /// The two element source slices of a CLEAN two-element plain-Svelte-JS function pair
    /// (`bind:value={get, set}`), or `None` when the target is not a valid plain-JS pair (a
    /// non-sequence, a wrong element count, or a TS-only construct in either element). The
    /// default-closed plain-Svelte-JS authority — see [`parse_plain_svelte_function_pair`].
    pub function_pair: Option<(String, String)>,
    /// The STRUCTURAL identifier keypath of the target, mirroring official svelte's
    /// `extract_all_identifiers_from_expression` (`utils/ast.js`): a pre-order walk that
    /// collects every IDENTIFIER name and LITERAL value, serializing a computed member's
    /// DIRECT identifier/literal index as `[name]` / `[value]` and everything else as plain
    /// `.`-joined segments — so `v` → `"v"`, `o.x` → `"o.x"`, `a[i]` → `"a.[i]"`, `a["x"]` →
    /// `"a.[\"x\"]"`, and `g[i+j]` / `g[i*j]` / `g[i + j]` ALL → `"g.i.j"` (the keypath is
    /// WHITESPACE- and OPERATOR-INSENSITIVE — operators are not identifiers, so they never
    /// enter the key). `None` only for a target that collects NO identifier/literal/`this`
    /// segment. This is the `bind:group` accumulator GROUPING identity: two `bind:group`
    /// inputs binding the SAME structural target (same keypath + same scope) share one
    /// `binding_group` accumulator; distinct targets get distinct accumulators — matching
    /// official's `[keypath, bindings]` group identity. Derived from the parsed OXC
    /// expression (NEVER a raw-source compare), so `o.x` and `o . x` canonicalize equal.
    pub target_keypath: Option<String>,
    /// Whether the AUTHORED bind value wraps a `SequenceExpression` in PARENTHESES
    /// (`bind:value={(get, set)}` / `{((a, b))}`) — distinct from a bare sequence
    /// (`{get, set}`, which is `is_sequence = true` but NOT parenthesized). Official
    /// svelte@5.56.3 REJECTS author parens around a bind sequence with `bind_invalid_parens`
    /// (a `(` between the `{` and the sequence start), while a parenthesized NON-sequence
    /// (`{(v)}`) stays accepted. Detected structurally: the fact is built from the
    /// `({source})`-wrapped parse, so the OUTER `ParenthesizedExpression` is Verter's
    /// synthetic wrapper; an AUTHOR paren means an EXTRA `ParenthesizedExpression` survives
    /// after peeling the one synthetic wrapper, with a `SequenceExpression` inside it.
    pub is_parenthesized_sequence: bool,
    /// Whether the target is a STRUCTURALLY-INVALID bind expression — a successfully-parsed
    /// target that is NEITHER a valid lvalue (`Identifier` / `Member`) NOR a valid two-element
    /// function-pair, AND carries no TS-only operator (a `CallExpression`, a literal, a
    /// 3+-element `SequenceExpression`, a binary / optional-chain / other non-assignable
    /// shape). This is the official `bind_invalid_expression` shape — bind-target SHAPE
    /// validation, NOT TS-grammar parity. A TS-wrapped lvalue (`name!` / `o!.x`) is EXCLUDED
    /// (that is the parse-error / D-26 class, flagged by
    /// [`lvalue_contains_ts`](Self::lvalue_contains_ts)); a parenthesized sequence is EXCLUDED
    /// (that is `bind_invalid_parens`, flagged by
    /// [`is_parenthesized_sequence`](Self::is_parenthesized_sequence)). A torn parse yields
    /// the default `false`, so the official-reject gate never flags an unparseable target.
    pub is_invalid_bind_expression: bool,
}

impl BindTargetFact {
    /// Build the fact by PARSING `source` as a wrapped TSX expression — the entry for the
    /// official-reject gate (which runs BEFORE the analysis arena exists, so it has no
    /// pre-parsed expression). A torn parse yields the empty default fact.
    #[must_use]
    pub fn from_source(alloc: &Allocator, source: &str) -> Self {
        let wrapped = format!("({source})");
        let parsed = Parser::new(alloc, alloc.alloc_str(&wrapped), SourceType::tsx()).parse();
        if parsed.panicked || !parsed.errors.is_empty() {
            return Self::default();
        }
        let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
            return Self::default();
        };
        Self::from_parsed_target(&stmt.expression, alloc, source)
    }

    /// Build the fact from an ALREADY-PARSED target expression (the wrapped
    /// `({source})` expression statement's body) plus the raw `source` — the entry for the
    /// single expression-analysis parse, so the structural fields reuse the existing parse
    /// and never re-parse. The optional plain-JS function-pair parse runs at most once here
    /// (gated on `is_sequence`), on the passed allocator.
    pub(crate) fn from_parsed_target(expr: &Expression, alloc: &Allocator, source: &str) -> Self {
        let is_sequence = target_expr_is_sequence(expr);
        // The plain-Svelte-JS function-pair slices are only meaningful for a sequence
        // target; gate the one optional `mjs` parse on that so a non-sequence expression
        // does NO extra parse.
        let function_pair = if is_sequence {
            parse_plain_svelte_function_pair(alloc, source)
        } else {
            None
        };
        let kind = classify_target_expr(expr);
        let lvalue_contains_ts = target_expr_lvalue_contains_ts(expr);
        let is_parenthesized_sequence = target_expr_is_parenthesized_sequence(expr);
        Self {
            kind,
            is_sequence,
            lvalue_contains_ts,
            root_ident: target_expr_root_ident(expr),
            function_pair,
            target_keypath: target_expr_keypath(expr),
            is_parenthesized_sequence,
            // The `bind_invalid_expression` shape: a parsed non-lvalue / non-pair target that
            // is NOT a TS class (that is `lvalue_contains_ts`, the D-26/parse-error class) and
            // NOT a parenthesized sequence (that is `bind_invalid_parens`). A 2-element pair is
            // `kind == Some(FunctionPair)`, so `kind.is_none()` already excludes it.
            is_invalid_bind_expression: kind.is_none()
                && !lvalue_contains_ts
                && !is_parenthesized_sequence,
        }
    }
}

/// Whether the AUTHORED bind value is a `SequenceExpression` wrapped in author PARENTHESES
/// (`bind:value={(get, set)}`), the official `bind_invalid_parens` shape — as opposed to a
/// bare sequence (`{get, set}`) or a parenthesized non-sequence (`{(v)}`, accepted).
///
/// `expr` is the `({source})`-wrapped parse, so its OUTER `ParenthesizedExpression` is
/// Verter's SYNTHETIC wrapper. Peel exactly that one synthetic wrapper: if an EXTRA
/// `ParenthesizedExpression` survives (an author paren) whose fully-unwrapped content is a
/// `SequenceExpression`, the author parenthesized a sequence. A bare sequence leaves the
/// `SequenceExpression` DIRECTLY under the synthetic wrapper (no author paren); a
/// parenthesized non-sequence leaves a non-sequence under the author paren.
fn target_expr_is_parenthesized_sequence(expr: &Expression) -> bool {
    let Expression::ParenthesizedExpression(synthetic) = expr else {
        return false;
    };
    let Expression::ParenthesizedExpression(author) = &synthetic.expression else {
        return false;
    };
    let mut inner = &author.expression;
    while let Expression::ParenthesizedExpression(p) = inner {
        inner = &p.expression;
    }
    matches!(inner, Expression::SequenceExpression(_))
}

/// Classify a `bind:` target's lvalue shape from an ALREADY-PARSED OXC expression (the
/// wrapped `({source})` body). Returns `Identifier` for a bare-identifier target, `Member`
/// for a member/computed-member target, `FunctionPair` for a two-element sequence, and
/// `None` for any other shape (a non-lvalue, or a non-two-element sequence). The structural
/// core used by [`BindTargetFact`].
fn classify_target_expr(expr: &Expression) -> Option<BindTargetKind> {
    let expr = peel_runtime_lvalue_expression(expr);
    match expr {
        Expression::Identifier(_) => Some(BindTargetKind::Identifier),
        Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => Some(BindTargetKind::Member),
        // A two-element sequence (`get, set`) is the function-pair target. Official
        // accepts EXACTLY two elements; a 1- or 3+-element sequence is not a valid pair.
        Expression::SequenceExpression(seq) if seq.expressions.len() == 2 => {
            Some(BindTargetKind::FunctionPair)
        }
        _ => None,
    }
}

/// Whether the transparent-paren-unwrapped target is a `SequenceExpression` of ANY arity
/// — the `bind:group` identifier/member-only policy violation signal (peels every outer
/// `ParenthesizedExpression`, matching the analysis arena's `unwrapped_is_sequence`).
fn target_expr_is_sequence(expr: &Expression) -> bool {
    let expr = peel_runtime_lvalue_expression(expr);
    matches!(expr, Expression::SequenceExpression(_))
}

/// Peel the syntax-only wrapper spine erased from a TypeScript bind target before
/// runtime lvalue classification. The original tree remains available to
/// [`target_expr_lvalue_contains_ts`] so plain-script TypeScript still fails closed.
fn peel_runtime_lvalue_expression<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(node) => &node.expression,
            Expression::TSAsExpression(node) => &node.expression,
            Expression::TSSatisfiesExpression(node) => &node.expression,
            Expression::TSNonNullExpression(node) => &node.expression,
            Expression::TSTypeAssertion(node) => &node.expression,
            Expression::TSInstantiationExpression(node) => &node.expression,
            _ => return expr,
        };
    }
}

/// Whether a parsed `bind:` target's would-be lvalue spine carries — ANYWHERE (the spine TOP,
/// a member-OBJECT chain link, a computed-INDEX expression, OR any deeper sub-expression of it,
/// possibly under parentheses) — a node that is NOT plain-Svelte-JS-faithful: a TS-only operator
/// (`name!` / `name as T` / `name satisfies T` / `<T>name` / `f<T>`), a call / new /
/// tagged-template carrying TS type arguments (`g<a,b>(c)` / `new C<T>()`), OR a TS-only node in
/// a deeper sub-expression (a typed arrow / function param `((x: number) => x)`, a typed local
/// `const k: number`, a type-parameter list). The structural core behind
/// [`BindTargetFact::lvalue_contains_ts`].
///
/// Peels the synthetic `({source})` wrapper + any author parens to the lvalue core; a
/// SEQUENCE (function-pair) core is NOT an lvalue spine (its rejection is owned by the plain-JS
/// function-pair lane), so it is `false`. Otherwise the SHARED
/// [`expression_contains_non_plain_svelte_js`] scan walks the core and flags ANY non-plain-JS
/// node — wholesale, so the class is closed by construction (NOT a per-form enumeration, NOT a
/// source-text scan): a member-object non-null (`o!.x`), a computed-index cast (`a[x as T]`), a
/// type-argument call (`a[g<a,b>(c)]`), and a typed sub-expression (`a[((x: number) => x)(0)]`)
/// are all caught. The TSX-strip lane would otherwise silently delete the TS and emit a
/// divergent bind, whereas official svelte parses the source as plain JS. A plain valid-JS
/// lvalue (`o.x`, `a[i]`, `arr[f(c)]`, an untyped IIFE index) carries no such node and stays
/// accepted.
fn target_expr_lvalue_contains_ts(expr: &Expression) -> bool {
    let mut core = expr;
    while let Expression::ParenthesizedExpression(p) = core {
        core = &p.expression;
    }
    if matches!(core, Expression::SequenceExpression(_)) {
        return false;
    }
    expression_contains_non_plain_svelte_js(core)
}

/// Whether `expr`'s subtree carries ANY node that is NOT plain-Svelte-JS-faithful — a TS-only
/// node (a type annotation / type argument, `as` / `satisfies` / `!` / type-assertion /
/// instantiation, a type-parameter list) OR a non-ECMAScript official-delta node (decorator,
/// auto-`accessor`, a TS-only class / member / param modifier). The SINGLE
/// plain-Svelte-JS-faithfulness authority shared by BOTH bind-target lanes: the single-lvalue
/// spine ([`target_expr_lvalue_contains_ts`]) and the function-pair element scan
/// ([`parse_plain_svelte_function_pair`]). A WHOLESALE, default-closed refusal — any such node
/// fails closed, so the class is anti-regrowth-complete by construction (a future OXC TS node is
/// caught by the [`StrictOfficialDeltaScan`] `visit_ts_type` / type-parameter arms WITHOUT a new
/// per-form override). Typed-IR / OXC-node only — NEVER a `source.contains` text scan. To be
/// subsumed by the shared plain-MJS template-expression authority (D-26).
fn expression_contains_non_plain_svelte_js(expr: &Expression) -> bool {
    use oxc_ast_visit::Visit;
    let mut scan = StrictOfficialDeltaScan { found: false };
    scan.visit_expression(expr);
    scan.found
}

/// The ROOT identifier name of a parsed target expression — the leftmost
/// identifier reached by walking down a member / computed-member chain — or `None` when the
/// chain bottoms out at a non-identifier. The structural core shared by [`BindTargetFact`]
/// and [`bind_target_root_ident`], and reused by the event-handler state-write gate for a
/// proxy DEEP MUTATION root (`o.a` → `o`).
pub(super) fn target_expr_root_ident(expr: &Expression) -> Option<String> {
    let mut node = expr;
    loop {
        match node {
            Expression::ParenthesizedExpression(p) => node = &p.expression,
            Expression::TSAsExpression(e) => node = &e.expression,
            Expression::TSSatisfiesExpression(e) => node = &e.expression,
            Expression::TSNonNullExpression(e) => node = &e.expression,
            Expression::TSTypeAssertion(e) => node = &e.expression,
            Expression::TSInstantiationExpression(e) => node = &e.expression,
            Expression::StaticMemberExpression(m) => node = &m.object,
            Expression::ComputedMemberExpression(m) => node = &m.object,
            Expression::PrivateFieldExpression(m) => node = &m.object,
            Expression::Identifier(id) => return Some(id.name.to_string()),
            _ => return None,
        }
    }
}

/// The STRUCTURAL identifier keypath of a `bind:` target, mirroring official svelte's
/// `extract_all_identifiers_from_expression` (`phases/2-analyze/.../utils/ast.js`) — the
/// `bind:group` accumulator GROUPING identity. A pre-order walk collects every IDENTIFIER
/// name and LITERAL value (plus `this`), serializing the DIRECT identifier/literal index of
/// a computed member as `[name]` / `[value]` and every other segment as a plain `.`-joined
/// name. Operators and other non-identifier nodes contribute NOTHING, so the key is
/// WHITESPACE- and OPERATOR-INSENSITIVE: `g[i+j]`, `g[i + j]`, `g[i*j]` all collapse to
/// `"g.i.j"` (matching svelte@5.56.3's single shared accumulator), while `a.x` (`"a.x"`)
/// stays DISTINCT from `a["x"]` (`"a.[\"x\"]"`). Returns `None` only for a target that
/// yields NO segment at all. Derived from the parsed OXC expression (the SAME node the rest
/// of the fact reads), never a raw-source slice — so `o.x` and `o . x` canonicalize equal.
fn target_expr_keypath(expr: &Expression) -> Option<String> {
    use oxc_ast_visit::Visit;
    let mut collector = KeypathSegments {
        segments: Vec::new(),
    };
    collector.visit_expression(expr);
    if collector.segments.is_empty() {
        None
    } else {
        Some(collector.segments.join("."))
    }
}

/// The pre-order identifier/literal-segment collector that replicates official svelte's
/// `extract_all_identifiers_from_expression` keypath. It reuses the default OXC walk for
/// every non-overridden node (so a computed index of ANY shape — `i+j`, `f(x)`, `b.c` —
/// surfaces its inner identifiers as plain VALUE-position names, exactly like svelte's
/// generic `walk`), and overrides ONLY the member nodes to serialize a DIRECT
/// identifier/literal index as the bracketed `[name]` / `[value]` segment. Parentheses are
/// transparent (svelte's Acorn tree has none), so an authored `a[(i)]` keys the same as
/// `a[i]`.
struct KeypathSegments {
    segments: Vec<String>,
}

impl KeypathSegments {
    /// The `String(value)` segment text for a literal in svelte's collector (a string is
    /// quoted; every other literal is its stringified value).
    fn literal_segment(expr: &Expression) -> Option<String> {
        match expr {
            Expression::StringLiteral(s) => Some(format!("\"{}\"", s.value.as_str())),
            Expression::NumericLiteral(n) => Some(n.value.to_string()),
            Expression::BigIntLiteral(n) => Some(n.value.as_str().to_string()),
            Expression::BooleanLiteral(b) => {
                Some(if b.value { "true" } else { "false" }.to_string())
            }
            Expression::NullLiteral(_) => Some("null".to_string()),
            _ => None,
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for KeypathSegments {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        // A VALUE-position identifier (object root, computed-index operand, callee, …).
        self.segments.push(it.name.to_string());
    }

    fn visit_this_expression(&mut self, _it: &oxc_ast::ast::ThisExpression) {
        self.segments.push("this".to_string());
    }

    fn visit_string_literal(&mut self, it: &oxc_ast::ast::StringLiteral<'a>) {
        // A VALUE-position string literal serializes quoted (`"x"`).
        self.segments.push(format!("\"{}\"", it.value.as_str()));
    }

    fn visit_numeric_literal(&mut self, it: &oxc_ast::ast::NumericLiteral<'a>) {
        self.segments.push(it.value.to_string());
    }

    fn visit_big_int_literal(&mut self, it: &oxc_ast::ast::BigIntLiteral<'a>) {
        self.segments.push(it.value.as_str().to_string());
    }

    fn visit_boolean_literal(&mut self, it: &oxc_ast::ast::BooleanLiteral) {
        self.segments
            .push(if it.value { "true" } else { "false" }.to_string());
    }

    fn visit_static_member_expression(&mut self, it: &oxc_ast::ast::StaticMemberExpression<'a>) {
        // Object in VALUE position, then the static property NAME (never bracketed).
        self.visit_expression(&it.object);
        self.segments.push(it.property.name.to_string());
    }

    fn visit_computed_member_expression(
        &mut self,
        it: &oxc_ast::ast::ComputedMemberExpression<'a>,
    ) {
        // Object in VALUE position.
        self.visit_expression(&it.object);
        // Parens are transparent (Acorn has none): peel them to test for a DIRECT
        // identifier/literal index, which serializes as the bracketed `[name]` / `[value]`
        // segment. Any OTHER index (`i+j`, `f()`, `b.c`) is walked in VALUE position so its
        // inner identifiers surface as plain names — exactly svelte's behavior.
        let mut index = &it.expression;
        while let Expression::ParenthesizedExpression(p) = index {
            index = &p.expression;
        }
        if let Expression::Identifier(id) = index {
            self.segments.push(format!("[{}]", id.name.as_str()));
        } else if let Some(seg) = Self::literal_segment(index) {
            self.segments.push(format!("[{seg}]"));
        } else {
            self.visit_expression(&it.expression);
        }
    }
}

/// Parse + validate a FUNCTION-PAIR bind target (`bind:value={get, set}`) as PLAIN
/// Svelte JS and return the two element source slices, or `None` to REFUSE.
///
/// This is the DEFAULT-CLOSED authority for the function-pair surface — it both DECIDES
/// acceptance and EXTRACTS the two element sources from ONE parse (a default-closed lane,
/// NOT a default-open enumerated allow-by-omission TS scan, and NOT a separate TSX slice).
/// For the SUPPORTED surface (a plain `.svelte`, NOT `lang="ts"`), official svelte@5.56.3
/// parses binding expressions with Acorn (`sourceType: "module"`), where ANY
/// TypeScript-only construct is a PARSE ERROR. The OXC equivalent is `SourceType::mjs()`
/// — NOT `tsx()` (TS-lenient) and NOT `jsx()`. So the acceptance decision is, in order:
///
/// 1. parse the wrapped pair as `SourceType::mjs()`; a parse error / panic REFUSES
///    (`as`/`satisfies`, expression-position `!`, type annotations, generic arrows,
///    `interface`/`enum`, `implements`, `abstract class`, etc. are all parse errors by
///    construction here — `mjs` rejects the bulk of TS without enumeration);
/// 2. validate the EXACT two-element sequence shape (a 1- or 3+-element sequence is not
///    a valid `{get, set}` pair → REFUSE);
/// 3. run the strict OFFICIAL-DELTA scan ([`StrictOfficialDeltaScan`]) over BOTH
///    elements — `mjs` is Acorn-equivalent for the bulk but TOLERATES an
///    OXC-over-Acorn residual (TS-only class/member fields, decorators,
///    `implements`/type-params on a recovered AST, auto-`accessor`) that official
///    REJECTS; any hit REFUSES.
///
/// When all three pass, the two element source slices are returned (sliced from the
/// original `source`, subtracting the one synthetic `(` byte). Each element is rewritten
/// INDEPENDENTLY through the PLAIN-JS rewrite lane (so a signal read/write inside an
/// inline arrow lowers, while a bare function identifier passes through), then passed
/// DIRECTLY to the helper. The decision is STRUCTURAL over the parsed AST — never a
/// `source.contains("as")` / `":"` text scan.
///
/// PRIVATE: the SOLE caller is [`BindTargetFact::from_parsed_target`], which computes the
/// pair ONCE per expression and stores the slices on the fact. Bind consumers read the
/// fact's [`function_pair`](BindTargetFact::function_pair) field — they never re-invoke this
/// (the compile-time fence against reintroduced per-consumer reparses).
#[must_use]
fn parse_plain_svelte_function_pair(alloc: &Allocator, source: &str) -> Option<(String, String)> {
    let wrapped = format!("({source})");
    // (1) Parse as PLAIN Svelte JS. A parse error / panic fails closed.
    let parsed = Parser::new(alloc, alloc.alloc_str(&wrapped), SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return None;
    };
    // Walk to the sequence through the synthetic wrap paren.
    let mut expr = &stmt.expression;
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    // (2) Validate the EXACT two-element sequence shape.
    let Expression::SequenceExpression(seq) = expr else {
        return None;
    };
    let [getter, setter] = seq.expressions.as_slice() else {
        return None;
    };
    // (3) The shared plain-Svelte-JS-faithfulness scan over BOTH elements — the SAME authority
    // the single-lvalue spine uses (`expression_contains_non_plain_svelte_js`, backed by
    // `StrictOfficialDeltaScan`), so both bind lanes share ONE TS / official-delta detector.
    if expression_contains_non_plain_svelte_js(getter)
        || expression_contains_non_plain_svelte_js(setter)
    {
        return None;
    }
    // (4) Slice the two element sources back out of `source` (the wrapped parse prefixes
    // one `(`; subtract it to index back into the original `source`).
    use oxc_span::GetSpan;
    let slice = |span: oxc_span::Span| -> Option<String> {
        let start = (span.start as usize).checked_sub(1)?;
        let end = (span.end as usize).checked_sub(1)?;
        source.get(start..end).map(str::to_string)
    };
    Some((slice(getter.span())?, slice(setter.span())?))
}

/// The STRICT OFFICIAL-DELTA scan over a function-pair element parsed as plain Svelte JS
/// (`SourceType::mjs()`). It flags the bounded set of constructs OXC's plain-JS parser
/// TOLERATES (on a clean or recovered AST) but official svelte@5.56.3's Acorn parser
/// REJECTS — the residual that `mjs` does NOT already reject at parse time. This is NOT
/// an open-ended "all TS" enumeration: `mjs` already rejects type annotations,
/// `as`/`satisfies`, expression-position `!`, generic arrows, typed params,
/// `interface`/`type`/`enum`, etc. by construction (a parse error caught upstream). The
/// residual caught here is TS-only class/member FIELDS, decorators, `implements` /
/// type-parameters on a recovered class, and auto-`accessor` members.
///
/// # Durable anti-regrowth (hermetic, compile-enforced) — and its precise scope
///
/// Each WATCHED OXC struct — `Class`, `PropertyDefinition`, `MethodDefinition`,
/// `FormalParameter` — is destructured WILDCARD-FREE: every field named explicitly, NO
/// `..` rest. When a future `oxc_ast` version ADDS a FIELD to one of THOSE structs, this
/// code FAILS TO COMPILE until the author classifies the new field (TS-only ⇒ flag it,
/// plain-JS ⇒ name it `_`). So an OXC upgrade cannot SILENTLY reopen the gate by adding a
/// new TS-only FIELD to a watched struct that nothing reads. This is compile-time +
/// cross-platform + hermetic (cf. the workspace `*_is_exhaustive_and_wildcard_free`
/// idiom), and deliberately NOT a runtime scan of the cargo-registry `oxc_ast` source for
/// new `#[ts]` attributes — that would be non-hermetic and path-varying per machine/CI (a
/// Cross-Platform Portability violation).
///
/// What this does NOT statically fence is a new TS-only NODE KIND (as opposed to a new
/// field on a watched struct). The common case is still covered at RUNTIME: most TS syntax
/// is a plain-JS parse error caught by the upstream `mjs` parse-error gate (in
/// [`parse_plain_svelte_function_pair`]), and the wholesale `visit_ts_type` /
/// `visit_ts_non_null_expression` / `visit_ts_type_parameter_declaration` /
/// `visit_accessor_property` reject arms below catch the type-position / non-null / generic
/// / auto-`accessor` node families. The RESIDUAL the strict-delta scan does NOT statically
/// fence is a hypothetical FUTURE `mjs`-PARSEABLE TS node family that is OUTSIDE the watched
/// structs AND not one of those wholesale-rejected kinds — it would enter through the
/// default visitor walk WITHOUT a compile error, and would need a new `visit_*` reject arm
/// here (plus an oracle-pinned negative test) to restore full coverage.
struct StrictOfficialDeltaScan {
    found: bool,
}

impl<'a> oxc_ast_visit::Visit<'a> for StrictOfficialDeltaScan {
    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        // Wildcard-free destructure (anti-regrowth — see the type doc).
        let oxc_ast::ast::Class {
            node_id: _,
            span: _,
            r#type: _,            // ClassDeclaration | ClassExpression — both plain JS
            decorators,           // OFFICIAL-DELTA: decorators are not plain ES (Acorn rejects)
            id: _,                // class name — plain JS
            type_parameters,      // TS-only (`class C<T>`)
            super_class: _,       // `extends <expr>` — plain JS (walked below)
            super_type_arguments, // TS-only (`extends B<T>`)
            implements,           // TS-only (`implements I`)
            body: _,              // class body — plain JS (walked below)
            r#abstract,           // TS-only (`abstract class`)
            declare,              // TS-only (`declare class`)
            scope_id: _,          // internal
        } = it;
        if !decorators.is_empty()
            || type_parameters.is_some()
            || super_type_arguments.is_some()
            || !implements.is_empty()
            || *r#abstract
            || *declare
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_class(self, it);
    }

    fn visit_property_definition(&mut self, it: &oxc_ast::ast::PropertyDefinition<'a>) {
        use oxc_ast::ast::PropertyDefinitionType;
        // Wildcard-free destructure (anti-regrowth — see the type doc).
        let oxc_ast::ast::PropertyDefinition {
            node_id: _,
            span: _,
            r#type,          // PropertyDefinition | TSAbstractPropertyDefinition
            decorators,      // OFFICIAL-DELTA: `@dec x = 1`
            key: _,          // property key — plain JS (walked)
            type_annotation, // TS-only (`x: number`)
            value: _,        // field initializer — plain JS (walked)
            computed: _,     // `['a'] = 1` — plain JS
            r#static: _,     // `static x` — plain JS
            declare,         // TS-only (`declare x`)
            r#override,      // TS-only (`override x`)
            optional,        // TS-only (`x?` member marker — NOT the JS `?.` operator)
            definite,        // TS-only (`x!` member marker — NOT an expression `!`)
            readonly,        // TS-only (`readonly x`)
            accessibility,   // TS-only (`public`/`private`/`protected`)
        } = it;
        if *r#type == PropertyDefinitionType::TSAbstractPropertyDefinition
            || !decorators.is_empty()
            || type_annotation.is_some()
            || *declare
            || *r#override
            || *optional
            || *definite
            || *readonly
            || accessibility.is_some()
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_property_definition(self, it);
    }

    fn visit_method_definition(&mut self, it: &oxc_ast::ast::MethodDefinition<'a>) {
        use oxc_ast::ast::MethodDefinitionType;
        // Wildcard-free destructure (anti-regrowth — see the type doc).
        let oxc_ast::ast::MethodDefinition {
            node_id: _,
            span: _,
            r#type,        // MethodDefinition | TSAbstractMethodDefinition
            decorators,    // OFFICIAL-DELTA
            key: _,        // method key — plain JS (walked)
            value: _,      // method body — plain JS (walked)
            kind: _,       // Constructor/Method/Get/Set — plain JS
            computed: _,   // plain JS
            r#static: _,   // plain JS
            r#override,    // TS-only (`override m()`)
            optional,      // TS-only (`m?()`)
            accessibility, // TS-only
        } = it;
        if *r#type == MethodDefinitionType::TSAbstractMethodDefinition
            || !decorators.is_empty()
            || *r#override
            || *optional
            || accessibility.is_some()
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_method_definition(self, it);
    }

    fn visit_accessor_property(&mut self, it: &oxc_ast::ast::AccessorProperty<'a>) {
        // The `accessor` auto-accessor keyword is itself non-plain-ECMAScript (a TC39
        // decorators-proposal construct official's Acorn parser REJECTS — verified
        // svelte@5.56.3: `Unexpected token`), so the node's very existence is the
        // refusal: there is NO plain-JS form that produces an `AccessorProperty`. A
        // wholesale reject is already anti-regrowth-complete (a new OXC field cannot
        // reopen a node that is unconditionally refused), so no per-field destructure is
        // required here.
        self.found = true;
        oxc_ast_visit::walk::walk_accessor_property(self, it);
    }

    fn visit_formal_parameter(&mut self, it: &oxc_ast::ast::FormalParameter<'a>) {
        // Wildcard-free destructure (anti-regrowth — see the type doc). A TS-only param
        // field (`optional`/`readonly`/`accessibility`/`override`/`decorators`/
        // `type_annotation`) is a plain-`.svelte` parse error in official; the plain
        // `pattern` (DEFAULT `x = 1` via `initializer`, REST `...x`, DESTRUCTURE `{a}`)
        // is plain JS and is NOT flagged.
        let oxc_ast::ast::FormalParameter {
            node_id: _,
            span: _,
            decorators,      // OFFICIAL-DELTA: `@dec x`
            pattern: _,      // binding pattern — plain JS (walked)
            type_annotation, // TS-only (`x: number`)
            initializer: _,  // default value — plain JS (walked)
            optional,        // TS-only (`x?`)
            accessibility,   // TS-only (param-property)
            readonly,        // TS-only (param-property)
            r#override,      // TS-only (param-property)
        } = it;
        if !decorators.is_empty()
            || type_annotation.is_some()
            || *optional
            || accessibility.is_some()
            || *readonly
            || *r#override
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_formal_parameter(self, it);
    }

    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {
        // Any type-position node (annotation / generic arg / `as`/`satisfies`/assertion
        // operand) is TS. Under `mjs` this fires only for a recovered `extends B<T>`
        // super-type-argument (also caught by the `Class` destructure); retained as
        // complete defense for any TS node a future `mjs` may tolerate.
        self.found = true;
    }

    fn visit_ts_non_null_expression(&mut self, it: &oxc_ast::ast::TSNonNullExpression<'a>) {
        // `name!` carries no `TSType` operand, so it needs its own flag. (Under `mjs` a
        // top-level `x!` is a parse error caught by the parse-error gate; defense here.)
        self.found = true;
        oxc_ast_visit::walk::walk_ts_non_null_expression(self, it);
    }

    fn visit_ts_type_parameter_declaration(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterDeclaration<'a>,
    ) {
        // A generic `<T, …>` list on an arrow / function / class. A constraint-less
        // `<T,>` carries no inner `TSType`, so this node is its own flag. (Under `mjs` a
        // generic arrow is a parse error; the class form is caught by the `Class`
        // destructure — this completes the type-parameter contract.)
        self.found = true;
    }
}

#[cfg(test)]
mod plain_svelte_function_pair_tests {
    use super::parse_plain_svelte_function_pair;
    use oxc_allocator::Allocator;

    /// The two element sources of a CLEAN plain-Svelte-JS function pair, or `None` to
    /// REFUSE. Drives the default-closed `parse_plain_svelte_function_pair` over the
    /// bind-expression `source` (the `{...}` content), the SAME entry the bind
    /// classifier + planner + name collector route through.
    fn pair(source: &str) -> Option<(String, String)> {
        let alloc = Allocator::default();
        parse_plain_svelte_function_pair(&alloc, source)
    }

    /// Whether the helper REFUSES `source` (returns `None`) — a fail-closed pair.
    fn refused(source: &str) -> bool {
        pair(source).is_none()
    }

    #[test]
    fn returns_clean_inline_pair_sources() {
        // A CLEAN inline arrow pair returns the two element source slices verbatim
        // (sliced from the original source, one synthetic `(` subtracted). These feed
        // the plain-JS rewrite lane + the named-decl admission set.
        assert_eq!(
            pair("() => v, (x) => v = x"),
            Some(("() => v".to_string(), "(x) => v = x".to_string())),
            "a clean inline pair must return the two element sources"
        );
    }

    #[test]
    fn returns_named_ident_pair_sources() {
        // A bare named-function pair returns the two identifiers (admitted as lowered
        // `function` declarations by the script-item allowlist).
        assert_eq!(
            pair("get, set"),
            Some(("get".to_string(), "set".to_string())),
            "a named-ident pair must return the two identifier sources"
        );
    }

    // --- strict official-delta: forms `mjs` PARSES CLEAN but the scan must REFUSE ---
    // (these are exactly the OXC-mjs-over-Acorn residual; the parse-error gate does NOT
    // catch them, so the delta scan is the load-bearing refusal — the class a default-open
    // enumerated scan would accept by omission.)

    #[test]
    fn refuses_class_accessibility_field() {
        // `class C { public x = 1 }` parses clean under `mjs` (populating
        // `PropertyDefinition.accessibility`); the delta scan refuses it. Official
        // svelte@5.56.3 rejects it (`Unexpected token`).
        assert!(refused("class C { public x = 1 }, set"));
        assert!(refused("class C { private x = 1 }, set"));
        assert!(refused("class C { protected x = 1 }, set"));
    }

    #[test]
    fn refuses_class_readonly_field() {
        assert!(refused("class C { readonly x = 1 }, set"));
    }

    #[test]
    fn refuses_class_optional_field() {
        // The TS `?` member marker (`PropertyDefinition.optional`), distinct from the JS
        // optional-chaining `?.` operator (which stays accepted — see positives).
        assert!(refused("class C { x? }, set"));
    }

    #[test]
    fn refuses_class_definite_field() {
        // The TS `!` member marker (`PropertyDefinition.definite`), distinct from an
        // expression-position non-null `!` (which is an `mjs` parse error).
        assert!(refused("class C { x! }, set"));
    }

    #[test]
    fn refuses_class_declare_field() {
        assert!(refused("class C { declare x }, set"));
    }

    #[test]
    fn refuses_class_field_decorator() {
        // `@dec x = 1` populates `PropertyDefinition.decorators` under `mjs`; the delta
        // scan refuses a non-empty decorator list (official rejects `@`).
        assert!(refused("class C { @dec x = 1 }, set"));
    }

    #[test]
    fn refuses_class_decorator() {
        // `@dec class C {}` populates `Class.decorators` under `mjs`; refused.
        assert!(refused("@dec class C {}, set"));
    }

    #[test]
    fn refuses_override_member() {
        // `override m()` populates `MethodDefinition.override` under `mjs`; refused.
        assert!(refused("class C { override m() {} }, set"));
    }

    #[test]
    fn refuses_accessor_property() {
        // `accessor x` produces an `AccessorProperty` node under `mjs`; the scan refuses
        // the node's existence (the `accessor` keyword is itself non-plain-JS).
        assert!(refused("class C { accessor x = 1 }, set"));
    }

    #[test]
    fn refuses_extends_type_arguments() {
        // `class C extends B<T> {}` parses clean under `mjs` (populating
        // `Class.super_type_arguments` + a recovered `TSType`); refused.
        assert!(refused("class C extends B<T> {}, set"));
    }

    // --- parse-error gate: forms `mjs` REJECTS at parse (fail closed by construction) ---

    #[test]
    fn refuses_class_implements() {
        // `implements` is an `mjs` parse error (and the recovered AST also populates
        // `Class.implements`, caught by the scan as defense). Either way refused.
        assert!(refused("class C implements I {}, set"));
    }

    #[test]
    fn refuses_abstract_class_and_member() {
        // `abstract` (class or member) is an `mjs` parse error → parse-gate refusal.
        assert!(refused("abstract class C {}, set"));
        assert!(refused("class C { abstract m() {} }, set"));
    }

    #[test]
    fn refuses_ts_operators_at_parse() {
        // The classic TS operators are `mjs` parse errors (`as`/`satisfies`, the
        // expression-position non-null `!`, a typed param, a generic arrow).
        assert!(refused("get as any, set"));
        assert!(refused("get satisfies T, set"));
        assert!(refused("get!, set"));
        assert!(refused("() => v, (x: number) => v = x"));
        assert!(refused("() => v, <T,>(x) => v = x"));
    }

    // --- shape: a non-two-element sequence is not a valid `{get, set}` pair ---

    #[test]
    fn refuses_non_two_element_sequence() {
        assert!(refused("get"), "a 1-element sequence is not a pair");
        assert!(refused("a, b, c"), "a 3-element sequence is not a pair");
    }

    // --- positives: plain JS that official ACCEPTS must STAY ACCEPTED ---

    #[test]
    fn accepts_plain_class_and_members() {
        // A plain `class C {}` and a class with plain fields/methods/static/private/
        // static-block carry no TS-only field — accepted (the carrier-stop is for
        // TS-only fields, not the class construct itself).
        assert!(pair("class C {}, set").is_some());
        assert!(
            pair("class C { x = 1; m() {} static s = 2; #p = 3; static { 1 } }, set").is_some()
        );
    }

    #[test]
    fn accepts_plain_param_features() {
        // DEFAULT / REST / DESTRUCTURED params are plain JS — accepted.
        assert!(pair("() => v, (x = 1) => v = x").is_some());
        assert!(pair("() => v, (...x) => v = x[0]").is_some());
        assert!(pair("() => v, ({a}) => v = a").is_some());
    }

    #[test]
    fn accepts_optional_chaining_and_literals() {
        // Optional chaining (`a?.b`), object/array literals are plain JS — accepted.
        // The JS optional-chaining `?.` (a `MemberExpression.optional` field) must NOT
        // be confused with the TS member `?` marker.
        assert!(pair("a?.b, set").is_some());
        assert!(pair("({a:1}), set").is_some());
        assert!(pair("[1,2], set").is_some());
    }

    #[test]
    fn discriminator_tag_type_arg_slice_is_not_ts_stripped() {
        // TRAP2: ``tag<string>`x` `` is a valid plain-JS RELATIONAL expression. Under
        // `mjs` it parses as `(tag < string) > `x` ` (NOT a tagged-template with TS type
        // arguments), so the helper ACCEPTS it and the getter SLICE is the verbatim
        // relational source — the `<string>` operands intact, never stripped to
        // ``tag`x` ``. Official svelte@5.56.3 likewise accepts + keeps the relational
        // form. The no-strip rewrite lane then preserves it end to end.
        let got = pair("tag<string>`x`, (x) => v = x");
        assert!(
            got.is_some(),
            "the relational `tag<string>`x`` pair must be accepted"
        );
        let (getter, _setter) = got.unwrap();
        assert_eq!(
            getter, "tag<string>`x`",
            "the getter slice must keep the relational `<string>` operands (no TS-strip)"
        );
        assert!(
            !getter.contains("tag`x`"),
            "the slice must NOT be the type-arg-stripped tagged template"
        );
    }
}
