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

/// Wrap a concise-arrow body that is an OBJECT LITERAL in parentheses
/// (`{ … }` → `({ … })`), so `() => { … }` returns the object instead of parsing
/// `{ … }` as a block statement. This is the official `b.arrow` parenthesization for
/// an `ObjectExpression` body — needed for a memoized `class:`/`style:` directives
/// object dep (`() => ({ foo: … })`). Any other body (a call, a `$.clsx(…)`, an
/// identifier) is returned unchanged. A leading `[` (an array dep) is valid as a bare
/// concise body and needs no wrap.
fn wrap_arrow_body(body: &str) -> String {
    if body.trim_start().starts_with('{') {
        format!("({body})")
    } else {
        body.to_string()
    }
}

/// Emit the grouped reactive-text `$.template_effect`, choosing the official shape:
///
/// - NO writes → nothing.
/// - No memoized deps, one write → the inline `$.template_effect(() => <write>)`.
/// - No memoized deps, many writes → the block `$.template_effect(() => { … })`.
/// - Any memoized deps → the deps-array form `$.template_effect(($0, …) => <body>,
///   [() => dep0, …])` (the parameter list is `$0 … $N-1`; the body is the single
///   write or a block of writes; the deps array is the second argument).
pub(super) fn emit_text_effect(out: &mut String, text_writes: &[String], deps: &[String]) {
    if text_writes.is_empty() {
        return;
    }
    if deps.is_empty() {
        // The non-memoized shapes (unchanged from the §1.2 / bare-read path).
        if text_writes.len() == 1 {
            out.push_str(&format!("\t$.template_effect(() => {});\n", text_writes[0]));
        } else {
            out.push_str("\t$.template_effect(() => {\n");
            for body in text_writes {
                out.push_str(&format!("\t\t{body};\n"));
            }
            out.push_str("\t});\n");
        }
        return;
    }
    // The MEMOIZED deps-array form. The arrow params are `$0 … $N-1`.
    let params = (0..deps.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let deps_array = deps
        .iter()
        .map(|d| format!("() => {}", wrap_arrow_body(d)))
        .collect::<Vec<_>>()
        .join(", ");
    if text_writes.len() == 1 {
        out.push_str(&format!(
            "\t$.template_effect(({params}) => {}, [{deps_array}]);\n",
            text_writes[0]
        ));
    } else {
        out.push_str(&format!("\t$.template_effect(({params}) => {{\n"));
        for body in text_writes {
            out.push_str(&format!("\t\t{body};\n"));
        }
        out.push_str(&format!("\t}}, [{deps_array}]);\n"));
    }
}
