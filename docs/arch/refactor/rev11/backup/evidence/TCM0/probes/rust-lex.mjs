// Shared Rust lexing and module-graph helpers for the tree derivations in this directory.
//
// Extracted so `typeprovider-call-site-derivation.mjs` and `capability-provider-hop-walk.mjs` share ONE
// implementation of the parts that are easy to get subtly wrong: telling a doc comment from code,
// telling a char literal from a lifetime, brace-matching with the enclosing block header retained, and
// resolving `#[cfg(test)] mod NAME;` FILE-module declarations to the files they gate. Two copies of that
// logic would drift, and a drift here silently reclassifies evidence.

import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

export const K_CODE = 0;
export const K_COMMENT = 1;
export const K_DOC = 2;
export const K_STR = 3;

export const IDENT = /[A-Za-z0-9_]/;

/** Lex a Rust source into per-character region kinds. */
export function lex(src) {
  const n = src.length;
  const kind = new Uint8Array(n);
  let i = 0;
  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      const doc = src[i + 2] === "/" || src[i + 2] === "!";
      let j = i;
      while (j < n && src[j] !== "\n") j++;
      kind.fill(doc ? K_DOC : K_COMMENT, i, j);
      i = j;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const doc = src[i + 2] === "*" || src[i + 2] === "!";
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (src[j] === "/" && src[j + 1] === "*") {
          depth++;
          j += 2;
        } else if (src[j] === "*" && src[j + 1] === "/") {
          depth--;
          j += 2;
        } else j++;
      }
      kind.fill(doc ? K_DOC : K_COMMENT, i, j);
      i = j;
      continue;
    }
    if ((c === "r" || c === "b") && !(i > 0 && IDENT.test(src[i - 1]))) {
      let h = i + 1;
      if (src[h] === "r" && c === "b") h++;
      let hashes = 0;
      while (src[h] === "#") {
        hashes++;
        h++;
      }
      if (src[h] === '"' && (hashes > 0 || h > i + 1 || c === "r")) {
        const term = '"' + "#".repeat(hashes);
        const end = src.indexOf(term, h + 1);
        const j = end === -1 ? n : end + term.length;
        kind.fill(K_STR, i, j);
        i = j;
        continue;
      }
    }
    if (c === '"') {
      let j = i + 1;
      while (j < n) {
        if (src[j] === "\\") j += 2;
        else if (src[j] === '"') {
          j++;
          break;
        } else j++;
      }
      kind.fill(K_STR, i, j);
      i = j;
      continue;
    }
    if (c === "'") {
      // char literal (`'x'`, `'\n'`, `'\u{1}'`) versus a lifetime (`'a`, `'static`).
      if (src[i + 1] === "\\") {
        let j = i + 2;
        while (j < n && src[j] !== "'" && src[j] !== "\n") j++;
        if (src[j] === "'") j++;
        kind.fill(K_STR, i, j);
        i = j;
        continue;
      }
      if (src[i + 2] === "'") {
        kind.fill(K_STR, i, i + 3);
        i += 3;
        continue;
      }
      i++;
      continue;
    }
    i++;
  }
  return kind;
}

/** Brace-depth events over code regions, each carrying the header text that opened the block. */
export function blockEvents(src, kind) {
  const events = [];
  let pending = "";
  for (let i = 0; i < src.length; i++) {
    if (kind[i] !== K_CODE) continue;
    const c = src[i];
    if (c === "{") {
      events.push({ open: true, pos: i, header: pending.trim().replace(/\s+/g, " ").slice(-320) });
      pending = "";
    } else if (c === "}") {
      events.push({ open: false, pos: i, header: "" });
      pending = "";
    } else if (c === ";") {
      pending = "";
    } else {
      pending += c;
      if (pending.length > 900) pending = pending.slice(-500);
    }
  }
  return events;
}

export function lineIndex(src) {
  const starts = [0];
  for (let i = 0; i < src.length; i++) if (src[i] === "\n") starts.push(i + 1);
  return starts;
}

export function lineOf(starts, pos) {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= pos) lo = mid;
    else hi = mid - 1;
  }
  return lo + 1;
}

/**
 * Derive the `TypeProvider` method list from the trait body itself. Never hand-listed: a method added
 * to the trait appears on the next run of either derivation with no edit to any script.
 */
