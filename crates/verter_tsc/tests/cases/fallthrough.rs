//! End-to-end attribute-fallthrough prop-checking matrix for `verter-tsc`.
//!
//! <https://github.com/pikax/verter/issues/97>: Vue 3 forwards every attribute
//! a component does not declare onto its single root element via `$attrs`, so
//! `<ChildDiv title="hello" />` renders correctly even though `ChildDiv`
//! declares no `title` prop. Verter rejected it.
//!
//! The oracle for this matrix is **Vue 3 runtime behaviour** — "does this
//! attribute actually reach the DOM as a type-checked attribute of that
//! element" — NOT vue-tsc's default output. vue-tsc reports nothing here by
//! default because `checkUnknownProps` defaults to `strictTemplates` (off); it
//! also reports nothing for the Reject* files below, which are genuine errors.
//! Verter is strict by default by design, and this fix makes the strict check
//! CORRECT rather than lenient.
//!
//! Both halves are asserted per file. Widening trades a false positive for a
//! false negative, and a false negative — an attribute accepted here that
//! reaches nothing at runtime — is the worse defect, so the Reject* half is
//! the load-bearing half.
//!
//! This drives the REAL producer: the `verter-tsc` binary on a real project,
//! through `get_public_api_batch` → the public-API `.tsc.tsx` stub → tsgo.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root")
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("fallthrough")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(s) = status {
        if !s.success() {
            let _ = std::os::windows::fs::symlink_dir(src, dest);
        }
    }
}

#[cfg(not(windows))]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(src, dest);
}

/// The workspace-root `node_modules`, which must be able to SERVE this test: it
/// must carry `vue` (the carrier's type contract) AND a TypeScript whose major
/// is >= 7 (the tsgo native engine `verter-tsc --noEmit` requires; there is no
/// tsc fallback on the typecheck path).
///
/// A missing prerequisite FAILS. It is never skipped: this test's prerequisite
/// is the workspace's OWN pinned `node_modules`, which `pnpm install` produces
/// and the canonical gate's preflight ensures, so its absence is a broken
/// environment rather than an ordinary condition. Returning early instead
/// reports a green result having executed ZERO fixtures — exactly the
/// "exit status 0 alone is FAIL" / "unexpected prerequisite skips must be zero"
/// bypass the repo's Verification Must Prove Execution rule forbids.
///
/// The workspace ROOT is deliberate. `packages/example/node_modules` carries
/// `vue` but pins TypeScript 6, so a temp project linked there resolves NO tsgo
/// engine and `verter-tsc` reports zero diagnostics.
fn engine_capable_node_modules() -> PathBuf {
    let node_modules = workspace_root().join("node_modules");
    assert!(
        node_modules.join("vue").exists(),
        "{} has no `vue`: this matrix cannot run and MUST NOT report a pass \
         having checked nothing. Run `pnpm install` at the workspace root.",
        node_modules.display()
    );

    let ts_manifest = node_modules.join("typescript").join("package.json");
    let manifest = std::fs::read_to_string(&ts_manifest).unwrap_or_else(|error| {
        panic!(
            "no readable TypeScript manifest at {}: {error}. Run `pnpm install` \
             at the workspace root.",
            ts_manifest.display()
        )
    });
    let major = manifest
        .split("\"version\"")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(1))
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok());
    assert!(
        major.is_some_and(|major| major >= 7),
        "{} pins TypeScript major {major:?}; the `--noEmit` typecheck path is \
         tsgo(TypeScript >= 7)-only with no tsc fallback, so this matrix would \
         check nothing. Install the workspace-pinned TypeScript.",
        ts_manifest.display()
    );
    node_modules
}

fn setup_temp_project() -> (tempfile::TempDir, PathBuf) {
    let node_modules_src = engine_capable_node_modules();

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_path = temp.path().to_path_buf();
    copy_dir_recursive(&fixture_dir(), &temp_path).expect("failed to copy fixture");

    let nm_dest = temp_path.join("node_modules");
    create_junction_or_symlink(&node_modules_src, &nm_dest);
    assert!(
        nm_dest.join("vue").exists(),
        "the node_modules junction/symlink must resolve — without it every \
         carrier fails module resolution and the matrix below measures nothing"
    );
    (temp, temp_path)
}

