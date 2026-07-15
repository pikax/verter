use super::*;

#[test]
fn auto_close_div() {
    let source = "<template><div></div></template>";
    // Cursor right after `<div>` at offset 15
    let result = auto_close_tag(source, 15);
    // Already has </div> immediately after
    assert!(
        result.is_none(),
        "should not close when </div> already exists"
    );
}

#[test]
fn auto_close_div_no_existing() {
    let source = "<template><div>\n</template>";
    let result = auto_close_tag(source, 15);
    assert_eq!(result, Some("$0</div>".to_string()));
}

/// HTML tag names are case-insensitive: an opening `<DIV>` already followed by a
/// differently-cased `</div>` is CLOSED, so the raw helper must NOT insert a second close. Fails
/// if the already-closed guard compares the `</tag` prefix case-sensitively.
#[test]
fn auto_close_raw_uppercase_open_already_closed_lowercase_is_none() {
    // `<DIV>` ends at offset 5; `</div>` follows immediately (different case).
    let source = "<DIV></div>";
    let result = auto_close_tag(source, 5);
    assert!(
        result.is_none(),
        "an already-closed (case-insensitively) tag must NOT be re-closed, got {result:?}"
    );
}

/// A following `</diverse>` shares the `</div` prefix but is a DIFFERENT, longer tag — it does
/// NOT already-close `<div>`, so the raw helper must still emit the close. Fails if the
/// already-closed guard treats `</div` as a prefix without requiring a tag-name terminator.
#[test]
fn auto_close_raw_longer_tag_sharing_prefix_still_closes() {
    let source = "<div></diverse>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag(source, off),
        Some("$0</div>".to_string()),
        "`</diverse>` does not close `<div>`; the close must still be emitted",
    );
}

/// Positive control: `<DIV>` with NO following close still auto-closes, preserving the typed
/// case in the inserted tag — proving the case-insensitive already-closed guard did not disable
/// the normal insertion.
#[test]
fn auto_close_raw_uppercase_open_unclosed_inserts_same_case() {
    let source = "<DIV>";
    let result = auto_close_tag(source, 5);
    assert_eq!(result, Some("$0</DIV>".to_string()));
}

#[test]
fn no_close_for_void_element() {
    let source = "<template><br></template>";
    let result = auto_close_tag(source, 14);
    assert!(result.is_none(), "void elements should not be closed");
}

#[test]
fn no_close_for_self_closing() {
    let source = "<template><MyComp /></template>";
    // Offset after `/>` — but `>` is at pos 19, so offset is 20
    let result = auto_close_tag(source, 20);
    assert!(result.is_none(), "self-closing tags should not be closed");
}

#[test]
fn auto_close_component() {
    let source = "<template><MyComponent>\n</template>";
    let result = auto_close_tag(source, 23);
    assert_eq!(result, Some("$0</MyComponent>".to_string()));
}

#[test]
fn auto_close_with_attributes() {
    let source = r#"<template><div class="foo" id="bar">"#;
    let result = auto_close_tag(source, 36);
    assert_eq!(result, Some("$0</div>".to_string()));
}

#[test]
fn no_close_for_closing_tag() {
    let source = "<template></div></template>";
    // Cursor after `</div>` at offset 16
    let result = auto_close_tag(source, 16);
    assert!(
        result.is_none(),
        "closing tags should not trigger auto-close"
    );
}

#[test]
fn no_close_for_comment() {
    let source = "<template><!-- comment --></template>";
    // This is `-->` so `>` at offset 25
    let result = auto_close_tag(source, 26);
    assert!(result.is_none(), "comments should not trigger auto-close");
}

#[test]
fn auto_close_template_tag() {
    let source = "<template>\n</template>";
    // Offset after first `<template>`
    let result = auto_close_tag(source, 10);
    // Already has </template> right after (with newline)
    assert!(
        result.is_none(),
        "should not close when </template> already exists after whitespace"
    );
}

#[test]
fn auto_close_preserves_case() {
    let source = "<template><MyButton>\n</template>";
    let result = auto_close_tag(source, 20);
    assert_eq!(
        result,
        Some("$0</MyButton>".to_string()),
        "should preserve original tag case"
    );
}

#[test]
fn no_close_for_void_input() {
    let source = r#"<template><input type="text"></template>"#;
    let result = auto_close_tag(source, 29);
    assert!(result.is_none(), "input is a void element");
}

