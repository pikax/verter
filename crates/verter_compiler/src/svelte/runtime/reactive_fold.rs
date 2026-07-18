//! Constant folding for the Svelte client mixed-template emitter — a faithful port of
//! official `svelte@5.56.3`'s constant evaluator (`scope.evaluate`, the `Evaluation`
//! class in `phases/scope.js`) driving the `build_template_chunk` fold (`phases/
//! 3-transform/client/visitors/shared/utils.js`).
//!
//! Two read-only helpers the client backend consults to match official's mixed-attribute
//! emission EXACTLY — neither rewrites a read nor emits JS:
//!
//! - [`mixed_chunk_fold`] — the evaluate-fold reworked to the decidable TRI-STATE contract:
//!   a statically-KNOWN, byte-exact chunk (`id="a {d + 1} b"` over a demoted `$state(5)`)
//!   `Fold`s to official's value, JS-`String()`-coerced; a known-but-not-byte-exact chunk
//!   `Live`-falls-back (ledgered); a compile-time-throw chunk `Refuse`s; an undecided chunk
//!   stays a plain live interpolation.
//! - [`mixed_chunk_nullish_wrap`] — the `?? ''` coercion of a LIVE chunk (official's
//!   `is_defined`/precedence rule): a provably-defined part is raw, an undecided part is
//!   `?? ''`-wrapped (parenthesized for a `&&`/`||` operand), a memoized `$N` slot is bare.
//!
//! Both drive purely from the OXC typed AST + the binding table (typed-IR only, no
//! source-text eval) and the shared [`crate::js_number`] `Number::toString`.

use oxc_allocator::Allocator;
use oxc_ast::ast::{CallExpression, Expression, Program, Statement};
use oxc_span::SourceType;

use super::expr::{reparse_module, BindingRuntimeKind, BindingTable, ScopeGraph, ScopeId};
// Re-exported `pub(super)` so the `globals` child module (declared via `#[path]`) can reach
// the tri-state reason vocabularies through its `super::` path.
pub(super) use super::reactive_fold_tristate::{ChunkFold, ConstFoldRefuse, LiveFallbackReason};

/// The tri-state classification of a MIXED-template constant chunk — the official
/// `build_template_chunk` evaluate-fold reworked to the decidable [`ChunkFold`] contract.
/// A chunk whose `scope.evaluate(value)` `is_known` and is PROVEN byte-exact folds into
/// the cooked literal text as `(evaluated.value ?? '') + ''` (so `id="a {d + 1} b"` over a
/// demoted `$state(5)` becomes `'a 6 b'`); a chunk Svelte would have a known-but-not-
/// byte-exact value for live-falls-back (ledgered); a chunk whose evaluation THROWS at
/// compile time refuses; everything else stays a plain (un-ledgered) live interpolation.
///
/// Folding applies ONLY inside the MULTI-chunk template path (the caller routes a
/// single-chunk quoted value through the single-expression path, matching official's
/// `build_attribute_value` `value.length === 1` branch which does NOT evaluate-fold). This
/// is a faithful port of official `svelte@5.56.3`'s constant evaluator: the `Evaluation`
/// class in `phases/scope.js` (`scope.evaluate`) driven by the fold in
/// `phases/3-transform/client/visitors/shared/utils.js` (`build_template_chunk`),
/// including its EAGERNESS — both logical operands and both conditional branches are
/// evaluated before a value is selected, so a throw in a NON-selected position
/// (`false && (1n / 0n)`) still refuses. A template literal stops after the first unknown
/// interpolation.
///
/// For Verter's const-fold surface the sole foldable IDENTIFIER is a DEMOTED `$state(<literal>)`
/// (a never-reassigned `$state`, lowered to a plain `let d = <lit>`, resolving to
/// `PlainLocal`); a bare `const`/`let`/`import` is refused upstream, so it cannot reach
/// here. Official's `Evaluation` recurses into the binding's initializer
/// (`!binding.updated && initial !== null && !is_prop`); the `PlainLocal` kind already
/// proves the demotion, and the initializer is the `$state(arg)` whose argument the
/// `$state` rune arm evaluates.
///
/// Returns the [`ChunkFold`] tri-state: [`ChunkFold::Fold`] (the byte-exact cooked value,
/// `null`/`undefined` → `""`), [`ChunkFold::Live`] (`ledger: Some` for a ledgered
/// live-fallback, `None` for a plain not-foldable chunk), or [`ChunkFold::Refuse`] (a
/// compile-time throw). Typed-IR only — walks the OXC typed AST, no string-eval.
#[must_use]
pub(super) fn mixed_chunk_fold(
    expr_source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    instance_source: Option<&str>,
) -> ChunkFold {
    // ONE arena holds BOTH the chunk expression AND the instance source, so a foldable
    // identifier's initializer is a REAL AST node the evaluator recurses into (no nested
    // re-parse, no span slicing). The instance program is optional (a chunk with no
    // identifier references — e.g. `String(5)` — folds without it).
    let alloc = Allocator::default();
    let wrapped = format!("({expr_source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        // A torn parse cannot prove anything — stay a plain live interpolation (the caller
        // already routes a torn chunk through the live rewriter).
        return ChunkFold::Live { ledger: None };
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return ChunkFold::Live { ledger: None };
    };
    let instance_program = instance_source.and_then(|src| reparse_module(&alloc, src));
    mixed_chunk_fold_parsed(
        &stmt.expression,
        scope,
        bindings,
        scopes,
        instance_program.as_ref(),
    )
}

/// Retained-AST form of [`mixed_chunk_fold`]. Template interpolation planning
/// uses this entry point so the evaluator shares the canonical lowering parse.
#[must_use]
pub(super) fn mixed_chunk_fold_parsed<'ast>(
    expression: &Expression<'ast>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    instance_program: Option<&'ast Program<'ast>>,
) -> ChunkFold {
    PreparedChunkEvaluator::new(bindings, scopes, instance_program).fold(expression, scope)
}

/// Immutable, per-client-plan constant-evaluation context. The top-level
/// initializer index is built once and reused by every interpolation's fold and
/// definedness decisions.
pub(super) struct PreparedChunkEvaluator<'analysis, 'ast> {
    bindings: &'analysis BindingTable,
    scopes: &'analysis ScopeGraph,
    inits: rustc_hash::FxHashMap<&'ast str, &'ast Expression<'ast>>,
}

impl<'analysis, 'ast> PreparedChunkEvaluator<'analysis, 'ast> {
    #[must_use]
    pub(super) fn new(
        bindings: &'analysis BindingTable,
        scopes: &'analysis ScopeGraph,
        instance_program: Option<&'ast Program<'ast>>,
    ) -> Self {
        Self {
            bindings,
            scopes,
            inits: instance_program
                .map(collect_top_level_inits)
                .unwrap_or_default(),
        }
    }

    #[must_use]
    pub(super) fn fold(&self, expression: &Expression<'ast>, scope: ScopeId) -> ChunkFold {
        let ctx = ChunkEvalCtx {
            bindings: self.bindings,
            scopes: self.scopes,
            scope,
            inits: &self.inits,
        };
        let mut visited = rustc_hash::FxHashSet::default();
        ctx.evaluate(expression, &mut visited).classify()
    }

    #[must_use]
    pub(super) fn nullish_wrap(
        &self,
        expression: &Expression<'ast>,
        scope: ScopeId,
        is_memoized: bool,
    ) -> NullishCoalesce {
        if is_memoized {
            return NullishCoalesce::Bare;
        }
        let ctx = ChunkEvalCtx {
            bindings: self.bindings,
            scopes: self.scopes,
            scope,
            inits: &self.inits,
        };
        let mut visited = rustc_hash::FxHashSet::default();
        if ctx.evaluate(expression, &mut visited).is_defined() {
            return NullishCoalesce::None;
        }
        if is_logical_andor(unwrap_parens(expression)) {
            NullishCoalesce::Parenthesized
        } else {
            NullishCoalesce::Bare
        }
    }

    #[cfg(test)]
    fn top_level_init_count(&self) -> usize {
        self.inits.len()
    }
}

