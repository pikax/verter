//! The shared `$.template_effect` memoizer + emitter for the Svelte client backend.
//!
//! A single combined effect groups EVERY reactive update (reactive text, dynamic
//! attribute / property writes, coalesced `$.set_class` / `$.set_style`) in source
//! order. A value that `has_call` is hoisted through the shared [`Memoizer`] into a
//! `$N` deps-array slot (the official `build_template_chunk` rule); [`emit_text_effect`]
//! renders the chosen effect shape (inline / block / deps-array). The memoizer is
//! shared across the whole effect so the placeholders are numbered `$0, $1, …` in
//! collection order, matching the official compiler.

/// The official `Memoizer` for a `$.template_effect` group — it hoists each
/// `has_call` reactive value into a `$N` placeholder and a `() => <expr>`
/// dependency, SHARED across the whole effect so the placeholders are numbered
/// `$0, $1, …` in collection order. A non-call value is returned inline (no
/// memoization). Mirrors `phases/3-transform/client/visitors/shared/utils.js`'s
/// `Memoizer` (the synchronous-deps half — async/`has_await` text is fail-closed
/// at 5j and never reaches here).
#[derive(Default)]
pub(super) struct Memoizer {
    /// The collected `() => <expr>` dependency bodies, in placeholder order.
    deps: Vec<String>,
}

impl Memoizer {
    /// Route a rewritten chunk through the memoizer: a `has_call` chunk is hoisted
    /// (its rewritten expression becomes the next `() => <expr>` dep and a `$N`
    /// placeholder is returned); a non-call chunk is returned inline unchanged.
    pub(super) fn add(&mut self, rewritten: String, has_call: bool) -> String {
        if !has_call {
            return rewritten;
        }
        let placeholder = format!("${}", self.deps.len());
        self.deps.push(rewritten);
        placeholder
    }

    /// The collected dependency bodies (`[expr0, expr1, …]`), consuming the
    /// memoizer.
    pub(super) fn into_deps(self) -> Vec<String> {
        self.deps
    }
}

/// One reactive UPDATE folded into the combined `$.template_effect`, distinguished by its
/// JS form:
///
/// - [`Expr`](Self::Expr) — an EXPRESSION write (`$.set_text(…)`, `$.set_attribute(…)`,
///   `el.p = v`): valid as a concise arrow body and `;`-terminated in the block form.
/// - [`Stmt`](Self::Stmt) — a STATEMENT write (the `bind:group` guarded
///   `if (<tracker> !== (<tracker> = V)) { … }` value update): it is already a complete
///   statement, so it FORCES the block-bodied arrow form (a concise `() => if (…)` is
///   invalid JS) and is emitted verbatim (NO trailing `;`).
pub(super) enum EffectBody {
    /// An expression write.
    Expr(String),
    /// A statement write (forces the block form).
    Stmt(String),
}

impl EffectBody {
    /// The raw body text (the expression or the statement).
    fn text(&self) -> &str {
        match self {
            EffectBody::Expr(s) | EffectBody::Stmt(s) => s,
        }
    }

    /// Whether this body is a STATEMENT (forces the block-bodied arrow form).
    fn is_stmt(&self) -> bool {
        matches!(self, EffectBody::Stmt(_))
    }
}

/// Append the effect bodies into a block-bodied arrow region: an [`EffectBody::Expr`] is
/// `;`-terminated; an [`EffectBody::Stmt`] (a complete `if {…}` statement) is emitted verbatim.
fn push_effect_bodies(out: &mut String, bodies: &[EffectBody]) {
    for body in bodies {
        match body {
            EffectBody::Expr(s) => out.push_str(&format!("\t\t{s};\n")),
            EffectBody::Stmt(s) => out.push_str(&format!("\t\t{s}\n")),
        }
    }
}

/// Emit the grouped reactive `$.template_effect`, choosing the official shape:
///
/// - NO bodies → nothing.
/// - No memoized deps, one EXPRESSION body → the inline `$.template_effect(() => <write>)`.
/// - No memoized deps, many bodies OR any STATEMENT body → the block
///   `$.template_effect(() => { … })`.
/// - Any memoized deps → the deps-array form `$.template_effect(($0, …) => <body>,
///   [() => dep0, …])` (the parameter list is `$0 … $N-1`; the body is the single
///   expression write or a block of writes; the deps array is the second argument).
///
/// A STATEMENT body (the `bind:group` guarded value update) forces the block form even for a
/// single body — a concise `() => if (…)` is not valid JS.
pub(super) fn emit_text_effect(out: &mut String, bodies: &[EffectBody], deps: &[String]) {
    if bodies.is_empty() {
        return;
    }
    // A statement body cannot be a concise arrow body, so it forces the block form.
    let concise = bodies.len() == 1 && !bodies.iter().any(EffectBody::is_stmt);
    if deps.is_empty() {
        // The non-memoized shapes (the §1.2 / bare-read path, plus the forced-block case).
        if concise {
            out.push_str(&format!(
                "\t$.template_effect(() => {});\n",
                bodies[0].text()
            ));
        } else {
            out.push_str("\t$.template_effect(() => {\n");
            push_effect_bodies(out, bodies);
            out.push_str("\t});\n");
        }
        return;
    }
    // The MEMOIZED deps-array form. The arrow params are `$0 … $N-1`.
    let params = (0..deps.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    // Each memoized dep is a `() => <expr>` concise arrow — a concise-arrow-from-payload body, so
    // it routes through the shared UNCONDITIONAL wrap (`() => (EXPR)`). A leading-`{` object dep
    // (`() => ({ color: … })`) returns the object instead of parsing a block body; over-wrapping a
    // non-object dep is behavior-preserving and invisible to the paren-insensitive comparator.
    let deps_array = deps
        .iter()
        .map(|d| {
            format!(
                "() => {}",
                super::client_codegen_helpers::concise_arrow_expr_body(d)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if concise {
        out.push_str(&format!(
            "\t$.template_effect(({params}) => {}, [{deps_array}]);\n",
            bodies[0].text()
        ));
    } else {
        out.push_str(&format!("\t$.template_effect(({params}) => {{\n"));
        push_effect_bodies(out, bodies);
        out.push_str(&format!("\t}}, [{deps_array}]);\n"));
    }
}
