//! Unit tests for the Svelte legacy store-`$x` auto-subscription classifier
//! ([`super::store_scan`]).
//!
//! Each test pins the OXC-structural store-sub classification: a bare `$NAME`
//! is a READ sub; an assignment target is a WRITE; runes / `$$`-magic /
//! lexically-declared `$NAME` locals are EXCLUDED; and the lexical-scope rules
//! (function/block/loop/catch/switch/class/module scopes, import-type locals,
//! TS type positions) neither over- nor under-suppress a real subscription.

use super::store_scan::*;

fn names(text: &str) -> Vec<(u32, u32, bool)> {
    scan_store_subscriptions(text)
        .into_iter()
        .map(|s| (s.dollar, s.ident_end, !matches!(s.kind, StoreSubKind::Read)))
        .collect()
}

#[test]
fn a_bare_store_reference_is_a_read_sub() {
    let text = "const x = $count + 1;";
    let subs = scan_store_subscriptions(text);
    assert_eq!(subs.len(), 1);
    let s = &subs[0];
    // `$count` begins at the `$` and the identifier ends after `count`.
    assert_eq!(&text[s.dollar as usize..s.ident_end as usize], "$count");
    assert!(
        matches!(s.kind, StoreSubKind::Read),
        "a bare reference is a READ"
    );
    assert_eq!(s.name, "count");
}

#[test]
fn free_host_rune_is_reported_without_misclassifying_shadowed_references() {
    let free = scan_store_subscriptions_and_host_with("$host()", &[]);
    assert!(free.subs.is_empty(), "$host is a rune, never a store sub");
    assert!(
        free.uses_host_rune,
        "a free $host reference selects runes mode"
    );

    let shadowed = scan_store_subscriptions_and_host_with("($host) => $host()", &[]);
    assert!(shadowed.subs.is_empty());
    assert!(
        !shadowed.uses_host_rune,
        "an expression-local parameter must suppress the rune meaning"
    );

    let script_shadowed = scan_store_subscriptions_and_host_with("$host()", &["$host".to_string()]);
    assert!(
        !script_shadowed.uses_host_rune,
        "a template expression must respect an enclosing authored $host binding"
    );
}

#[test]
fn a_simple_assignment_target_is_a_write_sub() {
    let text = "$count = 5;";
    let subs = scan_store_subscriptions(text);
    assert_eq!(subs.len(), 1);
    let s = &subs[0];
    assert_eq!(&text[s.dollar as usize..s.ident_end as usize], "$count");
    let StoreSubKind::SimpleWrite {
        eq,
        eq_end,
        rhs_end,
    } = s.kind
    else {
        panic!("a simple `=` target is a WRITE");
    };
    assert_eq!(&text[eq as usize..eq_end as usize], "=");
    // The RHS `5` ends at the digit's end.
    assert_eq!(text[eq_end as usize..rhs_end as usize].trim(), "5");
}

#[test]
fn runes_are_excluded() {
    // `$props()` / `$state.raw(0)` / `$derived(x)` / `$effect(...)` etc. are
    // runes — NEVER store-subs.
    assert!(names("const p = $props();").is_empty());
    assert!(names("let s = $state(0);").is_empty());
    assert!(names("let r = $state.raw(0);").is_empty());
    assert!(names("let d = $derived(x);").is_empty());
    assert!(names("let d2 = $derived.by(() => x);").is_empty());
    assert!(names("$effect(() => {});").is_empty());
    assert!(names("$inspect(x);").is_empty());
    assert!(names("const h = $host();").is_empty());
    assert!(names("let b = $bindable(0);").is_empty());
}

#[test]
fn double_dollar_magic_is_excluded() {
    // `$$props`/`$$restProps`/`$$slots` are F12 magic — NEVER store-subs.
    assert!(names("const a = $$props;").is_empty());
    assert!(names("const b = $$restProps;").is_empty());
    assert!(names("const c = $$slots.foo;").is_empty());
    // A WRITE to a `$$`-magic name is also excluded (defensive).
    assert!(names("$$props = {};").is_empty());
}

#[test]
fn a_local_dollar_binding_is_not_a_store_sub() {
    // A variable literally named `$x` (a local binding) is an ORDINARY
    // variable — its references are NOT store-subs (respect lexical
    // declarations).
    assert!(names("let $x = 1; const y = $x + 1;").is_empty());
    // A `$x` parameter shadows likewise.
    assert!(names("function f($x: number) { return $x; }").is_empty());
    // A destructured `$x` binding.
    assert!(names("const { $x } = obj; void $x;").is_empty());
}