/// How a LIVE (un-folded) mixed-template expression part is coerced to a string in the
/// `` `…${part}…` `` template — official's `build_template_chunk` `?? ''`/precedence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NullishCoalesce {
    /// The value is provably DEFINED (a number / string / boolean result) — emit the bare
    /// `${value}` with NO `?? ''` (official `evaluated.is_defined`).
    None,
    /// Add `?? ''` directly: `${value ?? ''}` — the value is not provably defined and its
    /// top-level operator chains with `??` unparenthesized (an identifier, a member, a
    /// `??` chain, a memoized `$N` slot, a parenthesized sequence).
    Bare,
    /// Parenthesize then add `?? ''`: `${(value) ?? ''}` — the value's top-level operator is
    /// `&&` / `||`, which JS forbids mixing with `??` without parens (official's
    /// `b.logical('??', value, '')` serializes the same necessary parens).
    Parenthesized,
}

/// The `?? ''` coercion decision for a LIVE mixed-template chunk — official's
/// `build_template_chunk`: it evaluates the chunk value (`scope.evaluate(value)`) and emits
/// it RAW when the result is provably DEFINED, else wraps it `?? ''` (parenthesized when the
/// top operator is `&&`/`||`).
///
/// The MEMOIZED case (`is_memoized` — the chunk `has_call`, so the emitter replaces the
/// expression with a synthetic `$N` slot BEFORE the evaluate) is decisive: official
/// evaluates that `$N` IDENTIFIER, which resolves to no binding ⇒ UNKNOWN ⇒ NOT defined ⇒
/// `$N ?? ''` ALWAYS (and never parenthesized — `$N` is an identifier). So a memoized chunk
/// is unconditionally `Bare`, regardless of the original expression's type.
///
/// Only meaningful for a LIVE chunk (one [`mixed_chunk_fold`] did NOT `Fold`); a folded
/// chunk is a literal and never reaches here. Typed-IR only.
#[must_use]
pub(super) fn mixed_chunk_nullish_wrap(
    expr_source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    instance_source: Option<&str>,
    is_memoized: bool,
) -> NullishCoalesce {
    // A memoized chunk is the synthetic `$N` slot — official evaluates that identifier to
    // UNKNOWN, so it is never defined → always `$N ?? ''`, never parenthesized.
    if is_memoized {
        return NullishCoalesce::Bare;
    }

    let alloc = Allocator::default();
    let wrapped = format!("({expr_source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        // A torn parse cannot prove definedness — default to the safe `?? ''` (official's
        // behavior for any non-statically-defined value).
        return NullishCoalesce::Bare;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return NullishCoalesce::Bare;
    };
    let inner = unwrap_parens(&stmt.expression);

    // The definedness gate — official `evaluated.is_defined`.
    let instance_program = instance_source.and_then(|src| reparse_module(&alloc, src));
    let inits = instance_program
        .as_ref()
        .map(collect_top_level_inits)
        .unwrap_or_default();
    let ctx = ChunkEvalCtx {
        bindings,
        scopes,
        scope,
        inits: &inits,
    };
    let mut visited = rustc_hash::FxHashSet::default();
    if ctx.evaluate(&stmt.expression, &mut visited).is_defined() {
        return NullishCoalesce::None;
    }

    // Not provably defined ⇒ `?? ''`. Parenthesize iff the top-level operator is `&&` / `||`
    // (JS forbids mixing those with `??` unparenthesized; official's `b.logical` serializes
    // the same necessary parens).
    if is_logical_andor(inner) {
        NullishCoalesce::Parenthesized
    } else {
        NullishCoalesce::Bare
    }
}

/// Strip redundant parentheses / TS non-null wrappers to reach the operative expression.
fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut node = expr;
    loop {
        node = match node {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSNonNullExpression(e) => &e.expression,
            other => return other,
        };
    }
}

/// Whether an expression's TOP-LEVEL operator is a logical `&&` / `||` (NOT `??`) — the
/// operands JS forbids mixing with `??` without parens.
fn is_logical_andor(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::LogicalExpression(l)
            if matches!(
                l.operator,
                oxc_syntax::operator::LogicalOperator::And
                    | oxc_syntax::operator::LogicalOperator::Or
            )
    )
}

/// Collect the top-level `let`/`const`/`var <name> = <init>` declarators of a program into
/// a `name → &init-Expression` map — the identifier-initializer lookup the evaluator's
/// `Identifier` arm recurses through (official `binding.initial`). A destructuring pattern
/// or an init-less declarator contributes no entry.
fn collect_top_level_inits<'a>(
    program: &'a Program<'a>,
) -> rustc_hash::FxHashMap<&'a str, &'a Expression<'a>> {
    let mut out = rustc_hash::FxHashMap::default();
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for d in &decl.declarations {
            let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &d.id else {
                continue;
            };
            if let Some(init) = &d.init {
                // First declarator wins (a duplicate top-level name is a compile error
                // upstream; `entry`-style first-wins keeps the lookup deterministic).
                out.entry(id.name.as_str()).or_insert(init);
            }
        }
    }
    out
}

/// The evaluation context — the binding/scope resolver (for the foldability gate) plus the
/// top-level initializer map (for the identifier-recursion value).
struct ChunkEvalCtx<'analysis, 'ast> {
    bindings: &'analysis BindingTable,
    scopes: &'analysis ScopeGraph,
    scope: ScopeId,
    inits: &'analysis rustc_hash::FxHashMap<&'ast str, &'ast Expression<'ast>>,
}

/// A faithful port of the value-set in official's `Evaluation` (`phases/scope.js`): a chunk
/// evaluates to a SET of possible values; the chunk is statically KNOWN iff that set holds
/// exactly one CONCRETE primitive (never a type-marker symbol). `Number`/`String`/`Function`
/// model official's `NUMBER`/`STRING`/`FUNCTION` symbols (a known-TYPE-but-not-known-VALUE),
/// and `Unknown` models `UNKNOWN`.
///
/// `BigInt` is a CONCRETE value (official's `NUMBER` symbol "Includes `BigInt`", but a
/// literal `1n` stores the actual `1n` JS value, so it `is_known` and folds —
/// `$state(1n)` + `{d}` → `'1'`, `String(5n)` → `'5'`, `typeof 1n` → `'bigint'`). Arrays /
/// objects are deliberately ABSENT: official's `Evaluation` has no `ArrayExpression` /
/// `ObjectExpression` arm, so they reach the `default → UNKNOWN` and never produce a
/// concrete value — `String([1,2])` never folds because the argument is UNKNOWN.
#[derive(Debug, Clone)]
pub(super) enum EvalValue {
    Str(String),
    Num(f64),
    BigInt(num_bigint::BigInt),
    Bool(bool),
    Null,
    Undefined,
    NumberType,
    StringType,
    FunctionType,
    Unknown,
}

/// The result of evaluating a chunk — the value SET (official's `Evaluation.values`) plus
/// the two CONTRACT side-channels the tri-state needs that official's evaluator does not
/// carry (because official simply lets the native op throw / never has a byte-exactness
/// gap):
///
/// - `throws`: a POISON discovered during EAGER traversal — once ANY evaluated subtree
///   would have thrown at compile time, the whole chunk REFUSES, regardless of which value
///   official ultimately selects (matching `Evaluation` evaluating both logical operands /
///   conditional branches before selecting). A throw takes ABSOLUTE priority (refuse beats
///   fold beats live).
/// - `live`: a LEDGERED byte-exactness gap — the value is known and non-throwing, but
///   Verter cannot prove byte-exact emission (BigInt-vs-Number precision compare, large
///   ToInt32/ToUint32, parseInt radix/whitespace, lone surrogate, transcendental libm), so
///   the chunk LIVE-falls-back with the reason rather than folding a wrong literal.
struct Eval {
    values: Vec<EvalValue>,
    /// A compile-time throw discovered anywhere in the eagerly-evaluated subtree (poison;
    /// forces [`ChunkFold::Refuse`]).
    throws: Option<ConstFoldRefuse>,
    /// A known-but-not-byte-exact value (poison; forces a ledgered [`ChunkFold::Live`]).
    live: Option<LiveFallbackReason>,
}

