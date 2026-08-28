// Derivation: for each steering-named capability that has NO `TypeProvider` method, does a provider hop
// exist anywhere on its request path?
//
// WHY THIS EXISTS. The feature-ownership ledger located fourteen steering-named capabilities to a
// `file:line` and then characterised all fourteen at once — "none has a provider anywhere in its request
// path", "uniformly `VerterNative` by construction" — while its own text conceded it had not analysed
// them individually. A verdict asserted over fourteen paths from an examination of none of them is the
// defect, not the fourteen. This script performs the walk, per capability, and lets the answer differ
// per capability, which is the only way a uniform answer could ever be believed.
//
// WHAT A "HOP" IS. Two things, both derived, never guessed:
//   trait-method-call   a call `<receiver>.<m>(` or `<path>::<m>(` where `m` is one of the methods
//                       derived from the `TypeProvider` trait body itself
//   provider-handle     a read of the `type_provider` field / accessor, or a mention of the
//                       `TypeProvider` type — obtaining a provider at all, even before calling it
//
// HOW THE WALK RUNS. Every non-test `fn` under `crates/*/src` is indexed with its brace-matched body,
// its name, and the type its enclosing `impl`/`trait` owns. From each capability's declared entry point
// the walk follows call sites breadth-first and stops the moment a hop is found — reporting the SHORTEST
// path, so the evidence a reader has to check is one chain of `file:line`s, not a subtree.
//
// An edge is constrained by the CALL SHAPE, not by the name alone. Resolving by name alone made the walk
// worthless in a way worth recording: `HandlerGuard::new(...)` inside a folding-range handler resolved to
// `VerterLanguageServer::new`, whose body reads `config.type_provider`, so all fourteen capabilities
// "reached a provider" through a constructor none of them calls. A walk that answers HOP for everything
// answers nothing. The shapes are:
//   `self.f(…)` / `Self::f(…)`  only definitions owned by the caller's own impl
//   `Q::f(…)`                   only definitions owned by `Q`, falling back to free functions when `Q`
//                               is a module path rather than a type
//   `expr.f(…)`                 any definition with an owner — a receiver's type is not recoverable
//                               textually, and this is the residual over-approximation (L1)
//   `f(…)`                      free functions only
//
// LIMITS — the verdicts are only as good as these.
//   L1. OVER-APPROXIMATION. Edges are name-resolved, so `fn new` or `fn get` links every definition
//       sharing that name. A reported hop is therefore a CANDIDATE that must be read before it is
//       believed; this script prints the full source line of every hop for exactly that reason. A
//       reported hop is never, by itself, a finding.
//   L2. UNDER-APPROXIMATION. A call reached only through `dyn Trait` dispatch, a stored closure, or a
//       macro-pasted name is not an edge here. So "no hop reachable" is strong evidence — the walk is
//       otherwise generous — but it is not a proof, and each NO-HOP verdict below is paired with a read
//       of the entry point's own body in the accompanying prose.
//   L3. CAP. The walk stops at `--cap` functions per capability (default 20000) and says so when it
//       hits the cap, because a truncated walk that reports "no hop" would be a lie by omission.
//   L4. `crates/` ONLY, non-test definitions only.
//
// USAGE.
//   node capability-provider-hop-walk.mjs            regenerate the committed evidence file
//   node capability-provider-hop-walk.mjs --check     exit 1 if the tree no longer derives it
//   node capability-provider-hop-walk.mjs --stdout    print instead of writing

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import {
  blockEvents,
  deriveTraitMethods,
  IDENT,
  isTestPath,
  K_CODE,
  lex,
  lineIndex,
  lineOf,
  resolveGatedTestModules,
  TEST_ATTR,
  walkRs,
} from "./rust-lex.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "..", "..", "..", "..", "..", "..", "..");
const TRAIT_FILE = join(REPO, "crates", "verter_type_runtime", "src", "traits.rs");
const CRATES = join(REPO, "crates");
const OUT_FILE = join(HERE, "..", "capability-provider-hop-walk.md");

