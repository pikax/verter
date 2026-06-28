//! The Svelte IDE-projection TYPE-CHECK VALIDITY gate.
//!
//! OXC parse-only is NOT sufficient: the projected `.svelte.tsx` must type-check
//! CLEAN through the typescript-go engine. This harness projects each fixture
//! through the real Svelte IDE projector, writes it into a hermetic temp project
//! (vendored `svelte` types + the in-repo `@verter/svelte-jsx` shim
//! `paths`-mapped — no npm install, Testing-Hermeticity), and runs the rc
//! `typescript` launcher (`node typescript/lib/tsc.js --noEmit`).
//!
//! GATE PRECONDITION: the pragma-parity fixture proves the
//! `@jsxImportSource @verter/svelte-jsx` pragma OVERRIDES a project-level
//! `jsxImportSource: "vue"` under the engine. If the engine fails the override,
//! the fallback is a STOP-and-redesign (escalate) — never a silent degrade.
//!
//! The harness is GATED behind the locally-resolvable rc `typescript` launcher
//! (`typescript/lib/tsc.js`): when it is not found (a machine without an
//! install) the tests skip with a clear message rather than failing spuriously.
//! On CI with the launcher present they run for real. When the environment
//! REQUIRES a checker (`CI` or `VERTER_REQUIRE_TYPECHECKER` set), a missing
//! checker is a HARD failure — the gate must never silently skip where it is
//! meant to run, which would mask a Svelte projection regression.

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

/// Returns `true` when the environment REQUIRES a type checker to be present:
/// CI runs (`CI` set) or an explicit opt-in (`VERTER_REQUIRE_TYPECHECKER`).
/// On such machines a missing checker is a HARD failure, not a silent skip —
/// otherwise the whole gate could mask a Svelte projection regression by
/// quietly skipping every test.
fn require_type_checker() -> bool {
    fn truthy(name: &str) -> bool {
        std::env::var_os(name).is_some_and(|v| {
            let v = v.to_string_lossy();
            let v = v.trim();
            !v.is_empty() && !v.eq_ignore_ascii_case("0") && !v.eq_ignore_ascii_case("false")
        })
    }
    truthy("CI") || truthy("VERTER_REQUIRE_TYPECHECKER")
}

/// Locate the `typescript@>=7` (rc) `tsc.js` launcher under the workspace
/// `node_modules`.
///
/// The rc `typescript` package ships its CLI as a thin Node launcher
/// (`typescript/lib/tsc.js`) that resolves the per-platform typescript-go
/// engine binary (`@typescript/typescript-<platform>-<arch>`) internally and
/// `execFileSync`s it. Invoking it through `node` (see [`typecheck_projected`])
/// is OS-AGNOSTIC: it needs only `node` on `PATH` and never depends on a
/// `.bin/tsc` pnpm shim (a `#!/bin/sh` / `.CMD` wrapper that `CreateProcess`
/// cannot launch directly on Windows → `Os 193`).
///
/// Returns `None` when the launcher is not present, so the gate SKIPS on
/// hermetic dev machines without an install. BUT when the environment REQUIRES
/// a checker ([`require_type_checker`] — CI, or an explicit
/// `VERTER_REQUIRE_TYPECHECKER`), a missing checker is a HARD failure: the gate
/// must not silently skip every test (and thereby mask a regression) exactly
/// where it is meant to run for real.
fn locate_type_checker() -> Option<PathBuf> {
    let node_modules = workspace_root().join("node_modules");

    // Hoisted `typescript/lib/tsc.js` (a pnpm symlink resolves through here).
    let hoisted = node_modules.join("typescript").join("lib").join("tsc.js");
    if hoisted.is_file() {
        return Some(assert_rc_engine_launcher(hoisted));
    }

    // pnpm virtual-store fallback: `.pnpm/typescript@<ver>/node_modules/typescript/lib/tsc.js`.
    // Enumerate the `typescript@*` store entries and keep the highest-VERSION one
    // (the rc/TS>=7 engine), not the lexicographically-last (which could be a
    // legacy `typescript@6.x` JS tsc).
    let pnpm_dir = node_modules.join(".pnpm");
    if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                // The JS `typescript@<ver>` package, not the `+`-named platform
                // package (which holds the engine binary, not `tsc.js`).
                if !name.starts_with("typescript@") || name.contains('+') {
                    return None;
                }
                let launcher = entry.path().join("node_modules/typescript/lib/tsc.js");
                launcher.is_file().then_some(launcher)
            })
            .collect();
        // Highest owning-package version wins (the rc engine), not lexicographic.
        candidates.sort_by(|a, b| {
            typescript_package_major(a)
                .cmp(&typescript_package_major(b))
                .then_with(|| a.cmp(b))
        });
        if let Some(launcher) = candidates.pop() {
            return Some(assert_rc_engine_launcher(launcher));
        }
    }

    assert!(
        !require_type_checker(),
        "the Svelte typecheck gate REQUIRES a type checker here \
         (CI / VERTER_REQUIRE_TYPECHECKER is set) but the rc `typescript` \
         launcher (`typescript/lib/tsc.js`) was not found under {}. A silent \
         skip would mask Svelte projection regressions — run `pnpm install` \
         or unset the env var for a local dev skip.",
        node_modules.display()
    );
    None
}

/// Read the major version of the `typescript` package owning a located
/// `tsc.js` launcher. `tsc.js` lives in `typescript/lib/`, so the package's
/// `package.json` is two directories up. Returns `None` when unreadable.
///
/// Dependency-free string scan for `"version": "X.Y.Z"` (no serde) — the same
/// approach as the runtime's `detect_ts_major_version`.
fn typescript_package_major(tsc_js: &std::path::Path) -> Option<u32> {
    let pkg_json = tsc_js.parent()?.parent()?.join("package.json");
    let content = std::fs::read_to_string(pkg_json).ok()?;
    let key = content.find("\"version\"")?;
    let after = &content[key..];
    let colon = after.find(':')?;
    let after_colon = after[colon + 1..].trim_start();
    let quote_start = after_colon.find('"')? + 1;
    let rest = &after_colon[quote_start..];
    let quote_end = rest.find('"')?;
    rest[..quote_end].split('.').next()?.parse::<u32>().ok()
}

/// Assert a located `tsc.js` belongs to the rc TS>=7 (typescript-go) engine and
/// return it unchanged. This gate is the "typescript-go" engine gate — running a
/// legacy `typescript@5/6` JS `tsc` here would type-check against the wrong
/// engine and mask (or fabricate) a Svelte projection regression. A wrong layout
/// (e.g. a hoisted legacy `typescript`) FAILS LOUDLY rather than silently
/// degrading. Cross-platform: reads the package version, no platform/version
/// hardcoding.
fn assert_rc_engine_launcher(tsc_js: PathBuf) -> PathBuf {
    let major = typescript_package_major(&tsc_js);
    assert!(
        major.is_some_and(|m| m >= 7),
        "the Svelte typecheck gate located a `tsc.js` whose owning `typescript` package is not \
         the rc TS>=7 (typescript-go) engine (major = {major:?}) at {}. Running a legacy JS tsc \
         here would type-check against the wrong engine — install the pinned rc `typescript` \
         (`pnpm install`).",
        tsc_js.display()
    );
    tsc_js
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
    typecheck_projected_with_options(projected_tsx, file_name, extra_files, vendor_svelte, false)
}

/// Like [`typecheck_projected`] but with an explicit `check_js` flag. A
/// `.svelte.js` rune module is checked under `allowJs` + `checkJs` (the live
/// provider config enables both) so its JS-valid rune prelude types the module.
fn typecheck_projected_with_options(
    projected_tsx: &str,
    file_name: &str,
    extra_files: &[(&str, &str)],
    vendor_svelte: bool,
    check_js: bool,
) -> Option<(bool, String)> {
    let tsc_js = locate_type_checker()?;
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
            "store.d.ts",
            "package.json",
        ] {
            std::fs::copy(gate_dir().join("vendor_svelte").join(f), dst.join(f))
                .expect("copy svelte vendor");
        }
    }

    // tsconfig: project-level `jsxImportSource: "vue"` (the live provider
    // default the pragma must override), `paths`-map `@verter/svelte-jsx`
    // directly at the in-repo package (no npm install).
    let shim_dir = workspace_root().join("packages/svelte-jsx");
    let shim = shim_dir.to_string_lossy().replace('\\', "/");
    // The live provider config enables `allowJs` + `checkJs` so a `.svelte.js`
    // rune module is type-checked. Mirror that here for the JS rune-module gate.
    let js_opts = if check_js {
        "\n    \"allowJs\": true,\n    \"checkJs\": true,"
    } else {
        ""
    };
    let include = if check_js {
        "[\"**/*.ts\", \"**/*.tsx\", \"**/*.js\"]"
    } else {
        "[\"**/*.ts\", \"**/*.tsx\"]"
    };
    // Hermetic `svelte` resolution. Vendoring into the temp project's
    // `node_modules/svelte` only serves files INSIDE the temp project; the
    // `@verter/svelte-jsx` shim is paths-mapped at its in-repo location, so its
    // own `import "svelte/elements"` / `import "svelte"` resolve relative to the
    // shim's physical path — outside the temp project, where no `svelte` is
    // installed. Paths-map the `svelte` subpaths directly at the vendored
    // declarations so BOTH the externally-located shim and the in-project
    // fixtures resolve the same hermetic vendored copy regardless of whether a
    // workspace-root `svelte` happens to be installed (Testing-Hermeticity).
    let svelte_paths = if vendor_svelte {
        let vendor = gate_dir().join("vendor_svelte");
        let v = vendor.to_string_lossy().replace('\\', "/");
        format!(
            ",\n      \"svelte\": [\"{v}/index.d.ts\"],\
             \n      \"svelte/elements\": [\"{v}/elements.d.ts\"],\
             \n      \"svelte/store\": [\"{v}/store.d.ts\"],\
             \n      \"svelte/transition\": [\"{v}/transition.d.ts\"],\
             \n      \"svelte/animate\": [\"{v}/animate.d.ts\"],\
             \n      \"svelte/attachments\": [\"{v}/attachments.d.ts\"]"
        )
    } else {
        String::new()
    };
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
    "allowImportingTsExtensions": true,{js_opts}
    "paths": {{
      "@verter/svelte-jsx/jsx-runtime": ["{shim}/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/jsx-dev-runtime": ["{shim}/jsx-dev-runtime.d.ts"],
      "@verter/svelte-jsx/svg/jsx-runtime": ["{shim}/svg/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/svg/jsx-dev-runtime": ["{shim}/svg/jsx-dev-runtime.d.ts"],
      "@verter/svelte-jsx/mathml/jsx-runtime": ["{shim}/mathml/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/mathml/jsx-dev-runtime": ["{shim}/mathml/jsx-dev-runtime.d.ts"]{svelte_paths}
    }}
  }},
  "include": {include}
}}"#
    );
    std::fs::write(root.join("tsconfig.json"), tsconfig).expect("write tsconfig");

    // Invoke the rc `typescript` launcher through `node` — OS-agnostic, no
    // dependence on a `.bin/tsc` shebang/`.CMD` shim. The launcher resolves the
    // per-platform typescript-go engine binary itself.
    let output = Command::new("node")
        .arg(&tsc_js)
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
    // GATE PRECONDITION: a `.svelte.tsx`-shaped file whose
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
         the named STOP-and-redesign — escalate, do NOT degrade.\n{out}"
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
fn component_on_event_correct_payload_type_checks() {
    // F13 (PRECISION): a component `on:select={h}` whose handler payload MATCHES
    // the child's `$events["select"]` type-checks clean through the
    // `__verter_event` helper. The child is a class-shaped component carrying an
    // exact `$events` map — the helper indexes it and checks the handler.
    let projected = project(
        "<script lang=\"ts\">\n\
         declare class Child { $events: { select: (id: number) => void }; $props: {} }\n\
         const handle = (id: number) => { void id; };\n\
         </script>\n\
         <Child on:select={handle} />",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "OnEventOk.svelte.tsx", &[], true) else {
        skip_note("component on:event correct payload");
        return;
    };
    assert!(
        ok,
        "a correctly-typed component `on:` handler must check via $events:\n{out}"
    );
}