// ========================================================================
// Carrier markup-context gate (BLOCKER 1 + 2)
//
// `auto_close_tag_in_carrier` is the gated entry point the on-type handler
// calls. It must fire ONLY in the TEMPLATE/MARKUP region of the carrier:
//   * Vue  — inside `<template>` content (NOT `<script>` / `<style>`).
//   * Svelte — at the root markup (NOT inside `<script>` / `<style>`).
// and within that region only for a real markup open tag — never for a
// TS-generic `>` (mustache expression) or a `>` inside a quoted attribute.
// ========================================================================

/// Byte offset of the position immediately AFTER `needle` in `source`.
fn after(source: &str, needle: &str) -> usize {
    source.find(needle).expect("needle present") + needle.len()
}

// ── Vue carrier ────────────────────────────────────────────────────────

#[test]
fn vue_template_closes_div() {
    let source = "<template><div>\n</template>\n<script>const x = 1;</script>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a `>` in the Vue template region must auto-close",
    );
}

/// HTML tag names are case-insensitive, so a `<DIV>` already followed by `</div>` (different
/// case) is ALREADY CLOSED — auto-close must not insert a duplicate `</DIV>`. Fails if the
/// already-closed guard compares the `</tag` prefix case-sensitively.
#[test]
fn already_closed_check_is_case_insensitive() {
    let source = "<template><DIV></div></template>";
    let off = after(source, "<DIV>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `<DIV>` already closed by `</div>` (different case) must NOT insert a duplicate close",
    );

    // Positive control: a `<DIV>` with NO following close still auto-closes (case preserved),
    // proving the case-insensitive guard did not over-fire.
    let open_only = "<template><DIV>\n</template>";
    let off = after(open_only, "<DIV>");
    assert_eq!(
        auto_close_tag_in_carrier(open_only, off, CarrierKind::Vue),
        Some("$0</DIV>".to_string()),
        "an unclosed `<DIV>` must still auto-close, preserving the original case",
    );
}

/// A following `</diverse>` shares the `</div` prefix but is a DIFFERENT, longer tag — it does
/// NOT already-close `<div>`, so the carrier helper must still emit the close. Fails if the
/// already-closed guard treats `</div` as a prefix without requiring a tag-name terminator.
#[test]
fn already_closed_check_requires_tag_name_boundary() {
    let source = "<template><div></diverse></template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "`</diverse>` does not close `<div>`; the close must still be emitted",
    );
}

#[test]
fn vue_script_generic_does_not_close() {
    // The `>` here closes a TS generic `Box<Foo>` inside `<script lang="ts">`.
    // It is NOT markup and must NOT insert `</Foo>` — BLOCKER 1/2.
    let source = "<template><div></div></template>\n<script lang=\"ts\">\nconst x: Box<Foo> = mk();\n</script>";
    let off = after(source, "Box<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a TS generic `>` inside <script lang=ts> must never auto-close",
    );
}

#[test]
fn vue_style_gt_does_not_close() {
    // A `>` child combinator inside `<style>` is CSS, not markup.
    let source = "<template><div></div></template>\n<style>\n.a > .b { color: red }\n</style>";
    let off = after(source, ".a >");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `>` inside <style> (CSS combinator) must never auto-close",
    );
}

#[test]
fn vue_attribute_value_gt_does_not_close() {
    // The `>` is INSIDE a quoted attribute value, not the tag-closing `>`.
    let source = "<template><div title=\"a>b\">\n</template>";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `>` inside a quoted attribute value must never auto-close",
    );
}

#[test]
fn vue_mustache_generic_does_not_close() {
    // The `>` closes a TS generic inside a `{{ }}` interpolation expression.
    let source = "<template><div>{{ mk<Foo>() }}</div>\n</template>";
    let off = after(source, "mk<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a TS generic `>` inside a `{{ }}` interpolation must never auto-close",
    );
}

#[test]
fn vue_template_real_tag_after_mustache_still_closes() {
    // Discriminator: a real markup `>` AFTER a closed mustache still closes —
    // proves the mustache guard does not over-reject the whole template.
    let source = "<template>{{ a }}<section>\n</template>";
    let off = after(source, "<section>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</section>".to_string()),
        "a real open tag after a closed mustache must still auto-close",
    );
}