#[test]
fn an_undeclared_dollar_reference_is_a_store_sub() {
    // `$store` with no local `store`/`$store` binding is a store-sub (TSGO
    // then checks `store` against `Readable<T>`).
    let subs = scan_store_subscriptions("const v = $store;");
    assert_eq!(subs.len(), 1);
    assert!(matches!(subs[0].kind, StoreSubKind::Read));
}

#[test]
fn object_shorthand_store_sub_is_a_shorthand_read() {
    // `const o = { $count };` — the shorthand property value is a store-sub.
    // It must classify as `ShorthandRead` (NOT a plain `Read`, which would
    // produce the invalid `{ __verter_store_get(count) }`).
    let subs = scan_store_subscriptions("const o = { $count };");
    assert_eq!(subs.len(), 1);
    assert!(
        matches!(subs[0].kind, StoreSubKind::ShorthandRead),
        "a shorthand object-property store-sub is a ShorthandRead"
    );
    assert_eq!(subs[0].name, "count");
    // A NON-shorthand property value (`{ k: $count }`) stays a plain Read.
    let explicit = scan_store_subscriptions("const o = { k: $count };");
    assert_eq!(explicit.len(), 1);
    assert!(matches!(explicit[0].kind, StoreSubKind::Read));
}

#[test]
fn member_access_on_a_store_sub_reads_the_base() {
    // `$store.foo` reads `$store` (the base) — one READ occurrence covering
    // `$store`; `.foo` stays verbatim.
    let subs = scan_store_subscriptions("const v = $store.foo;");
    assert_eq!(subs.len(), 1);
    let s = &subs[0];
    assert_eq!(
        &"const v = $store.foo;"[s.dollar as usize..s.ident_end as usize],
        "$store"
    );
    assert!(matches!(s.kind, StoreSubKind::Read));
}

#[test]
fn compound_assignment_is_a_compound_write() {
    // `$count += 1` is a COMPOUND WRITE — a Writable-checked read+set. The
    // operator-base is `+`, the operator span covers `+=`, and the RHS end is
    // recorded for the closing `))`.
    let text = "$count += 1;";
    let subs = scan_store_subscriptions(text);
    assert_eq!(subs.len(), 1);
    let s = &subs[0];
    assert_eq!(s.name, "count");
    let StoreSubKind::CompoundWrite {
        op_base,
        op,
        op_end,
        ..
    } = s.kind
    else {
        panic!("a compound `+=` is a CompoundWrite");
    };
    assert_eq!(op_base, "+");
    assert_eq!(&text[op as usize..op_end as usize], "+=");
    // A store-sub READ in the RHS of a compound assignment IS still recorded.
    let with_rhs = scan_store_subscriptions("$count += $other;");
    assert_eq!(
        with_rhs.len(),
        2,
        "the target write + the RHS `$other` read"
    );
    assert!(with_rhs
        .iter()
        .any(|s| matches!(s.kind, StoreSubKind::Read) && s.name == "other"));
}

#[test]
fn update_expression_is_an_update_write() {
    // `$count++` / `--$count`: a Writable-checked read+set with the ± delta.
    let post = scan_store_subscriptions("$count++;");
    assert_eq!(post.len(), 1);
    let StoreSubKind::Update {
        op_base, prefix, ..
    } = post[0].kind
    else {
        panic!("`$count++` is an Update");
    };
    assert_eq!(op_base, "+");
    assert!(!prefix, "postfix");

    let pre = scan_store_subscriptions("--$count;");
    assert_eq!(pre.len(), 1);
    let StoreSubKind::Update {
        op_base, prefix, ..
    } = pre[0].kind
    else {
        panic!("`--$count` is an Update");
    };
    assert_eq!(op_base, "-");
    assert!(prefix, "prefix");
}

#[test]
fn the_assignment_operator_inside_a_comment_is_not_mistaken_for_the_operator() {
    // `$count /* = */ = 1`: the structural `=`-operator scan SKIPS the `=`
    // inside the block comment and finds the REAL assignment operator, so the
    // recorded operator span is the actual `=` (not the comment's).
    let text = "$count /* = */ = 1;";
    let subs = scan_store_subscriptions(text);
    assert_eq!(subs.len(), 1);
    let StoreSubKind::SimpleWrite { eq, eq_end, .. } = subs[0].kind else {
        panic!("a write");
    };
    let (w_eq, w_eq_end) = (eq, eq_end);
    assert_eq!(
        &text[w_eq as usize..w_eq_end as usize],
        "=",
        "the operator span is the real `=`"
    );
    // The real `=` sits AFTER the closing `*/` of the comment.
    let comment_close = text.find("*/").unwrap();
    assert!(
        (w_eq as usize) > comment_close,
        "the operator is the real assignment `=`, not the comment's"
    );
}