impl Eval {
    /// A single concrete value (no throw, no byte-exactness gap).
    fn known(value: EvalValue) -> Self {
        Eval {
            values: vec![value],
            throws: None,
            live: None,
        }
    }

    /// A value SET (no throw, no byte-exactness gap).
    fn set(values: Vec<EvalValue>) -> Self {
        Eval {
            values,
            throws: None,
            live: None,
        }
    }

    /// A compile-time THROW — the value set is irrelevant (the chunk refuses).
    fn throws(reason: ConstFoldRefuse) -> Self {
        Eval {
            values: vec![EvalValue::Unknown],
            throws: Some(reason),
            live: None,
        }
    }

    /// A known-but-not-byte-exact value — a ledgered LIVE-fallback. The value set carries
    /// the (correct) markers so an enclosing op still type-classifies, but `live` forces
    /// the chunk to emit live.
    fn live_fallback(values: Vec<EvalValue>, reason: LiveFallbackReason) -> Self {
        Eval {
            values,
            throws: None,
            live: Some(reason),
        }
    }

    /// Merge another sub-evaluation's POISON (throw / live-fallback) into this one — used
    /// when an enclosing op must propagate a child's throw / byte-exactness gap even though
    /// it produces its own value set. A throw wins over a live-fallback. The FIRST throw /
    /// live reason discovered (source order) is retained.
    fn carry_poison_from(mut self, other: &Eval) -> Self {
        if self.throws.is_none() {
            self.throws = other.throws;
        }
        if self.live.is_none() {
            self.live = other.live;
        }
        self
    }

    /// The single known CONCRETE value, if the set is exactly one and it is not a
    /// type-marker symbol (official's `is_known`: `size === 1 && typeof value !== 'symbol'`).
    fn known_value(&self) -> Option<&EvalValue> {
        if self.values.len() != 1 {
            return None;
        }
        match &self.values[0] {
            EvalValue::NumberType
            | EvalValue::StringType
            | EvalValue::FunctionType
            | EvalValue::Unknown => None,
            // A concrete BigInt IS known (its `typeof` is `'bigint'`, never a symbol).
            concrete => Some(concrete),
        }
    }

    fn is_known(&self) -> bool {
        self.known_value().is_some()
    }

    /// Whether EVERY possible value is string-typed (official `is_string`).
    fn is_string(&self) -> bool {
        self.values
            .iter()
            .all(|v| matches!(v, EvalValue::Str(_) | EvalValue::StringType))
    }

    /// Whether EVERY possible value is number-typed (official `is_number`).
    fn is_number(&self) -> bool {
        self.values
            .iter()
            .all(|v| matches!(v, EvalValue::Num(_) | EvalValue::NumberType))
    }

    /// Whether the value is known to NOT be null/undefined — official's `is_defined`
    /// (FALSE when any possible value is `null` / `undefined` / `UNKNOWN`). Drives the
    /// `build_template_chunk` `?? ''` decision: a live but provably-defined chunk
    /// (`n + 1`, a number) is emitted RAW, an undecided chunk (`n`, `n && 1`) gets `?? ''`.
    fn is_defined(&self) -> bool {
        !self.values.iter().any(|v| {
            matches!(
                v,
                EvalValue::Null | EvalValue::Undefined | EvalValue::Unknown
            )
        })
    }

    /// The folded cooked string `(value ?? '') + ''` — JS `String()` coercion of the single
    /// known value; `null`/`undefined` fold to `""`. `None` when the chunk is not known.
    fn cooked(&self) -> Option<String> {
        let v = self.known_value()?;
        match v {
            EvalValue::Null | EvalValue::Undefined => Some(String::new()),
            // Type-marker symbols are filtered out by `known_value`.
            EvalValue::NumberType
            | EvalValue::StringType
            | EvalValue::FunctionType
            | EvalValue::Unknown => None,
            // Every concrete value (string / number / bigint / boolean) coerces via
            // JS `String()` (`(value ?? '') + ''`).
            _ => Some(string_coerce(v)),
        }
    }

    /// The tri-state classification of this chunk evaluation — the contract decision.
    /// PRIORITY: a compile-time THROW (refuse) beats a byte-exactness gap (ledgered
    /// live-fallback) beats a proven-exact fold; a plain not-statically-known chunk emits
    /// an un-ledgered live interpolation.
    fn classify(&self) -> ChunkFold {
        // (1) A throw anywhere in the eagerly-evaluated subtree → a deterministic refusal.
        if let Some(reason) = self.throws {
            return ChunkFold::Refuse(reason);
        }
        // (2) A known-but-not-byte-exact value → a LEDGERED live-fallback. (Checked before
        // the fold so a precision/surrogate/transcendental value never folds wrong.)
        if let Some(reason) = self.live {
            return ChunkFold::Live {
                ledger: Some(reason),
            };
        }
        // (3) A statically-known, byte-exact value → fold to the cooked literal.
        if let Some(cooked) = self.cooked() {
            return ChunkFold::Fold(cooked);
        }
        // (4) Not statically known (a signal read / member / call / sequence) → the normal
        // un-ledgered live interpolation.
        ChunkFold::Live { ledger: None }
    }
}