#[test]
fn component_on_event_wrong_payload_is_rejected() {
    // F13 (PRECISION, discriminating): a component `on:select={h}` whose handler
    // payload MISMATCHES the child's `$events["select"]` is REJECTED. A loose
    // `CustomEvent<any>` projection would have WRONGLY accepted this — its
    // rejection proves the payload is checked precisely.
    let projected = project(
        "<script lang=\"ts\">\n\
         declare class Child { $events: { select: (id: number) => void }; $props: {} }\n\
         const handle = (id: string) => { void id; };\n\
         </script>\n\
         <Child on:select={handle} />",
    );
    let Some((ok, out)) =
        typecheck_projected(&projected, "OnEventBadPayload.svelte.tsx", &[], true)
    else {
        skip_note("component on:event wrong payload");
        return;
    };
    assert!(
        !ok,
        "a wrong-payload component `on:` handler must be REJECTED via $events:\n{out}"
    );
}

#[test]
fn component_on_event_unknown_event_name_is_rejected() {
    // F13 (PRECISION, discriminating): a component `on:nope={h}` whose event name
    // is NOT in the child's `$events` map is REJECTED (the `K extends keyof
    // $events` constraint fails). An unknown event silently treated as a prop /
    // a loose projection would have accepted it.
    let projected = project(
        "<script lang=\"ts\">\n\
         declare class Child { $events: { select: (id: number) => void }; $props: {} }\n\
         const handle = (id: number) => { void id; };\n\
         </script>\n\
         <Child on:nope={handle} />",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "OnEventUnknown.svelte.tsx", &[], true)
    else {
        skip_note("component on:event unknown name");
        return;
    };
    assert!(
        !ok,
        "an unknown component `on:` event name must be REJECTED via keyof $events:\n{out}"
    );
}

#[test]
fn intrinsic_on_event_still_dom_rewrites_and_checks() {
    // F13 (intrinsic disambiguation): an INTRINSIC element's `on:click` keeps the
    // verbatim DOM `onclick` rewrite typed by `SvelteHTMLElements` — it does NOT
    // route the component event helper. A correctly-typed DOM handler checks.
    let projected = project(
        "<script lang=\"ts\">\n\
         const handle = (e: MouseEvent) => { void e; };\n\
         </script>\n\
         <button on:click={handle}>x</button>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "IntrinsicOn.svelte.tsx", &[], true)
    else {
        skip_note("intrinsic on:click DOM rewrite");
        return;
    };
    assert!(
        ok,
        "an intrinsic `on:click` must DOM-rewrite to a checkable `onclick`:\n{out}"
    );
    // The projected body routes the DOM rewrite, NEVER the component helper.
    assert!(
        render_body(&projected).contains("onclick={handle}"),
        "the intrinsic `on:click` projects to `onclick=`:\n{projected}"
    );
    assert!(
        !render_body(&projected).contains("__verter_event("),
        "an intrinsic `on:` must not route the component event helper:\n{projected}"
    );
}

#[test]
fn shim_events_index_checks_callback_and_dispatcher_payloads_precisely() {
    // F13 (PRECISION end-to-end through the `.svelte.ts` shim shape): the derived
    // `$events` index resolves callback-prop AND dispatcher events to their EXACT
    // payloads. A correct handler against `["$events"]["select"]` checks; a
    // wrong-typed handler FAILS. This mirrors the api-projector's shim render.
    let good = "/** @jsxImportSource @verter/svelte-jsx */\n\
        type __VerterFunction<T> = Extract<NonNullable<T>, (...a: any[]) => any>;\n\
        type __VerterCallbackEvents<P> = {\n\
          [K in keyof P as K extends `on${infer E}`\n\
            ? (E extends \"\" ? never : __VerterFunction<P[K]> extends never ? never : E)\n\
            : never]: __VerterFunction<P[K]>\n\
        };\n\
        type __VerterDispatcherEvents<E> = { [K in keyof E]: (e: CustomEvent<E[K]>) => void };\n\
        type __VerterProps = { label: string; onselect: (id: number) => void };\n\
        type __VerterEventsSurface = __VerterCallbackEvents<__VerterProps> & __VerterDispatcherEvents<{ save: string }>;\n\
        ;function __verter_render() {\n\
        // The callback-prop event `select` value is the callback handler ITSELF.\n\
        const ev: __VerterEventsSurface[\"select\"] = (id: number) => { void id; };\n\
        // The dispatcher event `save` value is the legacy CustomEvent handler with\n\
        // the EXACT payload detail (CustomEvent<string>), never `string`/any.\n\
        const sv: __VerterEventsSurface[\"save\"] = (e: CustomEvent<string>) => { void e.detail; };\n\
        void ev; void sv;\n\
        return (<></>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(good, "ShimEventsOk.svelte.tsx", &[], true) else {
        skip_note("shim $events precise index");
        return;
    };
    assert!(
        ok,
        "the derived $events index must resolve callback + dispatcher payloads precisely:\n{out}"
    );

    // DISCRIMINATING: a wrong-typed handler against the derived event payload FAILS.
    let bad = "/** @jsxImportSource @verter/svelte-jsx */\n\
        type __VerterFunction<T> = Extract<NonNullable<T>, (...a: any[]) => any>;\n\
        type __VerterCallbackEvents<P> = {\n\
          [K in keyof P as K extends `on${infer E}`\n\
            ? (E extends \"\" ? never : __VerterFunction<P[K]> extends never ? never : E)\n\
            : never]: __VerterFunction<P[K]>\n\
        };\n\
        type __VerterProps = { onselect: (id: number) => void };\n\
        type __VerterEventsSurface = __VerterCallbackEvents<__VerterProps>;\n\
        ;function __verter_render() {\n\
        // @ts-expect-error a string handler is not assignable to the (id: number) payload\n\
        const ev: __VerterEventsSurface[\"select\"] = (id: string) => { void id; };\n\
        void ev;\n\
        return (<></>);\n\
        }\nexport {};\n";
    let Some((bad_ok, bad_out)) = typecheck_projected(bad, "ShimEventsBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        bad_ok,
        "the @ts-expect-error must match a REAL wrong-payload error (proving the \
         derived event payload is precise, not any):\n{bad_out}"
    );

    // DISCRIMINATING (dispatcher handler shape): the dispatcher event `save`'s
    // handler must be `(e: CustomEvent<string>) => void` — a WRONG detail type
    // (`CustomEvent<number>`) FAILS, and a bare payload (`string`, not a handler)
    // FAILS. This proves the dispatcher half wraps payloads into precise handler
    // types (the gap a payload-typed `$events["save"]` would have hidden).
    let wrong_detail = "/** @jsxImportSource @verter/svelte-jsx */\n\
        type __VerterCallbackEvents<P> = {};\n\
        type __VerterDispatcherEvents<E> = { [K in keyof E]: (e: CustomEvent<E[K]>) => void };\n\
        type __VerterEventsSurface = __VerterCallbackEvents<{}> & __VerterDispatcherEvents<{ save: string }>;\n\
        ;function __verter_render() {\n\
        // @ts-expect-error a CustomEvent<number> handler is not assignable to CustomEvent<string>\n\
        const sv: __VerterEventsSurface[\"save\"] = (e: CustomEvent<number>) => { void e.detail; };\n\
        // @ts-expect-error a bare string is not a handler (the value is a CustomEvent handler)\n\
        const raw: __VerterEventsSurface[\"save\"] = \"ok\";\n\
        void sv; void raw;\n\
        return (<></>);\n\
        }\nexport {};\n";
    let Some((wd_ok, wd_out)) = typecheck_projected(
        wrong_detail,
        "ShimEventsDispatcherBad.svelte.tsx",
        &[],
        true,
    ) else {
        return;
    };
    assert!(
        wd_ok,
        "the @ts-expect-errors must match REAL errors — the dispatcher event value \
         is the precise CustomEvent<detail> HANDLER, not a payload / any:\n{wd_out}"
    );
}

#[test]
fn shim_slots_index_is_name_exact_and_binding_precise() {
    // F9 (PRECISION end-to-end through the `.svelte.ts` shim shape): the `$slots`
    // index is name-EXACT (an unknown slot name FAILS the `keyof` index) AND its
    // binding type is PRECISE — the snippet slot is CALLED with its binding, and a
    // WRONG-typed binding FAILS through tsgo while a correct binding PASSES. The
    // shim renders `$slots` as `{ row: __VerterProps["row"] }` over the snippet
    // prop's own type. This test FAILS if the slot binding surface were loosened
    // to `any` (a wrong binding against `any` would NOT error).
    let good = "/** @jsxImportSource @verter/svelte-jsx */\n\
        import type { Snippet } from \"svelte\";\n\
        type __VerterProps = { row: Snippet<[{ id: number }]> };\n\
        type __VerterSlotsSurface = { row: __VerterProps[\"row\"] };\n\
        ;function __verter_render(slots: __VerterSlotsSurface) {\n\
        // CALL the snippet slot with a CORRECT binding — `{ id: number }` checks.\n\
        slots.row({ id: 1 });\n\
        // @ts-expect-error `missing` is not a slot key\n\
        type _Missing = __VerterSlotsSurface[\"missing\"];\n\
        return (<></>);\n\
        }\nexport {};\n";
    let Some((ok, out)) = typecheck_projected(good, "ShimSlotsOk.svelte.tsx", &[], true) else {
        skip_note("shim $slots name-exact + binding-precise");
        return;
    };
    assert!(
        ok,
        "the $slots index must be name-exact + binding-precise (a correct snippet \
         binding checks; the @ts-expect-error matches the unknown-slot index error):\n{out}"
    );

    // DISCRIMINATING (binding precision, the real negative): calling the snippet
    // slot with a WRONG-typed binding (`{ id: "bad" }` against `{ id: number }`)
    // MUST FAIL through tsgo. If the binding surface were `any`, this would NOT
    // error — the @ts-expect-error would then be unsatisfied and tsgo would FAIL
    // the test from the other side, so this discriminates precise vs `any`.
    let bad = "/** @jsxImportSource @verter/svelte-jsx */\n\
        import type { Snippet } from \"svelte\";\n\
        type __VerterProps = { row: Snippet<[{ id: number }]> };\n\
        type __VerterSlotsSurface = { row: __VerterProps[\"row\"] };\n\
        ;function __verter_render(slots: __VerterSlotsSurface) {\n\
        // @ts-expect-error `{ id: string }` is not assignable to the `{ id: number }` binding\n\
        slots.row({ id: \"bad\" });\n\
        return (<></>);\n\
        }\nexport {};\n";
    let Some((bad_ok, bad_out)) = typecheck_projected(bad, "ShimSlotsBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        bad_ok,
        "the @ts-expect-error must match a REAL wrong-binding error (proving the \
         snippet slot binding is PRECISE `{{ id: number }}`, NOT `any`):\n{bad_out}"
    );
}