#[test]
fn the_assignment_operator_after_a_line_comment_is_found() {
    // `$count // = note\n = 1`: the `=` inside the LINE comment is skipped and
    // the real `=` on the next line is the recorded operator.
    let text = "$count // = note\n = 1;";
    let subs = scan_store_subscriptions(text);
    assert_eq!(subs.len(), 1);
    let StoreSubKind::SimpleWrite { eq, eq_end, .. } = subs[0].kind else {
        panic!("a write");
    };
    assert_eq!(&text[eq as usize..eq_end as usize], "=");
    let comment = text.find("//").unwrap();
    let newline = text.find('\n').unwrap();
    assert!(
        (eq as usize) > newline && (eq as usize) > comment,
        "the operator is the real `=` after the line comment, not inside it"
    );
}

#[test]
fn no_dollar_means_no_occurrences() {
    assert!(scan_store_subscriptions("const x = 1 + 2;").is_empty());
}

#[test]
fn unparseable_fragment_fails_open() {
    assert!(scan_store_subscriptions("$count = = =;;;{{{").is_empty());
}

#[test]
fn script_declared_names_are_excluded_in_a_separate_fragment() {
    // The `_with` form excludes script-declared `$`-names so a markup
    // expression respects a `let $x` from the component script (cross-fragment
    // lexical scope) — `$x` is NOT a store-sub when the script declared it.
    let declared = vec!["$x".to_string()];
    assert!(
        scan_store_subscriptions_with("$x + 1", &declared).is_empty(),
        "a script-declared `$x` is excluded in a separate markup fragment"
    );
    // A different undeclared `$store` in the same fragment IS still a sub.
    let subs = scan_store_subscriptions_with("$x + $store", &declared);
    assert_eq!(subs.len(), 1);
}

#[test]
fn a_nested_function_scope_dollar_binding_does_not_over_suppress_a_real_store_sub() {
    // SOUNDNESS (P1-4): a `$count` binding declared INSIDE an unrelated nested
    // function body must NOT suppress a top-level/markup `$count` store-sub.
    // The declared-`$`-name collection is LEXICALLY SCOPED — only the
    // reference's own scope + its enclosing scopes suppress it.
    let text = "function f() { let $count = 1; void $count; }\nconst v = $count;";
    let subs = scan_store_subscriptions(text);
    // The top-level `$count` (the trailing `const v = $count`) IS a store-sub:
    // it is NOT in scope of the nested function's `let $count`.
    assert!(
        subs.iter().any(|s| s.name == "count"
            && matches!(s.kind, StoreSubKind::Read)
            && &text[s.dollar as usize..s.ident_end as usize] == "$count"
            && (s.dollar as usize) > text.find("const v").unwrap()),
        "the TOP-LEVEL `$count` is a store-sub (the nested fn binding does not \
         suppress it): {subs:?}"
    );
    // The `$count` references INSIDE the nested function (the `let $count`
    // binding's own scope) are NOT store-subs.
    let inner_ref = text.find("void $count").unwrap() + "void ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner_ref),
        "the nested-scope `$count` reference is a local binding, NOT a store-sub: \
         {subs:?}"
    );
}

