//! The Svelte IDE-projection TYPE-CHECK VALIDITY gate (D-u / D-ae).
//!
//! OXC parse-only is NOT sufficient: the projected `.svelte.tsx` must type-check
//! CLEAN through the TSGO path. This harness projects each fixture through the
//! real Svelte IDE projector, writes it into a hermetic temp project (vendored
//! `svelte` types + the in-repo `@verter/svelte-jsx` shim `paths`-mapped — no
//! npm install, Testing-Hermeticity), and runs `tsgo --noEmit`.
//!
//! GATE PRECONDITION (D-ae): the pragma-parity fixture proves the
//! `@jsxImportSource @verter/svelte-jsx` pragma OVERRIDES a project-level
//! `jsxImportSource: "vue"` under TSGO. If TSGO fails the override, the named
//! D-ae fallback is a STOP-and-redesign (escalate) — never a silent degrade.
//!
//! The harness is GATED behind the locally-resolvable `tsgo` binary: when no
//! `tsgo`/`tsc` is found (a machine without the native-preview install) the
//! tests skip with a clear message rather than failing spuriously. On CI with
//! the binary present they run for real.

use std::path::PathBuf;
use std::process::Command;

use verter_compiler::svelte::ide::project_svelte_ide;
use verter_compiler::svelte::parser::parse_svelte;

/// The crate's test-fixture root.
fn gate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_typecheck_gate")
}

/// The workspace root (`<ws>/crates/verter_session` → `<ws>`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_session")
        .to_path_buf()
}

/// Locate a `tsgo` (or `tsc`) binary via the workspace `node_modules/.bin`.
/// Returns `None` when neither is present — the gate then SKIPS (hermetic
/// machines without the native-preview install).
fn locate_type_checker() -> Option<(PathBuf, bool)> {
    let bin = workspace_root().join("node_modules/.bin");
    let tsgo = bin.join("tsgo");
    if tsgo.exists() {
        return Some((tsgo, true));
    }
    let tsc = bin.join("tsc");
    if tsc.exists() {
        return Some((tsc, false));
    }
    None
}

/// Render a hermetic temp project for `projected_tsx` and run the type checker.
/// Returns `(success, combined_output)`. `extra_files` are additional
/// (relative-path, content) files to write (e.g. an imported types module).
fn typecheck_projected(
    projected_tsx: &str,
    file_name: &str,
    extra_files: &[(&str, &str)],
    vendor_svelte: bool,
) -> Option<(bool, String)> {
    let (checker, _is_tsgo) = locate_type_checker()?;
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();

    // The projected TSX file under test.
    std::fs::write(root.join(file_name), projected_tsx).expect("write tsx");
    for (rel, content) in extra_files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create dir");
        }
        std::fs::write(path, content).expect("write extra");
    }

    // Vendor `svelte` (hermetic) into node_modules/svelte.
    if vendor_svelte {
        let dst = root.join("node_modules/svelte");
        std::fs::create_dir_all(&dst).expect("svelte dir");
        for f in [
            "index.d.ts",
            "elements.d.ts",
            "attachments.d.ts",
            "package.json",
        ] {
            std::fs::copy(gate_dir().join("vendor_svelte").join(f), dst.join(f))
                .expect("copy svelte vendor");
        }
    }

    // tsconfig: project-level `jsxImportSource: "vue"` (the live provider
    // default the pragma must override), `paths`-map `@verter/svelte-jsx`
    // directly at the in-repo package (D-av — no npm install).
    let shim_dir = workspace_root().join("packages/svelte-jsx");
    let shim = shim_dir.to_string_lossy().replace('\\', "/");
    let tsconfig = format!(
        r#"{{
  "compilerOptions": {{
    "module": "esnext",
    "target": "esnext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "allowImportingTsExtensions": true,
    "paths": {{
      "@verter/svelte-jsx/jsx-runtime": ["{shim}/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/jsx-dev-runtime": ["{shim}/jsx-dev-runtime.d.ts"]
    }}
  }},
  "include": ["**/*.ts", "**/*.tsx"]
}}"#
    );
    std::fs::write(root.join("tsconfig.json"), tsconfig).expect("write tsconfig");

    let output = Command::new(&checker)
        .arg("--noEmit")
        .arg("-p")
        .arg(root.join("tsconfig.json"))
        .current_dir(root)
        .output()
        .expect("run type checker");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some((output.status.success(), combined))
}

