//! Architecture guards for the REFUSE-BY-DEFAULT Svelte client emission design.
//!
//! These static guards scan the Svelte client-path production source
//! (`crates/verter_compiler/src/svelte/runtime/*`) and FAIL if a banned pattern is
//! reintroduced. They are the durable defense against regressing to the
//! emit-by-default-with-a-denylist structure the structural refactor replaced:
//!
//! - `client_emitter_takes_narrow_plan_not_broad_ir` — the client emitter entry
//!   must consume the NARROW `ClientModulePlan`, NOT the broad `SvelteRuntimeIr`.
//!   A public emitter over broad IR would let a future broad-IR variant become
//!   emit-capable (emit-by-default).
//! - `no_emitting_wildcard_arm_in_client_emitter_or_classifier` — the default-deny
//!   classifier + the narrow plan builder must not contain a `_ => Ok(…)` /
//!   `_ => {}` ACCEPT arm. A default arm must REFUSE, never accept/emit.
//! - `no_verbatim_source_return_in_client_rewriting` — the client expression
//!   rewriter must never `return source.to_string()` / hand back verbatim source on
//!   a parse failure or an unsupported form. A refusal is a typed `Err`, never
//!   verbatim output.
//! - `bare_rune_classification_is_position_sensitive` — the unsupported-rune scan
//!   classifies bare runes by exact legal position, never an ungated define-rune
//!   skip.
//!
//! The strict finite element + static-attr allowlist guards:
//!
//! - `no_is_keyword_in_client_element_safety` — the client element-name safety check
//!   must use the pinned Svelte `RESERVED_WORDS` (`is_svelte_reserved_word`), NOT
//!   OXC's narrower `is_keyword`.
//! - `no_raw_element_tag_as_client_dom_var_stem` — the client emitter must derive
//!   every DOM var stem from `SupportedHtmlElement::var_stem()`, never the raw `tag`
//!   field of a `ClientNode::Element`.
//! - `no_client_serializer_silently_drops_an_accepted_static_attr` — the client
//!   static-attr classifier must route every static attr through the finite
//!   `SupportedStaticAttr::classify` allowlist and refuse a non-accepted name, never
//!   accept-a-name-then-let-the-serializer-drop-it (the `defaultValue` leak).
//!
//! Each guard pairs with a DISCRIMINATION self-test that proves it FAILS on the
//! banned pattern — exercised via an inline-string fixture, NEVER by editing
//! production source (a source-scanning guard self-test must not mutate the scanned
//! tree).

use std::path::PathBuf;

/// The Svelte runtime source directory (the client-emission path).
fn runtime_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/runtime")
}