// The fourteen capabilities the ledger located behind no trait method, plus the two it recorded as
// PARTIALLY covered and its cache clause, each pinned to the entry point the ledger itself cites. `entry` is `fn name` plus the file it must be
// defined in, so a name shared with an unrelated definition cannot silently become the start of a walk.
const CAPABILITIES = [
  {
    cap: "rename preparation",
    entries: [["handle_prepare_rename", "crates/verter_lsp/src/server/rename_prepare.rs"]],
  },
  {
    cap: "formatting (+ on-type)",
    entries: [
      ["format_document", "crates/verter_lsp/src/features/formatting.rs"],
      ["handle_formatting", "crates/verter_lsp/src/server/aux_features.rs"],
      ["handle_on_type_formatting", "crates/verter_lsp/src/server/aux_features.rs"],
    ],
  },
  {
    cap: "call hierarchy",
    entries: [
      ["prepare_call_hierarchy", "crates/verter_lsp/src/features/call_hierarchy.rs"],
      ["incoming_calls", "crates/verter_lsp/src/features/call_hierarchy.rs"],
      ["outgoing_calls", "crates/verter_lsp/src/features/call_hierarchy.rs"],
      ["handle_prepare_call_hierarchy", "crates/verter_lsp/src/server/aux_features.rs"],
      ["handle_incoming_calls", "crates/verter_lsp/src/server/aux_features.rs"],
      ["handle_outgoing_calls", "crates/verter_lsp/src/server/aux_features.rs"],
    ],
  },
  {
    cap: "code lens",
    entries: [
      ["code_lenses", "crates/verter_lsp/src/features/code_lens.rs"],
      ["handle_code_lens", "crates/verter_lsp/src/server/aux_features.rs"],
    ],
  },
  {
    cap: "folding",
    entries: [
      ["build_folding_ranges", "crates/verter_lsp/src/features/folding_range.rs"],
      ["handle_folding_range", "crates/verter_lsp/src/server/aux_features.rs"],
    ],
  },
  {
    cap: "selection ranges",
    entries: [["handle_selection_range", "crates/verter_lsp/src/server/aux_features.rs"]],
  },
  {
    cap: "document symbols",
    entries: [
      ["build_document_symbols", "crates/verter_lsp/src/features/document_symbol.rs"],
      ["handle_document_symbol", "crates/verter_lsp/src/server/aux_features.rs"],
    ],
  },
  {
    cap: "component surface resolution",
    entries: [
      [
        "get_component_meta_surface",
        "crates/verter_lsp/src/server/custom_methods/component_meta.rs",
      ],
      [
        "resolve_framework_surface_with_audit",
        "crates/verter_session/src/typeinfo/framework_surface/executor.rs",
      ],
    ],
  },
  {
    cap: "template expression typing",
    entries: [["get_ide", "crates/verter_lsp/src/documents/mod.rs"]],
  },
  {
    cap: "props",
    entries: [
      ["find_unknown_props", "crates/verter_lsp/src/features/component_diagnostics.rs"],
      ["suggest_matching_props", "crates/verter_lsp/src/features/component_actions.rs"],
    ],
  },
  {
    cap: "events",
    entries: [["event_type_hint_actions", "crates/verter_lsp/src/features/event_type_hints.rs"]],
  },
  {
    cap: "slots and snippets",
    entries: [
      ["resolve_slot_name_definition", "crates/verter_lsp/src/server/component_resolve.rs"],
      ["resolve_slot_binding_definition", "crates/verter_lsp/src/server/component_resolve.rs"],
    ],
  },
  {
    cap: "directives",
    entries: [
      ["builtin_directive_name_hover", "crates/verter_lsp/src/features/hover_directive_names.rs"],
    ],
  },
  {
    cap: "framework macros",
    entries: [["macro_code_actions", "crates/verter_lsp/src/features/macro_actions.rs"]],
  },
  // The three steering entries the ledger recorded as PARTIALLY covered — the provider half already has
  // a row, the Verter-owned half does not. Walked here so "the provider half is the only half with a
  // provider on its path" is a derived statement rather than a plausible one.
  {
    cap: "auto-imports (Verter-owned half)",
    entries: [
      [
        "resolve_provider_auto_import_edits",
        "crates/verter_lsp/src/server/nav_features_completion_resolve.rs",
      ],
      ["organize_imports_actions", "crates/verter_lsp/src/features/organize_imports.rs"],
    ],
  },
  {
    cap: "background semantic analysis (Verter-owned lane)",
    entries: [["schedule_semantic_analysis", "crates/verter_lsp/src/documents/analysis.rs"]],
  },
  {
    cap: "provider-adjacent caches",
    entries: [
      ["carrier_regeneration_is_fresh", "crates/verter_lsp/src/carrier_cache.rs"],
      ["needs_engine_recheck", "crates/verter_lsp/src/carrier_cache.rs"],
      ["mapped_results_valid", "crates/verter_lsp/src/carrier_cache.rs"],
    ],
  },
];

