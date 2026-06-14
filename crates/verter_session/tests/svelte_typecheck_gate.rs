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
            "transition.d.ts",
            "animate.d.ts",
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

/// The projected render body (everything AFTER the unmapped prelude) — residue
/// assertions on directive prefixes (`bind:this`, …) target the body, not the
/// prelude's own checker doc comments.
fn render_body(code: &str) -> &str {
    code.find("function __verter_render()")
        .map(|i| &code[i..])
        .unwrap_or(code)
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
fn bind_this_on_an_intrinsic_produces_valid_tsx_for_a_normal_declaration() {
    // F4: a normal `bind:this={el}` declaration (`let el: HTMLInputElement`, no
    // initializer — the idiomatic Svelte form) projects to valid TSX that
    // type-checks clean, with NO `bind:this` residue and NO bare `this={…}`
    // attribute (which the intrinsic table would reject).
    let projected = project(
        "<script lang=\"ts\">let el: HTMLInputElement;</script>\n\
         <input bind:this={el} />",
    );
    assert!(
        !render_body(&projected).contains("bind:this"),
        "no bind:this residue: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BindThisDecl.svelte.tsx", &[], true)
    else {
        skip_note("bind:this declaration");
        return;
    };
    assert!(
        ok,
        "a normal bind:this declaration must produce valid, clean TSX:\n{out}"
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
fn style_directive_strips_void_checks_and_surfaces_value_type_errors() {
    // F1: a `style:color={c}` directive is SUPPORTED — stripped from the JSX
    // position (no invalid `style:color` attribute), value void-checked, clean
    // TSX.
    let good =
        project("<script lang=\"ts\">let c = \"red\";</script>\n<div style:color={c}>x</div>");
    assert!(!good.contains("style:color"), "no style: residue: {good}");
    let Some((ok, out)) = typecheck_projected(&good, "StyleDir.svelte.tsx", &[], true) else {
        skip_note("style: directive");
        return;
    };
    assert!(
        ok,
        "a supported style: directive must void-check + produce valid TSX:\n{out}"
    );

    // DISCRIMINATING: a type error INSIDE the value surfaces (the value is
    // genuinely checked, not dropped).
    let bad = project(
        "<script lang=\"ts\">let c: number = 1;</script>\n<div style:color={c.nope}>x</div>",
    );
    let Some((bad_ok, _)) = typecheck_projected(&bad, "StyleDirBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a type error in the void-checked style: value must surface"
    );
}

#[test]
fn transition_directive_type_checks_clean_and_discriminates_params_and_non_function() {
    // F2: a `transition:fly={{ delay: 200 }}` projects to a REAL CALL
    // `__verter_transition(fly((null! as HostEl), { delay: 200 }))` and
    // type-checks CLEAN against the vendored `svelte/transition`. The fixture
    // imports `fly` so the called function resolves.
    let good = project(
        "<script lang=\"ts\">import { fly } from \"svelte/transition\";</script>\n\
         <div transition:fly={{ delay: 200 }}>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "TransOk.svelte.tsx", &[], true) else {
        skip_note("transition fly clean");
        return;
    };
    assert!(
        ok,
        "transition:fly={{ delay: 200 }} must type-check clean (host-element call):\n{out}"
    );

    // DISCRIMINATING (wrong params): a `{ delay: "200" }` (string where number is
    // expected) must FAIL — the params are genuinely checked at the `fly` call.
    let bad_params = project(
        "<script lang=\"ts\">import { fly } from \"svelte/transition\";</script>\n\
         <div transition:fly={{ delay: \"200\" }}>x</div>",
    );
    let Some((bp_ok, bp_out)) =
        typecheck_projected(&bad_params, "TransBadParams.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bp_ok,
        "a wrong-typed transition param (`delay: \"200\"`) must be REJECTED:\n{bp_out}"
    );

    // DISCRIMINATING (non-function value): a `transition:fn` whose `fn` is NOT a
    // transition function (a plain number) must FAIL — the projected `fn(...)`
    // call is not callable.
    let non_fn = project(
        "<script lang=\"ts\">const notAFn = 5;</script>\n\
         <div transition:notAFn={{ delay: 1 }}>x</div>",
    );
    let Some((nf_ok, nf_out)) = typecheck_projected(&non_fn, "TransNonFn.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !nf_ok,
        "a non-function `transition:` value must be REJECTED (not callable):\n{nf_out}"
    );
}

#[test]
fn animate_directive_type_checks_clean_and_discriminates_wrong_params() {
    // F3: an `animate:flip={{ delay: 0 }}` projects to a REAL CALL
    // `__verter_animate(flip((null! as HostEl), DIRECTIONS, { delay: 0 }))` and
    // type-checks CLEAN.
    let good = project(
        "<script lang=\"ts\">import { flip } from \"svelte/animate\";</script>\n\
         {#each [] as _ (_)}<div animate:flip={{ delay: 0 }}>x</div>{/each}",
    );
    let Some((ok, out)) = typecheck_projected(&good, "AnimOk.svelte.tsx", &[], true) else {
        skip_note("animate flip clean");
        return;
    };
    assert!(
        ok,
        "animate:flip={{ delay: 0 }} must type-check clean:\n{out}"
    );

    // DISCRIMINATING (wrong params): a `{ delay: "0" }` (string where number is
    // expected) must FAIL.
    let bad = project(
        "<script lang=\"ts\">import { flip } from \"svelte/animate\";</script>\n\
         {#each [] as _ (_)}<div animate:flip={{ delay: \"0\" }}>x</div>{/each}",
    );
    let Some((b_ok, b_out)) = typecheck_projected(&bad, "AnimBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !b_ok,
        "a wrong-typed animate param (`delay: \"0\"`) must be REJECTED:\n{b_out}"
    );
}

#[test]
fn transition_missing_required_params_fails_via_arg_count() {
    // F2 (required params): a `transition:fn` whose `fn` REQUIRES a params object,
    // written WITHOUT `={…}`, must FAIL — the projected `fn(node)` call is missing
    // the required 2nd argument (arg-count error). DISCRIMINATING: the same `fn`
    // WITH the params type-checks clean.
    let bad = project(
        "<script lang=\"ts\">\n\
         import type { TransitionConfig } from \"svelte/transition\";\n\
         function spin(node: Element, params: { turns: number }): TransitionConfig { void node; void params; return {}; }\n\
         </script>\n\
         <div transition:spin>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&bad, "TransNoParams.svelte.tsx", &[], true) else {
        skip_note("transition required params arg-count");
        return;
    };
    assert!(
        !ok,
        "a `transition:` whose fn REQUIRES params must FAIL without params \
         (arg-count error):\n{out}"
    );

    let good = project(
        "<script lang=\"ts\">\n\
         import type { TransitionConfig } from \"svelte/transition\";\n\
         function spin(node: Element, params: { turns: number }): TransitionConfig { void node; void params; return {}; }\n\
         </script>\n\
         <div transition:spin={{ turns: 2 }}>x</div>",
    );
    let Some((g_ok, g_out)) = typecheck_projected(&good, "TransWithParams.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        g_ok,
        "the same `transition:spin` WITH its required params must type-check clean:\n{g_out}"
    );
}

#[test]
fn custom_transition_with_optional_options_and_factory_return_type_checks_clean() {
    // F2 (custom transition shape): a userland transition with an OPTIONAL third
    // `options` param (the Svelte transition-fn shape — built-ins omit it; custom
    // transitions that DECLARE it must keep it optional so the projected
    // `fn(node, params)` 2-arg call applies) AND a DEFERRED-FACTORY return
    // (`() => TransitionConfig` / `(options?) => TransitionConfig`) must
    // type-check clean through the `__verter_transition` result-shape checker.
    let factory_arrow = project(
        "<script lang=\"ts\">\n\
         import type { TransitionConfig } from \"svelte/transition\";\n\
         function spin(node: Element, params: { turns: number }, options?: { direction: \"in\" | \"out\" | \"both\" }): () => TransitionConfig { void node; void params; void options; return () => ({}); }\n\
         </script>\n\
         <div transition:spin={{ turns: 2 }}>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&factory_arrow, "TransCustom.svelte.tsx", &[], true)
    else {
        skip_note("custom transition optional-options + factory return");
        return;
    };
    assert!(
        ok,
        "a custom transition with an optional options param + factory return must \
         type-check clean (2-arg call + factory result shape):\n{out}"
    );

    // DISCRIMINATING: a custom transition that returns the WRONG shape (a number,
    // not a TransitionConfig or factory) must FAIL the result-shape checker.
    let wrong_return = project(
        "<script lang=\"ts\">\n\
         function broken(node: Element, params: { turns: number }): number { void node; void params; return 5; }\n\
         </script>\n\
         <div transition:broken={{ turns: 2 }}>x</div>",
    );
    let Some((wr_ok, wr_out)) =
        typecheck_projected(&wrong_return, "TransWrongRet.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !wr_ok,
        "a transition fn returning a non-TransitionConfig must be REJECTED by the \
         result-shape checker:\n{wr_out}"
    );
}

#[test]
fn bind_this_on_an_intrinsic_binds_the_dom_element_type_discriminating() {
    // F4: `bind:this={el}` on an `<input>` checks the bound local against the
    // DOM element instance type (`HTMLInputElement`). DISCRIMINATING: a correctly
    // typed `HTMLInputElement` local checks clean; a `HTMLDivElement` local FAILS
    // (the wrong element type) — guarded by a `@ts-expect-error` so the fixture
    // discriminates BOTH ways under strict (an `any` leak would make the
    // `@ts-expect-error` unused → TS errors).
    let projected = project(
        "<script lang=\"ts\">let el: HTMLInputElement;</script>\n\
         <input bind:this={el} />",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BindThisOk.svelte.tsx", &[], true)
    else {
        skip_note("bind:this intrinsic");
        return;
    };
    assert!(
        ok,
        "bind:this on <input> must bind HTMLInputElement clean:\n{out}"
    );

    // A wrong element type (`HTMLDivElement`) must FAIL the host-instance check.
    let bad = project(
        "<script lang=\"ts\">let el: HTMLDivElement;</script>\n\
         <input bind:this={el} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "BindThisBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "bind:this on <input> bound to HTMLDivElement must be REJECTED:\n{bad_out}"
    );
}

#[test]
fn bind_this_on_a_component_binds_the_instance_type_discriminating() {
    // F4: `bind:this` on a component binds `InstanceType<typeof C>`. A local
    // typed with the instance type checks clean; a mismatched local FAILS.
    let good = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare function __verter_bind_this_assignable<Host, To extends Host>(): void;\n\
        declare class Child { $props: { label?: string }; method(): number; }\n\
        ;function __verter_render() {\n\
        let ref: InstanceType<typeof Child>;\n\
        return (<><Child {...((ref = (null! as InstanceType<typeof Child>)), __verter_bind_this_assignable<InstanceType<typeof Child>, typeof ref>(), {})} /></>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(good, "BindThisCompOk.svelte.tsx", &[], true) else {
        skip_note("bind:this component");
        return;
    };
    assert!(
        ok,
        "bind:this on a component must bind its InstanceType clean:\n{out}"
    );

    let bad = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare function __verter_bind_this_assignable<Host, To extends Host>(): void;\n\
        declare class Child { $props: { label?: string }; method(): number; }\n\
        ;function __verter_render() {\n\
        let ref: number;\n\
        return (<><Child {...((ref = (null! as InstanceType<typeof Child>)), __verter_bind_this_assignable<InstanceType<typeof Child>, typeof ref>(), {})} /></>);\n\
        }\nexport {};\n";
    let Some((bad_ok, bad_out)) = typecheck_projected(bad, "BindThisCompBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "bind:this on a component bound to a `number` local must be REJECTED:\n{bad_out}"
    );
}

#[test]
fn bind_group_checkbox_requires_an_array_and_radio_requires_a_scalar() {
    // F4: `bind:group` checkbox → array shape; radio → scalar; a loose `T | T[]`
    // is rejected by both.
    // Checkbox + array local: clean.
    let cb_ok = project(
        "<script lang=\"ts\">let selected: string[] = [];</script>\n\
         <input type=\"checkbox\" bind:group={selected} />",
    );
    let Some((ok, out)) = typecheck_projected(&cb_ok, "GroupCbOk.svelte.tsx", &[], true) else {
        skip_note("bind:group checkbox");
        return;
    };
    assert!(ok, "checkbox bind:group on a string[] must check:\n{out}");

    // Checkbox + scalar local: FAILS (not an array).
    let cb_bad = project(
        "<script lang=\"ts\">let selected: string = \"\";</script>\n\
         <input type=\"checkbox\" bind:group={selected} />",
    );
    let Some((cb_bad_ok, cb_bad_out)) =
        typecheck_projected(&cb_bad, "GroupCbBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !cb_bad_ok,
        "checkbox bind:group on a scalar must be REJECTED (array required):\n{cb_bad_out}"
    );

    // Radio + scalar local: clean.
    let radio_ok = project(
        "<script lang=\"ts\">let picked: string = \"\";</script>\n\
         <input type=\"radio\" bind:group={picked} />",
    );
    let Some((r_ok, r_out)) = typecheck_projected(&radio_ok, "GroupRadioOk.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(r_ok, "radio bind:group on a scalar must check:\n{r_out}");

    // Radio + array local: FAILS (radio shares a scalar).
    let radio_bad = project(
        "<script lang=\"ts\">let picked: string[] = [];</script>\n\
         <input type=\"radio\" bind:group={picked} />",
    );
    let Some((rb_ok, rb_out)) =
        typecheck_projected(&radio_bad, "GroupRadioBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !rb_ok,
        "radio bind:group on an array must be REJECTED (scalar required):\n{rb_out}"
    );

    // Loose `T | T[]` union local: rejected by BOTH (neither cleanly scalar nor
    // array). The radio gate must use the DISTRIBUTIVE conditional — a
    // non-distributive `[L] extends [readonly unknown[]]` would wrongly ACCEPT
    // the union.
    let radio_union = project(
        "<script lang=\"ts\">let picked: string | string[] = \"\";</script>\n\
         <input type=\"radio\" bind:group={picked} />",
    );
    let Some((ru_ok, ru_out)) =
        typecheck_projected(&radio_union, "GroupRadioUnion.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !ru_ok,
        "radio bind:group on a loose `T | T[]` union must be REJECTED:\n{ru_out}"
    );
    let cb_union = project(
        "<script lang=\"ts\">let selected: string | string[] = [];</script>\n\
         <input type=\"checkbox\" bind:group={selected} />",
    );
    let Some((cu_ok, cu_out)) =
        typecheck_projected(&cb_union, "GroupCbUnion.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !cu_ok,
        "checkbox bind:group on a loose `T | T[]` union must be REJECTED:\n{cu_out}"
    );
}

#[test]
fn readonly_function_binding_requires_null_get_and_rejects_a_getter() {
    // F5 (readonly direction): a readonly element binding's function form must be
    // the write-only `{null, set}` — `__verter_bind_fn_read` accepts a `null` get
    // and a `set`. A NON-null getter FAILS (readonly cannot read into the DOM).
    let null_get = project(
        "<script lang=\"ts\">const setW = (v: number): void => { void v; };</script>\n\
         <div bind:clientWidth={null, setW}></div>",
    );
    let Some((ok, out)) = typecheck_projected(&null_get, "RoFnOk.svelte.tsx", &[], true) else {
        skip_note("readonly function binding null get");
        return;
    };
    assert!(
        ok,
        "a readonly function binding `{{null, set}}` must check clean:\n{out}"
    );

    let with_getter = project(
        "<script lang=\"ts\">const getW = (): number => 1; const setW = (v: number): void => { void v; };</script>\n\
         <div bind:clientWidth={getW, setW}></div>",
    );
    let Some((g_ok, g_out)) = typecheck_projected(&with_getter, "RoFnBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !g_ok,
        "a readonly function binding with a non-null getter must be REJECTED:\n{g_out}"
    );
}

#[test]
fn bind_current_time_binds_number_and_readonly_bind_duration_rejects_a_write() {
    // F4: `bind:currentTime` binds a `number` (read-write); `bind:duration` is
    // readonly — assigning to a `const` (a write target) FAILS.
    let ct_ok = project(
        "<script lang=\"ts\">let t: number = 0;</script>\n\
         <video bind:currentTime={t}></video>",
    );
    let Some((ok, out)) = typecheck_projected(&ct_ok, "CurrentTime.svelte.tsx", &[], true) else {
        skip_note("bind:currentTime");
        return;
    };
    assert!(ok, "bind:currentTime must bind a number clean:\n{out}");

    // A wrong-typed `currentTime` local (string) FAILS the invariant check.
    let ct_bad = project(
        "<script lang=\"ts\">let t: string = \"\";</script>\n\
         <video bind:currentTime={t}></video>",
    );
    let Some((cb_ok, cb_out)) =
        typecheck_projected(&ct_bad, "CurrentTimeBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !cb_ok,
        "bind:currentTime on a string local must be REJECTED:\n{cb_out}"
    );

    // Readonly `bind:duration` to a `const` is a write-target the projection
    // ASSIGNS into → must FAIL (cannot assign to a constant).
    let dur_bad = project(
        "<script lang=\"ts\">const d: number = 0;</script>\n\
         <video bind:duration={d}></video>",
    );
    let Some((d_ok, d_out)) = typecheck_projected(&dur_bad, "DurationConst.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !d_ok,
        "a readonly bind:duration written to a `const` target must be REJECTED:\n{d_out}"
    );
    // DISCRIMINATING: the same readonly binding to a writable `let number` checks.
    let dur_ok = project(
        "<script lang=\"ts\">let d: number = 0;</script>\n\
         <video bind:duration={d}></video>",
    );
    let Some((do_ok, do_out)) = typecheck_projected(&dur_ok, "DurationLet.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        do_ok,
        "a readonly bind:duration into a writable `let number` must check:\n{do_out}"
    );
}

#[test]
fn function_binding_get_set_mismatch_fails_and_consistent_pair_checks() {
    // F5: `bind:files={get, set}` — the checker enforces get/set consistency
    // against the table type `FileList | null`. A consistent pair checks; a
    // get/set type mismatch FAILS.
    let ok_src = project(
        "<script lang=\"ts\">\n\
         const getFiles = (): FileList | null => null;\n\
         const setFiles = (f: FileList | null): void => { void f; };\n\
         </script>\n\
         <input type=\"file\" bind:files={getFiles, setFiles} />",
    );
    let Some((ok, out)) = typecheck_projected(&ok_src, "FnBindOk.svelte.tsx", &[], true) else {
        skip_note("function binding get/set");
        return;
    };
    assert!(
        ok,
        "a consistent get/set pair against FileList | null must check:\n{out}"
    );

    // Mismatch: `set` consumes a `string` while the target type is FileList|null.
    let bad_src = project(
        "<script lang=\"ts\">\n\
         const getFiles = (): FileList | null => null;\n\
         const setFiles = (s: string): void => { void s; };\n\
         </script>\n\
         <input type=\"file\" bind:files={getFiles, setFiles} />",
    );
    let Some((b_ok, b_out)) = typecheck_projected(&bad_src, "FnBindBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !b_ok,
        "a get/set type mismatch against the bind-target type must be REJECTED:\n{b_out}"
    );
}

#[test]
fn function_binding_on_value_intrinsic_type_checks_clean_for_a_correct_pair() {
    // F5: `bind:value={get,set}` on an `<input>` (NOT in the wide-family table)
    // derives `V` from `SvelteHTMLElements["input"]["value"]` (the producer-side
    // fix; the emitted target type is pinned by the `projector_tests`
    // `function_binding_on_value_derives_the_target_type_from_the_intrinsic_table`).
    // A correct union-handling get/set type-checks CLEAN.
    let ok_src = project(
        "<script lang=\"ts\">\n\
         const getV = (): string | number | undefined => \"x\";\n\
         const setV = (v: string | number | undefined): void => { void v; };\n\
         </script>\n\
         <input bind:value={getV, setV} />",
    );
    let Some((ok, out)) = typecheck_projected(&ok_src, "FnValueOk.svelte.tsx", &[], true) else {
        skip_note("function binding value intrinsic type");
        return;
    };
    assert!(
        ok,
        "a value function binding handling the intrinsic value type must check:\n{out}"
    );
}

#[test]
#[ignore = "known tsgo native-preview gap (same family as B8d-P2-1): under the \
            @jsxImportSource pragma, tsgo does NOT fully type-check a generic \
            checker call whose type argument is an indexed-access into an \
            imported interface (`SvelteHTMLElements[\"input\"][\"value\"]`) when it \
            is wrapped in a `{...(call(...), {})}` JSX spread, so a wrong-typed \
            get/set is silently accepted. The PRODUCER emits the correct type \
            (pinned by the projector test); an EXPLICIT type argument (e.g. \
            `FileList | null` for `bind:files`) discriminates reliably. Red \
            against current tsgo; flips green when the upstream bug is fixed."]
fn function_binding_on_value_rejects_a_wrong_typed_pair_known_tsgo_gap() {
    // R10 ledger (DISCRIMINATING): a `set` whose param is NOT in the intrinsic
    // value union (a `symbol`) SHOULD be REJECTED. Under tsgo native-preview the
    // indexed-access type argument is not enforced inside the JSX spread, so it
    // is silently accepted today.
    let bad_src = project(
        "<script lang=\"ts\">\n\
         const getV = (): symbol => Symbol();\n\
         const setV = (v: symbol): void => { void v; };\n\
         </script>\n\
         <input bind:value={getV, setV} />",
    );
    let Some((b_ok, b_out)) = typecheck_projected(&bad_src, "FnValueBad.svelte.tsx", &[], true)
    else {
        skip_note("function binding value wrong type known gap");
        return;
    };
    assert!(
        !b_ok,
        "a `symbol` get/set for bind:value SHOULD be REJECTED via the intrinsic \
         attribute type (currently a tsgo spread-context gap — see #[ignore]):\n{b_out}"
    );
}

#[test]
fn function_binding_on_a_component_checks_against_instancetype_props() {
    // F5: a component function binding derives `V` from
    // `InstanceType<typeof Child>["$props"]["value"]` — typed in the PROJECTED TSX
    // via TS (no Rust resolver). A consistent get/set checks; a mismatch FAILS.
    let good = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare function __verter_bind_fn<V>(get: (() => V) | null, set: (value: V) => void): void;\n\
        declare class Child { $props: { value?: number }; }\n\
        ;function __verter_render() {\n\
        const get = (): number | undefined => 1;\n\
        const set = (v: number | undefined): void => { void v; };\n\
        return (<><Child {...(__verter_bind_fn<InstanceType<typeof Child>[\"$props\"][\"value\"]>(get, set), {})} /></>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(good, "FnBindCompOk.svelte.tsx", &[], true) else {
        skip_note("component function binding");
        return;
    };
    assert!(
        ok,
        "a component function binding against $props[\"value\"]: number must check:\n{out}"
    );

    let bad = "/** @jsxImportSource @verter/svelte-jsx */\n\
        declare function __verter_bind_fn<V>(get: (() => V) | null, set: (value: V) => void): void;\n\
        declare class Child { $props: { value?: number }; }\n\
        ;function __verter_render() {\n\
        const get = (): number => 1;\n\
        const set = (v: string): void => { void v; };\n\
        return (<><Child {...(__verter_bind_fn<InstanceType<typeof Child>[\"$props\"][\"value\"]>(get, set), {})} /></>);\n\
        }\nexport {};\n";
    let Some((b_ok, b_out)) = typecheck_projected(bad, "FnBindCompBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !b_ok,
        "a component function binding whose set consumes the wrong type must be \
         REJECTED via $props:\n{b_out}"
    );
}

#[test]
#[ignore = "B8d-P2-1 known gap: tsgo native-preview silently accepts a wrong-typed \
            value on a transition-param property NOT shared with TransitionConfig \
            (e.g. fly's `y`) under the @jsxImportSource pragma. Red against \
            current tsgo; flips green when the upstream tsgo bug is fixed. \
            Upstream: contravariant optional-param inference loses the param \
            discrimination for non-shared interface members."]
fn transition_specific_param_wrong_type_is_rejected_known_tsgo_gap() {
    // B8d-P2-1 (R10 ledger, DISCRIMINATING): `transition:fly={{ y: "200" }}` — a
    // wrong-typed `y` (string where number is expected) on the `fly`-SPECIFIC
    // param SHOULD be rejected. Under tsgo native-preview it is silently ACCEPTED
    // (the `delay` fixture — a TransitionConfig-SHARED member — discriminates
    // reliably and is the live gate; this `y` case characterizes the gap). When
    // tsgo fixes the upstream bug this test (asserting REJECTION) flips to green
    // and the `#[ignore]` is removed.
    let bad = project(
        "<script lang=\"ts\">import { fly } from \"svelte/transition\";</script>\n\
         <div transition:fly={{ y: \"200\" }}>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&bad, "TransFlyY.svelte.tsx", &[], true) else {
        skip_note("transition fly y known gap");
        return;
    };
    assert!(
        !ok,
        "a wrong-typed transition-specific param (`y: \"200\"`) SHOULD be REJECTED \
         (currently a tsgo native-preview gap — see #[ignore]):\n{out}"
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
