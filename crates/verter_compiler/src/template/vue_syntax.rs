//! Pure Vue template syntax helpers shared by the runtime code generators
//! and the IDE projection: name casing, event-handler keys and modifiers,
//! handler-expression shape, built-in component tags and JS string escaping.
//! Nothing here records an edit or names a runtime helper import.

use crate::template::oxc::types::OxcParsedExpression;

/// The trimmed loop source starts inside the parser-owned right-side window.
pub(crate) fn v_for_iterable_start(
    source: &str,
    v_for: &crate::types::NodeProp,
    parsed: &crate::utils::oxc::vue::VForWithBindings<'_>,
) -> u32 {
    let start = parsed.right_offset();
    let end = v_for.value_end.expect("a parsed v-for has a value");
    let raw = &source[start as usize..end as usize];
    start + (raw.len() - raw.trim_start().len()) as u32
}

/// A Vue built-in component, recognised by its tag in PascalCase or
/// kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinComponent {
    /// `<Teleport>`.
    Teleport,
    /// `<Suspense>`.
    Suspense,
    /// `<KeepAlive>`.
    KeepAlive,
    /// `<BaseTransition>`.
    BaseTransition,
    /// `<Transition>`.
    Transition,
    /// `<TransitionGroup>`.
    TransitionGroup,
}

/// The Vue built-in component a tag names, if any. Handles both PascalCase
/// (`<Suspense>`) and lowercase/kebab-case (`<suspense>`, `<keep-alive>`).
#[must_use]
pub fn builtin_component(tag: &str) -> Option<BuiltinComponent> {
    match tag {
        "Teleport" | "teleport" => Some(BuiltinComponent::Teleport),
        "Suspense" | "suspense" => Some(BuiltinComponent::Suspense),
        "KeepAlive" | "keep-alive" => Some(BuiltinComponent::KeepAlive),
        "BaseTransition" | "base-transition" => Some(BuiltinComponent::BaseTransition),
        "Transition" | "transition" => Some(BuiltinComponent::Transition),
        "TransitionGroup" | "transition-group" => Some(BuiltinComponent::TransitionGroup),
        _ => None,
    }
}

impl BuiltinComponent {
    /// The name Vue exports this component under.
    #[must_use]
    pub const fn export_name(self) -> &'static str {
        match self {
            Self::Teleport => "Teleport",
            Self::Suspense => "Suspense",
            Self::KeepAlive => "KeepAlive",
            Self::BaseTransition => "BaseTransition",
            Self::Transition => "Transition",
            Self::TransitionGroup => "TransitionGroup",
        }
    }
}

/// Convert a hyphenated string to camelCase.
///
/// - `"my-event"` → `"myEvent"`
/// - `"foo-bar-baz"` → `"fooBarBaz"`
/// - `"click"` → `"click"` (unchanged)
/// - `"-leading"` → `"Leading"` (leading dash capitalizes)
pub fn camelize(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('-') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    std::borrow::Cow::Owned(result)
}

/// Append a formatted event handler key to an existing buffer.
///
/// Applies Vue's event naming convention: `"on"` + capitalize-first +
/// camelize, appended to `buf`.
pub fn format_event_handler_key_into(buf: &mut String, event_name: &str) {
    buf.push_str("on");
    // Vue lifecycle hooks: @vue:mounted → onVnodeMounted
    // Convert "vue:" prefix to "vnode-" so the camelCase logic produces the correct result.
    let chars: Box<dyn Iterator<Item = char>> = if let Some(rest) = event_name.strip_prefix("vue:")
    {
        Box::new("vnode-".chars().chain(rest.chars()))
    } else {
        Box::new(event_name.chars())
    };
    let mut capitalize_next = true;
    for ch in chars {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                buf.push(upper);
            }
            capitalize_next = false;
        } else {
            buf.push(ch);
        }
    }
}

/// Classification of a Vue event-handler modifier (mirrors `@vue/compiler-dom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventModifierKind {
    /// `capture` / `once` / `passive` — folded into the prop key name.
    Option,
    /// Event/system guards → `_withModifiers` (`stop`, `prevent`, `ctrl`, …).
    Runtime,
    /// Keyboard key filter → `_withKeys` (`enter`, `backspace`, `home`, …).
    Key,
}

