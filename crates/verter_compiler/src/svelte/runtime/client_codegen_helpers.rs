//! Free codegen helpers shared by the Svelte client plan + emitter.
//!
//! These are the small, self-contained string/argument-list utilities the
//! [`super::client_plan`] projection and the [`super::client`] emitter both build
//! the `$.set_class` / `$.set_style` / dynamic-attribute call shapes from — the
//! single-quoted JS-string escape, the backtick-template literal escape, the
//! directive object-key quoting, the style-object literal builder, the trailing-arg
//! trimmer, and the op-target predicate. They are typed-IR-only
//! (char loops, never `str::replace` / regex) so the carrier compiler path stays
//! within the no-string-munge rule.

use super::ir::{EventTarget, NodeId, RuntimeOp};

/// The DOM-node target of a runtime op (for the dead-options-attr skip). A
/// global-target event (`<svelte:window>` etc.) has no DOM node target — `None`
/// (such ops belong to a refused special surface anyway).
pub(super) fn op_target_node(op: &RuntimeOp) -> Option<NodeId> {
    match op {
        RuntimeOp::ReactiveText { target, .. }
        | RuntimeOp::ReactiveAttr { target, .. }
        | RuntimeOp::SpreadAttrs { target, .. }
        | RuntimeOp::StyleDirectiveTrigger { target }
        | RuntimeOp::Binding { target, .. }
        | RuntimeOp::Attachment { target, .. }
        | RuntimeOp::Action { target, .. }
        | RuntimeOp::Transition { target, .. }
        | RuntimeOp::Animation { target, .. }
        | RuntimeOp::NonStaticProperty { target, .. } => Some(*target),
        RuntimeOp::Event { target, .. } => match target {
            EventTarget::Node(node) => Some(*node),
            _ => None,
        },
    }
}

/// Trim trailing `None` entries from a call's argument list, then render each
/// remaining `Some` to its text (a remaining `None` becomes the `undefined` literal —
/// it sits BEFORE a later present arg, so it must be a real argument). Mirrors the
/// official `b.call`, which drops trailing `undefined` arguments but keeps an interior
/// one. (On the `set_class` / `set_style` paths, no interior `None` is ever produced —
/// `css_hash` / `prev` are always `Some` when `next` is present — so this only ever
/// drops a trailing tail.) Used by the emitter when it assembles the `$.set_class` /
/// `$.set_style` call from the structured op pieces.
pub(super) fn trim_trailing_none(mut args: Vec<Option<String>>) -> Vec<String> {
    while matches!(args.last(), Some(None)) {
        args.pop();
    }
    args.into_iter()
        .map(|a| a.unwrap_or_else(|| "undefined".to_string()))
        .collect()
}

/// Build a single-quoted JS string literal from `s`, via a char loop (NOT
/// `str::replace`, which the codegen post-hoc-munging guard forbids in the carrier
/// compiler path). Mirrors the official printer's string serializer (esrap@2.2.11
/// `quote`): it escapes the backslash, the quote character, the newline, and the
/// carriage return — and ONLY those. A tab / form-feed / vertical-tab / NUL / line-
/// or paragraph-separator passes through verbatim (each is a valid raw character
/// inside a single-quoted JS string, and the official serializer leaves it). Used for
/// the class/style base value + quoted object keys built by the plan.
pub(super) fn js_single_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Escape a literal text run for embedding inside a backtick template literal (the
/// mixed-attribute concatenation). Mirrors the official `sanitize_template_string`
/// (`/(`|${|\\)/g → \\$1`): a backslash, a backtick, and the template-literal
/// interpolation opener `${` are each backslash-escaped, via a char loop (NOT
/// `str::replace`, which the codegen post-hoc-munging guard forbids). A lone `$`
/// NOT followed by `{` is left verbatim (it does not open an interpolation).
pub(super) fn escape_template_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            // `${` opens a template interpolation — escape the `$` so the literal
            // `${` stays literal text (the official `sanitize_template_string`).
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\$");
            }
            _ => out.push(c),
        }
    }
    out
}