#[test]
fn vue_root_level_gt_does_not_close() {
    // Between blocks (root level of the SFC) is not markup for Vue.
    let source = "<template><div></div></template>\n<Foo>\n<script></script>";
    let off = after(source, "<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "Vue markup is only inside <template>; root-level tags must not close",
    );
}

#[test]
fn vue_template_void_still_not_closed() {
    // Void-element behavior is preserved through the gate.
    let source = "<template><br>\n</template>";
    let off = after(source, "<br>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "void elements stay un-closed inside the template region",
    );
}

#[test]
fn vue_template_quoted_attr_tag_still_closes() {
    // Discriminator: the attribute-value guard must NOT over-reject a normal
    // tag whose closing `>` follows quoted attribute values.
    let source = "<template><div class=\"foo\" id=\"bar\">\n</template>";
    let off = after(source, "id=\"bar\">");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a tag with quoted attributes must still auto-close on its real `>`",
    );
}

#[test]
fn vue_template_control_closes_but_void_sibling_does_not() {
    // Mirrors the e2e readiness pattern: a positive control `<article>` in
    // the same template closes, while the void `<br>` sibling does not — so
    // the e2e's "ready + correctly no edit" distinction is grounded here too.
    let source = "<template><article><br>\n</template>";
    assert_eq!(
        auto_close_tag_in_carrier(source, after(source, "<article>"), CarrierKind::Vue),
        Some("$0</article>".to_string()),
        "the control tag must close (proves readiness in the e2e)",
    );
    assert_eq!(
        auto_close_tag_in_carrier(source, after(source, "<br>"), CarrierKind::Vue),
        None,
        "the void sibling must not close even when a control nearby does",
    );
}

#[test]
fn vue_template_self_closing_still_not_closed() {
    let source = "<template><MyComp />\n</template>";
    let off = after(source, "<MyComp />");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "self-closing tags stay un-closed inside the template region",
    );
}

#[test]
fn vue_template_existing_close_not_doubled() {
    let source = "<template><div></div></template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "an already-closed tag must not be doubled",
    );
}

// ── Svelte carrier ─────────────────────────────────────────────────────

#[test]
fn svelte_root_markup_closes_section() {
    // Svelte markup lives at the root (NO <template> wrapper).
    let source = "<script lang=\"ts\">let n = 1;</script>\n<section>\n<p>hi</p>";
    let off = after(source, "<section>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        Some("$0</section>".to_string()),
        "a `>` in Svelte root markup must auto-close",
    );
}

#[test]
fn svelte_script_generic_does_not_close() {
    let source = "<script lang=\"ts\">\nconst x: Box<Foo> = mk();\n</script>\n<div></div>";
    let off = after(source, "Box<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a TS generic `>` inside a Svelte <script> must never auto-close",
    );
}

#[test]
fn svelte_style_gt_does_not_close() {
    let source = "<div></div>\n<style>\n.a > .b { color: red }\n</style>";
    let off = after(source, ".a >");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `>` inside a Svelte <style> (CSS combinator) must never auto-close",
    );
}

#[test]
fn svelte_attribute_value_gt_does_not_close() {
    let source = "<div title=\"a>b\">\n<p>hi</p>";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `>` inside a quoted attribute value must never auto-close (svelte)",
    );
}

#[test]
fn svelte_void_still_not_closed() {
    let source = "<br>\n<p>hi</p>";
    let off = after(source, "<br>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "void elements stay un-closed in Svelte root markup",
    );
}

// ── Svelte single-brace expression region (F2 / F6) ──────────────────────
//
// Svelte uses SINGLE-brace `{ expr }` for attribute bindings and logic
// blocks, NOT Vue's `{{ }}` mustache. A `>` (comparison or TS generic)
// inside a single-brace expression is NOT a tag close and must not fire.
// The opposite direction (F6): the REAL tag-closing `>` of a tag that has
// a `>`-containing single-brace attribute must still close.

#[test]
fn svelte_single_brace_comparison_does_not_close() {
    // `a > b` inside `disabled={...}` is a comparison expression, not markup.
    let source = "<button disabled={a > b}>\n<p>x</p>";
    let off = after(source, "{a >");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `>` inside a Svelte single-brace expression must never auto-close",
    );
}

#[test]
fn svelte_single_brace_class_comparison_does_not_close() {
    let source = "<div class={x > y}>";
    let off = after(source, "{x >");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `>` inside a Svelte `class={{x > y}}` expression must never auto-close",
    );
}