/// Classify one `@event.mod` token the way Vue's compiler does.
///
/// On keyboard events (`keydown` / `keyup` / `keypress`), any modifier that is
/// not an event option or a known non-key guard is a **key** filter — including
/// aliases like `backspace`, `home`, `end`, and arbitrary key names. Putting
/// those in `_withModifiers` is wrong: unknown runtime guards are no-ops, so
/// the handler runs for every key (and may `preventDefault` everything).
pub(crate) fn classify_event_modifier(mod_name: &str, event_arg: &str) -> EventModifierKind {
    match mod_name {
        "capture" | "once" | "passive" => EventModifierKind::Option,
        // Event propagation + system modifiers + exact + middle mouse button.
        "stop" | "prevent" | "self" | "ctrl" | "shift" | "alt" | "meta" | "exact" | "middle" => {
            EventModifierKind::Runtime
        }
        // `left` / `right` are mouse-button modifiers on pointer events and key
        // aliases on keyboard events (ArrowLeft / ArrowRight).
        "left" | "right" => {
            if is_keyboard_event_arg(event_arg) {
                EventModifierKind::Key
            } else {
                EventModifierKind::Runtime
            }
        }
        // Everything else on a keyboard event is a key filter (enter, tab,
        // delete, backspace, esc, space, up, down, home, end, page-up, …).
        // On non-keyboard events, treat unknown mods as runtime (Vue does the
        // same via isNonKeyModifier / maybeKeyModifier split).
        _ => {
            if is_keyboard_event_arg(event_arg) {
                EventModifierKind::Key
            } else {
                EventModifierKind::Runtime
            }
        }
    }
}

#[inline]
pub(crate) fn is_keyboard_event_arg(arg_name: &str) -> bool {
    // Strip optional Vue event prefixes; arg is usually `keydown` / `keyup` /
    // `keypress` (or `Keydown` after camelize — we see the raw template arg).
    let lower = arg_name.as_bytes();
    lower.eq_ignore_ascii_case(b"keydown")
        || lower.eq_ignore_ascii_case(b"keyup")
        || lower.eq_ignore_ascii_case(b"keypress")
        || arg_name.starts_with("key")
}

