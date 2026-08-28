// Derivation: own-spelling textual occurrences of every `TypeProvider` method under `crates/`.
//
// WHY THIS EXISTS. The feature-ownership ledger's "Current production callers" column is expressly
// labelled "representative" — a curated sample. A sample is silent about exactly the call site it
// omits, and the charter obligation is over EVERY call site, so the completeness claim cannot rest on
// a hand-maintained cell. This script derives the universe instead: the method list comes out of the
// trait body itself, and the occurrence set comes out of a lexer over the tree. The counts below are a
// build output, not an assertion.
//
// WHAT IT DOES.
//   1. Parses `crates/verter_type_runtime/src/traits.rs`, finds `pub trait TypeProvider`, brace-matches
//      its body, and reads every `fn` / `async fn` declared directly in it. The method list is NEVER
//      typed out here; a method added to the trait appears in the next run with no edit to this file.
//   2. Lexes every `.rs` file under `crates/` into code / doc-comment / plain-comment / string regions
//      (line comments, nested block comments, raw strings `r#"…"#`, byte strings, char literals versus
//      lifetimes) so a mention inside a doc comment is never counted as a call.
//   3. Tracks the enclosing block stack by brace depth, keeping the header text that opened each block
//      (attributes included, since `#[cfg(test)] mod tests` accumulates into one header). That yields
//      the enclosing `trait` / `impl` / `fn`, and whether the occurrence is inside a test region.
//   4. Resolves `#[cfg(test)] mod NAME;` FILE-module declarations to the files they gate, transitively.
//      This is load-bearing and not a nicety: `crates/verter_lsp/src/lib.rs` gates
//      `real_provider_tests`, `test_harness`, `integration_tests` and others that way, so those files
//      contain no `#[cfg(test)]` of their own. Reading in-file attributes alone would have promoted
//      hundreds of test calls into the production column — an error in the exact direction this
//      derivation exists to prevent.
//   5. Classifies every word-boundary occurrence of a trait method name.
//
// CLASSES EMITTED.
//   trait-declaration      the declaration in the `TypeProvider` trait body itself
//   impl-production        a `fn <name>` definition outside a test region (a provider impl, a wrapper)
//   impl-test              a `fn <name>` definition inside a test region (a mock provider)
//   call-production        a call in non-test code whose enclosing fn is NOT the same method
//   call-forwarding        a call in non-test code from inside a `fn <same name>` body — a delegating
//                          wrapper forwarding to an inner provider, not an independent consumer
//   call-trait-default     a call inside the trait's own default method bodies (e.g. the priority
//                          variants defaulting to their base method)
//   call-test              a call inside `#[cfg(test)]` / `#[test]` / a `tests/` or `benches/` file, a
//                          `*_tests.rs` sibling, or a file reached through a `#[cfg(test)] mod X;`
//                          FILE-module declaration, resolved transitively
//   ref-production         a code mention not followed by `(` (a fn item, a macro argument, a path)
//   ref-test               the same, inside a test region
//   doc-comment            `///` `//!` `/** */` `/*! */`
//   comment                a plain `//` or `/* */` comment
//   string-literal         inside a string or char literal
//
// LIMITS — READ THESE BEFORE READING THE COUNTS. This is a textual derivation and it cannot see:
//   L1. NAME COLLISIONS. `shutdown`, `child_pid`, `provider_id`, `close_file` are ordinary English
//       identifiers. A `.shutdown()` on a scheduler is indistinguishable, textually, from a
//       `.shutdown()` on a provider. Every occurrence therefore carries its enclosing-item header and a
//       receiver snippet so a reader can adjudicate. Counts can over-count collisions and under-count
//       renamed or macro-synthesised calls, so they are not a guaranteed upper or lower bound.
//   L2. GENERIC PARAMETERS. A call through `fn f<P: TypeProvider>(p: &P) { p.get_hover(..) }` is found
//       (the method is named), but this script cannot prove the receiver's bound is `TypeProvider`.
//       Same adjudication rule as L1.
//   L3. OWN-SPELLING DYNAMIC DISPATCH IS COVERED; RE-EXPORTS WITHOUT METHOD RENAMING ARE. `dyn
//       TypeProvider` reaches a method BY NAME, so those own-spelling textual calls appear. A re-export
//       of the trait (`verter_lsp/src/type_provider/traits.rs`
//       re-exports it) does not rename methods, so it is covered too. What is NOT covered is a call
//       reached under a DIFFERENT NAME: a renamed re-export (`use ... as other_name`), a method invoked
//       through a blanket-impl adapter that renames it, or a name synthesised by a macro. This script
//       does not determine whether such routes exist; it reports the trait's own-spelling occurrences.
//   L4. MACRO-GENERATED BODIES. A call emitted by `macro_rules!` expansion whose text does not contain
//       the method name (e.g. built by `concat_idents!` or paste-style token pasting) is invisible.
//   L5. `crates/` ONLY. TypeScript-side callers (`packages/`) are out of the trait's language and are
//       not scanned; the trait has no callers there.
//
// USAGE.
//   node typeprovider-call-site-derivation.mjs            regenerate the committed evidence file
//   node typeprovider-call-site-derivation.mjs --check     exit 1 if the tree no longer derives it
//   node typeprovider-call-site-derivation.mjs --stdout    print instead of writing
//
// `--check` is the falsifying command: it re-derives from the live tree and diffs against the
// committed output, so a new call site, a deleted one, or a new trait method turns the check red.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import {
  blockEvents,
  deriveTraitMethods,
  isTestPath,
  IDENT,
  K_CODE,
  K_DOC,
  K_COMMENT,
  K_STR,
  lex,
  lineIndex,
  lineOf,
  TEST_ATTR,
  resolveGatedTestModules,
  walkRs,
} from "./rust-lex.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..", "..", "..", "..", "..", "..");
const TRAIT_FILE = join(REPO, "crates", "verter_type_runtime", "src", "traits.rs");
const CRATES = join(REPO, "crates");
const OUT_FILE = join(HERE, "..", "typeprovider-call-sites.md");