#[test]
fn projected_snippet_ordering_fixture_type_checks_clean_discriminating_tdz() {
    // DISCRIMINATING: a `{@render mySnip()}` PRECEDING its `{#snippet}` in
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
        "snippet-before-render must type-check clean (hoist, no TDZ):\n{out}"
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
    // A mistyped `{@attach}` — an `Attachment<HTMLInputElement>` on a
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
    // A plain function passed where `Snippet<[T]>` is expected stays an
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
    // A `{const}` value is typed AND visible to a sibling. The hoist makes
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
         (hoist):\n{out}"
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
    // `--x={expr}` strips the JSX attribute (no `--` residue) and
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
fn projected_markup_await_expression_flows_the_resolved_value_type() {
    // F6: a markup `{(await fetchUser()).name}` projects to
    // `(__verter_await_expr(fetchUser())).name` — `Awaited<Promise<{name}>>`
    // flows so `.name` (a `string`) checks. `__verter_render` STAYS SYNC. DISCRIM
    // through the type checker (a `string` consumer accepts the resolved value).
    let projected = project(
        "<script lang=\"ts\">\n\
         async function fetchUser(): Promise<{ name: string }> { return { name: \"a\" }; }\n\
         </script>\n\
         <div>{(await fetchUser()).name}</div>",
    );
    assert!(
        projected.contains("__verter_await_expr(fetchUser())"),
        "the markup await is rewritten through the helper: {projected}"
    );
    assert!(
        !projected.contains("async function __verter_render"),
        "`__verter_render` must stay sync: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "AwaitExpr.svelte.tsx", &[], true) else {
        skip_note("markup await value type");
        return;
    };
    assert!(
        ok,
        "the resolved `Awaited<…>.name` value type must flow and check:\n{out}"
    );
}

#[test]
fn projected_markup_await_expression_missing_member_is_rejected() {
    // F6 (DISCRIMINATING): `(await fetchUser()).missing` accesses a member that
    // does NOT exist on the resolved value type — TSGO must REJECT it (the value
    // type genuinely flows, it is not `any`).
    let projected = project(
        "<script lang=\"ts\">\n\
         async function fetchUser(): Promise<{ name: string }> { return { name: \"a\" }; }\n\
         </script>\n\
         <div>{(await fetchUser()).missing}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "AwaitExprBad.svelte.tsx", &[], true)
    else {
        skip_note("markup await missing member");
        return;
    };
    assert!(
        !ok,
        "a missing member on the resolved await value type MUST be rejected:\n{out}"
    );
}

#[test]
fn projected_markup_await_of_a_non_promise_is_rejected() {
    // F6 (DISCRIMINATING): `{await 1}` — `1` is NOT `PromiseLike<unknown>`, so the
    // `T extends PromiseLike<unknown>` constraint on `__verter_await_expr` makes
    // TSGO REJECT it. A `@ts-expect-error` over the helper call asserts the
    // rejection lands (a clean compile then PROVES the error fired).
    let projected = project("<div>{await 1}</div>");
    assert!(
        projected.contains("__verter_await_expr(1)"),
        "the await-1 is rewritten through the helper: {projected}"
    );
    // Re-shape the projected helper call under a `@ts-expect-error` so a CLEAN
    // type-check proves the PromiseLike constraint fired (a non-rejected call
    // would make `@ts-expect-error` itself an unused-directive error).
    let guarded = projected.replace(
        "__verter_await_expr(1)",
        "(\n// @ts-expect-error a non-promise must fail the PromiseLike constraint\n__verter_await_expr(1)\n)",
    );
    let Some((ok, out)) = typecheck_projected(&guarded, "AwaitOne.svelte.tsx", &[], true) else {
        skip_note("markup await non-promise");
        return;
    };
    assert!(
        ok,
        "`@ts-expect-error` over `__verter_await_expr(1)` must compile CLEAN \
         (proving the PromiseLike constraint rejected the non-promise):\n{out}"
    );
}

#[test]
fn projected_markup_derived_await_flows_and_checks() {
    // F6: a markup `{$derived(await load())}` routes through the SAME rewrite —
    // `$derived(__verter_await_expr(load()))` — checkable against
    // `$derived<T>(expression: T): T`. The resolved `number` flows.
    let projected = project(
        "<script lang=\"ts\">\n\
         async function load(): Promise<number> { return 1; }\n\
         </script>\n\
         <div>{$derived(await load())}</div>",
    );
    assert!(
        projected.contains("$derived(__verter_await_expr(load()))"),
        "markup `$derived(await …)` routes through the helper: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "DerivedAwait.svelte.tsx", &[], true)
    else {
        skip_note("markup derived await");
        return;
    };
    assert!(
        ok,
        "a markup `$derived(await …)` must project valid, clean TSX:\n{out}"
    );
}

#[test]
fn projected_attribute_await_expression_flows_and_keeps_render_sync() {
    // F6 (regression guard): an await-EXPRESSION in an ATTRIBUTE value position
    // (`<img src={await fetchUrl()} />`) must ALSO be rewritten — a raw `await`
    // left in the sync render fn would be INVALID TSX. The resolved `string`
    // value flows to the `src` attribute and checks.
    let projected = project(
        "<script lang=\"ts\">\n\
         async function fetchUrl(): Promise<string> { return \"x\"; }\n\
         </script>\n\
         <img src={await fetchUrl()} />",
    );
    assert!(
        projected.contains("__verter_await_expr(fetchUrl())"),
        "the attribute-value await is rewritten through the helper: {projected}"
    );
    assert!(
        !projected.contains("async function __verter_render"),
        "`__verter_render` stays sync even with an attribute await: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "AttrAwait.svelte.tsx", &[], true) else {
        skip_note("attribute await");
        return;
    };
    assert!(
        ok,
        "an attribute-value await must project valid, clean TSX (resolved string \
         flows to `src`):\n{out}"
    );
}

#[test]
fn projected_dynamic_component_this_await_flows_and_keeps_render_sync() {
    // F6/F8 (regression guard — the markup-await leak class): an await-EXPRESSION
    // in the `<svelte:component this={await load()}>` value position is a MARKUP
    // expression — a raw `await` left in the sync render fn (the dynamic-component
    // IIFE) would be INVALID TSX (TS1308). The text path must route the await
    // rewrite: `__verter_await_expr(load())`, `__verter_render` STAYS SYNC, and the
    // resolved component value flows to `__verter_dynamic_component`.
    let projected = project(
        "<script lang=\"ts\">\n\
         declare class Comp { $props: { label: string }; }\n\
         async function load(): Promise<typeof Comp> { return Comp; }\n\
         </script>\n\
         <svelte:component this={await load()} label={\"ok\"} />",
    );
    assert!(
        projected.contains("__verter_await_expr(load())"),
        "the dynamic-component `this` await is rewritten through the helper: {projected}"
    );
    assert!(
        !render_body(&projected).contains("await "),
        "no raw `await` keyword survives in the render body (render stays sync): {projected}"
    );
    assert!(
        !projected.contains("async function __verter_render"),
        "`__verter_render` stays sync even with a dynamic-component await: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "DynCompAwait.svelte.tsx", &[], true)
    else {
        skip_note("dynamic-component this await");
        return;
    };
    assert!(
        ok,
        "a dynamic-component `this` await must project valid, clean TSX (resolved \
         component value flows to the helper):\n{out}"
    );
}

#[test]
fn projected_dynamic_component_this_await_of_a_non_promise_is_rejected() {
    // F6/F8 (DISCRIMINATING): `<svelte:component this={await 1} />` — `1` is NOT
    // `PromiseLike<unknown>`, so the `T extends PromiseLike<unknown>` constraint on
    // `__verter_await_expr` makes TSGO REJECT it. A `@ts-expect-error` over the
    // helper call asserts the rejection lands (a clean compile then PROVES the
    // PromiseLike constraint fired — not an `any` escape).
    let projected = project("<svelte:component this={await 1} />");
    assert!(
        projected.contains("__verter_await_expr(1)"),
        "the dynamic-component await-1 is rewritten through the helper: {projected}"
    );
    let guarded = projected.replace(
        "__verter_await_expr(1)",
        "(\n// @ts-expect-error a non-promise must fail the PromiseLike constraint\n__verter_await_expr(1)\n)",
    );
    let Some((ok, out)) = typecheck_projected(&guarded, "DynCompAwaitOne.svelte.tsx", &[], true)
    else {
        skip_note("dynamic-component await non-promise");
        return;
    };
    assert!(
        ok,
        "`@ts-expect-error` over `__verter_await_expr(1)` in the dynamic-component \
         `this` must compile CLEAN (proving the PromiseLike constraint rejected the \
         non-promise):\n{out}"
    );
}

#[test]
fn projected_fragment_dynamic_slot_await_flows_and_keeps_render_sync() {
    // F6/F9 (regression guard — the dynamic-slot markup-await position): an
    // await-EXPRESSION in a dynamic `<svelte:fragment slot={await name()}>` value
    // is void-checked in place inside the SYNC render fn — a raw `await` there
    // would be INVALID TSX (TS1308). The slot-expression text path must route the
    // await rewrite: `__verter_void(__verter_await_expr(name()))`, render stays
    // sync, and the resolved `string` flows to the void check.
    let projected = project(
        "<script lang=\"ts\">\n\
         async function name(): Promise<string> { return \"a\"; }\n\
         </script>\n\
         <svelte:fragment slot={await name()}><span>x</span></svelte:fragment>",
    );
    assert!(
        projected.contains("__verter_void(__verter_await_expr(name()))"),
        "the dynamic-slot await is rewritten through the helper: {projected}"
    );
    assert!(
        !render_body(&projected).contains("await "),
        "no raw `await` keyword survives in the render body (render stays sync): {projected}"
    );
    assert!(
        !projected.contains("async function __verter_render"),
        "`__verter_render` stays sync even with a dynamic-slot await: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "FragSlotAwait.svelte.tsx", &[], true)
    else {
        skip_note("fragment dynamic slot await");
        return;
    };
    assert!(
        ok,
        "a dynamic-slot await must project valid, clean TSX (resolved value flows \
         to the void check):\n{out}"
    );
}

#[test]
fn projected_fragment_dynamic_slot_await_of_a_non_promise_is_rejected() {
    // F6/F9 (DISCRIMINATING — mirrors the dynamic-component `this` non-promise
    // case): `<svelte:fragment slot={await 1}>` — `1` is NOT
    // `PromiseLike<unknown>`, so the `T extends PromiseLike<unknown>` constraint on
    // `__verter_await_expr` makes TSGO REJECT it. A `@ts-expect-error` over the
    // helper call asserts the rejection lands (a clean compile then PROVES the
    // PromiseLike constraint fired at the fragment-slot await position — the slot
    // value is NOT loosened to `any`).
    let projected = project("<svelte:fragment slot={await 1}><span>x</span></svelte:fragment>");
    assert!(
        projected.contains("__verter_await_expr(1)"),
        "the fragment-slot await-1 is rewritten through the helper: {projected}"
    );
    // Re-shape the projected helper call under a `@ts-expect-error` so a CLEAN
    // type-check proves the PromiseLike constraint fired (a non-rejected call would
    // make `@ts-expect-error` itself an unused-directive error). If the helper were
    // loosened to accept `any`, the directive would be unused and TSGO would FAIL —
    // so this discriminates on the constraint being present.
    let guarded = projected.replace(
        "__verter_await_expr(1)",
        "(\n// @ts-expect-error a non-promise must fail the PromiseLike constraint\n__verter_await_expr(1)\n)",
    );
    let Some((ok, out)) = typecheck_projected(&guarded, "FragSlotAwaitOne.svelte.tsx", &[], true)
    else {
        skip_note("fragment-slot await non-promise");
        return;
    };
    assert!(
        ok,
        "`@ts-expect-error` over `__verter_await_expr(1)` in the fragment-slot \
         value must compile CLEAN (proving the PromiseLike constraint rejected the \
         non-promise — the slot await is not loosened to `any`):\n{out}"
    );
}

#[test]
fn projected_top_level_script_await_type_checks_clean() {
    // F6: a top-level instance-script `await` is kept VERBATIM (valid top-level
    // await under `module/target: esnext`). The resolved value type checks and
    // the binding is visible to the markup.
    let projected = project(
        "<script lang=\"ts\">\n\
         async function fetchThing(): Promise<{ id: number }> { return { id: 1 }; }\n\
         const thing = await fetchThing();\n\
         const id: number = thing.id;\n\
         </script>\n\
         <div>{id}</div>",
    );
    // The top-level await is NOT rewritten in the script body (stays verbatim).
    assert!(
        projected.contains("const thing = await fetchThing();"),
        "the top-level script await is kept verbatim: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "TopLevelAwait.svelte.tsx", &[], true)
    else {
        skip_note("top-level script await");
        return;
    };
    assert!(
        ok,
        "a top-level script await must type-check clean (valid top-level await):\n{out}"
    );
}

#[test]
fn projected_inline_await_destructuring_binding_strands_no_close_brace() {
    // An INLINE `{#await p then {a,b}}` with a
    // DESTRUCTURING binding contains its OWN `}` — the open-tag close-brace search
    // must start PAST the binding span, else the pattern's inner `}` strands and
    // produces invalid TSX. The destructuring binding is also DECLARED so the body
    // resolves `{a}`/`{b}`.
    let projected = project(
        "<script lang=\"ts\">const p: Promise<{ a: number; b: string }> = Promise.resolve({ a: 1, b: \"x\" });</script>\n\
         <div>{#await p then { a, b }}<span>{a}{b}</span>{/await}</div>",
    );
    // No stranded raw await syntax / no malformed `(<>}` tail.
    assert!(
        !projected.contains("{#await") && !projected.contains("{/await}"),
        "no raw await-block residue: {projected}"
    );
    assert!(
        !projected.contains("(<>}"),
        "no stranded close brace producing `(<>}}`: {projected}"
    );
    let Some((ok, out)) =
        typecheck_projected(&projected, "InlineAwaitDestructure.svelte.tsx", &[], true)
    else {
        skip_note("inline await destructuring close-brace");
        return;
    };
    assert!(
        ok,
        "an inline await destructuring binding must project valid, clean TSX \
         (no stranded close brace, binding declared):\n{out}"
    );
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
#[ignore = "known tsgo native-preview limitation: under the \
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
#[ignore = "known tsgo native-preview limitation: it silently accepts a wrong-typed \
            value on a transition-param property NOT shared with TransitionConfig \
            (e.g. fly's `y`) under the @jsxImportSource pragma. Red against \
            current tsgo; flips green when the upstream tsgo bug is fixed. \
            Upstream: contravariant optional-param inference loses the param \
            discrimination for non-shared interface members."]
fn transition_specific_param_wrong_type_is_rejected_known_tsgo_gap() {
    // R10 ledger (DISCRIMINATING): `transition:fly={{ y: "200" }}` — a
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
    // A workspace WITHOUT `svelte` fails CLOSED — the shim's
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

// --- F8/F9/F10 special-element + namespace TSGO fixtures ---

#[test]
fn dynamic_component_wrong_prop_is_rejected() {
    // F8: a dynamic `<svelte:component this={C} label={x}>` checks `label`
    // against the props `P` inferred from the component's `{ $props: P }`
    // constructor member through `__verter_dynamic_component`. A correctly-typed
    // `label` checks; a wrong-typed `label` FAILS.
    let good = project(
        "<script lang=\"ts\">\n\
         declare const Dyn: abstract new (...a: never[]) => { $props: { label: string } };\n\
         let title = \"ok\";\n\
         </script>\n\
         <svelte:component this={Dyn} label={title} />",
    );
    let Some((ok, out)) = typecheck_projected(&good, "DynOk.svelte.tsx", &[], true) else {
        skip_note("dynamic component wrong-prop");
        return;
    };
    assert!(
        ok,
        "a correctly-typed dynamic-component prop must check through the \
         `{{ $props: P }}`-inferred props:\n{out}"
    );

    let bad = project(
        "<script lang=\"ts\">\n\
         declare const Dyn: abstract new (...a: never[]) => { $props: { label: string } };\n\
         let count = 123;\n\
         </script>\n\
         <svelte:component this={Dyn} label={count} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "DynBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a wrong-typed dynamic-component prop must be REJECTED:\n{bad_out}"
    );
}

#[test]
fn dynamic_component_non_component_this_is_rejected() {
    // F8: a NON-component `this` (a plain number) FAILS the
    // `__verter_dynamic_component` class-shaped-constructor constraint.
    let bad = project(
        "<script lang=\"ts\">\n\
         let notAComponent = 42;\n\
         </script>\n\
         <svelte:component this={notAComponent} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "DynNonComp.svelte.tsx", &[], true)
    else {
        skip_note("dynamic component non-component this");
        return;
    };
    assert!(
        !bad_ok,
        "a non-component `this` must FAIL the class-shaped-constructor \
         constraint:\n{bad_out}"
    );
}

#[test]
fn svelte_self_wrong_prop_is_rejected_against_the_local_contract() {
    // F8: `<svelte:self prop={x}>` checks against the LOCAL self-props contract
    // derived SYNTACTICALLY from this component's own `$props()` annotation. A
    // correct self-prop checks; a wrong-typed self-prop FAILS.
    let good = project(
        "<script lang=\"ts\">\n\
         interface Props { count: number }\n\
         let { count }: Props = $props();\n\
         </script>\n\
         <svelte:self count={count} />",
    );
    let Some((ok, out)) = typecheck_projected(&good, "SelfOk.svelte.tsx", &[], true) else {
        skip_note("svelte:self wrong-prop");
        return;
    };
    assert!(
        ok,
        "a correct self-prop must check against the local self contract:\n{out}"
    );

    let bad = project(
        "<script lang=\"ts\">\n\
         interface Props { count: number }\n\
         let { count }: Props = $props();\n\
         </script>\n\
         <svelte:self count={\"not a number\"} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "SelfBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a wrong-typed self-prop must be REJECTED against the local \
         self-props contract:\n{bad_out}"
    );
}

#[test]
fn svelte_self_contract_ignores_a_member_call_props_before_the_real_rune() {
    // F8 P1: a preceding `$props.id()` member call (NOT the props rune) must NOT
    // poison the LOCAL self-props contract. The SYNTACTIC OXC scan binds the
    // REAL `$props()` declarator's `Props` annotation, so a wrong self-prop still
    // FAILS — proving the contract is `Props`, not a permissive degrade.
    let bad = project(
        "<script lang=\"ts\">\n\
         const id = $props.id();\n\
         interface Props { count: number }\n\
         let { count }: Props = $props();\n\
         void id;\n\
         </script>\n\
         <svelte:self count={\"not a number\"} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "SelfMemberCall.svelte.tsx", &[], true)
    else {
        skip_note("svelte:self member-call props");
        return;
    };
    assert!(
        !bad_ok,
        "a wrong self-prop must be REJECTED against `Props` even when a \
         `$props.id()` member call precedes the real rune:\n{bad_out}"
    );
}

#[test]
fn svelte_fragment_children_type_check_transparently() {
    // F9: `<svelte:fragment slot="x">…</svelte:fragment>` projects its children
    // UNWRAPPED; they type-check, and the slot literal void-checks. A type error
    // INSIDE a child surfaces (the children are genuinely checked).
    let good = project(
        "<script lang=\"ts\">let label: string = \"hi\";</script>\n\
         <svelte:fragment slot=\"footer\"><span>{label}</span></svelte:fragment>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "FragOk.svelte.tsx", &[], true) else {
        skip_note("svelte:fragment children");
        return;
    };
    assert!(
        ok,
        "transparent fragment children must type-check clean:\n{out}"
    );

    // DISCRIMINATING: a type error inside a fragment child surfaces.
    let bad = project(
        "<script lang=\"ts\">let label: number = 1;</script>\n\
         <svelte:fragment slot=\"footer\"><span>{label.nope}</span></svelte:fragment>",
    );
    let Some((bad_ok, _)) = typecheck_projected(&bad, "FragBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "a type error in a transparent fragment child must surface (children \
         are genuinely checked)"
    );
}

#[test]
fn fragment_close_tag_inside_a_descendant_string_literal_projects_valid_tsx() {
    // P1 (literal-aware close-tag span): a child interpolation whose string
    // literal CONTAINS the text `</svelte:fragment>` must NOT be mistaken for the
    // element's real close tag. The projector reads the parser-recorded close
    // span (the parser's child walk is string/brace-aware), so the real close is
    // removed cleanly and the in-string text is preserved verbatim. A
    // literal-unaware source byte-scan would splice the close tag out of the
    // string and leave the real `</svelte:fragment>` residue → invalid TSX
    // (TS17015 unterminated/mismatched JSX). This fixture proves the projection
    // type-checks CLEAN through tsgo, AND that a type error in a sibling AFTER the
    // real close still surfaces (the sibling was NOT swallowed).
    let good = project(
        "<script lang=\"ts\">let s: string = \"hi\"; let tail: string = \"t\";</script>\n\
         <svelte:fragment slot=\"a\">{\"x </svelte:fragment> y\"}<span>{tail}</span></svelte:fragment>\n\
         <div>{s}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "FragLiteral.svelte.tsx", &[], true) else {
        skip_note("fragment close-tag-in-literal");
        return;
    };
    assert!(
        ok,
        "a `</svelte:fragment>` inside a descendant string literal must project \
         VALID TSX that type-checks clean (no TS17015 mismatched-tag):\n{out}"
    );

    // DISCRIMINATING: the sibling AFTER the real close tag was NOT swallowed by an
    // in-string close match — a type error in it surfaces.
    let bad = project(
        "<script lang=\"ts\">let n: number = 1; let tail: string = \"t\";</script>\n\
         <svelte:fragment slot=\"a\">{\"x </svelte:fragment> y\"}<span>{tail}</span></svelte:fragment>\n\
         <div>{n.nope}</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "FragLiteralBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a type error in the sibling AFTER the real close tag must surface (the \
         sibling was not swallowed by the in-string close):\n{bad_out}"
    );
}

#[test]
fn svg_namespace_component_type_checks_svg_intrinsics_and_rejects_html_only_attrs() {
    // F10: a `<svelte:options namespace="svg" />` component projects with the
    // svg-namespace pragma; its svg intrinsics (`<circle r={5} />`) check
    // through the svg table. An HTML-only attribute (`value`) on an svg element
    // FAILS — proving the svg table REPLACED the HTML table (svg-only).
    let good = project(
        "<svelte:options namespace=\"svg\" />\n\
         <svg viewBox=\"0 0 10 10\"><circle cx={1} cy={1} r={5} /></svg>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "SvgOk.svelte.tsx", &[], true) else {
        skip_note("svg namespace");
        return;
    };
    assert!(
        ok,
        "svg intrinsics must type-check under the svg-namespace pragma:\n{out}"
    );

    // DISCRIMINATING: an HTML-only attribute (`value` — present on `<input>` in
    // the HTML table, ABSENT from `SVGAttributes`) on an svg element FAILS,
    // proving the svg table is in effect (not the HTML table).
    let bad = project(
        "<svelte:options namespace=\"svg\" />\n\
         <svg><circle value=\"nope\" /></svg>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "SvgBad.svelte.tsx", &[], true) else {
        return;
    };
    assert!(
        !bad_ok,
        "an HTML-only attribute on an svg element must FAIL (svg table replaced \
         the HTML table):\n{bad_out}"
    );
}

// --- F11/F12 legacy store + magic-object TSGO fixtures ---

#[test]
fn store_read_checks_against_the_readable_value_type() {
    // F11: a `$count` store-sub READ projects to `__verter_store_get(count)` —
    // typed `number` from `Writable<number>` (a `Writable` IS a `Readable`). A
    // wrong consumer type FAILS, proving the read is genuinely typed (not `any`).
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const n: number = $count;\n\
         void n;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "StoreRead.svelte.tsx", &[], true) else {
        skip_note("store read");
        return;
    };
    assert!(
        ok,
        "a `$count` read must check `number` from `Writable<number>`:\n{out}"
    );

    // DISCRIMINATING (anti-`any`): assigning the read to a deliberately wrong type
    // FAILS — the value is `number`, not `any`.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const s: string = $count;\n\
         void s;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "StoreReadBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `$count` read typed `number` must NOT assign to `string` (proves the \
         read is not `any`):\n{bad_out}"
    );
}

#[test]
fn store_write_checks_the_writable_contract() {
    // F11: a `$count = 5` WRITE projects to `__verter_store_set(count, 5)` —
    // checking `5` against the store's `Writable<number>` value type. A correct
    // write checks clean; a wrong-typed write FAILS.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         $count = 5;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "StoreWrite.svelte.tsx", &[], true) else {
        skip_note("store write");
        return;
    };
    assert!(
        ok,
        "a `$count = 5` write must check against `Writable<number>`:\n{out}"
    );

    // DISCRIMINATING: a wrong-typed write FAILS the writable value contract.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         $count = \"not a number\";\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "StoreWriteBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `$count = \"str\"` write must be REJECTED against `Writable<number>`:\n{bad_out}"
    );
}

#[test]
fn store_write_in_an_array_destructuring_target_emits_residue_free_valid_tsx() {
    // F11: a `$count` in an array-DESTRUCTURING assignment TARGET (`[$count] =
    // xs`) is a store WRITE leaf. It must project to the writable-lvalue form
    // (`[__verter_store_lvalue(count).value] = xs`) — VALID TSX with NO raw
    // `$count` residue. Suppressing it (leaving raw `$count`) would surface a
    // spurious `Cannot find name '$count'`.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const xs: number[] = [1, 2, 3];\n\
         [$count] = xs;\n\
         </script>\n\
         <div>x</div>",
    );
    // No raw `$count` residue anywhere; rewritten to the writable lvalue.
    assert!(
        !good.contains("$count") && good.contains("__verter_store_lvalue(count).value"),
        "the `[$count] = xs` target must be rewritten (no raw `$count`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "StoreArrayDestructure.svelte.tsx", &[], true)
    else {
        skip_note("store array-destructure write");
        return;
    };
    assert!(
        ok,
        "a `[$count] = xs` array-destructure store write must type-check clean \
         (no phantom `Cannot find name '$count'`):\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$count'") && !out.contains("Cannot find name \"$count\""),
        "the `[$count] = xs` projection must NOT surface a phantom `$count` name \
         error:\n{out}"
    );

    // DISCRIMINATING: a wrong-element-typed destructure FAILS the writable value
    // contract — `string[]` elements are not assignable into a `number` lvalue.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const xs: string[] = [\"a\"];\n\
         [$count] = xs;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) =
        typecheck_projected(&bad, "StoreArrayDestructureBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `[$count] = (string[])` destructure must be REJECTED against the \
         `Writable<number>` element value:\n{bad_out}"
    );
}

#[test]
fn store_write_in_an_object_destructuring_target_emits_residue_free_valid_tsx() {
    // F11: a `$count` in an object-DESTRUCTURING assignment TARGET
    // (`({ x: $count } = obj)`) is a store WRITE leaf. Residue-free valid TSX.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const obj = { x: 0 };\n\
         ({ x: $count } = obj);\n\
         </script>\n\
         <div>x</div>",
    );
    assert!(
        !good.contains("$count") && good.contains("__verter_store_lvalue(count).value"),
        "the `({{ x: $count }} = obj)` target must be rewritten (no raw `$count`): {good}"
    );
    let Some((ok, out)) =
        typecheck_projected(&good, "StoreObjectDestructure.svelte.tsx", &[], true)
    else {
        skip_note("store object-destructure write");
        return;
    };
    assert!(
        ok,
        "an `({{ x: $count }} = obj)` object-destructure store write must \
         type-check clean (no phantom name error):\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$count'") && !out.contains("Cannot find name \"$count\""),
        "the object-destructure projection must NOT surface a phantom `$count` \
         name error:\n{out}"
    );
}

#[test]
fn store_write_in_a_for_of_target_emits_residue_free_valid_tsx() {
    // F11: a `$count` as a `for-of` assignment TARGET (`for ($count of xs)`) is a
    // store WRITE leaf — residue-free valid TSX referencing only `count`.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const xs: number[] = [1, 2, 3];\n\
         for ($count of xs) {}\n\
         </script>\n\
         <div>x</div>",
    );
    assert!(
        !good.contains("$count") && good.contains("__verter_store_lvalue(count).value"),
        "the `for ($count of xs)` target must be rewritten (no raw `$count`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "StoreForOf.svelte.tsx", &[], true) else {
        skip_note("store for-of write");
        return;
    };
    assert!(
        ok,
        "a `for ($count of xs)` store write target must type-check clean (no \
         phantom name error):\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$count'") && !out.contains("Cannot find name \"$count\""),
        "the for-of projection must NOT surface a phantom `$count` name error:\n{out}"
    );
}

#[test]
fn store_read_default_in_an_each_block_binding_is_rewritten_residue_free() {
    // F11: a store READ in a block-binding DEFAULT VALUE (`{#each rows as { x =
    // $fallback }}`) is an ordinary READ context — it must be rewritten to
    // `__verter_store_get(fallback)`, while the bound NAME `x` stays a local. NO
    // raw `$fallback` residue.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const fallback = writable(0);\n\
         const rows: { x?: number }[] = [];\n\
         </script>\n\
         {#each rows as { x = $fallback }}{x}{/each}",
    );
    assert!(
        !render_body(&good).contains("$fallback") && good.contains("__verter_store_get(fallback)"),
        "the each block-binding default `$fallback` must be rewritten to the read \
         helper (no raw `$fallback`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "EachStoreDefault.svelte.tsx", &[], true)
    else {
        skip_note("each block-binding store default");
        return;
    };
    assert!(
        ok,
        "an each block-binding store-read default must type-check clean:\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$fallback'")
            && !out.contains("Cannot find name \"$fallback\""),
        "the each block-binding default projection must NOT surface a phantom \
         `$fallback` name error:\n{out}"
    );
}

#[test]
fn store_read_default_in_a_snippet_param_is_rewritten_residue_free() {
    // F11: a store READ in a snippet PARAM DEFAULT (`{#snippet row($item =
    // $fallback)}`) is an ordinary READ context — `$fallback` is rewritten to
    // the read helper while the param NAME `$item` stays a local. No raw
    // `$fallback` residue.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const fallback = writable(0);\n\
         </script>\n\
         {#snippet row($item = $fallback)}{$item}{/snippet}",
    );
    assert!(
        good.contains("__verter_store_get(fallback)")
            && !good.contains("= $fallback")
            && !good.contains("$fallback)"),
        "the snippet param default `$fallback` must be rewritten to the read \
         helper (no raw `$fallback`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "SnippetStoreDefault.svelte.tsx", &[], true)
    else {
        skip_note("snippet param store default");
        return;
    };
    assert!(
        ok,
        "a snippet param store-read default must type-check clean:\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$fallback'")
            && !out.contains("Cannot find name \"$fallback\""),
        "the snippet param default projection must NOT surface a phantom \
         `$fallback` name error:\n{out}"
    );
}

#[test]
fn store_sub_against_a_non_store_is_rejected() {
    // F11: a `$count` where `count` is NOT a store (a plain `number`) FAILS the
    // `__verter_store_get` `Readable<T>` constraint — discriminating that the
    // helper genuinely requires a store.
    let bad = project(
        "<script lang=\"ts\">\n\
         const count = 0;\n\
         const v = $count;\n\
         void v;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&bad, "NonStore.svelte.tsx", &[], true) else {
        skip_note("non-store sub");
        return;
    };
    assert!(
        !ok,
        "a `$count` over a non-store `count: number` must FAIL the `Readable<T>` \
         constraint:\n{out}"
    );
}

#[test]
fn store_write_against_a_readonly_store_is_rejected() {
    // F11: a `$count = v` WRITE where `count` is a READONLY `Readable<number>`
    // (no `.set`) FAILS — `__verter_store_set` requires a `Writable<T>`. The READ
    // of the same readonly store checks clean (discriminating the direction).
    let read_ok = project(
        "<script lang=\"ts\">\n\
         import { readable } from \"svelte/store\";\n\
         const count = readable(0);\n\
         const n: number = $count;\n\
         void n;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&read_ok, "ReadonlyRead.svelte.tsx", &[], true)
    else {
        skip_note("readonly store read");
        return;
    };
    assert!(
        ok,
        "a READ of a readonly `Readable<number>` store must check clean:\n{out}"
    );

    let write_bad = project(
        "<script lang=\"ts\">\n\
         import { readable } from \"svelte/store\";\n\
         const count = readable(0);\n\
         $count = 5;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) =
        typecheck_projected(&write_bad, "ReadonlyWrite.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a WRITE to a readonly `Readable<number>` store must be REJECTED (no \
         `Writable` contract):\n{bad_out}"
    );
}

#[test]
fn legacy_props_magic_type_checks_with_the_documented_any_exception() {
    // F12: a legacy `$$props` / `$$restProps` fixture type-checks — the magic
    // objects are `Record<string, any>` (the OWNER-APPROVED anti-`any`-gate
    // exception). NO `@ts-expect-error` anti-`any` guard on the magic object —
    // the loose `any` is intentional for the legacy forwarded-attribute bag.
    let projected = project(
        "<script lang=\"ts\">\n\
         const label = $$props.label;\n\
         const rest = $$restProps;\n\
         void label;\n\
         void rest;\n\
         </script>\n\
         <div>{$$props.anything}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "LegacyProps.svelte.tsx", &[], true)
    else {
        skip_note("legacy $$props magic");
        return;
    };
    assert!(
        ok,
        "a legacy `$$props`/`$$restProps` fixture must type-check (the documented \
         `any` exception — arbitrary member access is permitted):\n{out}"
    );
}

#[test]
fn legacy_slots_magic_checks_boolean() {
    // F12: `$$slots.foo` is `boolean` (whether the `foo` slot was filled) — a
    // PRECISE type, NOT the `any` exception. A boolean consumer checks clean; a
    // non-boolean consumer FAILS, proving `$$slots` is `Record<string, boolean>`.
    let good = project(
        "<script lang=\"ts\">\n\
         const hasFooter: boolean = $$slots.footer;\n\
         void hasFooter;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "SlotsBool.svelte.tsx", &[], true) else {
        skip_note("legacy $$slots boolean");
        return;
    };
    assert!(ok, "`$$slots.footer` must check as `boolean`:\n{out}");

    // DISCRIMINATING: assigning `$$slots.foo` to a `number` FAILS (it is
    // `boolean`, not `any`) — proving `$$slots` is NOT under the `any` exception.
    let bad = project(
        "<script lang=\"ts\">\n\
         const n: number = $$slots.footer;\n\
         void n;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "SlotsBoolBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "`$$slots.footer` (boolean) must NOT assign to `number` (it is precisely \
         typed, NOT under the `any` exception):\n{bad_out}"
    );
}

#[test]
fn magic_and_store_coexist_without_the_magic_being_store_rewritten_discriminating() {
    // F11/F12 DISCRIMINATING negative: a LEGACY component using BOTH a store
    // `$count` AND the `$$props`/`$$slots` magic type-checks CLEAN — proving the
    // magic was NOT store-rewritten. If the classifier wrongly rewrote `$$props`
    // as `__verter_store_get($$props)`, that would FAIL (`$$props:
    // Record<string, any>` is not a `Readable<T>`), so the clean type-check
    // discriminates that the magic is EXCLUDED from the store rewrite while the
    // real store `$count` IS rewritten and checked.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const n: number = $count;\n\
         const label = $$props.label;\n\
         const hasFooter: boolean = $$slots.footer;\n\
         void n;\n\
         void label;\n\
         void hasFooter;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    // RESIDUE: the magic stays verbatim (not wrapped in a store helper); the real
    // store `$count` WAS rewritten (no `$count` residue in the body).
    assert!(
        !projected.contains("__verter_store_get($$props")
            && !projected.contains("__verter_store_set($$props")
            && !projected.contains("__verter_store_get($$slots"),
        "the `$$`-magic must NOT be wrapped as a store-sub: {projected}"
    );
    assert!(
        projected.contains("__verter_store_get(count)"),
        "the real store `$count` WAS rewritten: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "MagicAndStore.svelte.tsx", &[], true)
    else {
        skip_note("magic + store coexist");
        return;
    };
    assert!(
        ok,
        "a legacy component using BOTH a store `$count` and the `$$props`/`$$slots` \
         magic must type-check CLEAN — proving the magic was NOT store-rewritten:\n{out}"
    );
}

#[test]
fn a_rune_is_not_store_rewritten_discriminating() {
    // F11 DISCRIMINATING negative: a runes-mode component using `$state` — the
    // rune stays VERBATIM (typed by the prelude, NOT wrapped in a store-get). A
    // CLEAN type-check proves the rune was excluded from the store rewrite (a
    // `__verter_store_get($state(0))` would be invalid — a call result is not a
    // `Readable<T>`).
    let projected = project(
        "<script lang=\"ts\">\n\
         let s = $state(0);\n\
         const n: number = s;\n\
         void n;\n\
         </script>\n\
         <div>{s}</div>",
    );
    // The rune `$state(0)` stays verbatim; it is NOT wrapped in a store-get (the
    // prelude's own `__verter_store_get(store)` doc-comment / `<T>` declare are not
    // a `($state` wrap, so probe the precise wrap form).
    assert!(
        projected.contains("$state(0)"),
        "the `$state` rune call stays verbatim: {projected}"
    );
    assert!(
        !projected.contains("__verter_store_get($state"),
        "a rune must NOT be wrapped as a store-sub: {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "RuneNotStore.svelte.tsx", &[], true)
    else {
        skip_note("rune not store-rewritten");
        return;
    };
    assert!(
        ok,
        "a runes-mode `$state` component must type-check CLEAN (the rune was not \
         store-rewritten):\n{out}"
    );
}

#[test]
fn store_sub_in_a_block_condition_type_checks_against_the_store_value() {
    // F11: a store-sub in a markup BLOCK CONDITION (`{#if $ready}`) is rewritten
    // and type-checks against the store's `Readable<boolean>` value — proving the
    // expanded markup-expression coverage (beyond the bare interpolation) is in
    // effect AND genuinely typed. A wrong-typed each-iterable store FAILS.
    // (The `{#if}` carries an explicit `{:else}` — an else-LESS `{#if}` projects
    // to an incomplete ternary `{cond ? (<>…</>)}`, a PRE-EXISTING `{#if}`
    // projection gap unrelated to F11; the store-sub coverage is the same with
    // the else arm present.)
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const ready = writable(false);\n\
         const items = writable<number[]>([]);\n\
         </script>\n\
         {#if $ready}<ul>{#each $items as n}<li>{n}</li>{/each}</ul>{:else}<span>no</span>{/if}",
    );
    let Some((ok, out)) = typecheck_projected(&good, "BlockStore.svelte.tsx", &[], true) else {
        skip_note("store-sub in block condition");
        return;
    };
    assert!(
        ok,
        "a store-sub in a block condition / each iterable must type-check against \
         the store value type:\n{out}"
    );

    // DISCRIMINATING: a non-store value in a `{#each}` iterable position FAILS the
    // `Readable<T>` constraint (proving the each-iterable store-sub is checked).
    let bad = project(
        "<script lang=\"ts\">\n\
         const items = 5;\n\
         </script>\n\
         <ul>{#each $items as n}<li>{n}</li>{/each}</ul>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "BlockStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a non-store `$items` in an each iterable must FAIL the `Readable<T>` \
         constraint:\n{bad_out}"
    );
}

#[test]
fn store_sub_in_a_trailing_script_type_checks_clean() {
    // F11 (move + rewrite ordering): a store-sub in a TRAILING `<script>` (after
    // the markup, hence MOVED above the render fn) is rewritten BEFORE the move,
    // so it type-checks CLEAN — proving the rewrite was not dropped/stranded by
    // the script-body relocation.
    let projected = project(
        "<div>{label}</div>\n\
         <script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const name = writable(\"hi\");\n\
         const label: string = $name;\n\
         </script>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "TrailStore.svelte.tsx", &[], true)
    else {
        skip_note("trailing-script store-sub");
        return;
    };
    assert!(
        ok,
        "a store-sub in a trailing (moved) script must type-check clean (rewrite \
         applied before the move):\n{out}"
    );
}

#[test]
fn compound_store_assignment_type_checks_against_the_writable_value() {
    // F11: `$count += 1` → `__verter_store_set(count, __verter_store_get(count) +
    // (1))` type-checks against `Writable<number>`. A wrong-typed compound RHS
    // FAILS the writable value contract.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         $count += 1;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "CompoundStore.svelte.tsx", &[], true) else {
        skip_note("compound store assignment");
        return;
    };
    assert!(
        ok,
        "a `$count += 1` compound write must type-check against `Writable<number>`:\n{out}"
    );

    // DISCRIMINATING: a wrong-typed compound RHS (a string `+=`) FAILS — the set
    // value `number + string` is `string`, which is not assignable to the store's
    // `number`.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         $count += \"x\";\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) =
        typecheck_projected(&bad, "CompoundStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `$count += \"x\"` (string RHS) must be REJECTED against `Writable<number>`:\n{bad_out}"
    );
}

#[test]
fn update_store_expression_type_checks_against_the_writable() {
    // F11: `$count++` → `__verter_store_set(count, __verter_store_get(count) + 1)`
    // type-checks against `Writable<number>`. A NON-number store FAILS (you cannot
    // `++` a string store value).
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         $count++;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&good, "UpdateStore.svelte.tsx", &[], true) else {
        skip_note("update store expression");
        return;
    };
    assert!(
        ok,
        "a `$count++` update must type-check against `Writable<number>`:\n{out}"
    );

    // DISCRIMINATING: `$flag++` on a `Writable<boolean>` FAILS — the read+set
    // body `__verter_store_get(flag) + 1` is `boolean + number`, a TS arithmetic
    // error (the `+` operator rejects a boolean operand under strict). This proves
    // the update body is genuinely type-checked against the store value type.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const flag = writable(false);\n\
         $flag++;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "UpdateStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `$flag++` on a `Writable<boolean>` must be REJECTED (arithmetic on a \
         boolean store value):\n{bad_out}"
    );

    // DISCRIMINATING (bigint passes): `$big++` on a `Writable<bigint>` type-checks
    // — the `__verter_store_update<T extends number | bigint>` helper PRESERVES the
    // `bigint` type. A naive `get(big) + 1` model would FALSELY reject this
    // (`bigint + number` is a TS error), so its acceptance proves the helper is the
    // precise update contract.
    let bigint_ok = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const big = writable(0n);\n\
         $big++;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bok, bout)) =
        typecheck_projected(&bigint_ok, "UpdateStoreBigint.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        bok,
        "a `$big++` on a `Writable<bigint>` must type-check (the update helper \
         preserves `bigint`, not a `+ 1` number coercion):\n{bout}"
    );
}

#[test]
fn store_subs_in_spread_and_transition_param_surfaces_type_check() {
    // F11 (P1-2 surfaces): store-subs in a spread attribute (`{...$attrs}`) and a
    // `transition:fn={$params}` param are rewritten and type-check (no raw
    // `$attrs`/`$params`).
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         import { fly } from \"svelte/transition\";\n\
         const attrs = writable<{ id: string }>({ id: \"a\" });\n\
         const params = writable({ delay: 100 });\n\
         </script>\n\
         <div {...$attrs} transition:fly={$params}>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "SpreadTrans.svelte.tsx", &[], true)
    else {
        skip_note("store-subs in spread/transition surfaces");
        return;
    };
    assert!(
        !projected.contains("$attrs") && !projected.contains("$params"),
        "no raw `$attrs`/`$params` residue (both rewritten): {projected}"
    );
    assert!(
        ok,
        "store-subs in `{{...$attrs}}` and `transition:fly={{$params}}` must \
         type-check (both rewritten):\n{out}"
    );
}

#[test]
fn object_shorthand_store_sub_type_checks() {
    // F11: a shorthand `{ $count }` store-sub projects to
    // `{ $count: __verter_store_get(count) }` — VALID TSX that type-checks (a bare
    // `{ __verter_store_get(count) }` would be a syntax error). The `$count` key
    // takes the store's `number` value.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         const o: { $count: number } = { $count };\n\
         void o;\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "Shorthand.svelte.tsx", &[], true) else {
        skip_note("object shorthand store-sub");
        return;
    };
    assert!(
        ok,
        "a `{{ $count }}` shorthand store-sub must project valid, type-checking \
         TSX (`{{ $count: __verter_store_get(count) }}`):\n{out}"
    );
}

#[test]
fn forced_runes_option_keeps_the_magic_undeclared_so_a_magic_reference_fails() {
    // F12: an explicit `<svelte:options runes={true}>` forces RUNES mode — the
    // `$$props` magic is NOT declared, so a `$$props` reference is an UNDECLARED
    // name (a type error). DISCRIMINATING: the SAME reference in a legacy
    // component (no forced option, no rune) type-checks (the magic is declared).
    let forced_runes = project(
        "<svelte:options runes={true} />\n\
         <script lang=\"ts\">const x = $$props.foo; void x;</script>\n\
         <div>y</div>",
    );
    let Some((ok, out)) = typecheck_projected(&forced_runes, "ForcedRunes.svelte.tsx", &[], true)
    else {
        skip_note("forced-runes magic undeclared");
        return;
    };
    assert!(
        !ok,
        "under forced runes mode `$$props` is UNDECLARED — a reference must FAIL:\n{out}"
    );

    // DISCRIMINATING: the same reference in a legacy component type-checks (the
    // magic IS declared there).
    let legacy =
        project("<script lang=\"ts\">const x = $$props.foo; void x;</script>\n<div>y</div>");
    let Some((legacy_ok, legacy_out)) =
        typecheck_projected(&legacy, "LegacyMagicRef.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        legacy_ok,
        "a legacy component's `$$props` reference must type-check (magic declared):\n{legacy_out}"
    );
}

#[test]
fn store_sub_in_an_html_tag_type_checks() {
    // F11 (tag expression surface): a store-sub in `{@html $markup}` is rewritten
    // and type-checks (the tag inner value expression is routed through the store
    // rewrite).
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const markup = writable(\"<b>hi</b>\");\n\
         </script>\n\
         <div>{@html $markup}</div>",
    );
    let Some((ok, out)) = typecheck_projected(&projected, "TagStore.svelte.tsx", &[], true) else {
        skip_note("store-sub in @html tag");
        return;
    };
    assert!(
        ok,
        "a store-sub in `{{@html $markup}}` must type-check (the tag inner \
         expression is rewritten):\n{out}"
    );
}

#[test]
fn bind_value_with_a_store_sub_type_checks() {
    // F11 (P1-1): a store-sub in a `bind:value={$store}` value is rewritten
    // through the store-get and composes with the bind projection — the bound
    // value is the store's `Readable<string>` value. A correct-typed store checks
    // clean; a wrong-typed store value FAILS (proving the bind value is genuinely
    // typed from the store, not raw `$store` residue / `any`).
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const name = writable(\"\");\n\
         </script>\n\
         <input bind:value={$name} />",
    );
    // RESIDUE: no raw `$name` survives in the bind value (it WAS rewritten).
    assert!(
        good.contains("__verter_store_get(name)") && !render_body(&good).contains("$name"),
        "the `bind:value={{$name}}` store-sub was rewritten (no raw `$name`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "BindStore.svelte.tsx", &[], true) else {
        skip_note("bind:value store-sub");
        return;
    };
    assert!(
        ok,
        "a `bind:value={{$name}}` store-sub (a `Writable<string>`) must type-check \
         against the `<input>` value:\n{out}"
    );

    // DISCRIMINATING via a COMPONENT prop bind (strict `$props` typing — an
    // intrinsic `<input value>` is loosely typed by SvelteHTMLElements, so a
    // component bind is the discriminating surface). A `bind:label={$store}` over
    // a `Writable<number>` store bound to a `label: string` component prop FAILS —
    // proving the store-rewritten bind value is genuinely typed (the same
    // `rewrite_bind_to_attribute` path as the intrinsic `bind:value`).
    let comp_good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const Child = (null as any) as abstract new (...a: never[]) => \
         { $props: { label: string } };\n\
         const label = writable(\"hi\");\n\
         </script>\n\
         <Child bind:label={$label} />",
    );
    assert!(
        comp_good.contains("__verter_store_get(label)"),
        "the component `bind:label={{$label}}` store-sub was rewritten: {comp_good}"
    );
    let Some((cok, cout)) = typecheck_projected(&comp_good, "BindCompStore.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        cok,
        "a component `bind:label={{$label}}` (a `Writable<string>`) must check \
         against the `label: string` prop:\n{cout}"
    );

    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const Child = (null as any) as abstract new (...a: never[]) => \
         { $props: { label: string } };\n\
         const num = writable<number>(0);\n\
         </script>\n\
         <Child bind:label={$num} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "BindStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a component `bind:label={{$num}}` over a `Writable<number>` store must be \
         REJECTED against the `label: string` prop (proves the bind value is \
         store-typed, not `any`):\n{bad_out}"
    );
}