/// Convert a kebab-case or camelCase string to PascalCase.
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check whether an expression is a simple member expression (identifier or
/// dot-separated property access like `foo`, `foo.bar`, `_ctx.onClick`).
/// Used to distinguish event handler references from inline handlers.
pub fn is_member_expression(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Whether a `v-on` value is a handler REFERENCE rather than an inline
/// handler: an identifier other than `undefined`, or a member access —
/// static, computed (`handlers[current]`) or optional (`a?.b`) — under any
/// parentheses or TypeScript assertions, as Vue's `isMemberExpression` reads
/// the parsed expression. A value without a parse falls back to
/// [`is_member_expression`]; a statement list is never a reference.
pub(crate) fn is_handler_reference(value: &str, oxc_exp: Option<&OxcParsedExpression<'_>>) -> bool {
    use oxc_ast::ast::Expression;
    if is_multi_statement_handler(oxc_exp) {
        return false;
    }
    let Some(expr) = oxc_exp.and_then(|oxc| oxc.expression.as_ref()) else {
        return is_member_expression(value.trim());
    };
    match expr.get_inner_expression() {
        Expression::Identifier(ident) => ident.name != "undefined",
        Expression::ChainExpression(chain) => chain.expression.is_member_expression(),
        inner => inner.is_member_expression(),
    }
}

/// Whether a `v-on` handler value is an inline STATEMENT LIST rather than a
/// single expression (`@click="a = 1; b = 2"`).
///
/// This is the compiler's SOLE notion of "multi-statement": the parse fact the
/// template front-end records on `OxcParsedExpression::multi_statement` after
/// reading the value under the statement-list grammar. Every backend — VDOM,
/// Vapor and the IDE/TSX projection — asks this one question, so they cannot
/// disagree about the same input.
///
/// A raw-source `value.contains(';')` probe is NOT equivalent and must not be
/// reintroduced. It misses a newline-separated list (`@click="a = 1\nb = 2"` has
/// no `;` at all, yet is two statements) and fires on a `;` that is merely
/// inside a string literal (`@click="a = 'x;y'"` is one statement).
pub fn is_multi_statement_handler(exp: Option<&OxcParsedExpression<'_>>) -> bool {
    exp.is_some_and(|e| e.multi_statement)
}

/// True when `value` is already a function expression (arrow or `function`)
/// that can be used as an event handler as-is.
///
/// Multi-statement arrows like `(event: KeyboardEvent) => { if (...); }` contain
/// `;` and must NOT be re-wrapped as `$event => { (event) => {…} }` — that creates
/// a block whose body is an unused expression statement (no-op at runtime). This
/// is the reka-ui SliderImpl / DateField inline-handler pattern.
pub(crate) fn is_function_expression_value(
    value: &str,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
) -> bool {
    // A statement LIST is never a single function expression, whatever its text
    // looks like. The parse fact overrides the text fallback below, which scans
    // for a top-level `=>` and would read `a = 1; b = () => 2` as a bare handler
    // — emitting the statement list unwrapped into the props object literal.
    if is_multi_statement_handler(oxc_exp) {
        return false;
    }
    // A parsed value is classified by its root alone, under any parentheses
    // or TypeScript assertions (as Vue's `isFnExpression` unwraps them): an
    // assignment or call whose operands contain an arrow
    // (`handler = () => count++`) is not a function value, so the text scan
    // below never sees it.
    if let Some(expr) = oxc_exp.and_then(|oxc| oxc.expression.as_ref()) {
        use oxc_ast::ast::Expression;
        return matches!(
            expr.get_inner_expression(),
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        );
    }
    // Fallback without OXC: trim and look for arrow / function keyword.
    let t = value.trim_start();
    if t.starts_with("function") || t.starts_with("async function") {
        return true;
    }
    let t = if let Some(rest) = t.strip_prefix("async") {
        rest.trim_start()
    } else {
        t
    };
    // Scan for top-level `=>` (paren/brace depth aware).
    let bytes = t.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth -= 1,
            b'=' if depth == 0 && bytes[i + 1] == b'>' => return true,
            b'"' | b'\'' | b'`' => {
                // Skip string / template literal
                let q = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == q {
                        break;
                    }
                    // template ${}
                    if q == b'`' && bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{'
                    {
                        depth += 1;
                        i += 2;
                        continue;
                    }
                    if q == b'`' && bytes[i] == b'}' && depth > 0 {
                        depth -= 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Narrow a resolved handler body to the text an EXPRESSION container (`(…)`)
/// can hold, by dropping trailing whitespace and statement terminators.
///
/// The resolved text is the value's source slice with binding prefixes spliced
/// in, so a terminator the author wrote (`@click="foo();"`) survives into the
/// output and would make `$event => (foo();)` unparseable. Trimming is an
/// EMISSION concern, not a classification one: whether the body needs a
/// statement container is answered solely by [`is_multi_statement_handler`].
pub fn trim_handler_body(resolved: &str) -> &str {
    resolved.trim_end().trim_end_matches(';').trim_end()
}

/// Whether `body` references the `$event` pseudo-identifier as a whole
/// word (not merely as a substring of a longer identifier).
pub(crate) fn references_event_param(body: &str) -> bool {
    const NEEDLE: &str = "$event";
    let bytes = body.as_bytes();
    let mut start = 0;
    while let Some(pos) = body[start..].find(NEEDLE) {
        let idx = start + pos;
        let before_ok = idx == 0
            || !(bytes[idx - 1].is_ascii_alphanumeric()
                || bytes[idx - 1] == b'_'
                || bytes[idx - 1] == b'$');
        let after = idx + NEEDLE.len();
        let after_ok =
            after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
        if before_ok && after_ok {
            return true;
        }
        start = idx + NEEDLE.len();
    }
    false
}

/// Quick check whether a string needs any JS string escaping.
///
/// Returns `true` if the string contains `\`, `"`, newline, carriage return,
/// tab, null, or the first byte of U+2028/U+2029 (0xE2).
#[inline]
pub fn needs_js_escaping(s: &str) -> bool {
    s.bytes()
        .any(|b| matches!(b, b'\\' | b'"' | b'\n' | b'\r' | b'\t' | b'\0' | 0xe2) || b < 0x20)
}

/// Append a JS-string-escaped version of `s` to `buf`.
///
/// Escapes: `\` → `\\`, `"` → `\"`, `\n` → `\\n`, `\r` → `\\r`, `\t` → `\\t`,
/// `\0` → `\\0`, U+2028 → `\\u2028`, U+2029 → `\\u2029`, ASCII control → `\\xHH`.
pub fn escape_js_string_into(buf: &mut String, s: &str) {
    escape_js_string_into_with_controls(buf, s, true);
}

/// The component-prop carrier preserves authored control bytes for legacy parity.
pub(crate) fn escape_js_string_into_with_controls(
    buf: &mut String,
    s: &str,
    escape_controls: bool,
) {
    // Fast path: ~90% of template strings need no escaping
    if !needs_js_escaping(s) {
        buf.push_str(s);
        return;
    }

    // Bulk-copy optimization: track start of unmodified region
    let bytes = s.as_bytes();
    let mut copy_from = 0;

    for (i, ch) in s.char_indices() {
        let escape = match ch {
            '\\' => "\\\\",
            '"' => "\\\"",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' if escape_controls => "\\t",
            '\0' if escape_controls => "\\0",
            '\u{2028}' if escape_controls => "\\u2028",
            '\u{2029}' if escape_controls => "\\u2029",
            c if escape_controls && c.is_ascii_control() => {
                // Flush pending unmodified region
                if copy_from < i {
                    buf.push_str(&s[copy_from..i]);
                }
                use std::fmt::Write;
                let _ = write!(buf, "\\x{:02x}", c as u32);
                copy_from = i + 1;
                continue;
            }
            _ => {
                continue;
            }
        };

        // Flush pending unmodified region, then push escape sequence
        if copy_from < i {
            buf.push_str(&s[copy_from..i]);
        }
        buf.push_str(escape);
        copy_from = i + ch.len_utf8();
    }

    // Flush remaining
    if copy_from < bytes.len() {
        buf.push_str(&s[copy_from..]);
    }
}

// ======================== JS key validation ========================

/// Whether a property key needs to be quoted in a JS object literal.
///
/// Bare identifiers (`class`, `onClick`) don't need quotes.
/// Keys with special characters need quotes: `"data-id"`, `"onUpdate:modelValue"`.
pub(crate) fn needs_quoted_key(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let bytes = key.as_bytes();
    let first = bytes[0];
    // Must start with letter, _, or $
    if !first.is_ascii_alphabetic() && first != b'_' && first != b'$' {
        return true;
    }
    // Rest must be alphanumeric, _, or $
    bytes[1..]
        .iter()
        .any(|&b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'$')
}

#[cfg(test)]
#[path = "vue_syntax_tests.rs"]
mod tests;

/// Decode template text before applying Vue's ASCII whitespace policy.
/// Preserved text normalizes CRLF; non-ASCII spaces remain significant.
pub(crate) fn normalize_template_text(
    content: &str,
    preserve_whitespace: bool,
) -> std::borrow::Cow<'_, str> {
    use crate::template::code_gen::shared::helpers;
    use std::borrow::Cow;
    let decoded = if helpers::has_html_entities(content) {
        let mut value = String::with_capacity(content.len());
        helpers::decode_html_entities_into(&mut value, content);
        Cow::Owned(value)
    } else {
        Cow::Borrowed(content)
    };
    if preserve_whitespace {
        if decoded.contains("\r\n") {
            Cow::Owned(decoded.replace("\r\n", "\n"))
        } else {
            decoded
        }
    } else if helpers::has_consecutive_ws(&decoded)
        || decoded.bytes().any(|b| matches!(b, b'\t' | b'\n' | b'\r'))
    {
        let mut value = String::with_capacity(decoded.len());
        helpers::condense_whitespace_into(&mut value, &decoded);
        Cow::Owned(value)
    } else {
        decoded
    }
}