impl ChunkEvalCtx<'_, '_> {
    /// Evaluate a chunk expression to its value set — a faithful port of official's
    /// `Evaluation` constructor switch (`phases/scope.js`). `visited` guards identifier
    /// recursion cycles (official's `current_evaluations` map).
    fn evaluate(&self, expr: &Expression<'_>, visited: &mut rustc_hash::FxHashSet<String>) -> Eval {
        use oxc_syntax::operator::LogicalOperator as L;
        match expr {
            Expression::ParenthesizedExpression(p) => self.evaluate(&p.expression, visited),
            Expression::TSNonNullExpression(e) => self.evaluate(&e.expression, visited),

            // Literals.
            Expression::StringLiteral(s) => Eval::known(EvalValue::Str(s.value.to_string())),
            Expression::NumericLiteral(n) => Eval::known(EvalValue::Num(n.value)),
            Expression::BooleanLiteral(b) => Eval::known(EvalValue::Bool(b.value)),
            Expression::NullLiteral(_) => Eval::known(EvalValue::Null),
            // A BigInt literal is a CONCRETE value (official stores the JS `1n`), so it
            // folds: `$state(1n)` + `{d}` → `'1'`, `5n + 1n` → `'6'`, `typeof 1n` →
            // `'bigint'`. The oxc `value` is the base-10 digit string with no underscores.
            Expression::BigIntLiteral(b) => match b.value.parse::<num_bigint::BigInt>() {
                Ok(v) => Eval::known(EvalValue::BigInt(v)),
                Err(_) => Eval::known(EvalValue::Unknown),
            },

            // Identifier — official resolves the binding and recurses into its initializer
            // when `!updated && initial !== null && !is_prop`. In the const-fold surface the foldable case is a
            // DEMOTED `$state` (`PlainLocal`); `undefined` is the global undefined.
            Expression::Identifier(id) => self.evaluate_identifier(id.name.as_str(), visited),

            Expression::BinaryExpression(bin) => {
                // EAGER: both operands are evaluated (official's `Evaluation` does too), so a
                // throw / byte-exactness gap in EITHER operand poisons the result regardless
                // of the value finally produced.
                let a = self.evaluate(&bin.left, visited);
                let b = self.evaluate(&bin.right, visited);
                let out = if let (Some(av), Some(bv)) = (a.known_value(), b.known_value()) {
                    // Both operands known → the operator's own outcome (a value, a throw, a
                    // ledgered live-fallback, or "not foldable" → the per-family type set).
                    match eval_binary(bin.operator, av, bv) {
                        BinaryOutcome::Value(v) => Eval::known(v),
                        BinaryOutcome::Throws(r) => Eval::throws(r),
                        BinaryOutcome::Live(v, r) => Eval::live_fallback(vec![v], r),
                        BinaryOutcome::NotFoldable => {
                            Eval::set(binary_type_marker(bin.operator, &a, &b))
                        }
                    }
                } else {
                    // Unknown operands → the official per-family value SET (a comparison is
                    // `{true, false}`, an arithmetic op is NUMBER, `+` is STRING/NUMBER/both by
                    // operand types). The set is `!is_known` but stays `is_defined` (a NUMBER /
                    // STRING / boolean marker is never null/undefined).
                    Eval::set(binary_type_marker(bin.operator, &a, &b))
                };
                out.carry_poison_from(&a).carry_poison_from(&b)
            }

            Expression::LogicalExpression(log) => {
                // EAGER: official's `Evaluation` evaluates BOTH operands before selecting, so
                // a throw in the NON-selected operand (`false && (1n / 0n)`) still poisons.
                let a = self.evaluate(&log.left, visited);
                let b = self.evaluate(&log.right, visited);
                let selected = if let Some(av) = a.known_value() {
                    if let Some(bv) = b.known_value() {
                        Eval::known(eval_logical(log.operator, av, bv))
                    } else {
                        // Known left, unknown right — official short-circuits to the left
                        // value when the operator settles on it, else takes the right set.
                        let short_circuits = match log.operator {
                            L::And => !truthy(av),
                            L::Or => truthy(av),
                            L::Coalesce => !is_nullish(av),
                        };
                        if short_circuits {
                            Eval::known(av.clone())
                        } else {
                            Eval::set(b.values.clone())
                        }
                    }
                } else {
                    // Unknown left — union both sets.
                    Eval::set(a.values.iter().chain(b.values.iter()).cloned().collect())
                };
                selected.carry_poison_from(&a).carry_poison_from(&b)
            }

            Expression::ConditionalExpression(cond) => {
                // EAGER: official evaluates the test AND both branches before selecting, so a
                // throw in the NON-selected branch (`true ? 1 : (1n / 0n)`) still poisons.
                let test = self.evaluate(&cond.test, visited);
                let consequent = self.evaluate(&cond.consequent, visited);
                let alternate = self.evaluate(&cond.alternate, visited);
                let selected = if let Some(tv) = test.known_value() {
                    let taken = if truthy(tv) { &consequent } else { &alternate };
                    Eval::set(taken.values.clone())
                } else {
                    Eval::set(
                        consequent
                            .values
                            .iter()
                            .chain(alternate.values.iter())
                            .cloned()
                            .collect(),
                    )
                };
                selected
                    .carry_poison_from(&test)
                    .carry_poison_from(&consequent)
                    .carry_poison_from(&alternate)
            }

            Expression::UnaryExpression(un) => {
                let arg = self.evaluate(&un.argument, visited);
                let out = if let Some(av) = arg.known_value() {
                    match eval_unary(un.operator, av) {
                        UnaryOutcome::Value(v) => Eval::known(v),
                        UnaryOutcome::Throws(r) => Eval::throws(r),
                        UnaryOutcome::Live(v, r) => Eval::live_fallback(vec![v], r),
                    }
                } else {
                    // Unknown argument → the official per-operator value SET.
                    Eval::set(unary_type_marker(un.operator))
                };
                out.carry_poison_from(&arg)
            }

            Expression::TemplateLiteral(tpl) => {
                // Official: fold while every interpolation is known, accumulating the cooked
                // quasis; the first unknown interpolation makes the whole thing STRING-typed.
                // EAGER over the PREFIX: each interpolation up to (and including) the first
                // unknown is evaluated, so a throw / live-fallback in any evaluated
                // interpolation poisons — but evaluation STOPS at the first unknown (official
                // does not evaluate past it).
                let mut result = String::new();
                let mut poison = Eval::known(EvalValue::Str(String::new()));
                if let Some(first) = tpl.quasis.first() {
                    result.push_str(cooked_quasi(first));
                }
                for (i, e) in tpl.expressions.iter().enumerate() {
                    let ev = self.evaluate(e, visited);
                    poison = poison.carry_poison_from(&ev);
                    match ev.known_value() {
                        Some(v) => {
                            result.push_str(&string_coerce(v));
                            if let Some(q) = tpl.quasis.get(i + 1) {
                                result.push_str(cooked_quasi(q));
                            }
                        }
                        None => {
                            // STRING-typed (the first unknown stops the fold); the prefix's
                            // poison still propagates.
                            return Eval::known(EvalValue::StringType).carry_poison_from(&poison);
                        }
                    }
                }
                Eval::known(EvalValue::Str(result)).carry_poison_from(&poison)
            }

            Expression::CallExpression(call) => self.evaluate_call(call, visited),

            // A member access folds ONLY for a global constant (`Math.PI`); everything else
            // (a binding member, a computed member) is UNKNOWN — official `MemberExpression`.
            Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
                if let Some(v) = self.global_constant_member(expr) {
                    return Eval::known(v);
                }
                Eval::known(EvalValue::Unknown)
            }

            // Functions are the FUNCTION marker; everything else is UNKNOWN.
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                Eval::known(EvalValue::FunctionType)
            }

            _ => Eval::known(EvalValue::Unknown),
        }
    }

    /// The `Identifier` arm: resolve the binding kind, fold a DEMOTED `$state` (`PlainLocal`)
    /// by recursing into its initializer; `undefined` is the global; everything else is
    /// UNKNOWN (a reactive signal, a prop, an unresolved global).
    fn evaluate_identifier(&self, name: &str, visited: &mut rustc_hash::FxHashSet<String>) -> Eval {
        // A plain `let` is explicitly registered as `PlainLocal`. Top-level
        // `const`/`var` declarations intentionally have no runtime binding row,
        // but their canonical script initializer is still authoritative and
        // official evaluates it when it is unchanged. Reactive/import/prop rows
        // never enter this initializer fold.
        let kind = self.bindings.resolve_kind(self.scopes, self.scope, name);
        if kind == Some(BindingRuntimeKind::PlainLocal)
            || (kind.is_none() && self.inits.contains_key(name))
        {
            // Cycle guard: official's `current_evaluations` returns the in-flight evaluation
            // (which is empty / unknown) when an expression re-enters itself.
            if !visited.insert(name.to_string()) {
                return Eval::known(EvalValue::Unknown);
            }
            let result = match self.inits.get(name) {
                Some(init) => self.evaluate(init, visited),
                None => Eval::known(EvalValue::Unknown),
            };
            visited.remove(name);
            return result;
        }
        if name == "undefined" {
            // The global `undefined` (only when not shadowed — a shadowing binding would
            // have resolved above).
            if self
                .bindings
                .resolve_kind(self.scopes, self.scope, "undefined")
                .is_none()
            {
                return Eval::known(EvalValue::Undefined);
            }
        }
        Eval::known(EvalValue::Unknown)
    }

    /// The `CallExpression` arm: fold the `$state` / `$state.raw` / `$derived` rune (recurse
    /// into the argument) and a PURE-GLOBAL call from official's `globals` table (`String(…)`,
    /// `Number(…)`, `Math.floor(…)`, …) when every argument is known; everything else is
    /// UNKNOWN.
    fn evaluate_call(
        &self,
        call: &CallExpression<'_>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) -> Eval {
        // A rune callee (`$state(arg)` / `$state.raw(arg)` / `$derived(arg)`) folds to the
        // argument's evaluation — the producer side of a foldable identifier's initializer.
        if let Some(rune) = rune_callee_name(&call.callee) {
            return match rune {
                "$state" | "$state.raw" | "$derived" => match call.arguments.first() {
                    Some(arg) => match arg.as_expression() {
                        Some(e) => self.evaluate(e, visited),
                        None => Eval::known(EvalValue::Unknown),
                    },
                    // `$state()` (no arg) → `undefined`.
                    None => Eval::known(EvalValue::Undefined),
                },
                _ => Eval::known(EvalValue::Unknown),
            };
        }
        // A pure-global call from the `globals` table folds when every argument is known and
        // not a spread; otherwise it yields the table's TYPE marker (number/string-typed).
        if let Some((keypath, _)) = global_call_keypath(&call.callee, self) {
            if let Some(entry) = GLOBAL_CALLS.iter().find(|(k, _, _)| *k == keypath) {
                let has_spread = call
                    .arguments
                    .iter()
                    .any(|a| matches!(a, oxc_ast::ast::Argument::SpreadElement(_)));
                if !has_spread {
                    // EAGER: evaluate every argument (poison propagates from a throwing /
                    // not-byte-exact argument).
                    let arg_evals: Vec<Eval> = call
                        .arguments
                        .iter()
                        .filter_map(|a| a.as_expression())
                        .map(|e| self.evaluate(e, visited))
                        .collect();
                    let all_known = arg_evals.len() == call.arguments.len()
                        && arg_evals.iter().all(Eval::is_known);
                    // The global's outcome (a value, a throw on a known arg, a ledgered
                    // live-fallback, or the type marker) when every argument is known; the
                    // type marker otherwise.
                    let out = if let (Some(folder), true) = (entry.2, all_known) {
                        let known: Vec<EvalValue> = arg_evals
                            .iter()
                            .map(|e| e.known_value().unwrap().clone())
                            .collect();
                        match folder(&known) {
                            GlobalOutcome::Value(v) => Eval::known(v),
                            GlobalOutcome::Throws(r) => Eval::throws(r),
                            GlobalOutcome::Live(v, r) => Eval::live_fallback(vec![v], r),
                        }
                    } else {
                        Eval::known(entry.1.clone())
                    };
                    return arg_evals.iter().fold(out, Eval::carry_poison_from);
                }
            }
        }
        Eval::known(EvalValue::Unknown)
    }

    /// Whether a member chain is a global-constant keypath (`Math.PI`, `Math.E`, …) NOT
    /// shadowed by a binding, returning its concrete value. Official's `global_constants`.
    fn global_constant_member(&self, expr: &Expression<'_>) -> Option<EvalValue> {
        let keypath = static_member_keypath(expr)?;
        // The root must NOT resolve to a declared binding (else it is a member on a binding,
        // not a global constant) — official's `get_global_keypath` returns `null` then.
        let root = keypath.split('.').next()?;
        if self
            .bindings
            .resolve_kind(self.scopes, self.scope, root)
            .is_some()
        {
            return None;
        }
        GLOBAL_CONSTANTS
            .iter()
            .find(|(k, _)| *k == keypath)
            .map(|(_, v)| EvalValue::Num(*v))
    }
}

