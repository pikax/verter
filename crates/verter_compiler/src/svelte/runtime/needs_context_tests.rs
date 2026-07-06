//! Unit tests for the `needs_context` component-context analysis (the
//! `$.push($$props, true)` / `$.pop()` trigger) — extracted alongside the module
//! from the shared reactive-analysis test file.

use oxc_allocator::Allocator;

#[test]
fn inspect_with_chain_sets_needs_context_but_plain_and_trace_do_not() {
    // `$inspect(...).with(...)` FORCES the component frame in official production
    // output (`$.push($$props, true)` / `$.pop()` + the `$$props` param) even
    // though the statement itself is elided — the `.with` chain is a
    // `needs_context` trigger. Plain `$inspect(x)` and `$inspect.trace()` are rune
    // calls and must NOT trigger context (their elision leaves no frame).
    let alloc = oxc_allocator::Allocator::default();
    assert!(
        super::needs_context(
            &alloc,
            Some("let c = $state(0); $inspect(c).with(console.log);"),
            None,
            &[],
            &[],
        ),
        "`$inspect(...).with(...)` must set needs_context (the official frame)"
    );
    assert!(
        !super::needs_context(
            &alloc,
            Some("let c = $state(0); $inspect(c);"),
            None,
            &[],
            &[]
        ),
        "plain `$inspect(x)` must NOT set needs_context"
    );
    assert!(
        !super::needs_context(
            &alloc,
            Some("let c = $state(0);"),
            None,
            &["() => { $inspect.trace(); c++; }"],
            &[],
        ),
        "`$inspect.trace()` in a handler must NOT set needs_context"
    );
}

#[test]
fn render_callee_scan_peels_the_snippet_call_and_scans_only_the_callee() {
    // The `{@render}` dynamic-callee rail: the OUTER snippet call is excluded
    // from the unsafe-call trigger, the PEELED callee scans normally.
    let alloc = oxc_allocator::Allocator::default();
    let props = Some("let { children, o } = $props();");
    // SAFE — identifier callees (even prop-rooted) and ternaries of identifiers:
    // the outer call is not a trigger and an identifier read never is.
    assert!(
        !super::needs_context(&alloc, props, None, &[], &["children?.()"]),
        "a prop-identifier render callee must NOT set needs_context"
    );
    assert!(
        !super::needs_context(
            &alloc,
            Some("let { a, b } = $props(); let cond = $state(true);"),
            None,
            &[],
            &["(cond ? a : b)()"],
        ),
        "a ternary-of-identifiers render callee must NOT set needs_context"
    );
    // SAFE — a member rooted at a LOCAL (not an import / prop).
    assert!(
        !super::needs_context(
            &alloc,
            Some("let __r = $state(0);"),
            None,
            &[],
            &["localSnips.row()"],
        ),
        "a local-rooted member render callee must NOT set needs_context"
    );
    // UNSAFE — a member rooted at the `$host()` CALL RESULT (a non-identifier
    // leaf is never a safe identifier).
    assert!(
        super::needs_context(
            &alloc,
            Some("let __r = $state(0);"),
            None,
            &[],
            &["$host().snip()"]
        ),
        "a call-result-rooted member render callee must set needs_context"
    );
    // UNSAFE — a member rooted at an IMPORT, and a PROP-rooted member.
    assert!(
        super::needs_context(
            &alloc,
            Some("import Snips from './Snips.svelte';"),
            None,
            &[],
            &["Snips.row()"],
        ),
        "an import-rooted member render callee must set needs_context"
    );
    assert!(
        super::needs_context(&alloc, props, None, &[], &["o.snip()"]),
        "a prop-rooted member render callee must set needs_context"
    );
    // UNSAFE — a `new` expression anywhere in the peeled callee.
    assert!(
        super::needs_context(&alloc, None, None, &[], &["(new Date())()"]),
        "a new-expression render callee must set needs_context"
    );
    // UNSAFE — an object-literal-rooted member callee and an inner CALL callee
    // rooted at an import (`unsafeImport()()` peels ONE call; the inner call is
    // scanned).
    assert!(
        super::needs_context(&alloc, None, None, &[], &["({ snip(){} }).snip()"]),
        "an object-literal-rooted member render callee must set needs_context"
    );
    assert!(
        super::needs_context(
            &alloc,
            Some("import makeSnip from './Make.svelte';"),
            None,
            &[],
            &["makeSnip()()"],
        ),
        "an import-rooted inner-call render callee must set needs_context"
    );
    assert!(
        !super::needs_context(
            &alloc,
            Some("let __r = $state(0);"),
            None,
            &[],
            &["localFn()()"]
        ),
        "a local-rooted inner-call render callee must NOT set needs_context"
    );
    // UNSAFE — a conditional whose BRANCH carries an unsafe member (the branch
    // scans; only the outer call is excluded).
    assert!(
        super::needs_context(
            &alloc,
            Some("import Snips from './Snips.svelte'; let { b } = $props(); let cond = $state(true);"),
            None,
            &[],
            &["(cond ? Snips.row : b)()"],
        ),
        "a conditional branch carrying an unsafe member must set needs_context"
    );
    // Paren transparency: the parenthesized-callee spelling peels the same.
    assert!(
        super::needs_context(
            &alloc,
            Some("let __r = $state(0);"),
            None,
            &[],
            &["($host().snip)()"]
        ),
        "a paren-wrapped call-result-rooted callee must set needs_context"
    );
    // Conservative fallback: a NON-CALL render source scans whole (unreachable
    // for an emitted render — the projection refuses an uncalled render first).
    assert!(
        super::needs_context(
            &alloc,
            Some("import Snips from './Snips.svelte';"),
            None,
            &[],
            &["Snips.row"],
        ),
        "a non-call render source scans whole (conservative fallback)"
    );
}