#[test]
fn svelte_single_brace_generic_does_not_close() {
    // A TS generic `mk<Foo>()` inside a single-brace expression.
    let source = "<Comp value={mk<Foo>()}>";
    let off = after(source, "mk<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a TS generic `>` inside a Svelte single-brace expression must never auto-close",
    );
}

#[test]
fn svelte_real_close_after_single_brace_attr_still_closes() {
    // F6: the REAL closing `>` of `<button disabled={a > b}>` must still
    // insert `</button>` even though a `>` appears inside the brace expr.
    let source = "<button disabled={a > b}>\n<p>x</p>";
    // Offset after the FINAL `>` (the tag close), i.e. just past `}>`.
    let off = after(source, "{a > b}>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        Some("$0</button>".to_string()),
        "the real tag-closing `>` must still close even with a `>`-containing single-brace attr",
    );
}

#[test]
fn svelte_single_brace_does_not_disturb_vue_literal_brace() {
    // Discriminator: the Svelte single-brace handling must NOT bleed into
    // Vue. In a Vue template, `{` is literal text; a `>` after a literal
    // `{` that is a real tag close must still close. (Vue uses `{{ }}`.)
    let source = "<template><div>text { not an expr <span>\n</template>";
    let off = after(source, "<span>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</span>".to_string()),
        "Vue treats `{{` as literal text; a real tag close after it must still close",
    );
}

// ── Multi-line opening tag / multi-line quoted attribute value (F3) ──────
//
// When the opening `<` is on a PREVIOUS line, or a quoted value spans a
// newline, the `>` inside the quoted value must still be recognized as an
// attribute char (NOT a tag close). The old current-line anchor missed
// these because no `<` exists on the cursor's line.

#[test]
fn vue_multiline_open_tag_attr_gt_does_not_close() {
    // The `<div` is on a previous line; the `>` is inside `title="a>b"`.
    let source = "<template>\n<div\n title=\"a>b\">\n</template>";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `>` inside a quoted attr on a multi-line open tag must never auto-close (vue)",
    );
}

#[test]
fn vue_attr_value_spanning_newline_gt_does_not_close() {
    // The quoted value itself spans a newline; the `>` after the newline is
    // still inside the open quote.
    let source = "<template>\n<div title=\"a\nb>c\">\n</template>";
    let off = after(source, "b>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `>` inside a quoted value spanning a newline must never auto-close (vue)",
    );
}

#[test]
fn svelte_multiline_open_tag_attr_gt_does_not_close() {
    let source = "<div\n title=\"a>b\">\n<p>x</p>";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `>` inside a quoted attr on a multi-line open tag must never auto-close (svelte)",
    );
}

// ── Case-variant / unclosed script in a Svelte carrier (F4 / F5) ─────────

#[test]
fn svelte_case_variant_script_generic_does_not_close() {
    // F4: `<SCRIPT>` (uppercase) must still be a script region, so a TS
    // generic `>` inside it does not auto-close.
    let source = "<SCRIPT lang=\"ts\">\nconst x: Box<Foo> = mk();\n</SCRIPT>\n<div></div>";
    let off = after(source, "Box<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a TS generic `>` inside a case-variant <SCRIPT> must never auto-close (svelte)",
    );
}

#[test]
fn svelte_unclosed_script_generic_does_not_close() {
    // F5: an unclosed `<script>` (mid-typing) must still establish a
    // non-markup region to EOF, so a generic `>` inside it does not fire.
    let source = "<script lang=\"ts\">\nconst x: Box<Foo> = mk();";
    let off = after(source, "Box<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a TS generic `>` inside an unclosed <script> must never auto-close (svelte)",
    );
}

// ── Nested <template> inside a Vue SFC (F7) ──────────────────────────────

#[test]
fn vue_tag_after_nested_template_still_closes() {
    // A nested slot `<template #foo>...</template>` inside the SFC
    // `<template>` must NOT terminate the outer markup region. A real tag
    // typed AFTER the nested template still auto-closes.
    let source = "<template>\n<template #foo><span></span></template>\n<div>\n</template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a tag after a nested <template> must still auto-close (outer template region)",
    );
}

// ── Apostrophe in template/markup prose does not suppress a later close ───
//
// An apostrophe in ordinary template TEXT (`Bob's`, `don't`) is NOT an
// attribute-value quote: it must not desync the attribute-quote scan and
// suppress the auto-close of a LATER, genuine open tag. The attribute-quote
// scan must therefore anchor at the CANDIDATE tag's `<` (which sits AFTER
// the prose apostrophe), not at the whole markup window's start.