#[test]
fn declaration_tag_value_with_a_store_sub_type_checks() {
    // F11 (P1-2): a store-sub in a `{@const x = $store}` VALUE is rewritten
    // MOVE-SAFELY (the store-bearing inner is TEXT-rewritten and emitted at the
    // hoist anchor — the mapped-move boundary cannot carry the trailing
    // close-paren), so the hoisted `const x = __verter_store_get(store)`
    // declaration type-checks AND is visible to a following sibling. The
    // TRAILING-store form (`= $count`, the store as the inner's LAST token) is the
    // stranding-sensitive case — it must not strand a `)` at the original tag
    // position. A correct consumer checks clean; a wrong-typed consumer FAILS.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         </script>\n\
         <div>{@const c = $count}{@const doubled = c * 2}{doubled}</div>",
    );
    assert!(
        good.contains("const c = __verter_store_get(count)") && !good.contains("$count"),
        "the trailing-store `{{@const c = $count}}` was rewritten move-safely (no \
         stranded paren, no raw `$count`): {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "ConstStore.svelte.tsx", &[], true) else {
        skip_note("@const store-sub");
        return;
    };
    assert!(
        ok,
        "a trailing-store `{{@const c = $count}}` + a following sibling `{{@const \
         doubled = c * 2}}` must type-check (move-safe rewrite, sibling-visible):\n{out}"
    );

    // DISCRIMINATING: a wrong-typed consumer of the store-derived const FAILS —
    // the value is `number` (from `Writable<number>`), not `any`.
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         </script>\n\
         <div>{@const doubled = $count}{@const s = doubled as string}{s}</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "ConstStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `number`-typed store-derived `{{@const}}` value must NOT cast to \
         `string` (proves it is store-typed, not `any`):\n{bad_out}"
    );
}