#[test]
fn a_top_level_dollar_binding_still_suppresses_program_wide() {
    // DISCRIMINATING: a genuinely TOP-LEVEL `$x` binding suppresses references
    // in its own scope AND nested scopes (lexical enclosing) — proving the
    // scoping does not over-narrow.
    let text = "let $x = 1; function f() { return $x; } const y = $x;";
    assert!(
        scan_store_subscriptions(text).is_empty(),
        "a top-level `$x` binding suppresses every same-name reference (own + \
         nested scope): {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_nested_function_dollar_binding_is_not_collected_as_a_script_declared_name() {
    // P1-4 (cross-fragment half): `collect_declared_dollar_names` collects ONLY
    // top-level (script-scope) `$`-bindings — a `$count` declared inside a
    // nested function in the script must NOT be exported as a cross-fragment
    // suppressor (else a markup `$count` store-sub is silently dropped).
    let script = "function f() { let $count = 1; void $count; }";
    let declared = collect_declared_dollar_names(script);
    assert!(
        !declared.iter().any(|d| d == "$count"),
        "a nested-function `$count` is NOT a script-scope declared name: \
         {declared:?}"
    );
    // A genuinely top-level script binding IS collected.
    let top = collect_declared_dollar_names("let $top = 1;");
    assert!(top.iter().any(|d| d == "$top"));
}

#[test]
fn a_block_scoped_dollar_binding_does_not_over_suppress_an_outer_store_sub() {
    // SOUNDNESS: a `let $count` declared inside an ordinary BLOCK scope
    // (`{ … }`) must NOT suppress a `$count` store-sub OUTSIDE that block. A
    // block introduces its own lexical scope for `let`/`const`, so the binding
    // is local to the block — a later same-name reference at the enclosing
    // scope is a genuine store-sub (else it strands raw `$count` in TSX).
    let text = "{ let $count = 1; void $count; }\nconst v = $count;";
    let subs = scan_store_subscriptions(text);
    // The OUTER `$count` (the trailing `const v = $count`) IS a store-sub.
    assert!(
        subs.iter().any(|s| s.name == "count"
            && matches!(s.kind, StoreSubKind::Read)
            && (s.dollar as usize) > text.find("const v").unwrap()),
        "the block-OUTER `$count` is a store-sub (the block-local binding does \
         not suppress it): {subs:?}"
    );
    // The block-local `$count` references are NOT store-subs.
    let inner_ref = text.find("void $count").unwrap() + "void ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner_ref),
        "the block-local `$count` reference is a local binding: {subs:?}"
    );
}

#[test]
fn a_for_loop_scoped_dollar_binding_does_not_over_suppress_an_outer_store_sub() {
    // SOUNDNESS: a `for (let $i …)` binding is scoped to the loop — an outer
    // `$i` store-sub after the loop is genuine.
    let text = "for (let $i = 0; $i < 1; $i++) { void $i; }\nconst v = $i;";
    let subs = scan_store_subscriptions(text);
    assert!(
        subs.iter()
            .any(|s| s.name == "i" && (s.dollar as usize) > text.find("const v").unwrap()),
        "the loop-OUTER `$i` is a store-sub: {subs:?}"
    );
    // The loop-init `$i` references are local (inside the for's own scope).
    assert!(
        !subs
            .iter()
            .any(|s| (s.dollar as usize) < text.find("const v").unwrap()),
        "the loop-scoped `$i` references are local bindings: {subs:?}"
    );
}

#[test]
fn a_named_function_expression_id_does_not_over_suppress_an_outer_store_sub() {
    // SOUNDNESS: `const f = function $rec() {}` binds `$rec` ONLY inside the
    // function expression's own scope (the recursion name), NOT the enclosing
    // scope. An outer `$rec` store-sub must NOT be suppressed by it.
    let text = "const f = function $rec() { return $rec; };\nconst v = $rec;";
    let subs = scan_store_subscriptions(text);
    // The OUTER `$rec` (trailing `const v = $rec`) IS a store-sub.
    assert!(
        subs.iter()
            .any(|s| s.name == "rec" && (s.dollar as usize) > text.find("const v").unwrap()),
        "the enclosing `$rec` is a store-sub (the fn-expr id does not suppress \
         it): {subs:?}"
    );
    // The INNER `$rec` (the recursion reference inside the fn expr) is NOT a
    // store-sub — the fn-expr id is in scope there.
    let inner_ref = text.find("return $rec").unwrap() + "return ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner_ref),
        "the fn-expr's own `$rec` recursion reference is a local binding: \
         {subs:?}"
    );
}

#[test]
fn a_function_declaration_id_still_suppresses_in_the_enclosing_scope() {
    // DISCRIMINATING: a `function $f() {}` DECLARATION binds `$f` in the
    // ENCLOSING scope, so an enclosing `$f` reference IS suppressed (proving
    // the declaration-vs-expression distinction does not over-narrow).
    let text = "function $f() {}\nconst v = $f;";
    assert!(
        scan_store_subscriptions(text).is_empty(),
        "a `function $f(){{}}` declaration suppresses the enclosing `$f`: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn an_import_local_is_not_a_store_sub() {
    // FALSE-POSITIVE guard: an imported binding `$foo` is an ordinary local,
    // NOT a store-sub. A named/default/namespace import all bind locals.
    assert!(
        scan_store_subscriptions("import { foo as $foo } from \"m\"; void $foo;").is_empty(),
        "a named import local `$foo` is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("import $def from \"m\"; void $def;").is_empty(),
        "a default import local `$def` is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("import * as $ns from \"m\"; void $ns;").is_empty(),
        "a namespace import local `$ns` is not a store-sub"
    );
}