/// Project a `.svelte` source through the real IDE projector.
fn project(source: &str) -> String {
    let parsed = parse_svelte(source);
    project_svelte_ide(source, &parsed, Some("Comp.svelte"), true).code
}

fn skip_note(name: &str) {
    eprintln!(
        "SKIP {name}: no tsgo/tsc in node_modules/.bin (hermetic machine); \
         run on a machine with the native-preview install to exercise the gate"
    );
}

#[test]
fn precondition_pragma_overrides_project_level_vue_jsx_import_source_under_tsgo() {
    // The D-ae GATE PRECONDITION: a `.svelte.tsx`-shaped file whose
    // `@jsxImportSource @verter/svelte-jsx` pragma overrides the project-level
    // `jsxImportSource: "vue"`. The fixture uses a lowercase `onclick` — which
    // ONLY type-checks under the Svelte intrinsic table (Vue's JSX would reject
    // the lowercase literal attribute). If this fails, the pragma override does
    // NOT work under TSGO → STOP-and-redesign (escalate), never silently
    // degrade.
    let projected = "/** @jsxImportSource @verter/svelte-jsx */\n\
        ;function __verter_render() {\n\
        return (<button onclick={() => {}}>ok</button>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(projected, "Pragma.svelte.tsx", &[], true) else {
        skip_note("pragma-parity precondition");
        return;
    };
    assert!(
        ok,
        "PRAGMA-PARITY PRECONDITION FAILED: the @jsxImportSource pragma did not \
         override the project-level jsxImportSource:\"vue\" under TSGO. This is \
         the named D-ae STOP-and-redesign — escalate, do NOT degrade.\n{out}"
    );
}

#[test]
fn projected_runes_props_fixture_type_checks_clean() {
    let projected = project(
        "<script lang=\"ts\">\n\
         interface Props { label: string; count?: number }\n\
         let { label, count = 0 }: Props = $props();\n\
         </script>\n\
         <div>{label}{count}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Props.svelte.tsx", &[], true) else {
        skip_note("runes props");
        return;
    };
    assert!(
        ok,
        "projected runes-props TSX must type-check clean:\n{out}"
    );
}

#[test]
fn projected_event_attribute_currenttarget_checks_with_svelte_table() {
    // A lowercase `onchange` with a typed `currentTarget` — proves the Svelte
    // intrinsic table is in effect (Vue/React casing would reject the lowercase
    // attribute). `e.currentTarget.value` checks on an `<input>`.
    let projected = project(
        "<input onchange={(e) => { const v: string | number | undefined = e.currentTarget.value; void v; }} />",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Event.svelte.tsx", &[], true) else {
        skip_note("event currentTarget");
        return;
    };
    assert!(
        ok,
        "lowercase onchange with typed currentTarget must check (Svelte table):\n{out}"
    );
}

#[test]
fn camelcase_onchange_is_rejected_proving_the_retired_rename() {
    // P2-3 (retired-rename proof): a camelCase `onChange` is REJECTED by the
    // Svelte intrinsic table (lowercase-only). The retired `onclick → onClick`
    // rename would have made this pass — its rejection proves the rename is gone.
    let projected = project("<button onChange={() => {}}>x</button>");
    let Some((ok, out)) = typecheck_projected(&projected, "CamelEvent.svelte.tsx", &[], true)
    else {
        skip_note("camelCase onChange rejection");
        return;
    };
    assert!(
        !ok,
        "a camelCase `onChange` must be REJECTED by the Svelte table \
         (the onClick rename is retired):\n{out}"
    );
    assert!(
        out.contains("onChange") || out.to_lowercase().contains("does not exist"),
        "the rejection must name the unknown camelCase attribute:\n{out}"
    );
}

#[test]
fn lowercase_onintrostart_is_accepted_proving_the_svelte_table() {
    // P2-3 (chosen-table proof): `onintrostart` is a Svelte-specific transition
    // event attribute Vue's/React's JSX tables reject — its ACCEPTANCE proves
    // the Svelte intrinsic table is the one in effect.
    let projected = project("<div onintrostart={(e) => { void e; }}>x</div>");
    let Some((ok, out)) = typecheck_projected(&projected, "IntroStart.svelte.tsx", &[], true)
    else {
        skip_note("onintrostart acceptance");
        return;
    };
    assert!(
        ok,
        "`onintrostart` (Svelte-specific) must be ACCEPTED (Svelte table in effect):\n{out}"
    );
}