#[test]
fn dynamic_component_this_with_a_store_sub_type_checks() {
    // F11 (P1-3): a store-sub in `<svelte:component this={$store}>` is rewritten
    // through the store-get, so the dynamic-component checker sees the store's
    // (component-typed) value. A store holding a valid component checks clean; a
    // store holding a NON-component value FAILS the constructor constraint.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         type Dyn = abstract new (...a: never[]) => { $props: { label: string } };\n\
         const Cmp = writable<Dyn>(null as any);\n\
         </script>\n\
         <svelte:component this={$Cmp} label=\"hi\" />",
    );
    assert!(
        good.contains("__verter_store_get(Cmp)"),
        "the dynamic-component `this={{$Cmp}}` store-sub was rewritten: {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "DynStore.svelte.tsx", &[], true) else {
        skip_note("dynamic-component this store-sub");
        return;
    };
    assert!(
        ok,
        "a `<svelte:component this={{$Cmp}}>` store-sub holding a component must \
         type-check (the `this` expression is store-rewritten):\n{out}"
    );

    // DISCRIMINATING: a store holding a NON-component value FAILS the
    // `__verter_dynamic_component` constructor constraint — proving the `this`
    // value is the store value (not raw `$Cmp` / `any`).
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const Cmp = writable<number>(0);\n\
         </script>\n\
         <svelte:component this={$Cmp} />",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "DynStoreBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `<svelte:component this={{$Cmp}}>` over a `Writable<number>` store must \
         be REJECTED (the store value is not a component):\n{bad_out}"
    );
}