/// Whether a value is JS-truthy (official's `value ? …`).
fn truthy(v: &EvalValue) -> bool {
    match v {
        EvalValue::Bool(b) => *b,
        EvalValue::Num(n) => *n != 0.0 && !n.is_nan(),
        EvalValue::BigInt(n) => n.sign() != num_bigint::Sign::NoSign,
        EvalValue::Str(s) => !s.is_empty(),
        EvalValue::Null | EvalValue::Undefined => false,
        // Type markers are filtered before truthy is reached (operands are `known_value`).
        _ => false,
    }
}

/// Whether a value is `null`/`undefined` (official's `?? ` nullish check).
fn is_nullish(v: &EvalValue) -> bool {
    matches!(v, EvalValue::Null | EvalValue::Undefined)
}

/// JS `String()` coercion of a known concrete value (`(value ?? '') + ''`). `-0` →
/// `"0"`, `Infinity` → `"Infinity"`, `NaN` → `"NaN"` come from [`js_number_to_string`];
/// a BigInt renders its base-10 spelling (`5n` → `"5"`). Arrays / objects never reach
/// here (they evaluate to UNKNOWN), so there is no `'1,2'` / `'[object Object]'` case.
fn string_coerce(v: &EvalValue) -> String {
    match v {
        EvalValue::Str(s) => s.clone(),
        EvalValue::Num(n) => crate::js_number::js_number_to_string(*n),
        EvalValue::BigInt(n) => n.to_string(),
        EvalValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        EvalValue::Null => "null".to_string(),
        EvalValue::Undefined => "undefined".to_string(),
        _ => String::new(),
    }
}

/// JS `Number()` coercion of a known concrete value (for arithmetic operands). A string
/// goes through the exact ECMA-262 `StringToNumber` (`js_string_to_number`) — NOT Rust's
/// `str::parse`, so `'0x10'` → `16`, `''` → `0`, `'a 15 b'` → `NaN`. A BigInt coerces to
/// the nearest f64 (`Number(5n)` → `5`). A BigInt arithmetic operand is handled BEFORE
/// this (bigint arithmetic stays bigint); this is the cross-type coercion path only.
fn number_coerce(v: &EvalValue) -> f64 {
    match v {
        EvalValue::Num(n) => *n,
        EvalValue::BigInt(n) => bigint_to_f64(n),
        EvalValue::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        EvalValue::Null => 0.0,
        EvalValue::Undefined => f64::NAN,
        EvalValue::Str(s) => crate::js_number::js_string_to_number(s),
        _ => f64::NAN,
    }
}

/// `Number(bigint)` — the nearest f64 to a BigInt (`Number(5n)` → `5.0`). Renders the
/// magnitude as a decimal string and defers the correctly-rounded conversion to Rust's
/// f64 parser (which over/underflows to `±Infinity` / `0` exactly like JS).
fn bigint_to_f64(n: &num_bigint::BigInt) -> f64 {
    n.to_string().parse::<f64>().unwrap_or(f64::NAN)
}

/// Whether a value is a JS string (for the `+` operator's string-concat decision).
fn is_string_value(v: &EvalValue) -> bool {
    matches!(v, EvalValue::Str(_))
}

/// Whether a value is a concrete JS BigInt.
fn is_bigint_value(v: &EvalValue) -> bool {
    matches!(v, EvalValue::BigInt(_))
}

/// The outcome of folding a pure-global call (`String(…)`, `Math.floor(…)`, …) over KNOWN
/// arguments — the tri-state decision for that one global. The globals table's folders
/// ([`globals::GlobalFolder`]) return this. (A type-known-but-not-foldable global —
/// `Math.random` / `BigInt` — has NO folder; the caller yields the table's type marker for
/// it, so there is no `TypeMarker` outcome.)
pub(super) enum GlobalOutcome {
    /// A proven-exact value.
    Value(EvalValue),
    /// The global THROWS under the known argument (refuse): a BigInt arg to a numeric
    /// global (`Math.clz32(1n)`), an invalid `String.fromCodePoint` code point.
    Throws(ConstFoldRefuse),
    /// A known-but-not-byte-exact value (ledgered live-fallback) — a transcendental libm
    /// result, a huge-finite `ToInt32`, a parseInt/parseFloat whitespace/radix gap, a lone
    /// surrogate.
    Live(EvalValue, LiveFallbackReason),
}

/// The outcome of folding a binary operator over two KNOWN concrete operands — the
/// tri-state decision for that one op.
enum BinaryOutcome {
    /// A proven-exact single value.
    Value(EvalValue),
    /// The native op THROWS at compile time (refuse).
    Throws(ConstFoldRefuse),
    /// A known-but-not-byte-exact value (ledgered live-fallback) — the (live) value the
    /// op would produce plus the reason.
    Live(EvalValue, LiveFallbackReason),
    /// The op is outside the concretely-foldable set — fall back to the per-family type
    /// marker (official's unknown-operand value set).
    NotFoldable,
}

/// Whether a relational / equality comparison of `a` and `b` whose values mix a BigInt
/// with a Number / numeric-string would lose precision in Verter's f64 coercion. Official
/// compares the EXACT mathematical values; Verter coerces the BigInt to f64 first. Returns
/// `true` ONLY when one operand is a BigInt and the other is a Number / string (the
/// cross-type numeric comparison) — a BigInt-vs-BigInt comparison is exact (no coercion),
/// and a string-vs-string / number-vs-number comparison never touches a BigInt.
fn bigint_cross_type_compare(a: &EvalValue, b: &EvalValue) -> bool {
    let bigint_vs_numeric = |x: &EvalValue, y: &EvalValue| {
        is_bigint_value(x) && matches!(y, EvalValue::Num(_) | EvalValue::Str(_))
    };
    bigint_vs_numeric(a, b) || bigint_vs_numeric(b, a)
}