#[test]
fn render_with_wrong_arg_is_rejected() {
    // P2-3 ({@render} wrong-arg proof): `{@render snip(arg)}` projects to a call
    // `{snip(arg)}` checked through the Snippet call signature. A wrong-typed
    // arg against a `Snippet<[number]>` parameter must FAIL.
    let projected = "/** @jsxImportSource @verter/svelte-jsx */\n\
        import type { Snippet } from \"svelte\";\n\
        declare const snip: Snippet<[number]>;\n\
        ;function __verter_render() {\n\
        return (<>{snip(\"not a number\")}</>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(projected, "RenderArg.svelte.tsx", &[], true) else {
        skip_note("render wrong arg");
        return;
    };
    assert!(
        !ok,
        "a `{{@render snip(\"str\")}}` against Snippet<[number]> must be REJECTED:\n{out}"
    );
}

#[test]
fn class_object_clsx_form_checks_and_rejects_non_classvalue_payload() {
    // P2-3 (clsx fixture): `class={{ … }}` object form checks through
    // `SvelteHTMLElements`' `class?: ClassValue`. A `@ts-expect-error` guards a
    // non-`ClassValue` payload (a function is not assignable to ClassValue) — if
    // the class slot leaked `any`, the `@ts-expect-error` would be UNUSED and TS
    // would error (discriminating both ways under strict).
    let projected = "/** @jsxImportSource @verter/svelte-jsx */\n\
        ;function __verter_render() {\n\
        // @ts-expect-error a bigint is not a ClassValue\n\
        const bad: import(\"svelte/elements\").ClassValue = 1n;\n\
        void bad;\n\
        return (<>\n\
        <div class={{ active: true, disabled: false }}>ok</div>\n\
        </>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(projected, "Clsx.svelte.tsx", &[], true) else {
        skip_note("class clsx form");
        return;
    };
    assert!(
        ok,
        "the clsx object class form must check, and the @ts-expect-error must \
         match a real non-ClassValue error (proving class is typed, not any):\n{out}"
    );
}