export function deriveTraitMethods(traitFile, repo) {
  const src = readFileSync(traitFile, "utf8");
  const kind = lex(src);
  const decl = /pub trait TypeProvider\b/.exec(src);
  if (!decl) throw new Error("`pub trait TypeProvider` not found in " + traitFile);
  let open = src.indexOf("{", decl.index);
  while (open !== -1 && kind[open] !== K_CODE) open = src.indexOf("{", open + 1);
  if (open === -1) throw new Error("trait body brace not found");
  let depth = 0;
  let end = -1;
  for (let i = open; i < src.length; i++) {
    if (kind[i] !== K_CODE) continue;
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end === -1) throw new Error("unterminated trait body");
  const events = blockEvents(src, kind);
  const starts = lineIndex(src);
  const methods = [];
  const re = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    if (m.index < open || m.index > end) continue;
    if (kind[m.index] !== K_CODE) continue;
    // Directly in the trait body: exactly one enclosing brace (the trait's own).
    let d = 0;
    for (const ev of events) {
      if (ev.pos >= m.index) break;
      d += ev.open ? 1 : -1;
    }
    if (d !== 1) continue;
    methods.push({ name: m[1], line: lineOf(starts, m.index) });
  }
  return { methods, traitSpan: [open, end], file: relative(repo, traitFile).split("\\").join("/") };
}

export function isTestPath(rel) {
  const p = rel.split("\\").join("/");
  return (
    /(^|\/)tests\//.test(p) ||
    /(^|\/)benches\//.test(p) ||
    /_tests?\.rs$/.test(p) ||
    /(^|\/)test_(utils|support|helpers)[^/]*\.rs$/.test(p)
  );
}

export const TEST_ATTR =
  /#\s*\[\s*(cfg\s*\(\s*test\s*\)|cfg\s*\(\s*any\s*\([^)]*\btest\b|test\b|tokio::test|rstest\b|cfg_attr\s*\(\s*test)/;

/**
 * Resolve `#[cfg(test)] mod NAME;` FILE-module declarations to the files they gate, transitively.
 *
 * Without this the split is wrong in the direction that matters: `crates/verter_lsp/src/lib.rs`
 * carries `#[cfg(test)] mod real_provider_tests;`, so a whole directory of test code lives under
 * `src/` with no `#[cfg(test)]` inside its own files. Reading only in-file attributes would count
 * hundreds of test calls as production callers.
 */
export function resolveGatedTestModules(files, relOf) {
  const byRel = new Map(files.map((f) => [relOf(f), f]));
  const gated = new Set();
  const declsOf = new Map();

  const modDir = (rel) => {
    const parts = rel.split("/");
    const base = parts.pop();
    if (base === "mod.rs" || base === "lib.rs" || base === "main.rs") return parts.join("/");
    return parts.concat(base.replace(/\.rs$/, "")).join("/");
  };
  const resolveMod = (rel, name, pathAttr) => {
    const dir = modDir(rel);
    const cands = pathAttr
      ? [(dir ? dir + "/" : "") + pathAttr, rel.split("/").slice(0, -1).concat(pathAttr).join("/")]
      : [(dir ? dir + "/" : "") + name + ".rs", (dir ? dir + "/" : "") + name + "/mod.rs"];
    for (const c of cands) if (byRel.has(c)) return c;
    return undefined;
  };

  for (const abs of files) {
    const rel = relOf(abs);
    const src = readFileSync(abs, "utf8");
    const kind = lex(src);
    let code = "";
    for (let i = 0; i < src.length; i++) code += kind[i] === K_CODE ? src[i] : " ";
    const out = [];
    const re = /\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/g;
    let m;
    while ((m = re.exec(code)) !== null) {
      const back = code.slice(Math.max(0, m.index - 400), m.index);
      const run = back.slice(Math.max(back.lastIndexOf(";"), back.lastIndexOf("}")) + 1);
      const pathAttr = (/#\s*\[\s*path\s*=\s*"([^"]+)"/.exec(run) || [])[1];
      const target = resolveMod(rel, m[1], pathAttr);
      if (!target) continue;
      const isGated = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/.test(run);
      out.push(target);
      if (isGated) gated.add(target);
    }
    declsOf.set(rel, out);
  }
  // Transitive: everything a gated module declares is gated too.
  let grew = true;
  while (grew) {
    grew = false;
    for (const g of [...gated]) {
      for (const child of declsOf.get(g) || []) {
        if (!gated.has(child)) {
          gated.add(child);
          grew = true;
        }
      }
    }
  }
  return gated;
}

export function walkRs(dir, out) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      if (e.name === "target" || e.name === "node_modules" || e.name === ".git") continue;
      walkRs(p, out);
    } else if (e.isFile() && e.name.endsWith(".rs")) out.push(p);
  }
  return out;
}