/// Wrap a CONCISE-ARROW expression-payload body in one paren pair UNCONDITIONALLY
/// (`EXPR` → `(EXPR)`), so `() => (EXPR)` is always an expression body — never a
/// block body (`() => { … }` parses `{ … }` as a block returning `undefined`) and
/// never split (`() => a, b` would leak `b` as a positional arg). This is the official
/// `b.arrow` parenthesization applied unconditionally: over-wrapping a complete
/// expression is behavior-preserving and cosmetically invisible to the paren-insensitive
/// structural corpus comparator. Used at EVERY concise-arrow-from-payload site
/// (the `{@html}` thunk + the `$.template_effect` memoizer deps array). Any future
/// concise-arrow-from-payload path MUST route through this — there is no shape predicate.
pub(super) fn concise_arrow_expr_body(body: &str) -> String {
    format!("({body})")
}

/// The official `b.thunk` of a rewritten value: a bare zero-arg identifier
/// call unthunks to its callee (`rest()` → `rest`); every other shape keeps
/// the `() => <expr>` arrow. `unthunk_callee` is the ANALYZED unthunk fact —
/// the canonical parse's direct-zero-arg-identifier-call callee, kept only
/// while its rewrite stays a plain identifier (computed at preparation, see
/// `PreparedTemplateValue::unthunk_callee`) — so the decision reads typed
/// analysis facts, never a reparse of the generated text. Shared by the
/// component/slot spread thunks, the `{@render}` argument thunks, and the
/// legacy-wrap `$.untrack` payload.
pub(super) fn js_thunk(unthunk_callee: Option<&str>, rewritten: &str) -> String {
    match unthunk_callee {
        Some(callee) => callee.to_string(),
        None => format!("() => {rewritten}"),
    }
}

/// An object-literal KEY for a `class:` / `style:` directive name: a bare identifier
/// key for a plain JS-identifier name (`foo`, `fontWeight`), or a QUOTED key for a
/// name that is not a valid bare identifier (`--x`, `font-size`, `aria-label`). The
/// official `b.init(name, …)` emits a quoted key for a non-identifier property name.
pub(super) fn object_key(name: &str) -> String {
    if is_plain_js_identifier(name) {
        name.to_string()
    } else {
        js_single_quoted(name)
    }
}

/// Render a list of `(key, value)` style-directive entries into an object literal
/// `{ key: value, ... }` (an empty list → `{}`). The keys are already
/// [`object_key`]-quoted as needed.
pub(super) fn style_object(entries: &[(String, String)]) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let body = entries
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body} }}")
}

/// Render a `key: value` object-literal property, collapsing to the JS object
/// SHORTHAND `key` when the value text EQUALS the (unquoted) key — the official
/// printer's shorthand for `{ on: on }` → `{ on }` (a `class:on` / `class:on={on}` /
/// `style:color` directive whose condition is the same-named binding). A quoted key
/// (`'font-size'`) never shorthands.
pub(super) fn object_property(key: &str, value: &str) -> String {
    if key == value && is_plain_js_identifier(key) {
        key.to_string()
    } else {
        format!("{key}: {value}")
    }
}

/// The VALUE half of [`fold_style_directives`] — the merged style-directive
/// object (or the `[normal, important]` array) WITHOUT the `[$.STYLE]: ` key,
/// for the typed attribute-effect item that carries the key separately.
pub(super) fn fold_style_directives_value(entries: &[(String, bool)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let normal: Vec<&str> = entries
        .iter()
        .filter(|(_, imp)| !imp)
        .map(|(s, _)| s.as_str())
        .collect();
    let important: Vec<&str> = entries
        .iter()
        .filter(|(_, imp)| *imp)
        .map(|(s, _)| s.as_str())
        .collect();
    let obj = |props: &[&str]| -> String {
        if props.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", props.join(", "))
        }
    };
    Some(if important.is_empty() {
        obj(&normal)
    } else {
        format!("[{}, {}]", obj(&normal), obj(&important))
    })
}

/// Whether a string is a single plain JS identifier (`/^[A-Za-z_$][A-Za-z0-9_$]*$/`)
/// — used to decide whether a `class:` / `style:` directive name needs a quoted
/// object key, and whether a rewritten unthunk callee stayed a bare identifier.
pub(super) fn is_plain_js_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}