function receiverSnippet(src, pos) {
  let s = pos;
  const lineStart = src.lastIndexOf("\n", pos - 1) + 1;
  s = Math.max(lineStart, pos - 48);
  return src.slice(s, pos).replace(/\s+/g, " ").trim();
}

function main() {
  const args = process.argv.slice(2);
  const check = args.includes("--check");
  const toStdout = args.includes("--stdout");

  const { methods, traitSpan, file: traitRel } = deriveTraitMethods(TRAIT_FILE, REPO);
  const names = methods.map((m) => m.name);
  const byLength = [...new Set(names)].sort((a, b) => b.length - a.length);
  const combined = new RegExp("\\b(" + byLength.join("|") + ")\\b", "g");

  const files = walkRs(CRATES, []).sort();
  const relOf = (abs) => relative(REPO, abs).split("\\").join("/");
  const gatedTestModules = resolveGatedTestModules(files, relOf);
  const occurrences = [];

  for (const abs of files) {
    const src = readFileSync(abs, "utf8");
    combined.lastIndex = 0;
    if (!combined.test(src)) continue;
    const rel = relOf(abs);
    const kind = lex(src);
    const events = blockEvents(src, kind);
    const starts = lineIndex(src);
    const pathIsTest = isTestPath(rel) || gatedTestModules.has(rel);

    const hits = [];
    combined.lastIndex = 0;
    let m;
    while ((m = combined.exec(src)) !== null) hits.push({ name: m[1], pos: m.index });
    if (hits.length === 0) continue;

    // One replay of the block events, advancing through the sorted hits.
    const stack = [];
    let ei = 0;
    for (const hit of hits) {
      while (ei < events.length && events[ei].pos < hit.pos) {
        const ev = events[ei++];
        if (ev.open) stack.push(ev.header);
        else stack.pop();
      }
      const frames = stack.slice();
      const inTestRegion = pathIsTest || frames.some((h) => TEST_ATTR.test(h));
      const k = kind[hit.pos];
      let cls;
      let enclosing = "";
      for (let i = frames.length - 1; i >= 0; i--) {
        if (/\bfn\s+[A-Za-z_]/.test(frames[i]) || /\b(impl|trait)\b/.test(frames[i])) {
          enclosing = frames[i];
          break;
        }
      }
      const implFrame = frames.filter((h) => /\b(impl|trait)\b/.test(h)).slice(-1)[0] || "";

      if (k === K_DOC) cls = "doc-comment";
      else if (k === K_COMMENT) cls = "comment";
      else if (k === K_STR) cls = "string-literal";
      else {
        // Preceding non-space token.
        let b = hit.pos - 1;
        while (b >= 0 && /\s/.test(src[b])) b--;
        let word = "";
        let w = b;
        while (w >= 0 && IDENT.test(src[w])) {
          word = src[w] + word;
          w--;
        }
        let a = hit.pos + hit.name.length;
        while (a < src.length && /\s/.test(src[a])) a++;
        const nextChar = src[a];
        const isDef = word === "fn";
        const isCall = nextChar === "(" || nextChar === ":" || nextChar === "<";
        if (isDef) {
          const inTraitDecl = rel === traitRel && hit.pos > traitSpan[0] && hit.pos < traitSpan[1];
          cls = inTraitDecl ? "trait-declaration" : inTestRegion ? "impl-test" : "impl-production";
        } else if (nextChar === "(") {
          const inTraitBody = rel === traitRel && hit.pos > traitSpan[0] && hit.pos < traitSpan[1];
          if (inTraitBody) cls = "call-trait-default";
          else if (inTestRegion) cls = "call-test";
          else {
            const enclosingFn = frames
              .slice()
              .reverse()
              .find((h) => /\bfn\s+[A-Za-z_]/.test(h));
            const fnName = enclosingFn
              ? (/\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/.exec(enclosingFn) || [])[1]
              : undefined;
            cls = fnName === hit.name ? "call-forwarding" : "call-production";
          }
        } else if (isCall) {
          cls = inTestRegion ? "ref-test" : "ref-production";
        } else {
          cls = inTestRegion ? "ref-test" : "ref-production";
        }
      }
      occurrences.push({
        name: hit.name,
        file: rel,
        line: lineOf(starts, hit.pos),
        cls,
        context: (enclosing || implFrame).slice(-110),
        snippet: receiverSnippet(src, hit.pos),
      });
    }
  }

  const CLASSES = [
    "trait-declaration",
    "impl-production",
    "impl-test",
    "call-production",
    "call-forwarding",
    "call-trait-default",
    "call-test",
    "ref-production",
    "ref-test",
    "doc-comment",
    "comment",
    "string-literal",
  ];

  const per = new Map();
  for (const n of names) per.set(n, Object.fromEntries(CLASSES.map((c) => [c, 0])));
  for (const o of occurrences) per.get(o.name)[o.cls]++;

  const totals = Object.fromEntries(CLASSES.map((c) => [c, 0]));
  for (const o of occurrences) totals[o.cls]++;

  const L = [];
  L.push("# Derived: every `TypeProvider` call site in `crates/`");
  L.push("");
  L.push(
    "**This file is generated. Do not edit it.** Regenerate with " +
      "`node docs/arch/refactor/rev11/evidence/TCM0/probes/typeprovider-call-site-derivation.mjs`; " +
      "falsify with the same command and `--check`, which re-derives from the live tree and exits 1 on " +
      "any drift.",
  );
  L.push("");
  L.push(
    "The method list is read out of the `TypeProvider` trait body in " +
      "`" +
      traitRel +
      "` — it is never typed into the generator. The occurrence set is a lex " +
      "over every `.rs` file under `crates/`. Both counts below are build outputs.",
  );
  L.push("");
  L.push("## What this derivation cannot see");
  L.push("");
  L.push(
    "Read these before reading the counts; the generator's own header carries the same list as L1-L5.",
  );
  L.push("");
  L.push(
    "- **Name collisions (L1).** `shutdown`, `child_pid`, `close_file` and friends are ordinary " +
      "identifiers. A `.shutdown()` on something that is not a provider is textually identical to one " +
      "that is. Every row carries its enclosing item and a receiver snippet so a reader can adjudicate. " +
      "**The counts can include textual collisions and omit renamed or macro-synthesised calls, so " +
      "they are not a guaranteed upper or lower bound.**",
  );
  L.push(
    "- **Generic parameters (L2).** A call through `fn f<P: TypeProvider>(p: &P)` is found by name but " +
      "the bound is not proven here.",
  );
  L.push(
    "- **Dynamic dispatch is covered; a rename would not be (L3).** `dyn TypeProvider` reaches a method " +
      "by its own spelling, so every `dyn` call site appears, and the `verter_lsp` re-export of the " +
      "trait renames nothing. A call reached under a DIFFERENT name — a renamed `use ... as`, a " +
      "renaming adapter, a macro-synthesised identifier — would be invisible. None exists today; this " +
      "derivation cannot prove that, only that no occurrence of the trait's own spelling was missed.",
  );
  L.push(
    "- **Macro-pasted identifiers (L4).** A name built by token pasting carries no matchable text.",
  );
  L.push(
    "- **`crates/` only (L5).** The TypeScript packages are out of the trait's language and are not " +
      "scanned.",
  );
  L.push("");
  L.push("## Universe");
  L.push("");
  L.push("- trait methods derived from the trait body: **" + methods.length + "**");
  L.push("- `.rs` files under `crates/` walked: **" + files.length + "**");
  L.push("- classified occurrences: **" + occurrences.length + "**");
  L.push("");
  L.push("| class | count |");
  L.push("|---|---|");
  for (const c of CLASSES) L.push("| `" + c + "` | " + totals[c] + " |");
  L.push("");
  L.push("## Per-method counts");
  L.push("");
  L.push(
    "`call-forwarding` is a call from inside a `fn` of the same name — a delegating wrapper passing the " +
      "call down, not an independent consumer. It is separated because a method whose only non-test " +
      "callers are forwarders has no live consumer at all.",
  );
  L.push("");
  L.push(
    "| # | method | trait decl | impl prod | impl test | call prod | call fwd | call dflt | call test | ref prod | ref test | doc | comment | string |",
  );
  L.push("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
  methods.forEach((mm, i) => {
    const c = per.get(mm.name);
    L.push(
      "| " + (i + 1) + " | `" + mm.name + "` | " + CLASSES.map((k) => c[k]).join(" | ") + " |",
    );
  });
  L.push("");
  L.push("## Every occurrence");
  L.push("");
  L.push(
    "Grouped by method, then by class, then by path. `context` is the enclosing `fn`/`impl`/`trait` " +
      "header; `snippet` is the source text immediately preceding the occurrence on its line.",
  );
  for (const mm of methods) {
    const mine = occurrences.filter((o) => o.name === mm.name);
    L.push("");
    L.push("### `" + mm.name + "` — declared at `" + traitRel + ":" + mm.line + "`");
    L.push("");
    if (mine.length === 0) {
      L.push("_no occurrence anywhere in `crates/` beyond the declaration itself_");
      continue;
    }
    for (const c of CLASSES) {
      const rows = mine
        .filter((o) => o.cls === c)
        .sort((a, b) => (a.file === b.file ? a.line - b.line : a.file < b.file ? -1 : 1));
      if (rows.length === 0) continue;
      L.push("");
      L.push("**`" + c + "`** (" + rows.length + ")");
      L.push("");
      L.push("| site | context | snippet |");
      L.push("|---|---|---|");
      for (const r of rows) {
        L.push(
          "| `" +
            r.file +
            ":" +
            r.line +
            "` | `" +
            r.context.split("|").join("\\|") +
            "` | `" +
            r.snippet.split("|").join("\\|").split("`").join("'") +
            "` |",
        );
      }
    }
  }
  L.push("");
  const out = L.join("\n") + "\n";

  if (toStdout) {
    process.stdout.write(out);
    return;
  }
  if (check) {
    if (!existsSync(OUT_FILE)) {
      console.error("REFUSED — committed derivation missing at " + relative(REPO, OUT_FILE));
      process.exit(1);
    }
    const have = readFileSync(OUT_FILE, "utf8");
    if (have !== out) {
      console.error(
        "REFUSED — the tree no longer derives the committed output. Re-run without --check and read the diff.",
      );
      process.exit(1);
    }
    console.log(
      "ok — " +
        methods.length +
        " trait methods, " +
        occurrences.length +
        " occurrences, derivation matches the committed file",
    );
    return;
  }
  writeFileSync(OUT_FILE, out);
  console.log(
    "wrote " +
      relative(REPO, OUT_FILE) +
      " — " +
      methods.length +
      " methods, " +
      files.length +
      " files, " +
      occurrences.length +
      " occurrences",
  );
}

main();