#[test]
fn a_nested_function_scope_store_sub_is_not_over_suppressed_end_to_end() {
    // F11 (P1-4, end-to-end soundness): a top-level store `$count` used in markup
    // STILL rewrites to the store-get and type-checks even when an UNRELATED
    // nested function declares a local `let $count` — the over-suppression bug
    // (lexically-scoped declared-name collection) is gone. A raw `$count` residue
    // in the markup would be `Cannot find name '$count'`.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         function unrelated() { let $count = 1; return $count; }\n\
         void unrelated;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    assert!(
        projected.contains("__verter_store_get(count)"),
        "the TOP-LEVEL store `$count` in markup WAS rewritten despite the nested \
         `let $count`: {projected}"
    );
    // No raw `$count` reference survives in the render body (the markup `$count`
    // was rewritten; the nested-fn `$count` lives in the hoisted script body, a
    // valid local binding).
    let Some((ok, out)) = typecheck_projected(&projected, "NestedScopeStore.svelte.tsx", &[], true)
    else {
        skip_note("nested-scope store-sub not over-suppressed");
        return;
    };
    assert!(
        ok,
        "a markup store `$count` must rewrite + type-check even with an unrelated \
         nested-function `let $count` (no over-suppression, no raw `$count` \
         residue):\n{out}"
    );
}