#[test]
fn vue_apostrophe_in_text_does_not_suppress_later_tag_close() {
    // `Bob's` apostrophe is template prose. The later `<div>` must close.
    let source = "<template><p>Bob's</p>\n<div>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "an apostrophe in template text must not suppress a later tag's auto-close (vue)",
    );
}

#[test]
fn svelte_apostrophe_in_earlier_markup_does_not_suppress_later_close() {
    // `don't` apostrophe sits in earlier (script) content. The root `<div>`
    // must still close — the apostrophe must not desync the attr-quote scan.
    let source = "<script>// don't</script>\n<div>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        Some("$0</div>".to_string()),
        "an apostrophe in earlier markup must not suppress a later root tag's close (svelte)",
    );
}

// ── Literal `<template>` in a script comment does not leak the markup window ─
//
// A `<script>` block placed BEFORE the real `<template>` that contains a
// literal `<template>` in a comment/string must NOT be mistaken for the SFC
// template open: the markup window must be located via the structural SFC
// block scan (which skips script/style content), so a `Box<Foo>` generic in
// that script is never treated as Vue template markup.

#[test]
fn vue_template_literal_in_script_comment_does_not_leak_markup_window() {
    let source = "<script>\n// <template>\nconst x: Box<Foo> = mk();\n</script>\n<template><div></div></template>";
    let off = after(source, "Box<Foo>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a literal <template> in a script comment must not leak the markup window into the script",
    );
}

#[test]
fn vue_script_before_template_real_template_tag_still_closes() {
    // Discriminator for the structural-window fix: with a `<script>` block
    // BEFORE the real `<template>` (containing a decoy `<template>` in a
    // comment), a genuine open tag INSIDE the real template still closes —
    // the window must lock onto the real template, not over-reject it.
    let source = "<script>\n// <template>\nconst x = 1;\n</script>\n<template>\n<div>\n</template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a real tag inside the true template must still close despite a script-comment decoy",
    );
}

// ── Vue literal `{` must NOT make the tag-lookup brace-aware ──────────────
//
// Vue interpolation is `{{ }}`; a LONE `{` in a Vue template is literal text.
// The candidate-tag lookup ([`nearest_tag_lt`]) tracks `{`/`}` nesting only
// for Svelte (where `{ expr }` is an expression span whose inner `<` must be
// ignored). For Vue that brace tracking is WRONG: a literal `{` before a tag
// would hide that tag's `<` (recorded only at brace-depth 0) and also let a
// following `"` open a spurious attribute quote at brace-depth > 0. Both
// desync the attribute-value guard:
//   * the bug-case below would wrong-FIRE `</div>` inside an attribute value
//     (a literal `{` hides the `<`, so the guard declines and the brace-blind
//     fallback inserts a close mid-attribute — buffer corruption);
//   * the positive complement would wrong-SUPPRESS a real tag close (a
//     literal `{` then a literal `"` open a spurious quote that swallows the
//     real tag's `<`, so the guard reports "in attribute value" and the close
//     is refused).
// With Vue brace tracking OFF both are corrected.

#[test]
fn vue_literal_brace_before_tag_does_not_break_attr_guard() {
    // A literal `{` precedes `<div title="a>b">`. Typing the `>` INSIDE the
    // quoted value must NOT insert a close mid-attribute. Fails if a literal `{`
    // in Vue text makes the tag lookup hide the `<` so the attribute guard
    // declines and the fallback wrong-fires `</div>` inside the value.
    let source = "<template>{ <div title=\"a>b\">";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a literal `{{` before a tag must not make the attribute guard miss; \
         a `>` inside a quoted attr value must never auto-close (vue)",
    );
}

#[test]
fn vue_literal_brace_then_quote_does_not_suppress_real_close() {
    // Positive complement: a literal `{` AND a literal `"` in Vue text before
    // `<div>`. Typing the REAL `>` of `<div>` must still close. Fails if a
    // literal `"` at brace-depth > 0 opens a spurious quote that swallows the
    // real tag's `<` so the attribute guard suppresses the close. Braces are
    // inert in Vue text, so the literal `"` stays literal text and the close fires.
    let source = "<template><a>{ \"<div>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a literal `{{` then a literal `\"` in Vue text must not suppress a later real tag close",
    );
}