/// Read a runtime-path source file.
fn read_runtime_file(name: &str) -> String {
    let path = runtime_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Strip `//` line comments + `/* … */` block comments from Rust source so a guard
/// scan keys on real code, not a banned-pattern token mentioned in a doc comment.
/// (Crude but sufficient: it does not need to be a full Rust lexer — it only needs
/// to drop comment text so a `// emit_client_module(&SvelteRuntimeIr)` mention in a
/// doc comment does not false-trip the guard.)
fn strip_comments(src: &str) -> String {
    let bytes: Vec<char> = src.chars().collect();
    let n = bytes.len();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        let next = if i + 1 < n { bytes[i + 1] } else { '\0' };
        if c == '/' && next == '/' {
            while i < n && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == '*' {
            i += 2;
            while i < n && !(bytes[i] == '*' && i + 1 < n && bytes[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // String literals are kept (a banned pattern inside a string is unusual and
        // a string-content false-positive is acceptable for these structural scans).
        out.push(c);
        i += 1;
    }
    out
}

// ── Guard 1: the client emitter consumes the narrow plan, not the broad IR ──────

/// The verdict predicate (shared by the guard + its discrimination self-test): does
/// `code` declare a PUBLIC client-emitter function whose signature takes the broad
/// `SvelteRuntimeIr`? A `pub fn emit_client_module(... ir: &SvelteRuntimeIr ...)`
/// (or `pub(super)`/`pub(crate)`) is the banned shape. The emitter entry must take
/// `&ClientModulePlan`.
fn declares_public_emitter_over_broad_ir(code: &str) -> bool {
    let stripped = strip_comments(code);
    // Find every `fn emit_client_module` declaration and inspect its signature
    // window for `SvelteRuntimeIr`. A `pub`/`pub(...)` visibility immediately
    // before the `fn` keyword marks it public.
    for (idx, _) in stripped.match_indices("fn emit_client_module") {
        // The visibility prefix: scan back over whitespace to the preceding token.
        let before = &stripped[..idx];
        let is_public = before.trim_end().ends_with("pub") || before.trim_end().ends_with(')'); // `pub(super)` / `pub(crate)`
        if !is_public {
            continue;
        }
        // The signature window: from the fn name to the opening brace.
        let after = &stripped[idx..];
        let sig_end = after.find('{').unwrap_or(after.len());
        let signature = &after[..sig_end];
        if signature.contains("SvelteRuntimeIr") {
            return true;
        }
    }
    false
}

#[test]
fn client_emitter_takes_narrow_plan_not_broad_ir() {
    let code = read_runtime_file("client.rs");
    assert!(
        !declares_public_emitter_over_broad_ir(&code),
        "GUARD: the client emitter (`emit_client_module`) must consume the NARROW \
         `ClientModulePlan`, NOT the broad `SvelteRuntimeIr`. A public emitter over \
         broad IR lets a future broad-IR variant become emit-capable \
         (emit-by-default). Build the narrow plan via `SupportedClientIr::build` and \
         emit from it."
    );
    // POSITIVE: the emitter DOES take the narrow plan (the guard is non-vacuous —
    // the real entry exists and is correctly shaped).
    assert!(
        code.contains("fn emit_client_module(plan: &ClientModulePlan")
            || code.contains("emit_client_module(\n    plan: &ClientModulePlan"),
        "the emitter entry must take `&ClientModulePlan` (the narrow plan):\n\
         (no `emit_client_module(plan: &ClientModulePlan` found)"
    );
}

#[test]
fn guard1_discriminates_a_broad_ir_emitter_signature() {
    // DISCRIMINATION: a fabricated public emitter over broad IR MUST trip the guard
    // predicate; the real narrow-plan signature MUST NOT. Exercised on inline
    // strings — production source is never edited.
    let banned =
        "pub fn emit_client_module(ir: &SvelteRuntimeIr, html: &Plan) -> ClientModule { todo!() }";
    assert!(
        declares_public_emitter_over_broad_ir(banned),
        "the guard predicate must FLAG a public emitter taking `&SvelteRuntimeIr`"
    );
    let banned_pub_super =
        "pub(super) fn emit_client_module(ir: &SvelteRuntimeIr) -> ClientModule { todo!() }";
    assert!(
        declares_public_emitter_over_broad_ir(banned_pub_super),
        "the guard predicate must FLAG a `pub(super)` emitter taking `&SvelteRuntimeIr`"
    );
    let ok = "pub(super) fn emit_client_module(plan: &ClientModulePlan, html: &Plan) -> ClientModule { todo!() }";
    assert!(
        !declares_public_emitter_over_broad_ir(ok),
        "the guard predicate must NOT flag the narrow-plan signature"
    );
    // A doc-comment MENTION of the banned shape must NOT trip the guard (comments
    // are stripped before the scan).
    let comment_only = "/// The old `pub fn emit_client_module(ir: &SvelteRuntimeIr)` is gone.\npub(super) fn emit_client_module(plan: &ClientModulePlan) -> ClientModule { todo!() }";
    assert!(
        !declares_public_emitter_over_broad_ir(comment_only),
        "a doc-comment mention of the banned signature must NOT trip the guard"
    );
}

// ── Guard 2: no emitting/accepting wildcard arm in classifier / plan builder ────

/// The CLASSIFICATION/PROJECTION functions whose `match` over a parsed node / attr
/// / op DECIDES whether a surface emits. An accepting wildcard arm in ONE of these
/// is the emit-by-default hole; a wildcard skip elsewhere (a binding-kind
/// refusal-scan loop that ignores the non-advanced kinds) is not an emission
/// decision and is not scanned.
const EMISSION_DECISION_FNS: &[&str] = &[
    "fn classify_node",
    "fn classify_attr",
    "fn project_node",
    "fn project_attr",
    "fn build_ops",
];

/// Extract the body of the function whose declaration starts at `marker` (a
/// `fn <name>` prefix), by brace-matching from the first `{` after the marker.
/// Returns the body text, or `None` when the marker is absent.
fn fn_body(code: &str, marker: &str) -> Option<String> {
    let start = code.find(marker)?;
    let after = &code[start..];
    let open = after.find('{')?;
    let bytes: Vec<char> = after[open..].chars().collect();
    let mut depth = 0i32;
    let mut end = 0usize;
    for (i, &c) in bytes.iter().enumerate() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    Some(bytes[..=end].iter().collect())
}

/// The verdict predicate: does `code` contain a `_ =>` wildcard arm that ACCEPTS
/// (returns `Ok(…)`) or silently passes (`_ => {}`) INSIDE an emission-decision
/// function? In the default-deny classifier + the narrow plan projection, a
/// wildcard arm in those functions MUST refuse (`_ => Err(…)`) — a `_ => Ok(…)` /
/// `_ => {}` accept arm is the emit-by-default hole.
fn has_accepting_wildcard_arm(code: &str) -> bool {
    let stripped = strip_comments(code);
    // Scan only the emission-decision function bodies present in this file.
    let bodies: Vec<String> = EMISSION_DECISION_FNS
        .iter()
        .filter_map(|marker| fn_body(&stripped, marker))
        .collect();
    // If a file declares NONE of the decision functions, scan the whole file (the
    // inline discrimination fixtures have no named fn — they must still be flagged).
    let scan_targets: Vec<&str> = if bodies.is_empty() {
        vec![&stripped]
    } else {
        bodies.iter().map(|s| s.as_str()).collect()
    };
    for body in scan_targets {
        for (idx, _) in body.match_indices("_ =>") {
            let after = body[idx + 4..].trim_start();
            // A refusing wildcard `_ => Err(…)` / `_ => return Err(…)` /
            // `_ => unreachable!(` / `_ => continue` is allowed.
            if after.starts_with("Ok(") {
                return true;
            }
            let no_ws: String = after.chars().filter(|c| !c.is_whitespace()).collect();
            if no_ws.starts_with("{}") {
                return true;
            }
        }
    }
    false
}

#[test]
fn no_emitting_wildcard_arm_in_client_emitter_or_classifier() {
    for file in [
        "client_surface.rs",
        "client_plan.rs",
        "client_plan_script.rs",
    ] {
        let code = read_runtime_file(file);
        assert!(
            !has_accepting_wildcard_arm(&code),
            "GUARD: the default-deny classifier / narrow plan builder ({file}) must \
             not contain a `_ => Ok(…)` / `_ => {{}}` ACCEPT (or silent-pass) wildcard \
             arm — a default arm must REFUSE (`_ => Err(…)`), never accept/emit. An \
             accepting wildcard is the emit-by-default hole the refactor closed."
        );
    }
}

#[test]
fn guard2_discriminates_an_accepting_wildcard_arm() {
    // DISCRIMINATION on inline strings.
    let banned_ok = "match node { IrNode::Text => Ok(()), _ => Ok(()) }";
    assert!(
        has_accepting_wildcard_arm(banned_ok),
        "the guard must FLAG a `_ => Ok(…)` accept arm"
    );
    let banned_noop = "match op { RuntimeOp::Event { .. } => emit(), _ => {} }";
    assert!(
        has_accepting_wildcard_arm(banned_noop),
        "the guard must FLAG a `_ => {{}}` silent-pass arm"
    );
    let ok_refuse = "match node { IrNode::Text => Ok(()), _ => Err(Unsupported::Block) }";
    assert!(
        !has_accepting_wildcard_arm(ok_refuse),
        "the guard must NOT flag a refusing `_ => Err(…)` arm"
    );
    let ok_return_err = "match node { IrNode::Text => {}, _ => return Err(surface) }";
    assert!(
        !has_accepting_wildcard_arm(ok_return_err),
        "the guard must NOT flag a `_ => return Err(…)` arm"
    );
}

// ── Guard 3: no verbatim source return in client expression rewriting ───────────

/// The verdict predicate: does `code` contain a `return source.to_string()` /
/// `source.to_string()` AS A REFUSAL FALLBACK in the rewriter? The fallible
/// rewriter must return a typed `Err` on a parse failure / unsupported form — never
/// the verbatim source. The banned pattern is a bare `source.to_string()` returned
/// as the expression result (the old verbatim fallback).
fn returns_verbatim_source(code: &str) -> bool {
    let stripped = strip_comments(code);
    // The banned verbatim-fallback shapes: `return source.to_string()` or a bare
    // `source.to_string()` as the trailing expression of a fallback. (The rewriter's
    // legitimate string production is `RewrittenExpr { text: … }` / `.map(|r| r.text)`,
    // never `source.to_string()`.)
    stripped.contains("return source.to_string()")
        || stripped.contains("source.to_string();")
        // `Some(source.to_string())` / `=> source.to_string()` verbatim fallbacks.
        || stripped.contains("=> source.to_string()")
}

#[test]
fn no_verbatim_source_return_in_client_rewriting() {
    let code = read_runtime_file("expr_emit.rs");
    assert!(
        !returns_verbatim_source(&code),
        "GUARD: the client expression rewriter (`expr_emit.rs`) must never \
         `return source.to_string()` / hand back verbatim source on a parse failure \
         or an unsupported form — a refusal is a typed `Err(UnsupportedSvelteRuntimeSurface)`, \
         never verbatim output (verbatim output is the emit-by-default hole — an \
         unsupported expression emitted raw produces a runtime error)."
    );
}

#[test]
fn guard3_discriminates_a_verbatim_source_return() {
    // DISCRIMINATION on inline strings.
    let banned_return = "if parsed.panicked { return source.to_string(); }";
    assert!(
        returns_verbatim_source(banned_return),
        "the guard must FLAG `return source.to_string()`"
    );
    let banned_arrow = "let body = match x { None => source.to_string(), Some(b) => b };";
    assert!(
        returns_verbatim_source(banned_arrow),
        "the guard must FLAG a `=> source.to_string()` verbatim fallback"
    );
    let ok_fallible = "if parsed.panicked { return Err(UnsupportedSvelteRuntimeSurface::DestructuringWrite { span }); }";
    assert!(
        !returns_verbatim_source(ok_fallible),
        "the guard must NOT flag the fallible `Err(…)` refusal"
    );
    let ok_rewritten = "Ok(RewrittenExpr { text: body.to_string() })";
    assert!(
        !returns_verbatim_source(ok_rewritten),
        "the guard must NOT flag the legitimate `RewrittenExpr {{ text: … }}` production"
    );
}

// ── Guard 4: bare-rune classification is POSITION-SENSITIVE ──────────────────────

/// The verdict predicate (shared by the guard + its discrimination self-test): does
/// `code` perform POSITION-SENSITIVE bare-rune classification? A bare `$state` /
/// `$derived` / `$props` / `$effect` reference is supported ONLY in its exact legal
/// position; the scan must gate it on a precomputed supported-position set and
/// refuse everything else. The required machinery: a `classify_rune_position`
/// method (the per-identifier position gate), a `supported` position set it
/// consults (`self.supported.contains`), and a `visit_identifier_reference` that
/// drives the position check. The banned shape — a scan that unconditionally
/// EXEMPTS the four define-runes at the reference level "because they carry their
/// own emission", with NO position gate — lacks this machinery.
fn classifies_bare_runes_position_sensitively(code: &str) -> bool {
    let stripped = strip_comments(code);
    // The position gate must (a) be defined, (b) consult a supported-position set,
    // and (c) be wired into the identifier-reference visit.
    let has_position_classifier = stripped.contains("fn classify_rune_position");
    let consults_supported_set =
        stripped.contains("self.supported.contains") || stripped.contains("supported.contains(&(");
    let wired_into_identifier_visit = stripped.contains("fn visit_identifier_reference")
        && stripped.contains("self.classify_rune_position(");
    has_position_classifier && consults_supported_set && wired_into_identifier_visit
}

#[test]
fn bare_rune_classification_is_position_sensitive() {
    let code = read_runtime_file("rune_scan.rs");
    assert!(
        classifies_bare_runes_position_sensitively(&code),
        "GUARD: the unsupported-rune scan (`rune_scan.rs`) must classify bare \
         `$state` / `$derived` / `$props` / `$effect` references POSITION-SENSITIVELY \
         — a bare rune is supported ONLY in its exact legal position (a top-level \
         instance-script declarator init / statement), and refused everywhere else \
         (a default-param, a call-arg, a module-script rune, a bare-identifier \
         reference). The scan must gate each rune-root identifier on a precomputed \
         supported-position set (`classify_rune_position` + `self.supported.contains`, \
         wired into `visit_identifier_reference`). An unconditional skip of the four \
         define-runes (\"they carry their own emission\") is an emit-by-default hole."
    );
}

#[test]
fn guard4_discriminates_an_ungated_bare_rune_skip() {
    // DISCRIMINATION on inline strings: the ungated-skip shape (an unconditional
    // exemption of the four define-runes with NO position gate) must NOT satisfy
    // the predicate; the position-sensitive shape MUST.
    let banned_ungated = "fn visit_call_expression(&mut self, it: &CallExpression) { \
        if self.is_unshadowed_rune(root) && !matches!(root, \"$state\" | \"$derived\" | \"$props\" | \"$effect\") { \
        self.classify_rune_call(root, span); } }";
    assert!(
        !classifies_bare_runes_position_sensitively(banned_ungated),
        "the guard must NOT consider an ungated define-rune skip position-sensitive"
    );
    let ok_position_sensitive = "fn classify_rune_position(&mut self, root: &str, span: Span) { \
        if self.supported.contains(&(span.start, span.end)) { return; } self.record(surface); } \
        fn visit_identifier_reference(&mut self, it: &IdentifierReference) { \
        if self.is_unshadowed_rune(name) { self.classify_rune_position(name, to_span(it.span)); } }";
    assert!(
        classifies_bare_runes_position_sensitively(ok_position_sensitive),
        "the guard must recognize the position-sensitive classifier shape"
    );
}

// ── Guard 5: no `is_keyword` in the client element-safety path ───────────────────

/// The verdict predicate: does `code` reference OXC's `is_keyword` as the
/// client element-name safety authority? The strict element-name check MUST use the
/// pinned Svelte `RESERVED_WORDS` (`is_svelte_reserved_word`), NOT OXC's narrower
/// `is_keyword` (which omits `arguments` / `eval` / `implements` / `interface` /
/// `package` / `private` / `protected` / `public`). A bare `is_keyword(` CALL in the
/// classifier source is the banned shape (comments are stripped first, so the
/// rationale comment naming `is_keyword` does not false-trip).
fn references_is_keyword(code: &str) -> bool {
    let stripped = strip_comments(code);
    stripped.contains("is_keyword(")
}

#[test]
fn no_is_keyword_in_client_element_safety() {
    let code = read_runtime_file("client_surface.rs");
    assert!(
        !references_is_keyword(&code),
        "GUARD: the client element-name safety check (`client_surface.rs`) must use the \
         pinned Svelte `RESERVED_WORDS` (`is_svelte_reserved_word`), NOT OXC's narrower \
         `is_keyword`. `is_keyword` omits `arguments` / `eval` / `implements` / \
         `interface` / `package` / `private` / `protected` / `public`, so a tag like \
         `<interface>` would pass an `is_keyword` gate yet still synthesize an invalid \
         JS binding. Route the reserved-word check through `is_svelte_reserved_word`."
    );
    // POSITIVE (non-vacuity): the classifier DOES use the Svelte reserved-word
    // authority (so the guard guards a real, correctly-shaped path).
    assert!(
        code.contains("is_svelte_reserved_word"),
        "the client element classifier must use `is_svelte_reserved_word` (the strict \
         Svelte `RESERVED_WORDS` authority):\n(no `is_svelte_reserved_word` found)"
    );
}

#[test]
fn guard5_discriminates_an_is_keyword_element_check() {
    // DISCRIMINATION on inline strings.
    let banned =
        "if !crate::utils::oxc::bindings::keywords::is_keyword(tag.as_bytes()) { accept(); }";
    assert!(
        references_is_keyword(banned),
        "the guard must FLAG an `is_keyword(` call in the element-safety path"
    );
    let ok = "if is_svelte_reserved_word(&el.tag) { return Err(reserved); }";
    assert!(
        !references_is_keyword(ok),
        "the guard must NOT flag the `is_svelte_reserved_word` path"
    );
    // A doc-comment MENTION of `is_keyword` must NOT trip the guard (comments are
    // stripped before the scan) — this is exactly the rationale comment the real
    // classifier carries.
    let comment_only = "// uses RESERVED_WORDS not OXC's narrower `is_keyword`\nif is_svelte_reserved_word(&el.tag) { return Err(reserved); }";
    assert!(
        !references_is_keyword(comment_only),
        "a doc-comment mention of `is_keyword` must NOT trip the guard"
    );
}

// ── Guard 6: no raw element tag as a client DOM variable stem ─────────────────────

/// The verdict predicate: does `code` use a raw element TAG STRING as a client DOM
/// variable stem? The DOM clone-root / walk var stem MUST come from the typed
/// `SupportedHtmlElement::var_stem()`, NEVER the raw `tag` field of a
/// `ClientNode::Element`. The banned shapes are an `alloc_name(tag)` /
/// `alloc_name(&tag)` allocation, or a `ClientNode::Element { tag, .. } => tag.clone()`
/// var-stem projection (the two pre-restructure sites). The sanctioned shape allocates
/// `element.var_stem()`.
fn uses_raw_tag_as_var_stem(code: &str) -> bool {
    let stripped = strip_comments(code);
    // The collapsed (whitespace-free) view catches a line-wrapped `alloc_name(\n tag)`.
    let collapsed: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
    // `alloc_name(tag)` / `alloc_name(&tag)` — a raw tag allocated as a var stem.
    if collapsed.contains("alloc_name(tag)") || collapsed.contains("alloc_name(&tag)") {
        return true;
    }
    // `ClientNode::Element { tag, .. } => tag.clone()` / `=> tag.to_string()` — a
    // var-stem projection that hands back the raw tag.
    if collapsed.contains("ClientNode::Element{tag,..}=>tag.clone()")
        || collapsed.contains("ClientNode::Element{tag,..}=>tag.to_string()")
    {
        return true;
    }
    false
}

#[test]
fn no_raw_element_tag_as_client_dom_var_stem() {
    let code = read_runtime_file("client.rs");
    assert!(
        !uses_raw_tag_as_var_stem(&code),
        "GUARD: the client emitter (`client.rs`) must derive every DOM clone-root / walk \
         var STEM from the typed `SupportedHtmlElement::var_stem()`, NEVER the raw `tag` \
         field of a `ClientNode::Element`. A raw tag stem (`alloc_name(tag)` / \
         `ClientNode::Element {{ tag, .. }} => tag.clone()`) can synthesize an invalid / \
         reserved JS local (`var var = …`); the typed stem is identifier-safe by \
         construction."
    );
    // POSITIVE (non-vacuity): the emitter DOES use the typed var stem.
    assert!(
        code.contains("element.var_stem()"),
        "the client emitter must allocate the DOM var stem from `element.var_stem()`:\n\
         (no `element.var_stem()` found)"
    );
}

#[test]
fn guard6_discriminates_a_raw_tag_var_stem() {
    // DISCRIMINATION on inline strings.
    let banned_alloc =
        "if let ClientNode::Element { tag, .. } = node { return self.alloc_name(tag); }";
    assert!(
        uses_raw_tag_as_var_stem(banned_alloc),
        "the guard must FLAG an `alloc_name(tag)` raw-tag stem"
    );
    let banned_projection =
        "match node { ClientNode::Element { tag, .. } => tag.clone(), _ => \"node\".to_string() }";
    assert!(
        uses_raw_tag_as_var_stem(banned_projection),
        "the guard must FLAG a `ClientNode::Element {{ tag, .. }} => tag.clone()` stem"
    );
    let ok_typed = "match node { ClientNode::Element { element, .. } => element.var_stem().to_string(), _ => \"node\".to_string() }";
    assert!(
        !uses_raw_tag_as_var_stem(ok_typed),
        "the guard must NOT flag the typed `element.var_stem()` stem"
    );
    // Using the tag for a non-stem purpose (`for_children_of(tag)`) is allowed.
    let ok_namespace = "let child_ctx = ctx.for_children_of(tag);";
    assert!(
        !uses_raw_tag_as_var_stem(ok_namespace),
        "the guard must NOT flag a non-stem tag use (`for_children_of(tag)`)"
    );
}

// ── Guard 7: no client serializer path silently drops an accepted static attr ─────

/// The verdict predicate: does the client static-attr classifier route EVERY static
/// attribute through the finite `SupportedStaticAttr::classify` acceptance authority,
/// and REFUSE a non-accepted name (rather than accept-a-name-then-let-the-serializer-
/// drop-it)? The required shape in `classify_attr`'s `AttrIr::Static` arm: a
/// `SupportedStaticAttr::classify(` call whose `None` result REFUSES (`Err(`). The
/// banned shape — the pre-restructure `AttrIr::Static { name, .. } => { … Ok(()) }`
/// that accepts a name without proving the serializer keeps it (the `defaultValue`
/// leak: accepted at classification, dropped at serialization) — lacks the
/// `SupportedStaticAttr::classify` gate.
fn static_attr_acceptance_routes_through_finite_allowlist(code: &str) -> bool {
    let stripped = strip_comments(code);
    // (a) The static-attr arm consults the finite allowlist authority, and (b) a
    // non-accepted name refuses (`is_none()` → `return Err(`). The collapsed view
    // tolerates line-wrapping between the `is_none()` check and the `Err`.
    let calls_classify = stripped.contains("SupportedStaticAttr::classify(");
    let collapsed: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
    let refuses_on_none = collapsed.contains(".is_none(){returnErr(")
        || collapsed.contains(".is_none(){returnErr(UnsupportedSvelteRuntimeSurface");
    calls_classify && refuses_on_none
}

#[test]
fn no_client_serializer_silently_drops_an_accepted_static_attr() {
    let code = read_runtime_file("client_surface.rs");
    assert!(
        static_attr_acceptance_routes_through_finite_allowlist(&code),
        "GUARD: the client static-attr classifier (`client_surface.rs`) must route EVERY \
         static attribute through the finite `SupportedStaticAttr::classify` allowlist \
         and REFUSE (`Err`) a non-accepted name — NOT accept a name and let a downstream \
         serializer (`html.rs::serialize_static_attrs`) independently drop it. An \
         accept-then-drop split is exactly the `defaultValue` leak (accepted at \
         classification, dropped at serialization → a divergent skeleton). Every \
         accepted `SupportedStaticAttr` is serializable by construction (none is in \
         `cannot_be_set_statically`, an empty `class`, or a custom-element non-`is` \
         attr — all rejected before emission)."
    );
}

#[test]
fn guard7_discriminates_an_accept_then_drop_static_attr_arm() {
    // DISCRIMINATION on inline strings: the accept-then-drop shape (no finite-allowlist
    // gate) must NOT satisfy the predicate; the route-through-allowlist shape MUST.
    let banned_accept_then_drop =
        "AttrIr::Static { name, .. } => { if is_custom && name != \"is\" { return Err(host); } Ok(()) }";
    assert!(
        !static_attr_acceptance_routes_through_finite_allowlist(banned_accept_then_drop),
        "the guard must NOT consider an accept-then-drop static-attr arm safe"
    );
    let ok_routed = "AttrIr::Static { name, value } => { let literal = value.as_ref().map(|v| v.value.as_str()); if SupportedStaticAttr::classify(name, element, literal).is_none() { return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute { name: name.clone(), span: el_span }); } Ok(()) }";
    assert!(
        static_attr_acceptance_routes_through_finite_allowlist(ok_routed),
        "the guard must recognize the route-through-finite-allowlist shape"
    );
}

// ── Guard: the D-47 import-local discharge — no raw ImportDeclaration.specifiers walk ──

/// The verdict predicate (shared by the guard + its discrimination self-test):
/// does `code` (comments stripped) re-walk the raw OXC import node to collect
/// import LOCALS? The D-47 discharge routes every import LOCAL through the SHARED
/// `ClassifiedScriptImports` carrier (`script_imports.admitted(slot)` +
/// `import_binding_entries(import)`), classified ONCE at IR construction — never a
/// per-file re-walk of the OXC `Statement::ImportDeclaration` node. Import
/// bindings are reachable from the raw node by exactly TWO shapes, both banned:
/// (1) DIRECT field access — the locals live ONLY in `ImportDeclaration.specifiers`
/// (`ImportDeclarationSpecifier::{Import,ImportDefault,ImportNamespace}.local`), so
/// the `specifiers` token signals a direct walk; (2) DRIVING the OXC visitor over
/// the import node — `Visit::visit_import_declaration` / `walk_import_declaration`
/// internally walk `.specifiers` and fire `visit_binding_identifier` on each
/// `local`, collecting the locals WITHOUT the caller ever spelling `specifiers`,
/// so those visitor-entry tokens are banned too. Merely NAMING the
/// `Statement::ImportDeclaration` variant is NOT the defect: the exhaustive,
/// no-soft-wildcard scope-view erasure classifier (`statement_is_scope_erased`)
/// legitimately matches that arm to read `import_kind.is_type()` — a
/// statement-level erasure decision that touches no `specifiers` and drives no
/// import visitor, so it stays free (banning the bare variant name would
/// false-positive on it). As a SECONDARY substring tripwire this cannot catch a
/// walk hidden behind a cross-file helper; a PRIMARY AST-aware D-47 guard is a
/// tracked debt-ledger follow-up, out of scope here.
fn contains_raw_import_local_walk(code: &str) -> bool {
    let stripped = strip_comments(code);
    stripped.contains("specifiers")
        || stripped.contains("visit_import_declaration")
        || stripped.contains("walk_import_declaration")
}

#[test]
fn no_raw_import_specifier_walk_in_import_local_discharge_files() {
    // The two files whose D-47 discharge reads import LOCALS from the shared
    // `ClassifiedScriptImports` carrier — neither may re-walk the raw OXC import
    // node's specifiers (a second import-classification path is the D-47 defect).
    // The canonical component-scope binder (`component_scope_facts.rs`) owns the
    // reactive/unsafe-root import-local half through the same carrier; the reactive
    // analysis no longer discharges import locals directly.
    for name in ["needs_context.rs", "component_scope_facts.rs"] {
        let code = read_runtime_file(name);
        assert!(
            !contains_raw_import_local_walk(&code),
            "GUARD (D-47 discharge): `{name}` must NOT re-walk the raw OXC import \
             node's `.specifiers` to collect import LOCALS — the import-local half of \
             the unsafe-root / reactive-root set comes from the shared \
             `ClassifiedScriptImports` carrier (`script_imports.admitted(slot)` + \
             `import_binding_entries(import)`), classified ONCE at IR construction. A \
             raw specifier re-walk is a second import-classification path (the exact \
             D-47 dual-path defect); route through the carrier instead. (Naming the \
             `Statement::ImportDeclaration` variant to read `import_kind` for \
             statement-level scope erasure is NOT the defect — it collects no locals.)"
        );
    }
    // POSITIVE (non-vacuous): both files DO consume the shared carrier — the guard
    // is guarding a LIVE discharge, not an absent one.
    for name in ["needs_context.rs", "component_scope_facts.rs"] {
        let code = read_runtime_file(name);
        assert!(
            code.contains("ClassifiedScriptImports"),
            "{name} must consume the shared `ClassifiedScriptImports` carrier (the D-47 \
             discharge); the guard would be vacuous if the carrier route were gone"
        );
    }
}

#[test]
fn import_local_discharge_guard_discriminates_a_restored_specifier_walk() {
    // DISCRIMINATION on inline strings: a restored raw `ImportDeclaration`
    // specifier walk MUST trip the predicate; the shared-carrier route MUST NOT.
    let restored_walk = "if let Statement::ImportDeclaration(decl) = stmt { for spec in decl.specifiers.iter() { unsafe_roots.insert(spec.local.name.to_string()); } }";
    assert!(
        contains_raw_import_local_walk(restored_walk),
        "the guard must FLAG a restored raw `ImportDeclaration`/`.specifiers` import-local walk"
    );
    let carrier_route = "for import in script_imports.admitted(slot) { for (local, _kind) in import_binding_entries(import) { unsafe_roots.insert(local.to_string()); } }";
    assert!(
        !contains_raw_import_local_walk(carrier_route),
        "the guard must NOT flag the shared-carrier import-local route"
    );
    // The comment-strip discrimination: a banned token appearing ONLY in a doc
    // comment (`/// … ImportDeclaration … .specifiers …`) must NOT trip the guard —
    // it keys on real code, so the existing doc prose stays green.
    let comment_only =
        "// never an independent raw-AST ImportDeclaration re-walk of .specifiers\nlet x = 1;";
    assert!(
        !contains_raw_import_local_walk(comment_only),
        "the guard must ignore a banned token that appears ONLY in a comment"
    );
    // The legitimate exhaustive-match erasure arm names the `Statement::ImportDeclaration`
    // variant to read `import_kind` (a statement-level scope-erasure decision) but walks
    // NO `.specifiers` and collects NO locals — it MUST NOT trip. This is the exact
    // false-positive the `specifiers`-keyed predicate fixes: keying on the bare variant
    // name would wrongly flag this arm, which the `statement_is_scope_erased` classifier
    // in `component_scope_facts.rs` genuinely contains.
    let erasure_kind_check = "Statement::ImportDeclaration(i) => i.import_kind.is_type(),";
    assert!(
        !contains_raw_import_local_walk(erasure_kind_check),
        "the guard must NOT flag the statement-level `import_kind` scope-erasure arm \
         (it names the variant but walks no `.specifiers` and collects no locals)"
    );
    // A VISITOR-driven raw import-local walk collects the locals by DRIVING OXC's
    // `visit_import_declaration` over the import node (the generated walk fires
    // `visit_binding_identifier` on each specifier `local`) WITHOUT the caller ever
    // spelling `specifiers` — it MUST still trip. A `specifiers`-only predicate would
    // MISS this (the exact D-47 coverage gap this strengthening closes).
    let visitor_walk = "if let Statement::ImportDeclaration(decl) = stmt { \
         let mut collector = ImportLocalCollector::default(); \
         collector.visit_import_declaration(decl); \
         unsafe_roots.extend(collector.locals); }";
    assert!(
        contains_raw_import_local_walk(visitor_walk),
        "the guard must FLAG a visitor-driven raw import-local walk \
         (`visit_import_declaration`) that never spells `specifiers`"
    );
    assert!(
        !visitor_walk.contains("specifiers"),
        "sanity: the visitor-walk fixture must NOT contain the `specifiers` token \
         (else it would not exercise the visitor-path coverage gap)"
    );
}

// ── Guard: the compile-options resolver has a SINGLE production call site ────────

/// Count the CALL sites of `resolve_svelte_compile_options(` in `code` — occurrences
/// of the name immediately followed by `(` that are NOT the `fn` DEFINITION. Comments
/// are stripped first so a doc-comment mention never counts.
fn resolver_call_sites(code: &str) -> usize {
    let stripped = strip_comments(code);
    let needle = "resolve_svelte_compile_options(";
    stripped
        .match_indices(needle)
        .filter(|(idx, _)| {
            // Exclude the `fn resolve_svelte_compile_options(` definition.
            !stripped[..*idx].trim_end().ends_with("fn")
        })
        .count()
}

/// Every PRODUCTION runtime source file (a `.rs` under `src/svelte/runtime` that is
/// NOT a `*_tests.rs` test module).
fn production_runtime_files() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(runtime_dir())
        .expect("read runtime dir")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            (name.ends_with(".rs") && !name.ends_with("_tests.rs")).then_some(name)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn resolve_svelte_compile_options_has_single_production_call_site() {
    // GUARD: the compile-options fold is a SINGLE entry — exactly ONE production call
    // to `resolve_svelte_compile_options`, in `client_compile.rs` (the pipeline
    // driver). A second call site would be a divergent fold path (two resolvers
    // disagree on precedence / defaults / the fail-closed set).
    let mut total = 0usize;
    for name in production_runtime_files() {
        let count = resolver_call_sites(&read_runtime_file(&name));
        if count > 0 {
            assert_eq!(
                name, "client_compile.rs",
                "GUARD: `resolve_svelte_compile_options` is called from {name} — the fold \
                 must have a SINGLE production call site (`client_compile.rs`)."
            );
        }
        total += count;
    }
    assert_eq!(
        total, 1,
        "GUARD: expected EXACTLY one production call site of \
         `resolve_svelte_compile_options` (in `client_compile.rs`), found {total}."
    );
}

#[test]
fn resolver_single_entry_guard_discriminates_a_second_call_site() {
    // DISCRIMINATION on inline strings: the predicate counts CALL sites, excludes the
    // `fn` definition, and ignores comment mentions.
    let definition =
        "pub fn resolve_svelte_compile_options(source: &str) -> Result<T, E> { todo!() }";
    assert_eq!(
        resolver_call_sites(definition),
        0,
        "the definition site must NOT count as a call"
    );
    let one_call = "let resolved = resolve_svelte_compile_options(source, parsed, opts)?;";
    assert_eq!(
        resolver_call_sites(one_call),
        1,
        "a single call counts once"
    );
    let two_calls =
        "resolve_svelte_compile_options(a, b, c); foo(); resolve_svelte_compile_options(d, e, f);";
    assert_eq!(
        resolver_call_sites(two_calls),
        2,
        "a second call site must be counted (the banned shape)"
    );
    let comment_only = "// the old resolve_svelte_compile_options(x) call is gone\nlet x = 1;";
    assert_eq!(
        resolver_call_sites(comment_only),
        0,
        "a comment mention must NOT count as a call"
    );
}