#[test]
fn an_import_local_is_excluded_cross_fragment() {
    // The cross-fragment script-scope set must include import locals so a
    // markup `{$foo}` over a script `import { x as $foo }` stays an ordinary
    // reference (not a falsely-rewritten store-sub).
    let declared = collect_declared_dollar_names("import { x as $foo } from \"m\"; void $foo;");
    assert!(
        declared.iter().any(|d| d == "$foo"),
        "an import local is a script-scope declared name: {declared:?}"
    );
}

#[test]
fn an_exported_function_or_class_declaration_id_is_not_a_store_sub() {
    // FALSE-POSITIVE guard: `export function $f(){}` / `export class $C {}`
    // bind `$f`/`$C` in the enclosing scope — references are NOT store-subs.
    assert!(
        scan_store_subscriptions("export function $f() {}\nvoid $f;").is_empty(),
        "an exported function declaration id `$f` is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("export class $C {}\nvoid $C;").is_empty(),
        "an exported class declaration id `$C` is not a store-sub"
    );
}

#[test]
fn a_switch_case_scoped_dollar_binding_does_not_over_suppress_but_is_local() {
    // SOUNDNESS: a BRACE-LESS `let $x` directly in a switch case (the switch
    // body is its OWN shared lexical scope, no `{ }` block) is local to the
    // switch — its in-case references are NOT store-subs, and an outer `$x`
    // after the switch IS a store-sub.
    let text = "switch (k) { case 1: let $x = 1; void $x; }\nconst v = $x;";
    let subs = scan_store_subscriptions(text);
    // The switch-scoped `$x` references are NOT store-subs.
    let inner = text.find("void $x").unwrap() + "void ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner),
        "the switch-scoped `$x` is a local binding: {subs:?}"
    );
    // The OUTER `$x` after the switch IS a store-sub.
    assert!(
        subs.iter()
            .any(|s| (s.dollar as usize) > text.find("const v").unwrap()),
        "the post-switch `$x` is a store-sub: {subs:?}"
    );
}

#[test]
fn a_dollar_name_in_a_ts_type_position_is_not_a_store_sub() {
    // FALSE-POSITIVE guard: a `$`-prefixed identifier in a TYPE position is a
    // TYPE reference, NEVER a store-sub (the store rewrite is value-only). A
    // type annotation, a type alias body, and an `extends` heritage clause are
    // all type syntax — no store-get may be injected there.
    assert!(
        scan_store_subscriptions("let x: $Foo = null as any; void x;").is_empty(),
        "a `$Foo` in a type annotation is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("type T = $Foo; let y: T = null as any; void y;").is_empty(),
        "a `$Foo` in a type-alias body is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("interface I extends $Base {}").is_empty(),
        "a `$Base` in an interface heritage clause is not a store-sub"
    );
    assert!(
        scan_store_subscriptions("function f(): $Ret { return null as any; } void f;").is_empty(),
        "a `$Ret` in a return-type annotation is not a store-sub"
    );
    // A `$`-name in a VALUE position in the SAME fragment is still a store-sub.
    let mixed = scan_store_subscriptions("let x: $Foo = $store; void x;");
    assert_eq!(mixed.len(), 1, "only the value-position `$store` is a sub");
    assert_eq!(mixed[0].name, "store");
}