#[test]
fn render_callee_arrow_param_shadow_inside_callee_stays_frame_free() {
    // INTRA-EXPRESSION shadowing inside the PEELED callee: an arrow parameter
    // shadowing an unsafe import root makes the member read a LOCAL read — it
    // must NOT open the frame. The unshadowed twin (same import root, no
    // param) roots the member at the import and MUST. This is distinct from
    // the render-ARG shadow: the shadow scope lives inside the callee
    // expression itself.
    let alloc = oxc_allocator::Allocator::default();
    let instance = Some("import Snips from './Snips.svelte';");
    assert!(
        !super::needs_context(&alloc, instance, None, &[], &["((Snips) => Snips.row)()"]),
        "an arrow param shadowing the unsafe root inside the callee must NOT set needs_context"
    );
    assert!(
        super::needs_context(&alloc, instance, None, &[], &["(() => Snips.row)()"]),
        "the unshadowed twin (import-rooted member in the arrow body) must set needs_context"
    );
}

#[test]
fn module_script_import_locals_are_unsafe_roots() {
    // MODULE-slot import locals are unsafe roots exactly like instance imports
    // (they resolve up the lexical chain into template expressions): a member read
    // rooted at a module import opens the frame; a BARE read of the same local
    // does NOT; and a template-scope shadow of the name stays safe.
    let alloc = Allocator::default();
    let module = Some("import * as NS from './m.js';");
    assert!(
        super::needs_context(&alloc, None, module, &["NS.z"], &[]),
        "a member rooted at a MODULE import must set needs_context"
    );
    assert!(
        !super::needs_context(&alloc, None, module, &["NS"], &[]),
        "a BARE module-import read must NOT set needs_context"
    );
    // The shadow case uses an UNCALLED arrow (an IIFE's non-identifier callee is
    // itself an unsafe call — official `is_safe_identifier` — so it would trigger
    // regardless of shadowing).
    assert!(
        !super::needs_context(&alloc, None, module, &["(NS) => NS.z"], &[]),
        "a local shadowing the module-import name stays safe"
    );
    // The instance-import behaviour is unchanged by the module parameter.
    assert!(
        super::needs_context(
            &alloc,
            Some("import * as NS from './m.js';"),
            None,
            &["NS.z"],
            &[]
        ),
        "an instance-import member root still sets needs_context"
    );
}