// ── Carrier EXPRESSION spans never anchor the candidate tag (P2) ──────────
//
// The candidate-tag scan ([`nearest_tag_lt`]) must SKIP the carrier's
// expression spans (Vue `{{ }}` mustache, Svelte `{ }`) entirely. A `<` or a
// `"`/`'` INSIDE an expression span is not markup: it must never be recorded
// as the candidate tag's `<` (which would mis-anchor the attribute-value
// guard and let a `>` inside a later quoted attribute wrongly fire a close).
// A LONE single `{` in a Vue template is literal text — only `{{` opens a
// span — so it stays inert.

#[test]
fn vue_lt_inside_mustache_string_does_not_misanchor_attr_guard() {
    // A `<` lives literally inside a `{{ "<" }}` interpolation string. Typing
    // the `>` inside the LATER `title="a>b"` attribute must NOT close. Fails if
    // the Vue scan records the mustache-interior `<` as the candidate tag,
    // mis-anchors the quote walk, and wrong-fires `</div>` mid-attribute. The
    // `{{ }}` span is skipped, the real `<div` anchors the guard, the `>` is
    // seen in-quote → None.
    let source = "<template>{{ \"<\" }}<div title=\"a>b\">";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `<` inside a `{{ }}` mustache string must not anchor the attr guard; \
         a `>` inside a later quoted attr value must never auto-close (vue)",
    );
}

#[test]
fn vue_real_tag_after_mustache_with_lt_string_still_closes() {
    // Positive complement: the same `{{ "<" }}` span precedes `<div>`; typing
    // the REAL `>` of `<div>` must close. Fails if the unmatched `"` from the
    // mustache string leaks across the scan and opens a spurious quote that
    // swallows the real tag's `<` so the attr guard reports in-quote and
    // suppresses the close. The span is skipped, the real `<div` anchors
    // cleanly, and the close fires.
    let source = "<template>{{ \"<\" }}<div>\n</template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a real open tag after a `{{ }}` mustache containing a `<`/`\"` must still close (vue)",
    );
}

#[test]
fn svelte_lt_inside_single_brace_span_does_not_misanchor_attr_guard() {
    // A `<` lives inside a Svelte `{ a < b }` single-brace expression. Typing
    // the `>` inside the LATER `title="a>b"` attribute must NOT close — the
    // candidate-tag scan must skip the `{ }` span so the real `<div` anchors
    // the quote walk. (Confirms the carrier-aware skip keeps the established
    // Svelte brace behavior for the expression-span class.)
    let source = "{ a < b }<div title=\"a>b\">";
    let off = after(source, "title=\"a>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Svelte),
        None,
        "a `<` inside a Svelte single-brace expression must not anchor the attr guard; \
         a `>` inside a later quoted attr value must never auto-close (svelte)",
    );
}

// ── Vue `{{ }}` interpolation close-emitter parity (carrier-blind regression) ─
//
// The Vue close emitter must reason over markup the SAME way the gate guards
// do: the candidate `<` is the nearest preceding REAL-markup `<` (skipping a
// Vue `{{ … }}` mustache span). A `<` that lives only inside a closed
// interpolation must NEVER anchor the close emitter — typing the literal `>`
// right after the `}}` must produce NO close, not a garbage `</foo">`.

#[test]
fn vue_lt_inside_closed_mustache_string_then_gt_does_not_close() {
    // `<foo` lives only inside the closed `{{ "<foo" }}` interpolation; the
    // typed `>` sits immediately after `}}` in template text. There is no real
    // start tag here, so nothing closes.
    let source = "<template>{{ \"<foo\" }}>";
    let off = after(source, "}}>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        None,
        "a `<foo` that lives only inside a closed `{{ }}` interpolation must not \
         anchor the close emitter; a `>` typed after `}}` must never auto-close (vue)",
    );
}

#[test]
fn vue_real_tag_after_closed_mustache_with_lt_string_still_closes() {
    // Positive complement: the SAME `{{ "<foo" }}` span precedes a REAL
    // `<div>`; typing the real `>` of `<div>` must still close. The mustache
    // span is skipped, the real `<div` anchors the emitter, and the close
    // fires — proving the collapse onto the carrier-aware scanner did not break
    // Vue close behavior.
    let source = "<template>{{ \"<foo\" }}<div>\n</template>";
    let off = after(source, "<div>");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "a real open tag after a closed `{{ }}` interpolation containing a `<foo` \
         string must still auto-close (vue)",
    );
}