/// Whether a finite Number operand of a 32-bit bitwise / shift op is too large for an
/// exact `ToInt32` / `ToUint32` via a truncating `as i64` cast (`|x| >= 2^53`, where the
/// f64 no longer represents the integer exactly so the modulo-2^32 result diverges from
/// JS). A non-finite operand coerces to `0` exactly (so it is never the large case); a
/// BigInt never reaches the numeric bitwise path (a mixed BigInt bitwise op throws first).
fn large_for_to_int32(x: f64) -> bool {
    x.is_finite() && x.abs() >= 9_007_199_254_740_992.0
}

/// Evaluate a binary operator over two KNOWN concrete operands — official's `binary` table
/// (the native JS operator) reworked to the tri-state [`BinaryOutcome`]. A throwing op
/// (mixed BigInt+Number arithmetic / bitwise, BigInt `/`/`%` by zero, BigInt `>>>`, BigInt
/// negative exponent, `in` / `instanceof` over a known primitive RHS) → [`Throws`]; a
/// not-byte-exact value (BigInt-vs-Number precision comparison, a huge-finite `ToInt32`
/// bitwise op) → [`Live`]; everything else folds exactly.
fn eval_binary(
    op: oxc_syntax::operator::BinaryOperator,
    a: &EvalValue,
    b: &EvalValue,
) -> BinaryOutcome {
    use oxc_syntax::operator::BinaryOperator as B;

    // `in` / `instanceof` with KNOWN operands: the RHS is a known PRIMITIVE here (an object
    // RHS evaluates to UNKNOWN and never reaches this arm), and JS THROWS a TypeError for a
    // primitive RHS — a deterministic refusal, never a fold.
    match op {
        B::In => return BinaryOutcome::Throws(ConstFoldRefuse::InOnPrimitive),
        B::Instanceof => return BinaryOutcome::Throws(ConstFoldRefuse::InstanceofPrimitive),
        _ => {}
    }

    // BigInt arithmetic / bitwise: JS requires BOTH operands be BigInt (mixing with a
    // Number throws). `+` with a string operand is string concat (handled below).
    let both_bigint = is_bigint_value(a) && is_bigint_value(b);
    let either_bigint = is_bigint_value(a) || is_bigint_value(b);
    let either_string = is_string_value(a) || is_string_value(b);

    // Arithmetic / bitwise over two BigInts → a BigInt (except `**` with a negative
    // exponent and `/`/`%` by zero and `>>>`, which JS throws on → refuse).
    if both_bigint {
        if let (EvalValue::BigInt(x), EvalValue::BigInt(y)) = (a, b) {
            match eval_bigint_binary(op, x, y) {
                BigIntBinaryOutcome::Value(v) => return BinaryOutcome::Value(v),
                BigIntBinaryOutcome::Throws(r) => return BinaryOutcome::Throws(r),
                // Comparison / equality ops fall through to the cross-type handling below
                // (BigInt-vs-BigInt is exact there).
                BigIntBinaryOutcome::Comparison => {}
            }
        }
    } else if either_bigint && !either_string && op.is_arithmetic() {
        // A BigInt mixed with a non-string, non-BigInt operand in arithmetic THROWS in
        // JS (`1n + 1`) → refuse.
        return BinaryOutcome::Throws(ConstFoldRefuse::BigIntMixedArith);
    } else if either_bigint && op.is_bitwise() {
        // `1n & 3` / `1n << 2` (mixed) throws; `1n & 3n` is handled by `both_bigint`.
        return BinaryOutcome::Throws(ConstFoldRefuse::BigIntMixedArith);
    }

    // Numeric helpers (operands coerced to f64 / i32 as JS does). For a relational /
    // equality op a BigInt compares numerically against a Number (handled by `js_compare`
    // / `js_loose_eq`), so the coercion below only feeds the non-bigint arithmetic arms.
    let an = number_coerce(a);
    let bn = number_coerce(b);
    let to_i32 = |x: f64| -> i32 {
        if x.is_nan() || x.is_infinite() {
            0
        } else {
            (x.trunc() as i64 as u32) as i32
        }
    };
    let to_u32 = |x: f64| -> u32 {
        if x.is_nan() || x.is_infinite() {
            0
        } else {
            x.trunc() as i64 as u32
        }
    };
    // A 32-bit op over a huge-finite Number operand is NOT byte-exact via the truncating
    // cast (JS applies modulo-2^32) → ledgered live-fallback. (The value computed below is
    // the live value the op would yield; it is emitted live, not folded.)
    let is_bitwise_or_shift = matches!(
        op,
        B::ShiftLeft
            | B::ShiftRight
            | B::ShiftRightZeroFill
            | B::BitwiseOR
            | B::BitwiseXOR
            | B::BitwiseAnd
    );
    if is_bitwise_or_shift && (large_for_to_int32(an) || large_for_to_int32(bn)) {
        let v = match op {
            B::ShiftLeft => EvalValue::Num((to_i32(an).wrapping_shl(to_u32(bn) & 31)) as f64),
            B::ShiftRight => EvalValue::Num((to_i32(an) >> (to_u32(bn) & 31)) as f64),
            B::ShiftRightZeroFill => EvalValue::Num((to_u32(an) >> (to_u32(bn) & 31)) as f64),
            B::BitwiseOR => EvalValue::Num((to_i32(an) | to_i32(bn)) as f64),
            B::BitwiseXOR => EvalValue::Num((to_i32(an) ^ to_i32(bn)) as f64),
            _ => EvalValue::Num((to_i32(an) & to_i32(bn)) as f64),
        };
        return BinaryOutcome::Live(v, LiveFallbackReason::LargeToInt32);
    }
    // A cross-type BigInt-vs-Number COERCING comparison (`<`/`<=`/`>`/`>=` and `==`/`!=`)
    // loses precision in the f64 coercion → ledgered live-fallback (the boolean below is
    // the live value). STRICT equality (`===`/`!==`) does NOT coerce — a BigInt and a
    // Number are ALWAYS distinct types, so `5n === 5` is exactly `false` (no precision
    // loss) and stays an exact fold.
    let is_coercing_comparison = matches!(
        op,
        B::LessThan
            | B::LessEqualThan
            | B::GreaterThan
            | B::GreaterEqualThan
            | B::Equality
            | B::Inequality
    );
    if is_coercing_comparison && bigint_cross_type_compare(a, b) {
        let v = eval_comparison(op, a, b);
        return BinaryOutcome::Live(v, LiveFallbackReason::BigIntNumberPrecisionCompare);
    }
    BinaryOutcome::Value(match op {
        B::Addition => {
            // `+` is string concat when EITHER operand is a string (a BigInt concats too:
            // `1n + 'x'` → `'1x'`), else numeric.
            if either_string {
                EvalValue::Str(format!("{}{}", string_coerce(a), string_coerce(b)))
            } else {
                EvalValue::Num(an + bn)
            }
        }
        B::Subtraction => EvalValue::Num(an - bn),
        B::Multiplication => EvalValue::Num(an * bn),
        B::Division => EvalValue::Num(an / bn),
        B::Remainder => EvalValue::Num(an % bn),
        // `**` over Numbers is a transcendental (`powf`) — V8's fdlibm vs Rust's system
        // libm is not provably bit-identical cross-platform → ledgered live-fallback. (A
        // BigInt `**` is handled exactly by `eval_bigint_binary`; this is the Number arm.)
        B::Exponential => {
            return BinaryOutcome::Live(
                EvalValue::Num(an.powf(bn)),
                LiveFallbackReason::TranscendentalLibm,
            )
        }
        B::ShiftLeft => EvalValue::Num((to_i32(an).wrapping_shl(to_u32(bn) & 31)) as f64),
        B::ShiftRight => EvalValue::Num((to_i32(an) >> (to_u32(bn) & 31)) as f64),
        B::ShiftRightZeroFill => EvalValue::Num((to_u32(an) >> (to_u32(bn) & 31)) as f64),
        B::BitwiseOR => EvalValue::Num((to_i32(an) | to_i32(bn)) as f64),
        B::BitwiseXOR => EvalValue::Num((to_i32(an) ^ to_i32(bn)) as f64),
        B::BitwiseAnd => EvalValue::Num((to_i32(an) & to_i32(bn)) as f64),
        B::LessThan
        | B::LessEqualThan
        | B::GreaterThan
        | B::GreaterEqualThan
        | B::Equality
        | B::Inequality
        | B::StrictEquality
        | B::StrictInequality => eval_comparison(op, a, b),
        // `in` / `instanceof` handled above.
        B::In | B::Instanceof => return BinaryOutcome::NotFoldable,
    })
}

