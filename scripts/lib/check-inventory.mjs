// Mechanical check-inventory extraction shared by
// scripts/validate-mutation-suite.test.mjs.
//
// A "check" is one violation-emitting call site — `v(`...`)` in
// scripts/validate-program-state.mjs and scripts/validate-stack-window.mjs,
// or `push(`...`)` / `v.push(`...`)` / `problems.push(`...`)` in
// scripts/lib/stack-window-lib.mjs (the shared model both validators
// consume). The inventory is DERIVED FROM SOURCE, every run, by scanning for
// those call sites — it is deliberately NOT a hand-maintained list of check
// names. The mutation suite that consumes this module asserts every derived
// check was tripped by at least one mutation; a new `v(...)`/`push(...)`
// call site added with no mutation proven against it makes that assertion
// fail. Do not "simplify" this into a static array — that reproduces
// exactly the rot this module exists to prevent (see
// MAINTAINER-DIRECTIVE-HARDEN-ORCHESTRATION.md: "systematise it").
//
// Deliberately OUT OF SCOPE: the two `return { ok: false, problems: [...] }`
// literals in stack-window-lib.mjs's evaluateCheckpointException (unreadable
// / unparseable stack-window file) are usage/IO failures of the same shape
// as validate-program-state.mjs's own `usageFail`/`loadFile` paths — not
// "checks" over well-formed input in the sense every other entry here is.
// They are still exercised by tests in the mutation suite; they are just not
// counted by (or required to satisfy) this coverage mechanism.

import { readFileSync } from "node:fs";

// Excludes bare `.push(` on any OTHER receiver (`reasons.push`, `dagIds.push`,
// `nested.push`, `paragraph.push`, `resolvedRoots.push`, ...) via the
// negative lookbehind — those build ordinary arrays, they don't emit
// violations.
const CALLEE_RE = /(?<![.\w])(?:v\(|problems\.push\(|v\.push\(|push\()\s*(`(?:\\.|[^`\\])*`)/g;

// Turn a source-code template literal (still containing `${...}`
// placeholders and backslash escapes, UNEVALUATED — this is a textual scan
// of the .mjs source, not a require of it) into a RegExp that matches the
// rendered runtime message: every `${...}` becomes a non-greedy wildcard
// (depth-counted, so a nested `{`/`}` inside the interpolation expression
// doesn't truncate it early); every other character is regex-escaped.
export function templateToRegex(literal) {
  const inner = literal.slice(1, -1); // strip the surrounding backticks
  let pattern = "";
  let i = 0;
  while (i < inner.length) {
    const ch = inner[i];
    if (ch === "\\") {
      pattern += inner[i] + (inner[i + 1] ?? "");
      i += 2;
      continue;
    }
    if (ch === "$" && inner[i + 1] === "{") {
      let depth = 1;
      let j = i + 2;
      while (j < inner.length && depth > 0) {
        if (inner[j] === "{") depth++;
        else if (inner[j] === "}") depth--;
        j++;
      }
      pattern += "[\\s\\S]*?";
      i = j;
      continue;
    }
    pattern += ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    i++;
  }
  return new RegExp(pattern);
}

export function deriveChecks(file) {
  const text = readFileSync(file, "utf8");
  const out = [];
  let m;
  CALLEE_RE.lastIndex = 0;
  while ((m = CALLEE_RE.exec(text))) {
    const literal = m[1];
    const line = text.slice(0, m.index).split("\n").length;
    out.push({ file, line, literal, regex: templateToRegex(literal), covered: false });
  }
  return out;
}

// A registry over one or more files' derived checks, with lookup by anchor
// (a short, exact substring copied straight from the STATIC part of a
// check's own source message) — a mutation test names the check it
// exercises by the check's real current text, never by a hand-assigned id
// or a line number that silently drifts under unrelated edits above it.
export class CheckRegistry {
  constructor(files) {
    this.checks = files.flatMap((f) => deriveChecks(f));
    // "if your enumeration silently finds zero checks, that must FAIL, not
    // pass" — a broken extraction regex (or a validator rewritten to emit
    // violations some other way) must be a loud construction-time failure,
    // not a suite that trivially "passes" having proven nothing.
    if (this.checks.length === 0) {
      throw new Error(
        `CheckRegistry derived ZERO checks from [${files.join(", ")}] — the extraction regex found no v(...)/push(...) violation call sites; this is a broken inventory, not a clean validator, and must fail loudly`,
      );
    }
  }

  // Resolve exactly one check by (file, anchor substring). Throws if the
  // anchor is absent or ambiguous — never silently guesses.
  find(file, anchor) {
    const matches = this.checks.filter((c) => c.file === file && c.literal.includes(anchor));
    if (matches.length === 0) {
      throw new Error(`no derived check in ${file} contains anchor ${JSON.stringify(anchor)}`);
    }
    if (matches.length > 1) {
      throw new Error(
        `anchor ${JSON.stringify(anchor)} in ${file} matches ${matches.length} derived checks — ambiguous, use a more specific anchor`,
      );
    }
    return matches[0];
  }

  markCovered(check) {
    check.covered = true;
  }

  uncovered() {
    return this.checks.filter((c) => !c.covered);
  }
}