#[test]
fn member_write_on_a_store_sub_degrades_safe_relaxed_write_check() {
    // F11 documented bounded boundary: a MEMBER write rooted at a store-sub
    // (`$obj.x = 1`) projects to `__verter_store_get(obj).x = 1` — it mutates the
    // READ object's member (valid TSX, relaxed: it does not REQUIRE the store be
    // `Writable`, since Svelte's `$obj.x = v` is a whole-object store set). This
    // characterizes the SAFE-DEGRADE: the projection is VALID TSX and type-checks
    // (the member write checks against the store VALUE's member type, a real
    // check — not silently dropped), while the whole-object writable requirement
    // is relaxed. A precise read→mutate→set projection is a follow-up.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const obj = writable({ x: 0 });\n\
         $obj.x = 1;\n\
         </script>\n\
         <div>x</div>",
    );
    // RESIDUE: the base `$obj` is rewritten as a READ (`get(obj)`), and the `.x`
    // member write stays verbatim (the relaxed-write degrade).
    assert!(
        projected.contains("__verter_store_get(obj).x")
            && !projected.contains("__verter_store_set(obj"),
        "a `$obj.x = 1` member write projects the base as a READ (relaxed): \
         {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "MemberWrite.svelte.tsx", &[], true)
    else {
        skip_note("member-write safe-degrade");
        return;
    };
    assert!(
        ok,
        "a `$obj.x = 1` member write projects to valid TSX that type-checks (the \
         relaxed safe-degrade — member type checked, whole-object writable \
         relaxed):\n{out}"
    );

    // DISCRIMINATING: the member write is still a REAL check — a wrong-typed
    // member value FAILS (the `.x` is `number` from the store value, not `any`).
    let bad = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const obj = writable({ x: 0 });\n\
         $obj.x = \"not a number\";\n\
         </script>\n\
         <div>x</div>",
    );
    let Some((bad_ok, bad_out)) = typecheck_projected(&bad, "MemberWriteBad.svelte.tsx", &[], true)
    else {
        return;
    };
    assert!(
        !bad_ok,
        "a `$obj.x = \"str\"` member write must be REJECTED against the store \
         value's `x: number` member (the member check is real):\n{bad_out}"
    );
}

#[test]
fn a_block_scoped_dollar_binding_does_not_over_suppress_a_markup_store_sub_end_to_end() {
    // F11 (P1-4, second-pass soundness): a BLOCK-local `let $count` (inside a
    // function body block, an unrelated lexical scope) must NOT suppress the
    // top-level markup store `$count` — it still rewrites + type-checks. A raw
    // `$count` residue would be `Cannot find name '$count'`. Covers the
    // block/for/named-fn-expr scope precision (the classifier's lexical model is
    // not merely function-boundary granular).
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const count = writable(0);\n\
         function unrelated() { { let $count = 1; return $count; } }\n\
         void unrelated;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    assert!(
        projected.contains("__verter_store_get(count)"),
        "the markup store `$count` rewrites despite a block-local `let $count`: \
         {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BlockScopeStore.svelte.tsx", &[], true)
    else {
        skip_note("block-scope store-sub not over-suppressed");
        return;
    };
    assert!(
        ok,
        "a markup store `$count` must rewrite + type-check even with an unrelated \
         BLOCK-local `let $count` (no over-suppression, no raw residue):\n{out}"
    );
}

#[test]
fn an_imported_dollar_local_is_not_store_rewritten_in_markup_end_to_end() {
    // F11 FALSE-POSITIVE guard: a `$`-prefixed IMPORT local (`import { x as $foo }`)
    // is an ordinary value — a markup `{$foo}` must NOT be store-rewritten (it is
    // NOT a store-sub). A `__verter_store_get($foo)` wrap would FAIL (`$foo` is a
    // plain `number`, not a `Readable<T>`); the CLEAN type-check proves the import
    // local was excluded from the store rewrite while the real store IS rewritten.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         import { answer as $foo } from \"./vals\";\n\
         const count = writable(0);\n\
         </script>\n\
         <div>{$foo}{$count}</div>",
    );
    // RESIDUE: the import local `$foo` stays verbatim (NOT wrapped); the real
    // store `$count` IS rewritten.
    assert!(
        !projected.contains("__verter_store_get($foo")
            && !projected.contains("__verter_store_get(foo)")
            && projected.contains("__verter_store_get(count)"),
        "the import local `$foo` must NOT be store-rewritten while `$count` is: \
         {projected}"
    );
    let Some((ok, out)) = typecheck_projected(
        &projected,
        "ImportLocal.svelte.tsx",
        &[("vals.ts", "export const answer: number = 42;\n")],
        true,
    ) else {
        skip_note("import-local not store-rewritten");
        return;
    };
    assert!(
        ok,
        "an imported `$foo` local must type-check (NOT store-rewritten) alongside a \
         real store `$count`:\n{out}"
    );
}

#[test]
fn a_dollar_name_in_a_ts_type_position_is_not_store_rewritten_end_to_end() {
    // F11 FALSE-POSITIVE guard: a `$`-prefixed identifier in a TYPE position
    // (a type annotation / type-alias body) is a TYPE reference, NEVER a store-sub.
    // No `__verter_store_get` may be injected in type space — that would be invalid
    // TSX. A clean type-check with a real value-position store `$count` rewritten
    // discriminates that ONLY the value `$count` was rewritten.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         type $Foo = { a: number };\n\
         const count = writable(0);\n\
         const obj: $Foo = { a: 1 };\n\
         void obj;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    // RESIDUE: the type-position `$Foo` is NOT wrapped; the value `$count` IS.
    assert!(
        !projected.contains("__verter_store_get(Foo)")
            && !projected.contains("__verter_store_get($Foo")
            && projected.contains("__verter_store_get(count)"),
        "a `$Foo` type reference must NOT be store-rewritten while `$count` is: \
         {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "TypePosition.svelte.tsx", &[], true)
    else {
        skip_note("type-position $-name not store-rewritten");
        return;
    };
    assert!(
        ok,
        "a `$Foo` type reference must type-check (NOT store-rewritten) alongside a \
         real value store `$count`:\n{out}"
    );
}

#[test]
fn a_dollar_name_in_an_implements_or_cast_type_position_is_not_store_rewritten_end_to_end() {
    // F11 FALSE-POSITIVE guard (extended type-reference surfaces): a `$`-name
    // reached through a class `implements` clause or an `as` cast type is a TYPE
    // reference, NEVER a store-sub. These type positions do NOT pass through the
    // `TSType` umbrella visitor — they reach the shared type-name bridge directly —
    // so the classifier must intercept them via `visit_ts_type_name`. Injecting a
    // `__verter_store_get` there would be invalid TSX (a call expression where a
    // type name is required). A real value-position `$count` in the SAME component
    // discriminates that ONLY the value store was rewritten.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         interface $Shape { a: number }\n\
         class Impl implements $Shape { a = 1; }\n\
         const count = writable(0);\n\
         const widened = ({ a: 1 } as $Shape);\n\
         void new Impl(); void widened;\n\
         </script>\n\
         <div>{$count}</div>",
    );
    // RESIDUE: neither the `implements $Shape` nor the `as $Shape` cast is wrapped;
    // the value `$count` IS.
    assert!(
        !projected.contains("__verter_store_get(Shape)")
            && !projected.contains("__verter_store_get($Shape")
            && projected.contains("__verter_store_get(count)"),
        "a `$Shape` type reference (implements / as-cast) must NOT be store-rewritten \
         while `$count` is: {projected}"
    );
    let Some((ok, out)) =
        typecheck_projected(&projected, "ImplementsTypePosition.svelte.tsx", &[], true)
    else {
        skip_note("implements/cast type-position $-name not store-rewritten");
        return;
    };
    assert!(
        ok,
        "a `$Shape` type reference in `implements` / `as` position must type-check \
         (NOT store-rewritten) alongside a real value store `$count`:\n{out}"
    );
}