#[test]
fn a_dollar_name_in_an_extended_ts_type_position_is_not_a_store_sub() {
    // FALSE-POSITIVE guard (extended type-reference surfaces): a `$`-name reached
    // through a `TSTypeReference.type_name` `IdentifierReference`, a class
    // `implements` clause, an `as`/`satisfies` annotation, a generic type
    // argument, a type-parameter constraint/default, or a `typeof` type query is a
    // TYPE reference, NEVER a store-sub — no `__verter_store_get` may be injected
    // there (it would be invalid TSX). Each of these reaches the classifier
    // through a `TSType` node, so the type-syntax no-op visitors must intercept it.
    let cases = [
        // `implements` heritage clause (a `TSClassImplements` type ref).
        "class C implements $I {}",
        // `as` type assertion.
        "const x = (0 as unknown) as $T; void x;",
        // `satisfies` expression.
        "const y = ({} satisfies $Shape); void y;",
        // Generic type ARGUMENT in a value-position call.
        "const z = fn<$Arg>(); void z;",
        // Type-parameter CONSTRAINT and DEFAULT.
        "function g<T extends $Bound = $Def>(): void {} void g;",
        // `typeof` type query naming a `$`-value in TYPE space.
        "type Q = typeof $val; let q: Q = null as any; void q;",
        // Indexed-access / keyof over a `$`-type.
        "type K = keyof $Obj; let k: K = null as any; void k;",
    ];
    for src in cases {
        assert!(
            scan_store_subscriptions(src).is_empty(),
            "a `$`-name in an extended TS type position must NOT be a store-sub: {src:?} → {:?}",
            scan_store_subscriptions(src)
        );
    }
    // Discriminating: a real value-position `$store` in the SAME fragment as a
    // type-position `$T` is still classified (only the type ref is suppressed).
    let mixed = scan_store_subscriptions("const x = $store as $T; void x;");
    assert_eq!(
        mixed.len(),
        1,
        "only the value-position `$store` (not the `as $T` cast type) is a sub: {mixed:?}"
    );
    assert_eq!(mixed[0].name, "store");
}

#[test]
fn an_import_type_local_is_not_collected_as_a_value_binding() {
    // A `import type { Foo as $count }` is a TYPE-only import — it binds NO
    // value, so it must NOT suppress a real value-position `$count` store-sub
    // (which would strand a raw `$count`).
    let text = "import type { Foo as $count } from \"m\";\nconst v = $count;";
    let subs = scan_store_subscriptions(text);
    assert!(
        subs.iter().any(|s| s.name == "count"),
        "a value `$count` is a store-sub despite a TYPE-only `import type … as \
         $count`: {subs:?}"
    );
    // A per-specifier `import { type Foo as $x, bar as $y }` — `$x` is type-only
    // (does not suppress a value `$x`), `$y` is a value local (suppresses `$y`).
    let per = scan_store_subscriptions(
        "import { type Foo as $x, bar as $y } from \"m\";\nconst a = $x; void $y;",
    );
    assert!(
        per.iter().any(|s| s.name == "x"),
        "the type-only specifier `$x` does not suppress the value `$x`: {per:?}"
    );
    assert!(
        !per.iter().any(|s| s.name == "y"),
        "the value specifier `$y` suppresses the value `$y`: {per:?}"
    );
}

#[test]
fn a_static_block_scoped_dollar_binding_is_local_and_does_not_over_hoist() {
    // SOUNDNESS: a `let $x` / `var $x` in a class `static { }` block is local
    // to that block — its in-block references are NOT store-subs, and it does
    // NOT over-hoist to suppress an outer `$x` store-sub.
    let text = "class C { static { let $x = 1; void $x; } }\nconst v = $x;";
    let subs = scan_store_subscriptions(text);
    let inner = text.find("void $x").unwrap() + "void ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner),
        "the static-block `$x` is a local binding: {subs:?}"
    );
    assert!(
        subs.iter()
            .any(|s| (s.dollar as usize) > text.find("const v").unwrap()),
        "the outer `$x` is a store-sub (the static block does not over-hoist): \
         {subs:?}"
    );
}

#[test]
fn a_catch_param_destructuring_default_store_sub_is_classified() {
    // A `catch ({ x = $store })` default is a VALUE expression — the `$store`
    // there IS a store-sub (it must not be left raw). The destructured binding
    // `x` is a local, not a store-sub.
    let text = "try {} catch ({ x = $store }) { void x; }";
    let subs = scan_store_subscriptions(text);
    assert!(
        subs.iter()
            .any(|s| s.name == "store" && matches!(s.kind, StoreSubKind::Read)),
        "the catch-param default `$store` is a store-sub: {subs:?}"
    );
}

#[test]
fn a_named_class_expression_id_is_not_a_store_sub_in_its_body() {
    // FALSE-POSITIVE guard: `const C = class $C { m() { return $C; } }` binds
    // `$C` only inside the class expression body (the class name). The inner
    // `$C` reference is NOT a store-sub; an outer `$C` still is.
    let text = "const C = class $C { m() { return $C; } };\nconst v = $C;";
    let subs = scan_store_subscriptions(text);
    let inner = text.find("return $C").unwrap() + "return ".len();
    assert!(
        !subs.iter().any(|s| (s.dollar as usize) == inner),
        "the class-expr's own `$C` reference is a local binding: {subs:?}"
    );
    assert!(
        subs.iter()
            .any(|s| (s.dollar as usize) > text.find("const v").unwrap()),
        "the outer `$C` is a store-sub: {subs:?}"
    );
}