/// Evaluate a relational / equality binary operator over two known operands (the
/// non-throwing comparison arms shared by the exact and the cross-type-precision paths).
fn eval_comparison(
    op: oxc_syntax::operator::BinaryOperator,
    a: &EvalValue,
    b: &EvalValue,
) -> EvalValue {
    use oxc_syntax::operator::BinaryOperator as B;
    match op {
        B::LessThan => EvalValue::Bool(js_compare(a, b, |o| o == std::cmp::Ordering::Less)),
        B::LessEqualThan => EvalValue::Bool(js_compare(a, b, |o| {
            o == std::cmp::Ordering::Less || o == std::cmp::Ordering::Equal
        })),
        B::GreaterThan => EvalValue::Bool(js_compare(a, b, |o| o == std::cmp::Ordering::Greater)),
        B::GreaterEqualThan => EvalValue::Bool(js_compare(a, b, |o| {
            o == std::cmp::Ordering::Greater || o == std::cmp::Ordering::Equal
        })),
        B::Equality => EvalValue::Bool(js_loose_eq(a, b)),
        B::Inequality => EvalValue::Bool(!js_loose_eq(a, b)),
        B::StrictEquality => EvalValue::Bool(js_strict_eq(a, b)),
        B::StrictInequality => EvalValue::Bool(!js_strict_eq(a, b)),
        // Only the relational / equality ops are routed here.
        _ => EvalValue::Unknown,
    }
}

/// The value SET an unknown-operand binary yields — official's per-family fallback. The
/// comparison family is the 2-value `{true, false}` set (defined, not known); the
/// arithmetic / bitwise family is NUMBER; `+` is STRING / NUMBER / both by operand types.
/// `in` / `instanceof` are likewise `{true, false}`. Every result is `is_defined` (no
/// marker is null/undefined/UNKNOWN), matching official's `is_defined`.
fn binary_type_marker(
    op: oxc_syntax::operator::BinaryOperator,
    a: &Eval,
    b: &Eval,
) -> Vec<EvalValue> {
    use oxc_syntax::operator::BinaryOperator as B;
    match op {
        B::Equality
        | B::Inequality
        | B::StrictEquality
        | B::StrictInequality
        | B::LessThan
        | B::LessEqualThan
        | B::GreaterThan
        | B::GreaterEqualThan
        | B::In
        | B::Instanceof => vec![EvalValue::Bool(true), EvalValue::Bool(false)],
        B::Remainder
        | B::BitwiseAnd
        | B::Multiplication
        | B::Exponential
        | B::Subtraction
        | B::Division
        | B::ShiftLeft
        | B::ShiftRight
        | B::ShiftRightZeroFill
        | B::BitwiseXOR
        | B::BitwiseOR => vec![EvalValue::NumberType],
        B::Addition => {
            if a.is_string() || b.is_string() {
                vec![EvalValue::StringType]
            } else if a.is_number() && b.is_number() {
                vec![EvalValue::NumberType]
            } else {
                // STRING ∪ NUMBER — a 2-marker set: not known, but still defined.
                vec![EvalValue::StringType, EvalValue::NumberType]
            }
        }
    }
}

/// Evaluate a logical operator over two KNOWN operands — official's `logical` table.
fn eval_logical(
    op: oxc_syntax::operator::LogicalOperator,
    a: &EvalValue,
    b: &EvalValue,
) -> EvalValue {
    use oxc_syntax::operator::LogicalOperator as L;
    match op {
        L::And => {
            if truthy(a) {
                b.clone()
            } else {
                a.clone()
            }
        }
        L::Or => {
            if truthy(a) {
                a.clone()
            } else {
                b.clone()
            }
        }
        L::Coalesce => {
            if is_nullish(a) {
                b.clone()
            } else {
                a.clone()
            }
        }
    }
}

/// The outcome of folding a unary operator over a KNOWN operand.
enum UnaryOutcome {
    /// A proven-exact single value.
    Value(EvalValue),
    /// The native op THROWS at compile time (refuse) — unary `+` on a BigInt.
    Throws(ConstFoldRefuse),
    /// A known-but-not-byte-exact value (ledgered live-fallback) — `~` over a huge-finite
    /// Number (the modulo-2^32 `ToInt32` is not byte-exact via the truncating cast).
    Live(EvalValue, LiveFallbackReason),
}

/// The value SET an unknown-argument unary yields — official's per-operator fallback (`!`
/// / `delete` are the defined 2-value boolean set; numeric unaries are NUMBER; `typeof` is
/// STRING; `void` is `undefined`).
fn unary_type_marker(op: oxc_syntax::operator::UnaryOperator) -> Vec<EvalValue> {
    use oxc_syntax::operator::UnaryOperator as U;
    match op {
        U::LogicalNot | U::Delete => vec![EvalValue::Bool(true), EvalValue::Bool(false)],
        U::UnaryPlus | U::UnaryNegation | U::BitwiseNot => vec![EvalValue::NumberType],
        U::Typeof => vec![EvalValue::StringType],
        U::Void => vec![EvalValue::Undefined],
    }
}

/// Evaluate a unary operator over a KNOWN operand — official's `unary` table reworked to
/// the tri-state [`UnaryOutcome`]. A unary `+` on a BigInt THROWS (`+5n` is a TypeError) →
/// [`UnaryOutcome::Throws`]; `~` over a huge-finite Number is not byte-exact →
/// [`UnaryOutcome::Live`]; everything else folds exactly.
fn eval_unary(op: oxc_syntax::operator::UnaryOperator, a: &EvalValue) -> UnaryOutcome {
    use oxc_syntax::operator::UnaryOperator as U;
    UnaryOutcome::Value(match op {
        // `-5n` / `~5n` stay BigInt; over any other operand they coerce to a Number. A BigInt
        // unary whose result reaches V8's size limit (`~` adds a bit) throws `Maximum BigInt
        // size exceeded` — guard it (a 2^30-bit operand is never a real template constant).
        U::UnaryNegation => match a {
            EvalValue::BigInt(n) => {
                if bigint::unary_exceeds_max(n.bits()) {
                    return UnaryOutcome::Throws(ConstFoldRefuse::BigIntMaxSizeExceeded);
                }
                EvalValue::BigInt(-n)
            }
            _ => EvalValue::Num(-number_coerce(a)),
        },
        // `+5n` THROWS in JS (TypeError) — refuse.
        U::UnaryPlus => match a {
            EvalValue::BigInt(_) => return UnaryOutcome::Throws(ConstFoldRefuse::BigIntUnaryPlus),
            _ => EvalValue::Num(number_coerce(a)),
        },
        U::LogicalNot => EvalValue::Bool(!truthy(a)),
        U::BitwiseNot => match a {
            EvalValue::BigInt(n) => {
                if bigint::unary_exceeds_max(n.bits()) {
                    return UnaryOutcome::Throws(ConstFoldRefuse::BigIntMaxSizeExceeded);
                }
                EvalValue::BigInt(!n)
            }
            _ => {
                let n = number_coerce(a);
                // `~` over a huge-finite Number is not byte-exact via the truncating cast
                // (JS applies modulo-2^32 `ToInt32`) → ledgered live-fallback.
                if large_for_to_int32(n) {
                    let i = (n.trunc() as i64 as u32) as i32;
                    return UnaryOutcome::Live(
                        EvalValue::Num((!i) as f64),
                        LiveFallbackReason::LargeToInt32,
                    );
                }
                let i = if n.is_nan() || n.is_infinite() {
                    0i32
                } else {
                    (n.trunc() as i64 as u32) as i32
                };
                EvalValue::Num((!i) as f64)
            }
        },
        U::Typeof => EvalValue::Str(js_typeof(a).to_string()),
        U::Void => EvalValue::Undefined,
        // Official `unary.delete: () => true` — a known-operand `delete` folds to `true`.
        U::Delete => EvalValue::Bool(true),
    })
}