#[test]
fn an_each_dollar_binding_type_checks_clean_and_a_real_store_sub_still_rewrites() {
    // BLOCKER A: a `$`-named markup block binding (`{#each list as $item}`) is an
    // ORDINARY local in the each body, NOT a store-sub. Mis-classifying it would
    // emit `__verter_store_get(item)` — referencing a non-existent `item` store →
    // `Cannot find name 'item'`. The clean type-check proves the block binding is
    // NOT store-rewritten. A genuine `$count` store-sub in the SAME body still
    // rewrites and type-checks against the store value.
    let good = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const list: { id: number; label: string }[] = [];\n\
         const count = writable(0);\n\
         </script>\n\
         <ul>{#each list as $item}<li>{$item.label}-{$count}</li>{/each}</ul>",
    );
    // RESIDUE: the `$item` binding is NOT store-rewritten; the `$count` store IS.
    assert!(
        !good.contains("__verter_store_get(item)") && good.contains("__verter_store_get(count)"),
        "the `$item` each binding must NOT be store-rewritten while `$count` is: {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "EachDollarBinding.svelte.tsx", &[], true)
    else {
        skip_note("each $-binding type-check");
        return;
    };
    assert!(
        ok,
        "a `$`-named each binding must type-check clean (treated as a local, not a \
         store-sub) alongside a real `$count` store:\n{out}"
    );
}

#[test]
fn an_await_then_and_catch_dollar_binding_type_check_clean() {
    // BLOCKER A: `{#await p then $v}` / `{:catch $e}` bindings are locals, not
    // store-subs. A clean type-check proves no false `__verter_store_get(v)` /
    // `(e)` rewrite (which would reference non-existent stores).
    let good = project(
        "<script lang=\"ts\">\n\
         const p: Promise<number> = Promise.resolve(1);\n\
         </script>\n\
         {#await p}<span>loading</span>{:then $v}<span>{$v}</span>{:catch $e}<span>{String($e)}</span>{/await}",
    );
    assert!(
        !good.contains("__verter_store_get(v)") && !good.contains("__verter_store_get(e)"),
        "the await then/catch `$v`/`$e` bindings must NOT be store-rewritten: {good}"
    );
    let Some((ok, out)) = typecheck_projected(&good, "AwaitDollarBinding.svelte.tsx", &[], true)
    else {
        skip_note("await then/catch $-binding type-check");
        return;
    };
    assert!(
        ok,
        "the await then/catch `$`-bindings must type-check clean (locals, not \
         store-subs):\n{out}"
    );
}

#[test]
fn a_bind_this_store_target_emits_valid_tsx_without_a_cannot_find_name_error() {
    // BLOCKER D: a `$store` as a `bind:this` TARGET is invalid Svelte. The
    // projection must be SYNTACTICALLY VALID and must NOT surface a phantom
    // `Cannot find name '$el'` — it rewrites the target to the read-helper form so
    // the (genuine) lvalue error is on the actual construct, not a name error.
    let projected = project(
        "<script lang=\"ts\">\n\
         import { writable } from \"svelte/store\";\n\
         const el = writable<HTMLInputElement | null>(null);\n\
         </script>\n\
         <input bind:this={$el} />",
    );
    // No raw `$el` residue anywhere; rewritten to the read helper. (`$el` is not
    // a rune name, so the prelude never contains it.)
    assert!(
        !projected.contains("$el") && projected.contains("__verter_store_get(el)"),
        "the `bind:this={{$el}}` target must be rewritten (no raw `$el`): {projected}"
    );
    let Some((ok, out)) = typecheck_projected(&projected, "BindThisStore.svelte.tsx", &[], true)
    else {
        skip_note("bind:this store-target valid TSX");
        return;
    };
    // The projected TSX is INVALID Svelte input (a store as a `bind:this` target),
    // so it does NOT type-check clean — DISCRIMINATING: assert it FAILS, and that
    // the failure is the GENUINE lvalue error on the actual construct, NOT a
    // phantom `Cannot find name` (TS2304/TS2552) for a raw `$el`.
    assert!(
        !ok,
        "the `bind:this={{$el}}` projection (a store as a bind-target) is invalid \
         Svelte and MUST NOT type-check clean — the genuine lvalue error must \
         surface:\n{out}"
    );
    assert!(
        !out.contains("Cannot find name '$el'") && !out.contains("Cannot find name \"$el\""),
        "the `bind:this={{$el}}` projection must NOT surface a phantom `Cannot find \
         name '$el'` — the store-sub is rewritten so the error lands on the real \
         construct:\n{out}"
    );
    // The rewrite makes the LHS a call result (`__verter_store_get(el) = …`), so
    // the genuine diagnostic is TS2364 (the left-hand side of an assignment must
    // be a variable or a property access) — the real lvalue error on the actual
    // construct, exactly what we want surfaced instead of a phantom name error.
    assert!(
        out.contains("TS2364"),
        "the genuine lvalue error (TS2364 — LHS of assignment must be a variable \
         or property access) must surface on the rewritten construct:\n{out}"
    );
}

// ── Svelte rune-module (`.svelte.ts`/`.svelte.js`) TSGO validity ────────────
//
// A standalone rune module is a NON-COMPONENT carrier. Its provider surface
// (Channel B) is `<module rune prelude> + <real module bytes>`, served from the
// module's OWN canonical path so a consumer resolving disk bytes sees the
// inferred rune-derived exported types. These fixtures model that provider
// content exactly: the rune module written to disk IS the prelude-augmented
// content the host feeds the provider.

use verter_compiler::svelte::ide::prelude::{
    render_rune_prelude, RuneModuleSourceType, RunePreludeMode,
};

/// Build the rune-module provider content (Channel B) for `module_src` under
/// `source_type` — the SAME `<prelude> + <bytes>` the host feeds the provider.
fn rune_module_provider_content(module_src: &str, source_type: RuneModuleSourceType) -> String {
    let prelude = render_rune_prelude(RunePreludeMode::Module { source_type });
    format!("{prelude}{module_src}")
}

#[test]
fn ts_rune_module_with_module_scope_runes_type_checks_and_consumer_sees_inferred_type() {
    // A `.svelte.ts` rune module uses module-scope runes; the provider content
    // (prelude + bytes) type-checks, and a CONSUMER importing it sees the
    // inferred rune-derived type (`$state(0)` ⇒ `number`).
    let module_src = "export const s = $state(0);\n\
        export const d = $derived(s * 2);\n\
        export const by = $derived.by(() => s + 1);\n\
        $effect(() => { void s; });\n\
        $effect.pre(() => {});\n\
        const tracking: boolean = $effect.tracking();\n\
        void tracking;\n\
        $inspect(s);\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Ts);

    // The consumer resolves the rune module by its disk path and checks the
    // inferred export types. `@ts-expect-error` guards a WRONG assignment — if
    // `s` leaked `any`/`never`, the guarded line would not error and TS would
    // flag the UNUSED `@ts-expect-error` (discriminating both ways under strict).
    let consumer = "import { s, d, by } from \"./store.svelte.ts\";\n\
        const ok: number = s;\n\
        void ok;\n\
        const okd: number = d;\n\
        void okd;\n\
        const okby: number = by;\n\
        void okby;\n\
        // @ts-expect-error `s` is inferred `number`, not assignable to `string`\n\
        const bad: string = s;\n\
        void bad;\n";

    let Some((ok, out)) = typecheck_projected(
        &provider,
        "store.svelte.ts",
        &[("consumer.ts", consumer)],
        false,
    ) else {
        skip_note("ts rune module + consumer");
        return;
    };
    assert!(
        ok,
        "a `.svelte.ts` rune module (prelude + bytes) must type-check and the \
         consumer must see `s: number` (the @ts-expect-error proves the type is \
         number, not any/never):\n{out}"
    );
}

#[test]
fn js_rune_module_type_checks_under_checkjs_and_consumer_sees_inferred_type() {
    // A `.svelte.js` rune module gets the JS-valid (JSDoc-typed) rune prelude.
    // Under checkJs it type-checks and a consumer sees the inferred type.
    let module_src = "export const s = $state(0);\n\
        export const d = $derived(s * 2);\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Js);

    let consumer = "import { s, d } from \"./store.svelte.js\";\n\
        const ok: number = s;\n\
        void ok;\n\
        const okd: number = d;\n\
        void okd;\n\
        // @ts-expect-error `s` is inferred `number`, not assignable to `string`\n\
        const bad: string = s;\n\
        void bad;\n";

    // checkJs/allowJs are required for the .js module to be checked.
    let Some((ok, out)) = typecheck_projected_with_options(
        &provider,
        "store.svelte.js",
        &[("consumer.ts", consumer)],
        false,
        true,
    ) else {
        skip_note("js rune module + consumer");
        return;
    };
    assert!(
        ok,
        "a `.svelte.js` rune module (JS-valid prelude + bytes) must type-check \
         under checkJs and the consumer must see `s: number`:\n{out}"
    );
}

#[test]
fn rune_module_does_not_leak_runes_into_a_plain_ts_consumer() {
    // DISCRIMINATING per-file scoping (Channel B `export {};` module-local): the
    // rune prelude must NOT leak `$state` globally — a PLAIN `.ts` (no prelude)
    // cannot see `$state`. The rune module's own provider content is module-local.
    let module_src = "export const s = $state(0);\nvoid s;\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Ts);

    // A plain `.ts` file that tries to call `$state` — it must FAIL (no leak).
    let plain = "const leak = $state(1);\nvoid leak;\n";

    let Some((ok, out)) =
        typecheck_projected(&provider, "store.svelte.ts", &[("plain.ts", plain)], false)
    else {
        skip_note("rune no-leak into plain ts");
        return;
    };
    assert!(
        !ok,
        "a plain `.ts` must NOT see `$state` (the rune prelude is module-local — \
         no global leak); the plain file's `$state` call must FAIL:\n{out}"
    );
    assert!(
        out.contains("$state") || out.to_lowercase().contains("cannot find name"),
        "the failure must name the undefined `$state` in the plain file:\n{out}"
    );
}

#[test]
fn rune_module_rejects_component_only_runes_and_projection_helpers() {
    // DISCRIMINATING negative: a rune module's prelude EXCLUDES the
    // component-only runes (`$props`/`$bindable`/`$host`) and every `__verter_*`
    // projection helper, so referencing them in a rune module FAILS — they are
    // not in scope outside a component.
    for (label, bad_src) in [
        ("$props", "export const p = $props();\nvoid p;\n"),
        ("$host", "export const h = $host();\nvoid h;\n"),
        ("$bindable", "export const b = $bindable();\nvoid b;\n"),
        ("__verter_void", "__verter_void(1);\nexport const x = 1;\n"),
    ] {
        let provider = rune_module_provider_content(bad_src, RuneModuleSourceType::Ts);
        let Some((ok, out)) = typecheck_projected(&provider, "store.svelte.ts", &[], false) else {
            skip_note("rune module rejects component-only runes");
            return;
        };
        assert!(
            !ok,
            "a rune module must REJECT the component-only / projection name `{label}` \
             (not in scope outside a component):\n{out}"
        );
    }
}

#[test]
fn ts_rune_module_zero_arg_state_type_checks_and_infers_optional() {
    // PARITY (P1b): the TS rune prelude carries the zero-arg `$state()` overload
    // (`$state<T>(): T | undefined`). A valid zero-arg rune module type-checks,
    // a later value assigns, and a wrong-typed use still FAILS.
    let module_src = "let count = $state<number>();\n\
        count = 5;\n\
        export const c = count;\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Ts);

    let Some((ok, out)) = typecheck_projected(&provider, "store.svelte.ts", &[], false) else {
        skip_note("ts zero-arg $state");
        return;
    };
    assert!(
        ok,
        "a zero-arg `$state<number>()` rune module must type-check (the zero-arg \
         overload is present) and a later numeric assignment must work:\n{out}"
    );
}

#[test]
fn js_rune_module_zero_arg_state_type_checks() {
    // PARITY (P1b) — the DISCRIMINATING arity test: a valid zero-arg `$state()`
    // JS rune module under checkJs. PRE-FIX the JS prelude made the `initial`
    // arg REQUIRED (no zero-arg overload), so this FAILED with "Expected 1
    // arguments, but got 0"; POST-FIX the `@overload` zero-arg form is present
    // and it type-checks.
    let module_src = "export const count = $state();\nvoid count;\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Js);

    let Some((ok, out)) =
        typecheck_projected_with_options(&provider, "store.svelte.js", &[], false, true)
    else {
        skip_note("js zero-arg $state");
        return;
    };
    assert!(
        ok,
        "a zero-arg `$state()` JS rune module must type-check under checkJs (the \
         JS prelude carries the zero-arg overload):\n{out}"
    );
}

#[test]
fn js_rune_module_zero_arg_state_is_unknown_not_any() {
    // DISCRIMINATING anti-`any` WITHOUT a variable annotation (the gap codex
    // flagged): an UNANNOTATED zero-arg `$state()` must infer `unknown` (the
    // sound TS mirror), NOT `any`. A generic `T | undefined` JS overload would
    // collapse the unbound `T` to the UNSOUND `any` (a JS call site has no place
    // to bind `T`), and `any` IS assignable to `number` — so the consumer's
    // `@ts-expect-error` would NOT fire and TS would flag the unused directive,
    // FAILING this test. `unknown` is NOT assignable to `number` without
    // narrowing, so the directive fires and the file type-checks clean. This
    // discriminates `unknown` from `any` with no annotation crutch.
    let module_src = "export const count = $state();\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Js);

    let consumer = "import { count } from \"./store.svelte.js\";\n\
        // @ts-expect-error `count` is `unknown` (NOT `any`) — not assignable to `number`\n\
        const n: number = count;\n\
        void n;\n";

    let Some((ok, out)) = typecheck_projected_with_options(
        &provider,
        "store.svelte.js",
        &[("consumer.ts", consumer)],
        false,
        true,
    ) else {
        skip_note("js zero-arg $state unknown-not-any");
        return;
    };
    assert!(
        ok,
        "an UNANNOTATED zero-arg `$state()` must infer `unknown` (not `any`): the \
         `@ts-expect-error` on `const n: number = count` fires for `unknown` and \
         the file type-checks clean; if it inferred `any` the directive would be \
         unused and this would FAIL:\n{out}"
    );
}

#[test]
fn js_rune_module_explicitly_typed_state_use_still_fails() {
    // DISCRIMINATING the other way (the `$state(initial)` first overload binds
    // `T`): an explicitly-typed `$state(0)` must infer `number`, and a wrong
    // assignment (to `string`) must still FAIL under checkJs — the JS surface is
    // NOT a loose `any`.
    let module_src = "export const count = $state(0);\n";
    let provider = rune_module_provider_content(module_src, RuneModuleSourceType::Js);

    let consumer = "import { count } from \"./store.svelte.js\";\n\
        // @ts-expect-error `count` is `number`, not assignable to `string`\n\
        const bad: string = count;\n\
        void bad;\n";

    let Some((ok, out)) = typecheck_projected_with_options(
        &provider,
        "store.svelte.js",
        &[("consumer.ts", consumer)],
        false,
        true,
    ) else {
        skip_note("js explicitly-typed $state");
        return;
    };
    assert!(
        ok,
        "the consumer's `@ts-expect-error` must fire (`$state(0)` is `number`, \
         not `string`) — proving the JS first overload binds the generic, not a \
         loose `any` surface:\n{out}"
    );
}