// --- BLOCKER B: lvalue / assignment-target positions never get the READ helper ---
//
// A `$NAME` in a WRITE/lvalue position (destructuring-assignment target, for-of/
// for-in target, compound/update target) must NEVER be classified as a READ —
// the projection would emit `__verter_store_get(NAME)` in lvalue position, which
// is invalid TSX. VALUE sub-positions (destructuring defaults, computed keys)
// inside the SAME construct stay READ subs.

/// Whether a READ store-sub for `name` was classified at the given source offset.
fn has_read_at(text: &str, name: &str, at: usize) -> bool {
    scan_store_subscriptions(text).iter().any(|s| {
        s.name == name && (s.dollar as usize) == at && matches!(s.kind, StoreSubKind::Read)
    })
}

/// Whether an LVALUE-WRITE store-sub for `name` was classified at `at` — the
/// destructuring / for-of WRITE-TARGET projection (`__verter_store_lvalue(name)
/// .value`), distinct from a READ and from raw residue.
fn has_lvalue_write_at(text: &str, name: &str, at: usize) -> bool {
    scan_store_subscriptions(text).iter().any(|s| {
        s.name == name && (s.dollar as usize) == at && matches!(s.kind, StoreSubKind::LvalueWrite)
    })
}

/// Whether a SHORTHAND lvalue-write store-sub for `name` was classified at `at`
/// (the `({ $name } = obj)` shorthand target — keyed `{ $name:
/// __verter_store_lvalue(name).value }`).
fn has_shorthand_lvalue_write_at(text: &str, name: &str, at: usize) -> bool {
    scan_store_subscriptions(text).iter().any(|s| {
        s.name == name
            && (s.dollar as usize) == at
            && matches!(s.kind, StoreSubKind::ShorthandLvalueWrite)
    })
}