/// `file(line,col): error TSxxxx: message` → the `.vue` basename.
fn error_files(stdout: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in stdout.lines() {
        let Some(paren) = line.find('(') else {
            continue;
        };
        let rest = &line[paren..];
        if !rest.contains("): error TS") {
            continue;
        }
        let file = line[..paren].replace('\\', "/");
        if let Some(name) = file.rsplit('/').next() {
            files.push(name.to_string());
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Every parent fixture, with the required outcome AND the reason. The
/// `Accept` half must produce ZERO errors; the `Reject` half must produce at
/// least one. Both are derived from the fixture set below, not hard-coded
/// counts.
const MATRIX: &[(&str, bool, &str)] = &[
    (
        "AcceptGlobalAttr.vue",
        true,
        "`title` is a global HTML attribute; ChildDiv's single native <div> root \
         receives it through $attrs and it reaches the DOM",
    ),
    (
        "AcceptNativeListener.vue",
        true,
        "`@click` becomes `onClick` in $attrs and is bound on the root <div>",
    ),
    (
        "AcceptThroughComponentRoot.vue",
        true,
        "ChildWrapsComp's root is <Grandchild/>, whose root is <div> — the \
         resolver propagates through the component root and `title` still \
         reaches the DOM",
    ),
    (
        "AcceptChildPropThroughComponentRoot.vue",
        true,
        "WrapsTypedChild's root is <ChildOptionalProp/>, which DECLARES `optProp`. Vue \
         forwards an undeclared attribute onto the root component, where a \
         declared prop of that name CONSUMES it — so `optProp` is legal on the \
         wrapper. This is issue #97's shape and the case where the carrier used \
         to disagree with the Verter lint, which reads the same resolver",
    ),
    (
        "RejectChildPropWrongTypeThroughComponentRoot.vue",
        false,
        "`optProp` is `number` where ChildOptionalProp declares it — the inherited \
         surface must carry the child's REAL member type, not `unknown`, or the \
         widening becomes an unbounded false negative in the value dimension",
    ),
    (
        "AcceptLeafPropThroughNoInheritRoot.vue",
        true,
        "WrapsNoInheritLeaf's root is a component with `inheritAttrs: false` that \
         declares `tone`. The attribute is consumed as that component's PROP, \
         before its inheritance flag is even relevant — the resolver marks this \
         branch Resolved and the carrier must not discard it",
    ),
    (
        "RejectAttrThroughNoInheritComponentRoot.vue",
        false,
        "`title` is not declared by the `inheritAttrs: false` leaf and reaches no \
         element through it, so the element half of that branch must stay \
         fail-closed even though the declared-prop half is projected",
    ),
    (
        "AcceptDataAttr.vue",
        true,
        "`data-testid` is universally valid HTML and Vue forwards it onto the \
         root element verbatim. NOTE: this row pins the TEMPLATE half only — \
         TypeScript skips excess-property checking on JSX attributes whose names \
         are not valid identifiers, so the template path accepts `data-*` \
         regardless of the surface. The $props/constructor half (real \
         HTMLAttributes has no `data-*` member and no index signature, so the \
         widened arm must carry a closed `data-${string}` key domain) is pinned \
         by `WidenedWithDataAttr` in the hermetic \
         `vue_macro_tsc_typecheck_gate`, which DOES discriminate it",
    ),
    (
        "AcceptBranchOnlyAttrOnConditionalRoot.vue",
        true,
        "ConditionalRoot renders <input v-if> / <div v-else>; `checked` is an \
         <input>-only attribute and reaches the DOM on the input branch, so the \
         exact-union of the branches accepts it",
    ),
    (
        "RejectUnknownOnConditionalRoot.vue",
        false,
        "`notARealThing` is a member of NEITHER branch's element type — the \
         conditional union is a union of real element props types, never an open \
         surface",
    ),
    (
        "AcceptClassStyle.vue",
        true,
        "class/style reach every component through AllowedComponentProps",
    ),
    (
        "AcceptClassStyleOnNoInherit.vue",
        true,
        "class/style are merged, never consumed by fallthrough — they stay \
         accepted under `inheritAttrs: false`, which proves they do NOT come \
         from the fallthrough widening",
    ),
    (
        "AcceptClassStyleOnMultiRoot.vue",
        true,
        "class/style stay accepted on a fragment child for the same reason",
    ),
    (
        "AcceptDeclaredCollidingProp.vue",
        true,
        "ChildCollidingProp declares `title: number` while <div> types `title?: \
         string`. The widening must exclude declared keys structurally, or the \
         intersection collapses to `never` and this fix becomes a NEW false \
         positive on every prop named after an HTML attribute",
    ),
    (
        "AcceptCollidingPropThroughComponentRoot.vue",
        true,
        "WrapsCollidingChild's root is <ChildOptionalColliding/>, which DECLARES \
         `title: number` while <div> types `title?: string`. Vue consumes the \
         forwarded `title` AT THE CHILD — it never reaches the DOM — so the two \
         channels are alternatives, not simultaneous constraints. Intersecting \
         them makes `title` `string & number` = `never`, and no value satisfies \
         a prop Vue forwards perfectly at runtime",
    ),
    (
        "RejectCollidingPropWrongTypeThroughComponentRoot.vue",
        false,
        "the overlay is right-biased, not permissive: the child's declared \
         `number` WINS over <div>'s `title?: string`, so a string is an error. \
         Without this row the collision fix could be a blanket `unknown`",
    ),
    (
        "AcceptNumberTitleOnMixedConditionalRoot.vue",
        true,
        "MixedConditionalRoot renders <ChildOptionalColliding v-if> / <div \
         v-else>. `title` is `number` on the component branch and `string` on \
         the element branch; exactly one branch renders, so the surface is the \
         exact UNION of the two. Intersecting collapses `title` to `never` and \
         rejects BOTH spellings",
    ),
    (
        "AcceptStringTitleOnMixedConditionalRoot.vue",
        true,
        "the other side of the same union — the <div> branch's own `title?: \
         string`. Paired with the row above, this is what an intersection \
         cannot satisfy",
    ),
    (
        "RejectUnknownOnMixedConditionalRoot.vue",
        false,
        "`notARealThing` is a member of NEITHER branch. The union must stay a \
         union of real props types; widening it to an open surface would make \
         the two rows above pass for the wrong reason",
    ),
    (
        "AcceptAnchorAttrOnOptionsApiChild.vue",
        true,
        "OptionsApiAnchorRoot is an Options-API `<script>` component with an \
         `<a>` root. Its carrier is parent-facing exactly like a `<script \
         setup>` one, and `generate_options_api_stub` returned BEFORE the \
         projection was applied — so `href` was rejected on every Options-API \
         and `defineComponent` component in the workspace. The row also passes \
         a DECLARED prop, so the widening must not have replaced the \
         component's own construct signature",
    ),
    (
        "RejectUnknownAttrOnOptionsApiChild.vue",
        false,
        "the Options-API carrier's widened surface is still the element's real \
         props type — never an index signature",
    ),
    (
        "RejectDeclaredPropWrongTypeOnOptionsApiChild.vue",
        false,
        "the Options-API component's own `declaredProp: number` keeps its type \
         through the widening; a stub that swapped the declared surface for a \
         fallthrough-only one would accept this",
    ),
    (
        "AcceptAnchorAttrOnJsOptionsApiChild.vue",
        true,
        "JsOptionsApiAnchorRoot is the JAVASCRIPT (`<script>` with no `lang`) \
         twin of OptionsApiAnchorRoot. Vue forwards `href` onto its `<a>` root \
         identically in both dialects, so the parent-facing surface must not \
         depend on the child's script language. Its `.vue.js` stub cannot hold \
         TypeScript-only syntax (TS8006/8008/8009/8010 on generated code), so \
         the widening is spelled in JSDoc there — this row is exactly the \
         issue-#97 residue where the JS child rejected an attribute its \
         byte-identical TS twin accepted",
    ),
    (
        "RejectUnknownAttrOnJsOptionsApiChild.vue",
        false,
        "the JSDoc-widened surface is still the `<a>` element's real props type \
         — never an index signature and never the `any` a broken JSDoc cast \
         chain would leak",
    ),
    (
        "RejectDeclaredPropWrongTypeOnJsOptionsApiChild.vue",
        false,
        "the JavaScript Options-API component's own `declaredProp: number` keeps \
         its type through the JSDoc widening; the `/** @type {any} */` cast is a \
         conduit inside the tail, not the published surface",
    ),
    (
        "AcceptAnchorAttrOnJsSetupChild.vue",
        true,
        "a JavaScript `<script setup>` child projects a generated TypeScript \
         declaration surface, so its widening never needed the JSDoc form — \
         this row pins the third dialect cell so a regression cannot hide \
         behind the Options-API-only fix",
    ),
    (
        "RejectUnknownAttrOnJsSetupChild.vue",
        false,
        "and that surface stays the element's real props type",
    ),
    (
        "AcceptAnchorAttrOnAliasImportedChild.vue",
        true,
        "the child is imported through the tsconfig `@/*` alias. IDE codegen \
         deliberately keeps that specifier bare, so unless the generated \
         validation TSX canonicalizes it the carrier resolves through the \
         ambient `*.vue` wildcard shim — an EMPTY `DefineComponent<{}, {}, \
         any>` — and the widening is simply absent for every alias-importing \
         consumer",
    ),
    (
        "RejectUnknownAttrOnAliasImportedChild.vue",
        false,
        "and resolving the alias must reach the child's REAL surface, not an \
         open one: a name the <a> root does not accept is still an error",
    ),
    (
        "AcceptAnchorAttrOnScriptlessChild.vue",
        true,
        "a scriptless SFC is parent-facing exactly like any other carrier; its \
         `<a>` root accepts `href` and its generation path (`generate_empty_stub`) \
         dropped the projection entirely",
    ),
    (
        "RejectUnknownAttrOnScriptlessChild.vue",
        false,
        "the scriptless carrier's widened surface is still the element's real \
         props type — never an index signature",
    ),
    (
        "AcceptAnchorOnlyAttr.vue",
        true,
        "`href` is an <a>-only attribute and ChildAnchorRoot's root IS an <a>, \
         so the widened surface is that element's real props type — not a \
         generic HTMLAttributes stand-in",
    ),
    (
        "RejectUnknownAttr.vue",
        false,
        "`notARealThing` is not a member of the <div> props type, so it must \
         still be rejected — the widened arm is the element's real, \
         member-typed props type and NEVER an index signature",
    ),
    (
        "RejectOnNoInherit.vue",
        false,
        "`inheritAttrs: false` ⇒ no inherited surface at all; `title` reaches \
         nothing",
    ),
    (
        "RejectOnMultiRoot.vue",
        false,
        "a fragment has no single root to inherit into; Vue warns at runtime",
    ),
    (
        "RejectUnknownOnTypedChild.vue",
        false,
        "`bogus-attr` is neither declared by ChildTyped nor a <div> attribute",
    ),
    (
        "RejectDeclaredPropWrongType.vue",
        false,
        "the declared `realProp: number` must keep its own type — the widening \
         must not relax declared-prop checking",
    ),
    (
        "RejectAnchorOnlyAttrOnDivChild.vue",
        false,
        "`href` is an <a> attribute; ChildDiv's root is a <div>, which does not \
         accept it — proves the widening is keyed on the RESOLVED root element, \
         not a generic HTML surface",
    ),
];

/// DISCRIMINATING both ways. Against `main` the four `Accept*` fallthrough
/// files (global attr, native listener, component-root chain, anchor-only
/// attr) FAIL — that is issue #97. After the fix they pass while every
/// `Reject*` file keeps erroring.
#[test]
fn fallthrough_attrs_accepted_only_where_they_reach_the_dom() {
    let (temp_dir, temp_path) = setup_temp_project();

    let bin = env!("CARGO_BIN_EXE_verter-tsc");
    let output = Command::new(bin)
        .arg("--noEmit")
        .arg("-p")
        .arg(temp_path.join("tsconfig.json"))
        .output()
        .expect("failed to execute verter-tsc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let failing = error_files(&stdout);

    // The CHILD's exit status is evidence, not decoration. `verter-tsc` exits
    // 0 when the project checks clean, 1 when it reported type errors, and 2
    // when the engine itself failed (absent tsgo, connect / init /
    // updateSnapshot / protocol / project-not-found, or an unreadable config);
    // a signal death leaves no code at all. This fixture ALWAYS has errors (the
    // Reject* half), so 1 is the ONLY status that means "the engine ran and
    // checked". Without this, expected-looking diagnostics followed by an
    // engine crash satisfy the matrix below.
    assert_eq!(
        output.status.code(),
        Some(1),
        "verter-tsc must exit 1 (type errors reported) — 0 means it found nothing \
         to complain about in a fixture whose Reject* half must always error, 2 \
         means the engine itself failed, and no code at all means it was killed. \
         Any of those makes the matrix below meaningless.\n\
         === STDERR ===\n{stderr}\n=== STDOUT ===\n{stdout}"
    );

    // FAIL, never skip. `setup_temp_project` already proved a >= 7 TypeScript
    // is installed, so the engine was available — and this fixture ALWAYS has
    // failing files when the engine runs (the Reject* half). An empty
    // diagnostic set therefore means the run did not check, which must not be
    // reported as a pass.
    assert!(
        !failing.is_empty(),
        "verter-tsc reported no diagnostics at all, but this fixture's Reject* half \
         must always produce errors when the engine runs — the typecheck did not \
         happen, so a green result here would prove nothing.\n\
         === STDERR ===\n{stderr}\n=== STDOUT ===\n{stdout}"
    );

    let mut wrong: Vec<String> = Vec::new();
    for (file, must_pass, why) in MATRIX {
        let errored = failing.iter().any(|f| f == file);
        if *must_pass && errored {
            wrong.push(format!("{file}: expected NO error — {why}"));
        } else if !*must_pass && !errored {
            wrong.push(format!(
                "{file}: expected an error and got none (a FALSE NEGATIVE, the worse \
                 half of this trade) — {why}"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "attribute-fallthrough prop checking is wrong for {} of {} fixtures:\n  {}\n\
         --- files verter-tsc reported errors in ---\n{:#?}\n--- STDOUT ---\n{stdout}",
        wrong.len(),
        MATRIX.len(),
        wrong.join("\n  "),
        failing
    );

    // The child components themselves must type-check cleanly — a widened
    // stub that broke its own component would show up here.
    for child in [
        "ChildDiv.vue",
        "ChildTyped.vue",
        "ChildNoInherit.vue",
        "ChildMultiRoot.vue",
        "ChildWrapsComp.vue",
        "Grandchild.vue",
        "ChildCollidingProp.vue",
        "ChildAnchorRoot.vue",
        "WrapsTypedChild.vue",
        "LeafNoInheritWithProp.vue",
        "WrapsNoInheritLeaf.vue",
        "ConditionalRoot.vue",
        "ChildOptionalColliding.vue",
        "WrapsCollidingChild.vue",
        "MixedConditionalRoot.vue",
        "OptionsApiAnchorRoot.vue",
        "ScriptlessAnchorRoot.vue",
        "AliasChildAnchorRoot.vue",
        "JsOptionsApiAnchorRoot.vue",
        "JsSetupAnchorRoot.vue",
    ] {
        assert!(
            !failing.iter().any(|f| f == child),
            "{child} must type-check cleanly; got errors.\n--- STDOUT ---\n{stdout}"
        );
    }

    // ZERO TS8xxx anywhere in the run. The `.vue.js` stub of a JavaScript
    // Options-API child copies the authored body verbatim AND carries its
    // fallthrough widening — that widening must be spelled in JSDoc, because
    // TypeScript-only syntax in a `.js`/`.jsx` carrier reports TS8006 ("'types'
    // can only be used in TypeScript files") / TS8008 / TS8009 / TS8010 on
    // Verter's OWN generated lines, whether or not `checkJs` is on. This is the
    // exact regression the old dialect gate existed to prevent; the gate is
    // gone, so this assertion holds the line instead.
    let ts8xxx: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains("error TS8"))
        .collect();
    assert!(
        ts8xxx.is_empty(),
        "the fixture run must report ZERO TS8xxx syntax diagnostics on generated \
         carriers — TypeScript-only syntax leaked into a JavaScript stub:\n{}",
        ts8xxx.join("\n")
    );

    drop(temp_dir);
}