#[test]
fn component_tag_wrong_prop_is_rejected_via_element_attributes_property() {
    // P2-3 (component-tag wrong-prop): the JSX `ElementAttributesProperty
    // { $props }` checks a component tag's props against the synth's `$props`
    // member. A class-shaped component with `$props: { label: string }` rejects a
    // wrong-typed `label` AND an unknown prop — discriminating the contract.
    let good = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare class MyComp { $props: { label: string }; }\n\
        ;function __verter_render() {\n\
        return (<><MyComp label=\"ok\" /></>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(good, "CompPropOk.svelte.tsx", &[], true) else {
        skip_note("component prop contract");
        return;
    };
    assert!(
        ok,
        "a correctly-typed component prop must check through $props:\n{out}"
    );

    let bad = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare class MyComp { $props: { label: string }; }\n\
        ;function __verter_render() {\n\
        return (<><MyComp label={123} /></>);\n\
        }\nexport {};\n";
    let Some((bad_ok, bad_out)) = typecheck_projected(bad, "CompPropBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a wrong-typed component prop must be REJECTED via $props:\n{bad_out}"
    );
}

#[test]
fn projected_snippet_ordering_fixture_type_checks_clean_discriminating_tdz() {
    // D-ap DISCRIMINATING: a `{@render mySnip()}` PRECEDING its `{#snippet}` in
    // the same scope type-checks clean. In-place declarator projection would
    // fail with a TS use-before-declaration error — the hoist (declarators at
    // module scope, above the render fn) makes it pass.
    let projected =
        project("<div>{@render mySnip()}{#snippet mySnip()}<span>hi</span>{/snippet}</div>");
    let Some((ok, out)) = typecheck_projected(&projected, "Snippet.svelte.tsx", &[], true) else {
        skip_note("snippet ordering");
        return;
    };
    assert!(
        ok,
        "snippet-before-render must type-check clean (D-ap hoist, no TDZ):\n{out}"
    );
}

#[test]
fn runes_props_member_is_not_any_discriminating() {
    // DISCRIMINATING anti-`any`: a `$props()` member assigned to a deliberately
    // wrong type must FAIL — proving the member is typed (not `any`). If the
    // projection leaked `any`, this would pass (the bug the gate catches).
    let projected = project(
        "<script lang=\"ts\">\n\
         interface Props { label: string }\n\
         let { label }: Props = $props();\n\
         const wrong: number = label;\n\
         void wrong;\n\
         </script>\n\
         <div>{label}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "AntiAny.svelte.tsx", &[], true) else {
        skip_note("anti-any");
        return;
    };
    assert!(
        !ok,
        "a `$props()` member typed `string` must NOT assign to `number` \
         (proves the member is not `any`):\n{out}"
    );
    assert!(out.contains("label") || out.to_lowercase().contains("not assignable"));
}

#[test]
fn attach_mistyped_attachment_fails_typecheck_discriminating() {
    // D-ad: a mistyped `{@attach}` — an `Attachment<HTMLInputElement>` on a
    // `<canvas>` — FAILS type-check (discriminating both ways). The projector
    // routes `{@attach e}` through `__verter_attach(e)`; here we probe the
    // checker directly with an input-typed attachment used where a canvas is
    // expected.
    let projected = "/** @jsxImportSource @verter/svelte-jsx */\n\
        import type { Attachment } from \"svelte/attachments\";\n\
        declare function __verter_attach<E extends EventTarget>(a: Attachment<E>): void;\n\
        const inputAttach: Attachment<HTMLInputElement> = (el) => { void el.value; };\n\
        const canvasAttach: Attachment<HTMLCanvasElement> = inputAttach;\n\
        void canvasAttach;\n\
        export {};\n";
    let Some((ok, out)) = typecheck_projected(projected, "Attach.svelte.tsx", &[], true) else {
        skip_note("attach mistype");
        return;
    };
    assert!(
        !ok,
        "an Attachment<HTMLInputElement> must NOT be assignable to \
         Attachment<HTMLCanvasElement> (discriminating mistype):\n{out}"
    );
}

#[test]
fn snippet_brand_rejects_a_plain_function_discriminating() {
    // D-ae(c): a plain function passed where `Snippet<[T]>` is expected stays an
    // ERROR (the brand). DISCRIMINATING: the `__verter_snippet`-bridged binding
    // is accepted; a bare arrow is not.
    let projected = "/** @jsxImportSource @verter/svelte-jsx */\n\
        import type { Snippet } from \"svelte\";\n\
        const plain = (x: number) => x;\n\
        const asSnippet: Snippet<[number]> = plain;\n\
        void asSnippet;\n\
        export {};\n";
    let Some((ok, out)) = typecheck_projected(projected, "Brand.svelte.tsx", &[], true) else {
        skip_note("snippet brand");
        return;
    };
    assert!(
        !ok,
        "a plain function must NOT be assignable to a branded Snippet:\n{out}"
    );
}

#[test]
fn projected_each_with_else_type_checks_clean() {
    // The each-else projection must be valid TSX (not a malformed `.map` close).
    let projected = project(
        "<script lang=\"ts\">const items: string[] = [];</script>\n\
         <ul>{#each items as item}<li>{item}</li>{:else}<li>empty</li>{/each}</ul>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "EachElse.svelte.tsx", &[], true) else {
        skip_note("each-else");
        return;
    };
    assert!(
        ok,
        "each-with-else must type-check clean (valid TSX):\n{out}"
    );
}

#[test]
fn projected_empty_else_clause_type_checks_clean_no_raw_residue() {
    // P1-1: an EMPTY `{:else}` (no expr, no children) must rewrite to a valid
    // ternary arm — a raw `{:else}` would leak into the JSX expression container
    // and TSGO would reject it. DISCRIMINATING through the type checker.
    let projected = project(
        "<script lang=\"ts\">const c = true;</script>\n\
         <div>{#if c}<span>a</span>{:else}{/if}</div>",
    );
    assert!(
        !projected.contains("{:else}"),
        "no raw empty-else residue: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "EmptyElse.svelte.tsx", &[], true) else {
        skip_note("empty else clause");
        return;
    };
    assert!(
        ok,
        "an empty `{{:else}}` must produce valid, clean-type-checking TSX:\n{out}"
    );
}

#[test]
fn projected_empty_then_catch_clauses_type_check_clean_no_raw_residue() {
    // P1-1: empty `{:then}` / `{:catch}` (no binding, no children) must rewrite
    // cleanly — a raw `{:then}`/`{:catch}` would leak invalid TSX.
    let projected = project(
        "<script lang=\"ts\">const p: Promise<number> = Promise.resolve(1);</script>\n\
         <div>{#await p}loading{:then}{:catch}{/await}</div>",
    );
    assert!(
        !projected.contains("{:then}") && !projected.contains("{:catch}"),
        "no raw empty then/catch residue: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "EmptyThenCatch.svelte.tsx", &[], true)
    else {
        skip_note("empty then/catch clause");
        return;
    };
    assert!(
        ok,
        "empty `{{:then}}`/`{{:catch}}` must produce valid, clean-type-checking TSX:\n{out}"
    );
}

#[test]
fn projected_key_block_type_checks_clean() {
    // The `{#key}` projection must be valid TSX and check the key expression.
    let projected = project(
        "<script lang=\"ts\">const id = 1;</script>\n\
         <div>{#key id}<span>{id}</span>{/key}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Key.svelte.tsx", &[], true) else {
        skip_note("key block");
        return;
    };
    assert!(ok, "key block must type-check clean (valid TSX):\n{out}");
}

#[test]
fn projected_declaration_tag_is_visible_to_a_following_sibling() {
    // D-ap: a `{const}` value is typed AND visible to a sibling. The hoist makes
    // `{total}` after `{const total = …}` resolve cleanly.
    let projected = project(
        "<script lang=\"ts\">let a = 1; let b = 2;</script>\n\
         <div>{const total = a + b}<span>{total}</span></div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Decl.svelte.tsx", &[], true) else {
        skip_note("declaration tag");
        return;
    };
    assert!(
        ok,
        "a declaration-tag const must be visible to a following sibling \
         (D-ap hoist):\n{out}"
    );
}

#[test]
fn projected_special_element_and_trailing_style_type_check_clean() {
    // `<svelte:window>` close-tag rewrite + trailing `<style>` strip must leave
    // valid TSX (no `</svelte:window>` / `</style>` residue).
    let projected = project(
        "<svelte:window onkeydown={(e) => { void e; }} />\n\
         <div>x</div>\n\
         <style>.a { color: red; }</style>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Special.svelte.tsx", &[], true) else {
        skip_note("special + trailing style");
        return;
    };
    // No raw residue survived the projection.
    assert!(
        !projected.contains("</svelte:window>"),
        "no svelte close residue"
    );
    assert!(!projected.contains("</style>"), "no style close residue");
    assert!(
        ok,
        "special element + trailing style must type-check clean:\n{out}"
    );
}

#[test]
fn projected_css_custom_property_strips_and_void_checks() {
    // D-ap: `--x={expr}` strips the JSX attribute (no `--` residue) and
    // void-checks the value. A deliberate type error in the value surfaces.
    let good = project("<script lang=\"ts\">let c = \"red\";</script>\n<div --accent={c}>x</div>");
    let Some((ok, out)) = typecheck_projected(&good, "CssProp.svelte.tsx", &[], true) else {
        skip_note("css custom prop");
        return;
    };
    assert!(
        !good.contains("--accent"),
        "no `--` attribute residue: {good}"
    );
    assert!(ok, "css custom-prop value must void-check clean:\n{out}");

    // DISCRIMINATING: a type error INSIDE the value surfaces (the value is
    // genuinely checked, not dropped).
    let bad =
        project("<script lang=\"ts\">let c: number = 1;</script>\n<div --accent={c.nope}>x</div>");
    let Some((bad_ok, _)) = typecheck_projected(&bad, "CssPropBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a type error in the void-checked value must surface"
    );
}

#[test]
fn projected_trailing_script_after_markup_type_checks_clean() {
    // A script AFTER the markup must be hoisted above the render fn (not left
    // inside/after the fragment) — valid TSX.
    let projected = project("<div>{a}</div>\n<script lang=\"ts\">const a = 1;</script>");
    let Some((ok, out)) = typecheck_projected(&projected, "Trailing.svelte.tsx", &[], true) else {
        skip_note("trailing script");
        return;
    };
    // The render fragment closes BEFORE the hoisted script does not strand it.
    assert!(
        ok,
        "a trailing script must type-check clean (hoisted):\n{out}"
    );
}

#[test]
fn projected_await_catch_binding_resolves() {
    // The `{:catch e}` binding must be declared so the catch body's `{e}`
    // resolves — valid TSX.
    let projected = project(
        "<script lang=\"ts\">const p: Promise<number> = Promise.resolve(1);</script>\n\
         <div>{#await p}loading{:then v}{v}{:catch e}{e}{/await}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Await.svelte.tsx", &[], true) else {
        skip_note("await catch");
        return;
    };
    assert!(ok, "await catch binding must resolve (valid TSX):\n{out}");
}

#[test]
fn projected_attribute_shorthand_type_checks_clean() {
    // `<input {value} />` shorthand must become `value={value}` (valid TSX).
    let projected = project("<script lang=\"ts\">const value = \"x\";</script>\n<input {value} />");
    let Some((ok, out)) = typecheck_projected(&projected, "Shorthand.svelte.tsx", &[], true) else {
        skip_note("attribute shorthand");
        return;
    };
    assert!(
        !projected.contains("<input {value}"),
        "shorthand rewritten: {projected}"
    );
    assert!(ok, "attribute shorthand must type-check clean:\n{out}");
}

#[test]
fn out_of_scope_bind_this_void_checks_and_type_checks_clean() {
    // P1-2: an out-of-scope `bind:this` must be stripped + void-checked (NOT
    // left as a bare invalid `this={…}` attribute) and produce valid TSX. A
    // `bind:this` left as a bare attribute would be rejected by TSGO (no `this`
    // on the input intrinsic).
    let projected = project(
        "<script lang=\"ts\">let el: HTMLInputElement | null = null;</script>\n\
         <input bind:this={el} />",
    );
    assert!(
        !projected.contains("bind:this"),
        "no bind:this residue: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BindThis.svelte.tsx", &[], true) else {
        skip_note("out-of-scope bind:this");
        return;
    };
    assert!(
        ok,
        "an out-of-scope bind:this must void-check + produce valid TSX:\n{out}"
    );

    // DISCRIMINATING: a type error INSIDE the void-checked bound expression
    // surfaces (the expression is genuinely checked, not dropped).
    let bad = project(
        "<script lang=\"ts\">let el: HTMLInputElement | null = null;</script>\n\
         <input bind:this={el.nope} />",
    );
    let Some((bad_ok, _)) = typecheck_projected(&bad, "BindThisBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a type error in the void-checked bound expression must surface"
    );
}

#[test]
fn function_binding_void_check_type_checks_clean_with_variadic_void() {
    // A function binding `bind:x={get, set}` void-checks BOTH expressions as
    // `__verter_void(get, set)` — the variadic `__verter_void` declaration must
    // accept multiple args (a single-param decl would emit an extra-argument
    // error under strict). Valid TSX, both exprs checked.
    let projected = project(
        "<script lang=\"ts\">const get = () => \"x\"; const set = (v: string) => { void v; };</script>\n\
         <input bind:value={get, set} />",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "FnBind.svelte.tsx", &[], true) else {
        skip_note("function binding void-check arity");
        return;
    };
    assert!(
        ok,
        "a function-binding void-check `__verter_void(get, set)` must type-check \
         clean (variadic void):\n{out}"
    );
}

#[test]
fn bind_value_shorthand_type_checks_clean() {
    // `<input bind:value />` shorthand must become `value={value}` (NOT a bare
    // boolean `value`) — valid TSX checking the bound local.
    let projected =
        project("<script lang=\"ts\">const value = \"x\";</script>\n<input bind:value />");
    assert!(
        projected.contains("value={value}"),
        "shorthand self-bound: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BindShorthand.svelte.tsx", &[], true)
    else {
        skip_note("bind:value shorthand");
        return;
    };
    assert!(ok, "bind:value shorthand must type-check clean:\n{out}");
}

#[test]
fn out_of_scope_style_directive_void_checks_and_type_checks_clean() {
    // P2-1: a `style:` directive must be stripped + void-checked (NOT left as a
    // bare invalid `style:color` attribute) and produce valid TSX.
    let projected =
        project("<script lang=\"ts\">let c = \"red\";</script>\n<div style:color={c}>x</div>");
    assert!(
        !projected.contains("style:color"),
        "no style: residue: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "StyleDir.svelte.tsx", &[], true) else {
        skip_note("out-of-scope style: directive");
        return;
    };
    assert!(
        ok,
        "an out-of-scope style: directive must void-check + produce valid TSX:\n{out}"
    );
}

#[test]
fn missing_svelte_package_fails_closed_with_module_not_found() {
    // D-ae(d): a workspace WITHOUT `svelte` fails CLOSED — the shim's
    // `import type { Snippet } from "svelte"` is module-not-found, no ambient
    // stub, no `any`. DISCRIMINATING: do NOT vendor svelte here.
    let projected = project("<div>{x}</div>");
    let Some((ok, out)) = typecheck_projected(&projected, "NoSvelte.svelte.tsx", &[], false) else {
        skip_note("missing-svelte fail-closed");
        return;
    };
    assert!(
        !ok,
        "without a vendored `svelte`, the projection must fail CLOSED \
         (module-not-found), not silently pass:\n{out}"
    );
    assert!(
        out.contains("svelte") || out.to_lowercase().contains("cannot find module"),
        "the failure must be the missing `svelte` module:\n{out}"
    );
}