#[test]
fn an_array_destructuring_assignment_target_is_an_lvalue_write_sub() {
    // `[$count] = xs` — `$count` is a WRITE leaf classified as `LvalueWrite` (the
    // `__verter_store_lvalue(count).value` projection), NOT a read and NOT raw
    // residue. The no-read-helper-in-lvalue invariant holds AND the leaf is
    // rewritten.
    let text = "[$count] = xs;";
    let target_at = text.find("$count").unwrap();
    assert!(
        !has_read_at(text, "count", target_at),
        "a `[$count]` destructuring TARGET must not be a READ sub: {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_lvalue_write_at(text, "count", target_at),
        "a `[$count]` destructuring TARGET must be an LVALUE-WRITE sub (rewritten, \
         not stranded raw): {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn an_object_destructuring_assignment_target_is_an_lvalue_write_sub() {
    // `({ x: $count } = obj)` — `$count` in keyed target position is an
    // `LvalueWrite` leaf (rewritten to the writable-lvalue member access).
    let text = "({ x: $count } = obj);";
    let target_at = text.find("$count").unwrap();
    assert!(
        !has_read_at(text, "count", target_at),
        "an object-destructuring TARGET `$count` must not be a READ sub: {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_lvalue_write_at(text, "count", target_at),
        "an object-destructuring TARGET `$count` must be an LVALUE-WRITE sub: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn an_object_shorthand_destructuring_assignment_target_is_a_shorthand_lvalue_write_sub() {
    // `({ $count } = obj)` — the shorthand target identifier is a
    // `ShorthandLvalueWrite` leaf (a synthesised key is inserted: `{ $count:
    // __verter_store_lvalue(count).value }`).
    let text = "({ $count } = obj);";
    let target_at = text.find("$count").unwrap();
    assert!(
        !has_read_at(text, "count", target_at),
        "a shorthand-destructuring TARGET `$count` must not be a READ sub: {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_shorthand_lvalue_write_at(text, "count", target_at),
        "a shorthand-destructuring TARGET `$count` must be a SHORTHAND-LVALUE-WRITE \
         sub: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_destructuring_default_value_inside_a_target_is_a_read_while_the_target_is_an_lvalue_write() {
    // `[$count = $read] = xs` — the TARGET `$count` is an `LvalueWrite` leaf,
    // while the DEFAULT `$read` is a value expression that IS a read sub. This
    // DISCRIMINATES the lvalue/value split inside one destructuring target.
    let text = "[$count = $read] = xs;";
    let target_at = text.find("$count").unwrap();
    let default_at = text.find("$read").unwrap();
    assert!(
        has_lvalue_write_at(text, "count", target_at),
        "the destructuring TARGET `$count` is an LVALUE-WRITE (not a read): {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_read_at(text, "read", default_at),
        "the destructuring DEFAULT `$read` IS a READ sub: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_computed_member_target_key_is_a_read_but_the_member_object_base_is_a_read_too() {
    // `$obj[$key] = v` — the member-write base `$obj` is the documented READ
    // safe-degrade, the computed key `$key` is a value READ. NEITHER becomes a
    // raw/lvalue residue; both classify as reads (valid TSX
    // `__verter_store_get(obj)[__verter_store_get(key)] = v`).
    let text = "$obj[$key] = v;";
    let obj_at = text.find("$obj").unwrap();
    let key_at = text.find("$key").unwrap();
    assert!(
        has_read_at(text, "obj", obj_at),
        "the member-write base `$obj` is a READ: {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_read_at(text, "key", key_at),
        "the computed key `$key` is a READ: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_for_of_assignment_target_is_an_lvalue_write_sub_but_the_iterable_is_a_read() {
    // `for ($count of $xs) {}` — the loop TARGET `$count` is an `LvalueWrite`
    // (rewritten to the writable-lvalue member access), while the iterable `$xs`
    // IS a read sub. DISCRIMINATING.
    let text = "for ($count of $xs) { void 0; }";
    let target_at = text.find("$count").unwrap();
    let iter_at = text.find("$xs").unwrap();
    assert!(
        has_lvalue_write_at(text, "count", target_at),
        "a `for ($count of …)` TARGET must be an LVALUE-WRITE sub: {:?}",
        scan_store_subscriptions(text)
    );
    assert!(
        has_read_at(text, "xs", iter_at),
        "the `for (… of $xs)` iterable IS a read sub: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_for_in_assignment_target_is_an_lvalue_write_sub() {
    // `for ($key in obj) {}` — the loop TARGET `$key` is an `LvalueWrite` leaf.
    let text = "for ($key in obj) { void 0; }";
    let target_at = text.find("$key").unwrap();
    assert!(
        has_lvalue_write_at(text, "key", target_at),
        "a `for ($key in …)` TARGET must be an LVALUE-WRITE sub: {:?}",
        scan_store_subscriptions(text)
    );
}

#[test]
fn a_for_of_declaration_binding_is_not_a_read_and_does_not_suppress_a_sibling() {
    // `for (const $x of xs) {}` — the `const $x` binding is a local (not a read);
    // a sibling outer `$x` store-sub is NOT over-suppressed.
    let text = "for (const $x of xs) { void $x; }\nconst v = $x;";
    let outer_at = text
        .find("const v")
        .map(|i| text[i..].find("$x").unwrap() + i)
        .unwrap();
    assert!(
        has_read_at(text, "x", outer_at),
        "the outer `$x` is a store-sub (the loop binding does not over-suppress): {:?}",
        scan_store_subscriptions(text)
    );
}

// --- BLOCKER C: catch param-default ordering does not over-suppress a real read ---

#[test]
fn a_catch_param_default_is_not_over_suppressed_by_a_body_declaration() {
    // `catch ({ x = $store }) { let $store; }` — the param-default `$store` is a
    // VALUE read evaluated in the PARAM scope, which does NOT see the body's later
    // `let $store`. The body declaration must NOT over-suppress the param-default
    // read (lexical correctness — the param scope encloses the body scope).
    let text = "try {} catch ({ x = $store }) { let $store; void $store; }";
    let default_at = text.find("$store").unwrap();
    assert!(
        has_read_at(text, "store", default_at),
        "the catch-param DEFAULT `$store` is a READ sub despite the body `let \
         $store` (no over-suppression): {:?}",
        scan_store_subscriptions(text)
    );
    // The body reference `void $store` (after the `let $store`) IS suppressed —
    // it refers to the local body binding.
    let body_ref_at = text.rfind("$store").unwrap();
    assert!(
        !has_read_at(text, "store", body_ref_at),
        "the body `$store` reference is the local binding (suppressed): {:?}",
        scan_store_subscriptions(text)
    );
}