/// JS `typeof` of a known concrete value.
fn js_typeof(v: &EvalValue) -> &'static str {
    match v {
        EvalValue::Str(_) | EvalValue::StringType => "string",
        EvalValue::Num(_) | EvalValue::NumberType => "number",
        EvalValue::BigInt(_) => "bigint",
        EvalValue::Bool(_) => "boolean",
        EvalValue::Undefined => "undefined",
        EvalValue::Null => "object",
        EvalValue::FunctionType => "function",
        EvalValue::Unknown => "undefined",
    }
}

/// JS abstract relational comparison (`<`, `<=`, `>`, `>=`) over two known operands: string
/// comparison when BOTH are strings, else numeric. A BigInt compares numerically against a
/// Number / another BigInt (`5n < 6` is `true`). The `pick` closure maps the resulting
/// ordering to the operator's boolean; a `NaN` operand is unordered → `false`.
fn js_compare(a: &EvalValue, b: &EvalValue, pick: impl Fn(std::cmp::Ordering) -> bool) -> bool {
    if is_string_value(a) && is_string_value(b) {
        let (EvalValue::Str(sa), EvalValue::Str(sb)) = (a, b) else {
            return false;
        };
        return pick(sa.cmp(sb));
    }
    // BigInt-vs-BigInt compares exactly (no f64 precision loss).
    if let (EvalValue::BigInt(x), EvalValue::BigInt(y)) = (a, b) {
        return pick(x.cmp(y));
    }
    let (an, bn) = (number_coerce(a), number_coerce(b));
    match an.partial_cmp(&bn) {
        Some(o) => pick(o),
        None => false, // NaN
    }
}

/// JS strict equality (`===`) over two known concrete values. `5n === 5` is `false`
/// (BigInt and Number are distinct types); `5n === 5n` compares exactly.
fn js_strict_eq(a: &EvalValue, b: &EvalValue) -> bool {
    match (a, b) {
        (EvalValue::Str(x), EvalValue::Str(y)) => x == y,
        (EvalValue::Num(x), EvalValue::Num(y)) => x == y, // NaN !== NaN, -0 === 0 both hold in f64
        (EvalValue::BigInt(x), EvalValue::BigInt(y)) => x == y,
        (EvalValue::Bool(x), EvalValue::Bool(y)) => x == y,
        (EvalValue::Null, EvalValue::Null) => true,
        (EvalValue::Undefined, EvalValue::Undefined) => true,
        // Cross-type (incl. BigInt vs Number) is always `false` under `===`.
        _ => false,
    }
}

/// JS loose equality (`==`) over two known concrete values — the subset reachable from const
/// folds (string/number/bigint/boolean/null/undefined). `null == undefined` is `true`;
/// otherwise numeric coercion applies across types (`5n == 5` is `true`).
fn js_loose_eq(a: &EvalValue, b: &EvalValue) -> bool {
    match (a, b) {
        (EvalValue::Null | EvalValue::Undefined, EvalValue::Null | EvalValue::Undefined) => true,
        (EvalValue::Null | EvalValue::Undefined, _)
        | (_, EvalValue::Null | EvalValue::Undefined) => false,
        (EvalValue::Str(x), EvalValue::Str(y)) => x == y,
        (EvalValue::BigInt(x), EvalValue::BigInt(y)) => x == y,
        (EvalValue::Bool(_), _) | (_, EvalValue::Bool(_)) => number_coerce(a) == number_coerce(b),
        (EvalValue::Num(_), EvalValue::Str(_)) | (EvalValue::Str(_), EvalValue::Num(_)) => {
            number_coerce(a) == number_coerce(b)
        }
        (EvalValue::Num(x), EvalValue::Num(y)) => x == y,
        // BigInt vs Number / String coerces both to numbers (`5n == 5`, `5n == '5'`).
        (EvalValue::BigInt(_), EvalValue::Num(_) | EvalValue::Str(_))
        | (EvalValue::Num(_) | EvalValue::Str(_), EvalValue::BigInt(_)) => {
            number_coerce(a) == number_coerce(b)
        }
        _ => false,
    }
}

/// The cooked text of a template-literal quasi (`cooked` is `None` only for an invalid
/// escape, which a clean parse never produces here — fall back to the raw text).
fn cooked_quasi<'a>(q: &'a oxc_ast::ast::TemplateElement<'a>) -> &'a str {
    q.value
        .cooked
        .as_ref()
        .map(|c| c.as_str())
        .unwrap_or_else(|| q.value.raw.as_str())
}

/// The rune keypath of a call's callee (`$state` / `$state.raw` / `$derived` / …) when the
/// callee is the bare `$`-rooted rune identifier or `$x.y` member, else `None`.
fn rune_callee_name(callee: &Expression<'_>) -> Option<&'static str> {
    match callee {
        Expression::Identifier(id) => match id.name.as_str() {
            "$state" => Some("$state"),
            "$derived" => Some("$derived"),
            _ => None,
        },
        Expression::StaticMemberExpression(m) => {
            let Expression::Identifier(obj) = &m.object else {
                return None;
            };
            match (obj.name.as_str(), m.property.name.as_str()) {
                ("$state", "raw") => Some("$state.raw"),
                ("$derived", "by") => Some("$derived.by"),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The dotted keypath of a static-member chain (`Math.PI` → `"Math.PI"`), or a bare
/// identifier (`String` → `"String"`). Computed / non-identifier members abort with `None`
/// (official `get_global_keypath`).
fn static_member_keypath(expr: &Expression<'_>) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut node = expr;
    loop {
        match node {
            Expression::StaticMemberExpression(m) => {
                parts.push(m.property.name.as_str());
                node = &m.object;
            }
            Expression::Identifier(id) => {
                parts.push(id.name.as_str());
                break;
            }
            _ => return None,
        }
    }
    parts.reverse();
    Some(parts.join("."))
}

/// The global keypath of a CALL callee (`String` / `Math.floor`) NOT shadowed by a binding,
/// for the pure-global-call fold. Returns `(keypath, ())`.
fn global_call_keypath(
    callee: &Expression<'_>,
    ctx: &ChunkEvalCtx<'_, '_>,
) -> Option<(String, ())> {
    let keypath = static_member_keypath(callee)?;
    let root = keypath.split('.').next()?;
    if ctx
        .bindings
        .resolve_kind(ctx.scopes, ctx.scope, root)
        .is_some()
    {
        return None; // a call on a binding is not a pure global
    }
    Some((keypath, ()))
}

/// The pure-global call / constant tables — a faithful COMPLETE port of official
/// `scope.js`'s `globals` + `global_constants`, extracted to keep both files under the
/// size guard. The `evaluate_call` / `global_constant_member` arms consult
/// [`globals::GLOBAL_CALLS`] / [`globals::GLOBAL_CONSTANTS`].
#[path = "reactive_fold_globals.rs"]
mod globals;

use globals::{GLOBAL_CALLS, GLOBAL_CONSTANTS};

/// The native-JS BigInt operator table (arithmetic / bitwise / arbitrary-precision shifts)
/// plus the CHEAP size guard that refuses an oversize `<<` / `**` without ever attempting
/// the multi-gigabit allocation — extracted to keep this file under the size guard. The
/// `eval_binary` arm consults [`bigint::eval_bigint_binary`] when both operands are BigInts.
#[path = "reactive_fold_bigint.rs"]
mod bigint;

use bigint::{eval_bigint_binary, BigIntBinaryOutcome};

#[cfg(test)]
#[path = "reactive_fold_tests.rs"]
mod tests;