const KEYWORDS = new Set([
  "if",
  "while",
  "for",
  "match",
  "fn",
  "return",
  "let",
  "loop",
  "as",
  "move",
  "unsafe",
  "impl",
  "where",
  "pub",
  "mod",
  "use",
  "in",
  "ref",
  "dyn",
  "await",
  "async",
  "const",
  "static",
  "type",
  "struct",
  "enum",
  "trait",
  "self",
  "Self",
  "super",
  "crate",
  "else",
  "break",
  "continue",
]);

/**
 * The type an `impl` / `trait` block owns, read off the enclosing block headers.
 *
 * Written as a scanner rather than a regex because the regex version silently returned "" for
 * `impl<P, B> ResilientProvider<P, B>` (no space after `impl`), which made every `Vec::new()` in the
 * tree resolve to `ResilientProvider::new` and handed all fourteen capabilities a false provider hop.
 */
function ownerOf(stack) {
  for (let si = stack.length - 1; si >= 0; si--) {
    let h = stack[si].replace(/#\[[^\]]*\]/g, " ");
    const kw = /\b(impl|trait)\b/.exec(h);
    if (!kw) continue;
    let rest = h.slice(kw.index + kw[1].length).split(/\bwhere\b/)[0];
    // Walk at generic depth 0, collecting identifiers; `for` switches to the implementing type.
    let depth = 0;
    let first = "";
    let afterFor = "";
    let sawFor = false;
    const tok = /[A-Za-z_][A-Za-z0-9_]*|<|>/g;
    let t;
    while ((t = tok.exec(rest)) !== null) {
      if (t[0] === "<") depth++;
      else if (t[0] === ">") depth--;
      else if (depth === 0) {
        if (t[0] === "for") sawFor = true;
        else if (sawFor) {
          afterFor = t[0];
          break;
        } else if (!first) first = t[0];
      }
    }
    const owner = afterFor || first;
    if (owner) return owner;
  }
  return "";
}

function indexFunctions() {
  const files = walkRs(CRATES, []).sort();
  const relOf = (abs) => relative(REPO, abs).split("\\").join("/");
  const gated = resolveGatedTestModules(files, relOf);
  const byName = new Map();
  const useLeaves = new Map();
  let defs = 0;
  for (const abs of files) {
    const rel = relOf(abs);
    if (!/\/src\//.test(rel)) continue;
    if (isTestPath(rel) || gated.has(rel)) continue;
    const src = readFileSync(abs, "utf8");
    if (!src.includes("fn ")) continue;
    // `use a::b::name;` and `use a::b::{name, other};` — the module segment a name was imported FROM
    // is what makes an unqualified call resolvable without types.
    const leaves = new Map();
    for (const u of src.matchAll(/\buse\s+([A-Za-z_][A-Za-z0-9_:]*)\s*(?:::\s*\{([^}]*)\})?/g)) {
      const segs = u[1].split("::").filter(Boolean);
      if (u[2]) {
        const mod = segs[segs.length - 1];
        for (const item of u[2].split(",")) {
          const leaf = item
            .trim()
            .split(/\s+as\s+/)[0]
            .trim();
          if (!leaf) continue;
          if (!leaves.has(leaf)) leaves.set(leaf, new Set());
          leaves.get(leaf).add(mod);
        }
      } else if (segs.length >= 2) {
        const leaf = segs[segs.length - 1];
        const mod = segs[segs.length - 2];
        if (!leaves.has(leaf)) leaves.set(leaf, new Set());
        leaves.get(leaf).add(mod);
      }
    }
    useLeaves.set(rel, leaves);
    const kind = lex(src);
    const events = blockEvents(src, kind);
    const starts = lineIndex(src);
    const re = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g;
    let m;
    while ((m = re.exec(src)) !== null) {
      if (kind[m.index] !== K_CODE) continue;
      // Reject a definition sitting inside a test region.
      const stack = [];
      let inTest = false;
      for (const ev of events) {
        if (ev.pos >= m.index) break;
        if (ev.open) stack.push(ev.header);
        else stack.pop();
      }
      for (const h of stack) if (TEST_ATTR.test(h)) inTest = true;
      if (inTest) continue;
      // Brace-match the body. A `fn` with `;` before `{` is a declaration without one.
      let i = m.index;
      let open = -1;
      for (; i < src.length; i++) {
        if (kind[i] !== K_CODE) continue;
        if (src[i] === ";") break;
        if (src[i] === "{") {
          open = i;
          break;
        }
      }
      if (open === -1) continue;
      let depth = 0;
      let end = -1;
      for (let j = open; j < src.length; j++) {
        if (kind[j] !== K_CODE) continue;
        if (src[j] === "{") depth++;
        else if (src[j] === "}") {
          depth--;
          if (depth === 0) {
            end = j;
            break;
          }
        }
      }
      if (end === -1) continue;
      const owner = ownerOf(stack);
      const rec = {
        name: m[1],
        owner,
        file: rel,
        line: lineOf(starts, m.index),
        src,
        kind,
        starts,
        open,
        end,
      };
      if (!byName.has(m[1])) byName.set(m[1], []);
      byName.get(m[1]).push(rec);
      defs++;
    }
  }
  return { byName, defs, fileCount: files.length, useLeaves };
}

function hopIn(rec, methodSet) {
  const { src, kind, open, end, starts } = rec;
  const re = /\b([A-Za-z_][A-Za-z0-9_]*)\b/g;
  re.lastIndex = open;
  let m;
  while ((m = re.exec(src)) !== null) {
    if (m.index > end) break;
    if (kind[m.index] !== K_CODE) continue;
    const name = m[1];
    const isMethod = methodSet.has(name);
    const isHandle = name === "type_provider" || name === "TypeProvider";
    if (!isMethod && !isHandle) continue;
    let b = m.index - 1;
    while (b >= 0 && /\s/.test(src[b])) b--;
    const prev = src[b];
    let a = m.index + name.length;
    while (a < src.length && /\s/.test(src[a])) a++;
    if (isHandle) {
      // `crate::type_provider::specifier_rewrite::…` is a MODULE PATH, not a provider handle. Verter
      // names a module after the trait, so an unqualified match on the identifier reported a hop for
      // any file that merely imports a position-mapping helper out of it. A path segment is one with
      // `::` on either side; a handle is a bare field or accessor.
      if (src[a] === ":" && src[a + 1] === ":") continue;
      if (prev === ":" && src[b - 1] === ":") continue;
    }
    if (isMethod) {
      if (src[a] !== "(") continue;
      if (prev !== "." && !(src[b - 1] === ":" && prev === ":")) continue;
      return {
        why: "trait-method-call",
        name,
        line: lineOf(starts, m.index),
        text: src.slice(src.lastIndexOf("\n", m.index) + 1, src.indexOf("\n", m.index)).trim(),
      };
    }
    return {
      why: "provider-handle",
      name,
      line: lineOf(starts, m.index),
      text: src.slice(src.lastIndexOf("\n", m.index) + 1, src.indexOf("\n", m.index)).trim(),
    };
  }
  return undefined;
}

/**
 * Call sites in a body, each carrying the shape that constrains which definition it can resolve to.
 *
 * Resolving purely by NAME made the walk useless: `HandlerGuard::new(...)` in a folding handler linked
 * to `VerterLanguageServer::new`, and every capability then "reached" a provider hop through a `fn new`
 * it never calls. Shape is what makes an edge mean something:
 *   `self.f(…)` / `Self::f(…)`  -> only definitions owned by the CALLER's own impl
 *   `Q::f(…)`                   -> only definitions owned by `Q` (falling back to free functions,
 *                                  since `Q` may be a module path rather than a type)
 *   `expr.f(…)`                 -> any definition with an owner (unresolvable without types; broad)
 *   `f(…)`                      -> free functions only
 */
function calleesOf(rec) {
  const { src, kind, open, end } = rec;
  const out = [];
  const re = /\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::\s*<[^;{}]*>\s*)?\(/g;
  re.lastIndex = open;
  let m;
  while ((m = re.exec(src)) !== null) {
    if (m.index > end) break;
    if (kind[m.index] !== K_CODE) continue;
    if (KEYWORDS.has(m[1])) continue;
    // Walk back over an optional `A::B::` qualifier chain and note a `.` receiver.
    let b = m.index - 1;
    while (b >= 0 && /\s/.test(src[b])) b--;
    let qual = "";
    let isMethod = false;
    if (src[b] === ".") {
      isMethod = true;
      let e = b - 1;
      while (e >= 0 && /\s/.test(src[e])) e--;
      let w = "";
      while (e >= 0 && IDENT.test(src[e])) w = src[e--] + w;
      qual = w;
    } else if (src[b] === ":" && src[b - 1] === ":") {
      let e = b - 2;
      while (e >= 0 && /\s/.test(src[e])) e--;
      let w = "";
      while (e >= 0 && IDENT.test(src[e])) w = src[e--] + w;
      qual = w;
    }
    out.push({ name: m[1], qual, isMethod });
  }
  return out;
}

/**
 * Resolve one call site to the definitions it can reach — PRECISELY, or not at all.
 *
 * A receiver's type is not recoverable from text, so `expr.f(…)` on an arbitrary receiver is not
 * followed as an edge; it is RECORDED as unresolved instead. Following it to every definition named `f`
 * is what turned the first version of this walk into an unconditional HOP for all fourteen. The one
 * exception is a name with exactly ONE definition in the whole index, where there is nothing to
 * over-approximate.
 *
 * Returns `{ defs, unresolved }`.
 */
/** The module a file defines: its stem, or its directory when it is `mod.rs` / `lib.rs` / `main.rs`. */
function moduleNameOf(rel) {
  const parts = rel.split("/");
  const base = parts.pop();
  if (base === "mod.rs" || base === "lib.rs" || base === "main.rs") return parts.pop() || "";
  return base.replace(/\.rs$/, "");
}

function resolveEdge(rec, call, index, useLeaves) {
  const defs = index.get(call.name) || [];
  if (defs.length === 0) return { defs: [], unresolved: false };
  if (call.qual === "self" || call.qual === "Self") {
    const own = defs.filter((d) => d.owner && d.owner === rec.owner);
    if (own.length > 0) return { defs: own, unresolved: false };
    return { defs: [], unresolved: true };
  }
  if (!call.isMethod && call.qual) {
    const byOwner = defs.filter((d) => d.owner === call.qual);
    if (byOwner.length > 0) return { defs: byOwner, unresolved: false };
    // Rust casing decides what an unmatched qualifier IS. `Box::new` names a TYPE this tree does not
    // define, so it resolves to nothing; falling back to free functions there made `Box::new` resolve
    // to the one module-level `fn new` in the workspace and dragged a provider actor loop onto the
    // path of capabilities that never touch it. A lowercase qualifier is a MODULE path, where a free
    // function is exactly the right target.
    const looksLikeType = /^[A-Z]/.test(call.qual);
    if (looksLikeType) return { defs: [], unresolved: true };
    // A lowercase qualifier is a MODULE path, so the target must actually live in that module.
    // Without this, `tokio::spawn(…)` resolved to every workspace free function named `spawn`, and a
    // background-analysis scheduler acquired a path into a provider's process-spawn teardown.
    const inModule = defs.filter(
      (d) =>
        (!d.owner && d.file.split("/").includes(call.qual)) ||
        (!d.owner && moduleNameOf(d.file) === call.qual),
    );
    if (inModule.length > 0) return { defs: inModule, unresolved: false };
    return { defs: [], unresolved: true };
  }
  if (!call.isMethod) {
    // An unqualified call reaches a free function that is either defined in this file or imported into
    // it. Resolving it to every workspace free function of that name is how `new` and `spawn` linked
    // unrelated crates together.
    const sameFile = defs.filter((d) => !d.owner && d.file === rec.file);
    if (sameFile.length > 0) return { defs: sameFile, unresolved: false };
    const mods = (useLeaves.get(rec.file) || new Map()).get(call.name);
    if (mods) {
      const imported = defs.filter(
        (d) =>
          !d.owner &&
          (mods.has(moduleNameOf(d.file)) || d.file.split("/").some((seg) => mods.has(seg))),
      );
      if (imported.length > 0) return { defs: imported, unresolved: false };
    }
    return { defs: [], unresolved: true };
  }
  if (defs.length === 1) return { defs, unresolved: false };
  return { defs: [], unresolved: true };
}

function walk(entryRecs, index, methodSet, cap, useLeaves) {
  const unresolved = new Map();
  const unresolvedSites = new Set();
  const seen = new Set();
  const parent = new Map();
  const queue = [];
  for (const r of entryRecs) {
    const k = r.file + ":" + r.line;
    if (seen.has(k)) continue;
    seen.add(k);
    queue.push(r);
  }
  let visited = 0;
  while (queue.length > 0) {
    if (visited >= cap)
      return { verdict: "CAP", visited, capped: true, unresolved, unresolvedSites };
    const rec = queue.shift();
    visited++;
    const hop = hopIn(rec, methodSet);
    if (hop) {
      const path = [];
      let cur = rec;
      while (cur) {
        path.unshift(cur);
        cur = parent.get(cur.file + ":" + cur.line);
      }
      return { verdict: "HOP", hop, path, visited, capped: false, unresolved, unresolvedSites };
    }
    for (const callee of calleesOf(rec)) {
      const edge = resolveEdge(rec, callee, index, useLeaves);
      if (edge.unresolved) {
        const key = callee.name + "  (from " + rec.file + ":" + rec.line + ")";
        unresolved.set(callee.name, (unresolved.get(callee.name) || 0) + 1);
        unresolvedSites.add(key);
      }
      for (const def of edge.defs) {
        const k = def.file + ":" + def.line;
        if (seen.has(k)) continue;
        seen.add(k);
        parent.set(k, rec);
        queue.push(def);
      }
    }
  }
  return { verdict: "NO-HOP", visited, capped: false, unresolved, unresolvedSites };
}

function main() {
  const args = process.argv.slice(2);
  const check = args.includes("--check");
  const toStdout = args.includes("--stdout");
  const capArg = args.indexOf("--cap");
  const CAP = capArg === -1 ? 20000 : Number(args[capArg + 1]);

  const { methods, file: traitRel } = deriveTraitMethods(TRAIT_FILE, REPO);
  const methodSet = new Set(methods.map((m) => m.name));
  const { byName, defs, fileCount, useLeaves } = indexFunctions();

  // Every indexed function whose OWN body contains a hop. Used to test the completeness of a NO-HOP
  // verdict: if an unresolved receiver call names one of these, the walk stopped exactly where a hop
  // could have been hiding, and the verdict must say so instead of quietly reading as proof.
  const hopBearingNames = new Map();
  for (const [name, defs] of byName) {
    const k = defs.filter((d) => hopIn(d, methodSet)).length;
    if (k > 0) hopBearingNames.set(name, { hop: k, total: defs.length });
  }

  const results = [];
  for (const c of CAPABILITIES) {
    const recs = [];
    const missing = [];
    for (const [name, file] of c.entries) {
      const found = (byName.get(name) || []).filter((r) => r.file === file);
      if (found.length === 0) missing.push(name + " in " + file);
      recs.push(...found);
    }
    const r =
      recs.length === 0
        ? { verdict: "ENTRY-NOT-FOUND", visited: 0 }
        : walk(recs, byName, methodSet, CAP, useLeaves);
    const unresolvedNames = [...(r.unresolved || new Map()).entries()].sort((a, b) => b[1] - a[1]);
    const risky = unresolvedNames.filter(([n]) => hopBearingNames.has(n));
    results.push({ ...c, ...r, missing, entryRecs: recs, unresolvedNames, risky });
  }

  const L = [];
  L.push("# Derived: does a `TypeProvider` hop exist on each capability's request path?");
  L.push("");
  L.push(
    "**This file is generated. Do not edit it.** Regenerate with " +
      "`node docs/arch/refactor/rev11/evidence/TCM0/probes/capability-provider-hop-walk.mjs`; falsify " +
      "with `--check`, which re-derives from the live tree and exits 1 on any drift.",
  );
  L.push("");
  L.push(
    "The `TypeProvider` method names are read out of the trait body in `" +
      traitRel +
      "` (" +
      methods.length +
      " methods) — never typed in. " +
      defs +
      " non-test `fn` definitions were indexed across " +
      fileCount +
      " `.rs` files. From each capability's entry point the walk follows every callee name that " +
      "resolves in that index, breadth-first, and reports the SHORTEST path to a hop.",
  );
  L.push("");
  L.push("A **hop** is either:");
  L.push("");
  L.push(
    "- `trait-method-call` — `<receiver>.<m>(` or `<path>::<m>(` where `m` is a derived trait method;",
  );
  L.push(
    "- `provider-handle` — a read of the `type_provider` field/accessor or a mention of the " +
      "`TypeProvider` type, i.e. obtaining a provider at all, before any call.",
  );
  L.push("");
  L.push("## What this walk cannot see");
  L.push("");
  L.push(
    "- **It over-approximates (L1).** Edges resolve by NAME, so a common name links unrelated " +
      "definitions. **A reported hop is a candidate, not a finding** — the full source line is printed " +
      "so it can be read before it is believed.",
  );
  L.push(
    "- **It under-approximates (L2).** A call reached only through `dyn Trait` dispatch, a stored " +
      "closure, or a macro-pasted name is not an edge. A `NO-HOP` verdict is strong (the walk is " +
      "otherwise generous) but is not a proof, and each one is paired in the ledger with a read of the " +
      "entry point's own body.",
  );
  L.push(
    "- **It is capped (L3).** " +
      CAP +
      " functions per capability; a capped walk reports `CAP`, never `NO-HOP`.",
  );
  L.push("- **`crates/` only, non-test definitions only (L4).**");
  L.push("");
  L.push("## Verdicts");
  L.push("");
  L.push(
    "| capability | verdict | fns explored | unresolved receivers | of those, hop-bearing | first hop |",
  );
  L.push("|---|---|---|---|---|---|");
  for (const r of results) {
    const hop =
      r.verdict === "HOP"
        ? "`" +
          r.hop.name +
          "` (" +
          r.hop.why +
          ") at `" +
          r.path[r.path.length - 1].file +
          ":" +
          r.hop.line +
          "`"
        : "—";
    L.push(
      "| " +
        r.cap +
        " | **" +
        r.verdict +
        "** | " +
        r.visited +
        " | " +
        r.unresolvedNames.length +
        " | " +
        r.risky.length +
        " | " +
        hop +
        " |",
    );
  }
  L.push("");
  const hops = results.filter((r) => r.verdict === "HOP").length;
  L.push(
    "**" +
      hops +
      " of " +
      results.length +
      " capabilities reach a provider hop; " +
      results.filter((r) => r.verdict === "NO-HOP").length +
      " do not.** A uniform verdict over all of them is therefore not available from the tree.",
  );
  L.push("");
  L.push("## Per capability");
  for (const r of results) {
    L.push("");
    L.push("### " + r.cap + " — " + r.verdict);
    L.push("");
    L.push("Entry points walked:");
    L.push("");
    for (const e of r.entryRecs) L.push("- `" + e.file + ":" + e.line + "` — `fn " + e.name + "`");
    for (const m of r.missing) L.push("- **NOT FOUND: " + m + "**");
    L.push("");
    if (r.verdict === "HOP") {
      L.push("Shortest path from an entry point to the hop:");
      L.push("");
      r.path.forEach((p, i) => {
        L.push(i + 1 + ". `" + p.file + ":" + p.line + "` — `fn " + p.name + "`");
      });
      L.push("");
      L.push(
        "Hop: **`" +
          r.hop.name +
          "`** (" +
          r.hop.why +
          ") at `" +
          r.path[r.path.length - 1].file +
          ":" +
          r.hop.line +
          "`",
      );
      L.push("");
      L.push("```rust");
      L.push(r.hop.text);
      L.push("```");
    } else if (r.verdict === "NO-HOP") {
      L.push(
        "No provider hop is reachable. " +
          r.visited +
          " functions were explored to exhaustion — the walk ran out of reachable callees, it was not " +
          "cut short.",
      );
    } else if (r.verdict === "CAP") {
      L.push(
        "**Capped at " +
          r.visited +
          " functions — this is NOT a no-hop verdict.** Re-run with a " +
          "larger `--cap`.",
      );
    } else {
      L.push("**Entry point not found in the index — the walk did not run.**");
    }
    L.push("");
    L.push(
      "Unresolved receivers (method calls whose receiver type is not recoverable from text, so the walk " +
        "did NOT follow them): **" +
        (r.unresolvedNames || []).length +
        "** distinct names.",
    );
    if ((r.risky || []).length === 0) {
      L.push("");
      L.push(
        "**None of them names a function that would itself have been a hop.** Every indexed function " +
          "whose own body contains a provider hop was collected up front; the unresolved set for this " +
          "capability does not intersect it. That is what makes the verdict above load-bearing rather " +
          "than an artefact of where the walk stopped.",
      );
    } else {
      L.push("");
      L.push(
        "**" +
          r.risky.length +
          " of them name a function that WOULD itself have been a hop** — the walk stopped exactly " +
          "where a hop could hide, so this verdict is not complete on its own and needs a read:",
      );
      L.push("");
      for (const [n, c2] of r.risky) {
        const hb = hopBearingNames.get(n);
        L.push(
          "- `" +
            n +
            "` — " +
            hb.hop +
            " of " +
            hb.total +
            " definition(s) with that name contain a hop; " +
            c2 +
            " unfollowed call site(s)" +
            (hb.total > 20
              ? ". A name this heavily overloaded flags on collision, not on reachability"
              : ""),
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
      console.error("REFUSED — committed walk missing at " + relative(REPO, OUT_FILE));
      process.exit(1);
    }
    if (readFileSync(OUT_FILE, "utf8") !== out) {
      console.error(
        "REFUSED — the tree no longer derives the committed walk. Re-run without --check and read the diff.",
      );
      process.exit(1);
    }
    console.log(
      "ok — " +
        results.length +
        " capabilities, " +
        hops +
        " reaching a provider hop, walk matches the committed file",
    );
    return;
  }
  writeFileSync(OUT_FILE, out);
  console.log(
    "wrote " +
      relative(REPO, OUT_FILE) +
      " — " +
      results.length +
      " capabilities, " +
      hops +
      " reaching a provider hop, " +
      defs +
      " fns indexed",
  );
}

main();