#[test]
fn vue_carrier_emitter_closes_div_with_attributes() {
    // The unified carrier emitter (driven through the gated entry) must close a
    // plain Vue template tag whose closing `>` follows quoted attribute values —
    // the behavior the legacy carrier-blind `auto_close_tag` provided, now
    // preserved through the single carrier-aware scanner.
    let source = "<template><div class=\"foo\" id=\"bar\">";
    let off = after(source, "id=\"bar\">");
    assert_eq!(
        auto_close_tag_in_carrier(source, off, CarrierKind::Vue),
        Some("$0</div>".to_string()),
        "the unified carrier emitter must close a Vue tag with quoted attributes \
         on its real `>`",
    );
}

// ───────────────── carrier_kind_for_language: fail-closed classifier ──────────
//
// The shared descriptor-identity carrier-kind classifier maps the two REAL
// built-in carriers to their `CarrierKind` and FAILS CLOSED (`None`) for every
// non-carrier language AND for a hypothetical THIRD markup carrier that has no
// `CarrierKind` arm yet. The third-carrier case is the discriminating one: the
// retired `!is_svelte()` "a framework carrier that is not Svelte ⇒ Vue" fallback
// would have mis-classified it as Vue.

/// The built-in Vue SFC carrier maps to `CarrierKind::Vue`.
#[test]
fn carrier_kind_for_language_maps_vue_carrier_to_vue() {
    assert_eq!(
        carrier_kind_for_language(&verter_session::FileLanguage::vue()),
        Some(CarrierKind::Vue),
    );
}

/// The built-in Svelte component carrier maps to `CarrierKind::Svelte`.
#[test]
fn carrier_kind_for_language_maps_svelte_carrier_to_svelte() {
    assert_eq!(
        carrier_kind_for_language(&verter_session::FileLanguage::svelte()),
        Some(CarrierKind::Svelte),
    );
}

/// A non-carrier `FileLanguage` (a plain script) has no markup region, so the
/// classifier returns `None` — the on-type auto-close and the import-preamble
/// re-anchor both correctly decline.
#[test]
fn carrier_kind_for_language_returns_none_for_plain_script() {
    assert_eq!(
        carrier_kind_for_language(&verter_session::FileLanguage::script_ts()),
        None,
    );
}

/// THE FAIL-CLOSED DISCRIMINATOR: a framework CARRIER row whose adapter is
/// NEITHER Vue NOR Svelte (a hypothetical third markup carrier) is NOT a
/// registered `built_in_descriptors()` row, so the descriptor-identity classifier
/// maps it to `None` and every markup feature drops it cleanly.
///
/// This is exactly what the retired `!is_svelte()` fallback got WRONG: for this
/// same third-carrier row `FileLanguage::is_svelte()` is `false`, so the old
/// "not-Svelte framework carrier ⇒ Vue" form would have returned
/// `Some(CarrierKind::Vue)` and spliced Vue-only markup behaviour onto an unknown
/// carrier. The assertion below pins the corrected fail-closed result; the
/// in-test recomputation of the old predicate documents the divergence so a
/// regression to the open fallback breaks this test.
#[test]
fn carrier_kind_for_language_fails_closed_for_unknown_third_carrier() {
    // A synthetic third markup carrier: a `Framework` row for an adapter id that
    // no built-in descriptor registers.
    let third_carrier = verter_session::FileLanguage::Framework {
        adapter_id: verter_session::FrameworkAdapterId::new("third_markup_carrier"),
        language_id: verter_session::LanguageId::new("third_markup_carrier"),
    };

    // The corrected, descriptor-identity classifier FAILS CLOSED.
    assert_eq!(
        carrier_kind_for_language(&third_carrier),
        None,
        "an unregistered third markup carrier must fail closed to None, never be \
         silently classified as Vue",
    );

    // DISCRIMINATION: the retired `!is_svelte()` Vue fallback would have returned
    // Vue for this exact row (it is a framework carrier and it is not Svelte).
    assert!(
        third_carrier.is_framework_carrier() && !third_carrier.is_svelte(),
        "the synthetic row must satisfy the OLD fallback's predicate (framework \
         carrier && !is_svelte), proving the new helper diverges from it",
    );
}
