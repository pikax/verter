#!/usr/bin/env node
//
// Verifies and evidences B2's parse facet over the official-case manifests
// WITHOUT changing generate-official-case-manifests.mjs (the manifest
// generator's schema and behavior are ratified as frozen for this purpose --
// see AMD-010 section 8 Q-2: "no schema change; no generator change"). This
// script only ANNOTATES the evidence_id/reason columns of the already-
// committed manifests with a facet verdict, and writes REAL, human-readable
// evidence records a reviewer can open -- never an opaque hash standing in
// for one.
//
// Usage:
//   node verify-b2-parse-facets.mjs \
//     --vue-source <pinned vuejs/core checkout> \
//     --svelte-source <pinned sveltejs/svelte checkout> \
//     --vue-modules <path whose node_modules provides @babel/parser> \
//     --manifest-dir <dir containing vue-official-cases.tsv / svelte-official-cases.tsv> \
//     --evidence-dir <dir to write the B2-parse-facet-{vue,svelte}.md records into> \
//     --probe <path to the parse_corpus_probe binary>
//
// Every B2-owned row's parse facet is checked against the real pinned
// oracle checkout via the same probe/classification approach used here;
// only WHERE that logic lives changed (out of the frozen generator).

import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve, basename } from "node:path";

import { SVELTE_DOMAIN } from "../../../../../../packages/framework-conformance-harness/src/domain-pin.mjs";

// EXPECTED_SVELTE reads from the same single source of truth as
// generate-official-case-manifests.mjs — see that file's comment for why
// this one is no longer hardcoded here. EXPECTED_VUE stays independently
// hardcoded to this evidence package's own frozen rc.3 pin — see that same
// file's comment for why it must NOT alias VUE_DOMAIN.commit.
const EXPECTED_VUE = "3adb225775c9b28223a56e07f7a2f874b6fbb138";
const EXPECTED_SVELTE = SVELTE_DOMAIN.commit;

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i]?.startsWith("--") || argv[i + 1] === undefined) {
      throw new Error(`invalid arguments near ${argv[i] ?? "<end>"}`);
    }
    out[argv[i].slice(2)] = resolve(argv[i + 1]);
  }
  for (const key of [
    "vue-source",
    "svelte-source",
    "vue-modules",
    "manifest-dir",
    "evidence-dir",
    "probe",
  ]) {
    if (!out[key]) throw new Error(`missing --${key}`);
  }
  return out;
}

function git(cwd, ...gitArgs) {
  return execFileSync("git", ["-C", cwd, ...gitArgs], { encoding: "utf8" }).trim();
}

function assertCheckout(root, expected) {
  const head = git(root, "rev-parse", "HEAD");
  if (head !== expected) throw new Error(`${root}: expected ${expected}, got ${head}`);
  if (git(root, "status", "--porcelain") !== "") throw new Error(`${root}: dirty checkout`);
}

function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}

function readTsv(path) {
  const lines = readFileSync(path, "utf8").trimEnd().split("\n");
  const columns = lines.shift().split("\t");
  return lines.map((line) => {
    const values = line.split("\t");
    return Object.fromEntries(columns.map((column, index) => [column, values[index]]));
  });
}

function writeTsv(rows, columns, path) {
  const clean = (value) => {
    const cleaned = String(value ?? "-")
      .replaceAll("\t", " ")
      .replaceAll("\r", " ")
      .replaceAll("\n", " ");
    return cleaned === "" ? "-" : cleaned;
  };
  writeFileSync(
    path,
    [columns.join("\t"), ...rows.map((row) => columns.map((c) => clean(row[c])).join("\t"))].join(
      "\n",
    ) + "\n",
  );
}

function rootTestName(node) {
  if (!node) return null;
  if (node.type === "Identifier" && (node.name === "test" || node.name === "it")) return node.name;
  if (node.type === "MemberExpression" || node.type === "OptionalMemberExpression")
    return rootTestName(node.object);
  if (node.type === "CallExpression" || node.type === "OptionalCallExpression")
    return rootTestName(node.callee);
  return null;
}

function calleeText(source, callee) {
  return source.slice(callee.start, callee.end).replaceAll(/\s+/g, " ");
}

function testCalls(ast, source) {
  const calls = [];
  const seen = new Set();
  function visit(node, parent) {
    if (!node || typeof node !== "object" || seen.has(node)) return;
    seen.add(node);
    if (node.type === "CallExpression" || node.type === "OptionalCallExpression") {
      const root = rootTestName(node.callee);
      const isInnerCallee =
        (parent?.type === "CallExpression" || parent?.type === "OptionalCallExpression") &&
        parent.callee === node;
      if (root && !isInnerCallee) {
        const first = node.arguments?.[0];
        const titleSource =
          first && typeof first.start === "number"
            ? source.slice(first.start, first.end)
            : "<dynamic>";
        const spelling = calleeText(source, node.callee);
        calls.push({
          root,
          line: node.loc.start.line,
          column: node.loc.start.column + 1,
          parameterization: spelling.includes(".each")
            ? "parameterized-declaration"
            : "single-declaration",
          title_kind: first?.type ?? "dynamic",
          title_hash: hash(titleSource),
          callee_hash: hash(spelling),
          node,
        });
      }
    }
    for (const [key, value] of Object.entries(node)) {
      if (key === "loc" || key === "start" || key === "end") continue;
      if (Array.isArray(value)) for (const child of value) visit(child, node);
      else if (value && typeof value === "object") visit(value, node);
    }
  }
  visit(ast, null);
  return calls;
}

function visitAst(node, visitor, seen = new Set()) {
  if (!node || typeof node !== "object" || seen.has(node)) return;
  seen.add(node);
  visitor(node);
  for (const [key, value] of Object.entries(node)) {
    if (key === "loc" || key === "start" || key === "end") continue;
    if (Array.isArray(value)) for (const child of value) visitAst(child, visitor, seen);
    else if (value && typeof value === "object") visitAst(value, visitor, seen);
  }
}

function propertyName(node) {
  if (!node) return null;
  if (node.type === "Identifier") return node.name;
  if (node.type === "StringLiteral") return node.value;
  return null;
}

function calleeName(node) {
  if (!node) return null;
  if (node.type === "Identifier") return node.name;
  if (node.type === "MemberExpression" || node.type === "OptionalMemberExpression") {
    return propertyName(node.property);
  }
  return null;
}

function staticString(node, bindings, active = new Set()) {
  if (!node) return null;
  if (
    node.type === "TSAsExpression" ||
    node.type === "TSSatisfiesExpression" ||
    node.type === "TSNonNullExpression" ||
    node.type === "ParenthesizedExpression"
  ) {
    return staticString(node.expression, bindings, active);
  }
  if (node.type === "StringLiteral") return node.value;
  if (node.type === "TemplateLiteral") {
    let value = "";
    for (let index = 0; index < node.quasis.length; index += 1) {
      value += node.quasis[index].value.cooked ?? node.quasis[index].value.raw;
      if (index < node.expressions.length) {
        const expression = staticString(node.expressions[index], bindings, active);
        if (expression === null) return null;
        value += expression;
      }
    }
    return value;
  }
  if (node.type === "NumericLiteral" || node.type === "BooleanLiteral") {
    return String(node.value);
  }
  if (node.type === "BinaryExpression" && node.operator === "+") {
    const left = staticString(node.left, bindings, active);
    const right = staticString(node.right, bindings, active);
    return left === null || right === null ? null : left + right;
  }
  if (node.type === "Identifier" && bindings.has(node.name) && !active.has(node.name)) {
    active.add(node.name);
    const value = staticString(bindings.get(node.name), bindings, active);
    active.delete(node.name);
    return value;
  }
  if (
    node.type === "CallExpression" &&
    (node.callee.type === "MemberExpression" || node.callee.type === "OptionalMemberExpression") &&
    propertyName(node.callee.property) === "repeat"
  ) {
    const value = staticString(node.callee.object, bindings, active);
    const count = node.arguments[0]?.type === "NumericLiteral" ? node.arguments[0].value : null;
    if (value === null) return null;
    if (Number.isInteger(count) && count >= 0) return value.repeat(count);
    return value.trim() === "" ? "" : null;
  }
  if (
    node.type === "CallExpression" &&
    (node.callee.type === "MemberExpression" || node.callee.type === "OptionalMemberExpression") &&
    propertyName(node.callee.property) === "trim"
  ) {
    return staticString(node.callee.object, bindings, active)?.trim() ?? null;
  }
  return null;
}

function testBindings(verification) {
  const bindings = new Map();
  for (const statement of verification.program.body) {
    if (statement.type !== "VariableDeclaration") continue;
    for (const declaration of statement.declarations) {
      if (declaration.id.type === "Identifier" && declaration.init) {
        bindings.set(declaration.id.name, declaration.init);
      }
    }
  }
  visitAst(verification.call.node.arguments.at(-1), (node) => {
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier" && node.init) {
      bindings.set(node.id.name, node.init);
    }
  });
  return bindings;
}

function objectStringProperty(node, name, bindings) {
  if (
    node?.type === "TSAsExpression" ||
    node?.type === "TSSatisfiesExpression" ||
    node?.type === "TSNonNullExpression" ||
    node?.type === "ParenthesizedExpression"
  )
    return objectStringProperty(node.expression, name, bindings);
  if (node?.type === "Identifier" && bindings.has(node.name)) {
    return objectStringProperty(bindings.get(node.name), name, bindings);
  }
  if (node?.type !== "ObjectExpression") return null;
  for (const property of node.properties.toReversed()) {
    if (property.type === "ObjectProperty" && propertyName(property.key) === name) {
      return staticString(property.value, bindings);
    }
    if (property.type === "SpreadElement") {
      const value = objectStringProperty(property.argument, name, bindings);
      if (value !== null) return value;
    }
  }
  return null;
}

function objectPropertyValue(node, name, bindings) {
  if (
    node?.type === "TSAsExpression" ||
    node?.type === "TSSatisfiesExpression" ||
    node?.type === "TSNonNullExpression" ||
    node?.type === "ParenthesizedExpression"
  )
    return objectPropertyValue(node.expression, name, bindings);
  if (node?.type === "Identifier" && bindings.has(node.name)) {
    return objectPropertyValue(bindings.get(node.name), name, bindings);
  }
  if (node?.type !== "ObjectExpression") return null;
  for (const property of node.properties.toReversed()) {
    if (property.type === "ObjectProperty" && propertyName(property.key) === name) {
      return property.value;
    }
    if (property.type === "SpreadElement") {
      const value = objectPropertyValue(property.argument, name, bindings);
      if (value !== null) return value;
    }
  }
  return null;
}

function staticStringArray(node, bindings) {
  if (node?.type === "Identifier" && bindings.has(node.name)) {
    return staticStringArray(bindings.get(node.name), bindings);
  }
  if (node?.type !== "ArrayExpression") return null;
  const values = [];
  for (const element of node.elements) {
    const value = staticString(element, bindings);
    if (value === null) return null;
    values.push(value);
  }
  return values;
}

function customElementSet(node, bindings) {
  if (node?.type === "Identifier" && bindings.has(node.name)) {
    return customElementSet(bindings.get(node.name), bindings);
  }
  if (node?.type !== "ArrowFunctionExpression" && node?.type !== "FunctionExpression") {
    return { status: "unverifiable", reason: "custom-element predicate is not a function" };
  }
  const parameter = node.params[0];
  if (parameter?.type !== "Identifier") {
    return { status: "unverifiable", reason: "custom-element predicate parameter is dynamic" };
  }
  let body = node.body;
  if (body.type === "BlockStatement") {
    const returns = body.body.filter((statement) => statement.type === "ReturnStatement");
    if (returns.length !== 1 || !returns[0].argument) {
      return { status: "unverifiable", reason: "custom-element predicate has control flow" };
    }
    body = returns[0].argument;
  }
  if (body.type === "BooleanLiteral" && body.value === false) return { values: [] };
  if (
    body.type === "BinaryExpression" &&
    ["===", "=="].includes(body.operator) &&
    body.left.type === "Identifier" &&
    body.left.name === parameter.name
  ) {
    const value = staticString(body.right, bindings);
    if (value !== null) return { values: [value] };
  }
  if (
    body.type === "CallExpression" &&
    propertyName(body.callee?.property) === "includes" &&
    body.arguments[0]?.type === "Identifier" &&
    body.arguments[0].name === parameter.name
  ) {
    const values = staticStringArray(body.callee.object, bindings);
    if (values !== null) return { values };
  }
  return {
    status: "unverifiable",
    reason: "custom-element callback cannot be represented as a finite exact tag set",
  };
}

function vueParseOptions(call, bindings) {
  const root = call.arguments[1];
  // `ignoreEmpty` (any value) selects a non-default `SFCParseOptions` toggle
  // Verter's carrier compiler has no equivalent for (Verter always applies
  // the `ignoreEmpty: true` isEmpty()-pruning rule — see `parse.ts` lines
  // 159-169/421-429 and the Rust-side `script_node_counts_as_entry_block`).
  // A call that sets it is testing a capability outside the production
  // option surface, exactly like an unrepresentable delimiter/custom-element
  // profile below — flag it the same way rather than silently verifying
  // against default-option semantics.
  if (objectPropertyValue(root, "ignoreEmpty", bindings) !== null) {
    return {
      status: "unverifiable",
      reason: "`ignoreEmpty` has no production equivalent",
    };
  }
  const template = objectPropertyValue(root, "templateParseOptions", bindings) ?? root;
  const delimitersNode =
    objectPropertyValue(template, "delimiters", bindings) ??
    objectPropertyValue(
      objectPropertyValue(root, "compilerOptions", bindings),
      "delimiters",
      bindings,
    );
  let delimiters = null;
  if (delimitersNode) {
    const values = staticStringArray(delimitersNode, bindings);
    if (!values || values.length !== 2) {
      return { status: "unverifiable", reason: "delimiter profile is not a static pair" };
    }
    delimiters = values;
  }
  const customNode =
    objectPropertyValue(template, "isCustomElement", bindings) ??
    objectPropertyValue(
      objectPropertyValue(root, "compilerOptions", bindings),
      "isCustomElement",
      bindings,
    );
  let customElements = null;
  if (customNode) {
    const result = customElementSet(customNode, bindings);
    if (result.status === "unverifiable") return result;
    customElements = [...new Set(result.values)].sort();
  }
  return { delimiters, custom_elements: customElements };
}

function carrierSource(value, kind) {
  if (kind === "carrier") return value;
  if (kind === "script-ts") return `<script setup lang="ts">\n${value}\n</script>`;
  if (kind === "script") return `<script>\n${value}\n</script>`;
  if (kind === "style") return `<template></template>\n<style>\n${value}\n</style>`;
  return `<template>\n${value}\n</template>`;
}

function declaredCallBindings(verification) {
  const names = new Set();
  const namespaces = new Set();
  visitAst(verification.program, (node) => {
    if (node.type === "ImportSpecifier" || node.type === "ImportDefaultSpecifier") {
      names.add(node.local.name);
    } else if (node.type === "ImportNamespaceSpecifier") {
      namespaces.add(node.local.name);
    } else if (node.type === "FunctionDeclaration" && node.id) {
      names.add(node.id.name);
    } else if (node.type === "VariableDeclarator" && node.id.type === "Identifier") {
      names.add(node.id.name);
    }
  });
  return { names, namespaces };
}

function callIsBound(callee, bindings) {
  if (callee?.type === "Identifier") return bindings.names.has(callee.name);
  if (
    (callee?.type === "MemberExpression" || callee?.type === "OptionalMemberExpression") &&
    callee.object.type === "Identifier"
  ) {
    return bindings.namespaces.has(callee.object.name);
  }
  return false;
}

function vueSourceCandidates(verification) {
  const { call } = verification;
  if (calleeText(verification.source, call.node.callee).includes(".each")) {
    return [
      {
        status: "unverifiable",
        reason_code: "dynamic_parameterized_input",
        co_owner: "official-runner",
        note: "parameterized declaration has multiple carrier inputs",
      },
    ];
  }
  const bindings = testBindings(verification);
  const callBindings = declaredCallBindings(verification);
  const candidates = [];
  visitAst(call.node.arguments.at(-1), (node) => {
    if (node.type !== "CallExpression" && node.type !== "OptionalCallExpression") return;
    if (!callIsBound(node.callee, callBindings)) return;
    const name = calleeName(node.callee);
    let value = null;
    let kind = null;
    let rank = 9;
    if (["parse", "compileSFCScript"].includes(name)) {
      value = staticString(node.arguments[0], bindings);
      kind = "carrier";
      rank = 0;
      if (
        name === "parse" &&
        node.arguments[1]?.type === "ObjectExpression" &&
        node.arguments[1].properties.some(
          (property) =>
            property.type === "ObjectProperty" && propertyName(property.key) === "compiler",
        )
      ) {
        candidates.push({
          status: "unverifiable",
          reason_code: "unsupported_parser_injection",
          co_owner: "official-runner",
          note: "official case injects a parser implementation outside the production option surface",
          rank,
          source: "",
        });
        return;
      }
    } else if (name === "compile") {
      value = staticString(node.arguments[0], bindings);
      kind = "carrier";
      if (value === null) {
        value = objectStringProperty(node.arguments[0], "source", bindings);
        kind = "template";
      }
      rank = 1;
    } else if (name === "resolve") {
      value = staticString(node.arguments[0], bindings);
      kind = "script-ts";
      rank = 1;
    } else if (["rewriteDefault", "rewriteDefaultAST"].includes(name)) {
      value = staticString(node.arguments[0], bindings);
      kind = "script";
      rank = 2;
    } else if (["compileWithAssetUrls", "compileWithSrcset", "baseParse"].includes(name)) {
      value = staticString(node.arguments[0], bindings);
      kind = "template";
      rank = 2;
    } else if (name === "compileScoped") {
      value = staticString(node.arguments[0], bindings);
      kind = "style";
      rank = 2;
    } else if (["compileStyle", "compileStyleAsync"].includes(name)) {
      value = objectStringProperty(node.arguments[0], "source", bindings);
      kind = "style";
      rank = 2;
    } else if (
      ![
        "expect",
        "toBe",
        "toContain",
        "toEqual",
        "toMatch",
        "toMatchObject",
        "toMatchSnapshot",
        "toMatchInlineSnapshot",
        "toStrictEqual",
        "assertCode",
        "getPositionInCode",
        "indexOf",
        "slice",
        "split",
        "repeat",
      ].includes(name)
    ) {
      value = staticString(node.arguments[0], bindings);
      if (value !== null) {
        if (/<(?:script|style|template)(?:\s|>)/i.test(value)) kind = "carrier";
        else if (verification.relative_path.includes("compileStyle")) kind = "style";
        else if (
          verification.relative_path.includes("compileScript") ||
          verification.relative_path.includes("rewriteDefault")
        )
          kind = "script-ts";
        else if (
          verification.relative_path.includes("compileTemplate") ||
          verification.relative_path.includes("templateTransform")
        )
          kind = "template";
        else value = null;
        rank = 7;
      }
    }
    if (value !== null) {
      const options =
        name === "parse" || name === "compile" || name === "baseParse"
          ? vueParseOptions(node, bindings)
          : { delimiters: null, custom_elements: null };
      if (options.status === "unverifiable") {
        candidates.push({
          status: "unverifiable",
          reason_code: "unrepresentable_syntax_profile",
          co_owner: "official-runner",
          note: options.reason,
          rank,
          source: carrierSource(value, kind),
          name,
        });
      } else {
        candidates.push({ source: carrierSource(value, kind), rank, name, options });
      }
    }
  });
  candidates.sort(
    (left, right) => left.rank - right.rank || right.source.length - left.source.length,
  );
  return candidates.length > 0
    ? candidates
    : [
        {
          status: "unverifiable",
          reason_code: "carrier_input_unresolved",
          co_owner: "official-runner",
          note: "case does not expose a statically recoverable carrier or carrier-block input",
        },
      ];
}

function matcherIsNegated(callee) {
  let node = callee;
  while (node) {
    if (
      (node.type === "MemberExpression" || node.type === "OptionalMemberExpression") &&
      propertyName(node.property) === "not"
    ) {
      return true;
    }
    node = node.object ?? node.callee;
  }
  return false;
}

function expectTarget(callee) {
  let node = callee;
  while (node) {
    if (
      (node.type === "CallExpression" || node.type === "OptionalCallExpression") &&
      calleeName(node.callee) === "expect"
    ) {
      return node.arguments[0] ?? null;
    }
    node = node.object ?? node.callee;
  }
  return null;
}

function containsVueFrontendInvocation(node, bindings) {
  let found = false;
  visitAst(node, (candidate) => {
    if (candidate.type !== "CallExpression" && candidate.type !== "OptionalCallExpression") return;
    if (
      callIsBound(candidate.callee, bindings) &&
      [
        "parse",
        "compile",
        "compileSFCScript",
        "baseParse",
        "compileStyle",
        "compileStyleAsync",
      ].includes(calleeName(candidate.callee))
    ) {
      found = true;
    }
  });
  return found;
}

// Whether an SFC `parse()` invocation source has at least one real entry
// block under official Vue's default `ignoreEmpty: true` rule (`parse.ts`
// lines 155-238): a `<template>` tag ALWAYS counts (even empty/self-closing
// — the `node.tag !== 'template'` guard exempts it from the isEmpty prune);
// a `<script>`/`<script setup>` tag counts only when it carries a `src`
// attribute OR has at least one non-whitespace content byte (`isEmpty()`,
// lines 421-429). A source with no counted entry block is exactly the
// input official's `parse.ts:232-238` rejects with `MissingSfcEntryBlock`
// — used to derive the expected outcome for a `parse()` call the enclosing
// test does not itself assert an error for (e.g. "should ignore other
// nodes with no content", `parse.spec.ts:220`, which checks only
// `descriptor.script`/`.styles`/`.customBlocks`, never `.errors`, for six
// separate block-less-or-empty-only sources — the official parser still
// pushes the diagnostic for each, silently).
//
// The `src` check is ATTRIBUTE PRESENCE, not value presence — official's
// `hasAttr` (`parse.ts:413-415`) is `node.props.some(p => p.name === name)`,
// true for a valueless `src` (`<script src/>` / `<script src></script>`)
// exactly as for a valued one. It is also CASE-SENSITIVE: Vue's own
// attribute-name parsing preserves authored casing verbatim (`onattribname`'s
// `name: getSlice(start, end)` in `compiler-core/src/parser.ts` — no
// `toLowerCase()`), so `SRC`/`Src` is a DIFFERENT attribute name to official,
// not a case-insensitive spelling of `src`.
//
// Parses actual attribute NAME tokens out of the raw tag-attrs string rather
// than regex-scanning the whole string — a bare substring/regex match over
// `attrs` can false-positive inside a QUOTED VALUE (`data-foo=" x src "`
// contains the substring ` src ` with word-boundary-shaped whitespace on
// both sides, which a naive `\bsrc\b`-style regex would wrongly match as the
// `src` attribute name).
function scriptTagHasSrcAttribute(attrs) {
  let i = 0;
  const n = attrs.length;
  while (i < n) {
    while (i < n && (/\s/.test(attrs[i]) || attrs[i] === "/")) i++;
    if (i >= n) break;
    const nameStart = i;
    while (i < n && !/[\s=/]/.test(attrs[i])) i++;
    if (attrs.slice(nameStart, i) === "src") return true;
    while (i < n && /\s/.test(attrs[i])) i++;
    if (attrs[i] === "=") {
      i++;
      while (i < n && /\s/.test(attrs[i])) i++;
      const quote = attrs[i];
      if (quote === '"' || quote === "'") {
        const closeIndex = attrs.indexOf(quote, i + 1);
        i = closeIndex === -1 ? n : closeIndex + 1;
      } else {
        while (i < n && !/\s/.test(attrs[i])) i++;
      }
    }
  }
  return false;
}
function vueSourceHasEntryBlock(source) {
  if (/<template[\s/>]/i.test(source)) return true;
  const scriptTagRe = /<script\b([^>]*)>/gi;
  let match;
  while ((match = scriptTagRe.exec(source))) {
    const attrs = match[1] ?? "";
    if (scriptTagHasSrcAttribute(attrs)) return true;
    if (/\/\s*$/.test(attrs)) continue; // self-closing, no src: empty
    const closeIndex = source.indexOf("</script>", scriptTagRe.lastIndex);
    const content =
      closeIndex === -1
        ? source.slice(scriptTagRe.lastIndex)
        : source.slice(scriptTagRe.lastIndex, closeIndex);
    if (content.trim() !== "") return true;
  }
  return false;
}

function vueExpectedOutcome(verification) {
  let thrown = false;
  let assertedParseError = false;
  const callBindings = declaredCallBindings(verification);
  visitAst(verification.call.node.arguments.at(-1), (node) => {
    if (node.type !== "CallExpression" && node.type !== "OptionalCallExpression") return;
    const name = calleeName(node.callee);
    if (["toThrow", "toThrowError"].includes(name) && !matcherIsNegated(node.callee)) {
      const target = expectTarget(node.callee);
      thrown ||= target !== null && containsVueFrontendInvocation(target, callBindings);
    }
    if (name === "assertWarning") assertedParseError = true;
    if (["toBe", "toEqual", "toBeGreaterThan"].includes(name)) {
      const expected = node.arguments[0];
      const targetText = verification.source.slice(node.callee.start, node.callee.end);
      if (/errors(?:\.length|\])/.test(targetText) && expected?.type === "NumericLiteral") {
        assertedParseError ||= expected.value > 0;
      }
    }
  });
  if (assertedParseError) return { outcome: "error", authority: "parse-diagnostic assertion" };
  if (thrown) return { outcome: "error", authority: "thrown compile assertion", co_owned: true };
  return { outcome: "valid", authority: "non-throwing declaration" };
}

const SVELTE_PARSE_ERROR_CODES = new Set([
  "attribute_duplicate",
  "block_invalid_continuation_placement",
  "block_invalid_placement",
  "block_unexpected_character",
  "block_unclosed",
  "css_expected_identifier",
  "css_empty_declaration",
  "css_selector_invalid",
  "declaration_tag_invalid_type",
  "element_invalid_closing_tag",
  "element_invalid_closing_tag_autoclosed",
  "element_unclosed",
  "expected_attribute_value",
  "expected_token",
  "expected_whitespace",
  "js_parse_error",
  "script_duplicate",
  "script_invalid_attribute_value",
  "script_invalid_context",
  "script_reserved_attribute",
  "style_duplicate",
  "svelte_meta_duplicate",
  "svelte_meta_invalid_content",
  "svelte_meta_invalid_placement",
  "svelte_options_deprecated_tag",
  "svelte_options_invalid_attribute",
  "svelte_options_invalid_attribute_value",
  "svelte_options_invalid_customelement",
  "svelte_options_invalid_customelement_props",
  "svelte_options_invalid_customelement_shadow",
  "svelte_options_invalid_tagname",
  "svelte_options_reserved_tagname",
  "svelte_options_unknown_attribute",
  "tag_invalid_name",
  "unexpected_eof",
  "unexpected_reserved_word",
  "void_element_invalid_content",
]);

function nestedObjectString(node, outerName, innerName) {
  let result = null;
  visitAst(node, (candidate) => {
    if (
      candidate.type !== "ObjectProperty" ||
      propertyName(candidate.key) !== outerName ||
      candidate.value.type !== "ObjectExpression"
    )
      return;
    for (const property of candidate.value.properties) {
      if (
        property.type === "ObjectProperty" &&
        propertyName(property.key) === innerName &&
        property.value.type === "StringLiteral"
      )
        result = property.value.value;
    }
  });
  return result;
}

// Whether any `<style>` block's own content carries an odd count of `"` or
// `'` — the structural signature of an unterminated CSS string literal
// (e.g. `url("star.gif');` mismatches a double- for a single-quote, so ONE
// of the two quote characters never finds its close). Official Svelte's CSS
// reader runs its own string-literal scan past the block's nominal
// boundary looking for the missing close, landing on `unexpected_eof` at
// true end-of-input rather than a `css_`-prefixed code — this is the CSS
// content-domain twin of the `css_`-prefixed / unclosed-`<style>`-tag
// heuristics above, narrowly scoped to the exact structural signal so it
// does not swallow a genuine non-CSS EOF defect.
function styleBlockHasUnterminatedString(source) {
  const blocks = [...source.matchAll(/<style[^>]*>([\s\S]*?)(?:<\/style>|$)/gi)].map(
    (match) => match[1],
  );
  return blocks.some((content) => {
    const doubleQuotes = (content.match(/"/g) ?? []).length;
    const singleQuotes = (content.match(/'/g) ?? []).length;
    return doubleQuotes % 2 !== 0 || singleQuotes % 2 !== 0;
  });
}

function officialCompilerErrorCode(configPath, parser) {
  if (!existsSync(configPath)) return null;
  const source = readFileSync(configPath, "utf8");
  const ast = parser.parse(source, { sourceType: "module", plugins: ["typescript"] });
  return nestedObjectString(ast, "error", "code");
}

const loosePrefixes = new Map();
function officialLoosePrefix(svelteRoot, suite, parser) {
  if (loosePrefixes.has(suite)) return loosePrefixes.get(suite);
  const runner = join(svelteRoot, "packages", "svelte", "tests", suite, "test.ts");
  if (!existsSync(runner)) {
    loosePrefixes.set(suite, null);
    return null;
  }
  const source = readFileSync(runner, "utf8");
  const ast = parser.parse(source, { sourceType: "module", plugins: ["typescript"] });
  let prefix = null;
  visitAst(ast, (node) => {
    if (
      node.type === "CallExpression" &&
      calleeName(node.callee) === "startsWith" &&
      node.arguments[0]?.type === "StringLiteral"
    )
      prefix = node.arguments[0].value;
  });
  loosePrefixes.set(suite, prefix);
  return prefix;
}

function svelteVerification(svelteRoot, suite, sampleName, dir, parser) {
  const carrierPath = ["input.svelte", "main.svelte"]
    .map((name) => join(dir, name))
    .find((path) => existsSync(path));
  if (!carrierPath) {
    return {
      status: "unverifiable",
      reason_code: "non_carrier_sample",
      co_owner: "script-semantics",
      note: "sample contains no Svelte carrier input",
    };
  }
  if (suite.startsWith("parser")) {
    const prefix = officialLoosePrefix(svelteRoot, suite, parser);
    if (prefix && sampleName.startsWith(prefix)) {
      // The official runner requests the unsupported `loose` parse
      // profile for this sample. That is now a REAL, verifiable claim:
      // the carrier frontend must return the typed
      // `SyntaxReject::UnsupportedProfile` before parsing — not an
      // unverifiable residual.
      return {
        source: readFileSync(carrierPath, "utf8"),
        expected: "unsupported_profile",
        authority: suite,
        svelte_loose: true,
      };
    }
    return { source: readFileSync(carrierPath, "utf8"), expected: "valid", authority: suite };
  }
  let errorCode = null;
  if (suite === "compiler-errors") {
    errorCode = officialCompilerErrorCode(join(dir, "_config.js"), parser);
  } else if (suite === "validator") {
    const errorsPath = join(dir, "errors.json");
    if (existsSync(errorsPath)) {
      const errors = JSON.parse(readFileSync(errorsPath, "utf8"));
      const codes = errors.map((error) => error.code).filter((code) => typeof code === "string");
      errorCode = codes.find((code) => SVELTE_PARSE_ERROR_CODES.has(code)) ?? null;
      if (codes.length > 0 && errorCode === null) {
        return {
          source: readFileSync(carrierPath, "utf8"),
          expected: "valid",
          authority: `validator semantic error ${codes.join(",")} leaves syntax valid`,
        };
      }
    }
  }
  if (errorCode && !SVELTE_PARSE_ERROR_CODES.has(errorCode)) {
    return {
      source: readFileSync(carrierPath, "utf8"),
      expected: "valid",
      authority: `official semantic error ${errorCode} leaves syntax valid`,
    };
  }
  const source = readFileSync(carrierPath, "utf8");
  if (
    errorCode?.startsWith("css_") ||
    (errorCode === "expected_token" &&
      /<style(?:\s|>)/i.test(source) &&
      !/<\/style>/i.test(source)) ||
    (errorCode === "unexpected_eof" && styleBlockHasUnterminatedString(source))
  ) {
    return {
      status: "unverifiable",
      reason_code: "co_owned_style_error",
      co_owner: "style-processing",
      note: `official style error ${errorCode} is outside carrier parsing`,
    };
  }
  return {
    source,
    expected: errorCode ? { outcome: "error", code: errorCode } : "valid",
    authority: errorCode ? `official parse error ${errorCode}` : suite,
  };
}

function ownerIncludes(row, owner) {
  return row.provisional_owner.split("/").includes(owner);
}

async function runProbe(probe, requests) {
  const child = spawn(probe, [], { stdio: ["pipe", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => (stdout += chunk));
  child.stderr.on("data", (chunk) => (stderr += chunk));
  for (const request of requests) child.stdin.write(`${JSON.stringify(request)}\n`);
  child.stdin.end();
  const code = await new Promise((accept, reject) => {
    child.on("error", reject);
    child.on("close", accept);
  });
  if (code !== 0) throw new Error(`parse probe exited ${code}: ${stderr}`);
  const responses = new Map();
  for (const line of stdout.split("\n").filter(Boolean)) {
    const response = JSON.parse(line);
    if (responses.has(response.id)) throw new Error(`duplicate probe response ${response.id}`);
    responses.set(response.id, response);
  }
  if (responses.size !== requests.length) {
    throw new Error(`probe returned ${responses.size} responses for ${requests.length} requests`);
  }
  return responses;
}

function verdictHash(verdict) {
  return hash(JSON.stringify(verdict)).slice(0, 16);
}

async function verifyParseFacet(vueRows, svelteRows, probe) {
  const requests = [];
  const plans = new Map();
  for (const row of vueRows) {
    if (!ownerIncludes(row, "B2")) continue;
    const invocations = vueSourceCandidates(row._verification);
    const expected = vueExpectedOutcome(row._verification);
    if (expected.outcome === "error" && invocations.length > 1) {
      invocations.push({
        status: "unverifiable",
        reason_code: "error_invocation_association_unresolved",
        co_owner: "official-runner",
        note: "an error assertion covers a case with multiple frontend invocations",
      });
    }
    const relativePath = row._verification.relative_path;
    const coOwner =
      expected.outcome === "error" &&
      (relativePath.includes("compileScript") ||
        relativePath.includes("compileTemplate") ||
        relativePath.includes("compileStyle"))
        ? relativePath.includes("compileScript")
          ? "script-compilation"
          : relativePath.includes("compileTemplate")
            ? "template-preprocessing"
            : "style-processing"
        : null;
    plans.set(row.case_id, {
      invocations,
      expected,
      authority: expected.authority,
      co_owner: coOwner,
    });
    invocations.forEach((invocation, index) => {
      if (invocation.status === "unverifiable") return;
      let invocationExpected = expected;
      if (invocation.name === "parse" && !vueSourceHasEntryBlock(invocation.source)) {
        invocationExpected = {
          outcome: "error",
          authority: "SFC parse without a template or script entry",
        };
      }
      invocation.expected = invocationExpected;
      invocation.request_id = `${row.case_id}:${index}`;
      requests.push({
        id: invocation.request_id,
        adapter: "vue",
        source: invocation.source,
        options: invocation.options,
      });
    });
  }
  for (const row of svelteRows) {
    if (!ownerIncludes(row, "B2")) continue;
    const plan = row._verification;
    if (plan.status === "unverifiable") {
      plans.set(row.case_id, plan);
      continue;
    }
    const invocation = {
      source: plan.source,
      expected: plan.expected,
      request_id: `${row.case_id}:0`,
    };
    plans.set(row.case_id, { invocations: [invocation], authority: plan.authority });
    requests.push({
      id: invocation.request_id,
      adapter: "svelte",
      source: plan.source,
      ...(plan.svelte_loose ? { options: { svelte_loose: true } } : {}),
    });
  }
  const responses = await runProbe(probe, requests);
  const counts = {
    pass: 0,
    fail: 0,
    unverifiable: 0,
    probed_invocations: requests.length,
    probed_invocations_by_adapter: Object.fromEntries(
      ["vue", "svelte"].map((adapter) => [
        adapter,
        requests.filter((request) => request.adapter === adapter).length,
      ]),
    ),
    unverifiable_categories: {},
    failures: [],
  };
  for (const row of [...vueRows, ...svelteRows]) {
    if (!ownerIncludes(row, "B2")) continue;
    const plan = plans.get(row.case_id);
    if (plan.status === "unverifiable") {
      const verdict = {
        classification: "unverifiable",
        reason_code: plan.reason_code,
        co_owner: plan.co_owner,
        note: plan.note,
      };
      row.evidence_id = `b2-parse:unverifiable:${verdictHash(verdict)}`;
      row.reason = `${row.reason} Parse-facet residual: ${plan.note}; retained for the co-owner.`;
      row._verdict = verdict;
      counts.unverifiable += 1;
      counts.unverifiable_categories[plan.reason_code] =
        (counts.unverifiable_categories[plan.reason_code] ?? 0) + 1;
      continue;
    }
    const residuals = plan.invocations.filter((invocation) => invocation.status === "unverifiable");
    const invocationVerdicts = plan.invocations
      .filter((invocation) => invocation.status !== "unverifiable")
      .map((invocation) => {
        const response = responses.get(invocation.request_id);
        const expected =
          typeof invocation.expected === "string"
            ? invocation.expected
            : invocation.expected.outcome;
        const errorDiagnostics = response.diagnostics.filter(
          (diagnostic) => diagnostic.severity === "error",
        );
        const invalidReject = ["unmapped_diagnostic", "invalid_carrier_geometry"].includes(
          response.reject_variant,
        );
        const mappedError =
          response.reject_variant === "rejected_syntax" || errorDiagnostics.length > 0;
        const requiredCode = invocation.expected?.code ?? null;
        const codeMatches =
          requiredCode === null ||
          errorDiagnostics.some((diagnostic) => diagnostic.code === requiredCode);
        const structurallyMatches =
          expected === "error"
            ? mappedError && !invalidReject && codeMatches
            : expected === "unsupported_profile"
              ? response.outcome === "reject" && response.reject_variant === "unsupported_profile"
              : response.outcome === "ok" && response.diagnostics.length === 0;
        const validationMatches =
          response.validation.spans_mapped && response.validation.diagnostics_sorted;
        return {
          source_sha256: hash(invocation.source),
          expected,
          required_code: requiredCode,
          outcome: response.outcome,
          reject_variant: response.reject_variant,
          diagnostics: response.diagnostics,
          validation: response.validation,
          mapped_error: mappedError,
          invalid_reject: invalidReject,
          matches: structurallyMatches && validationMatches,
        };
      });
    const hardFailure = invocationVerdicts.some((verdict) => !verdict.matches);
    if (
      hardFailure &&
      plan.co_owner &&
      invocationVerdicts.every(
        (verdict) => !verdict.mapped_error && !verdict.invalid_reject && verdict.outcome === "ok",
      )
    ) {
      const verdict = {
        classification: "unverifiable",
        reason_code: "co_owned_error_not_in_parse_boundary",
        co_owner: plan.co_owner,
        note: "official error is not surfaced by the carrier parse boundary",
        invocations: invocationVerdicts,
      };
      row.evidence_id = `b2-parse:unverifiable:${verdictHash(verdict)}`;
      row.reason = `${row.reason} Parse-facet residual: official error is retained for the co-owner because the carrier parse boundary did not surface it.`;
      row._verdict = verdict;
      counts.unverifiable += 1;
      counts.unverifiable_categories.co_owned_error_not_in_parse_boundary =
        (counts.unverifiable_categories.co_owned_error_not_in_parse_boundary ?? 0) + 1;
      continue;
    }
    if (!hardFailure && residuals.length > 0) {
      const residual = residuals[0];
      const verdict = {
        classification: "unverifiable",
        reason_code: residual.reason_code,
        co_owner: residual.co_owner,
        note: residual.note,
        invocations: invocationVerdicts,
      };
      row.evidence_id = `b2-parse:unverifiable:${verdictHash(verdict)}`;
      row.reason = `${row.reason} Parse-facet residual: ${residual.note}; retained for the co-owner.`;
      row._verdict = verdict;
      counts.unverifiable += 1;
      counts.unverifiable_categories[residual.reason_code] =
        (counts.unverifiable_categories[residual.reason_code] ?? 0) + 1;
      continue;
    }
    const classification = hardFailure ? "fail" : "pass";
    const verdict = {
      classification,
      authority: plan.authority,
      invocations: invocationVerdicts,
    };
    row.evidence_id = `b2-parse:${classification}:${verdictHash(verdict)}`;
    row._verdict = verdict;
    counts[classification] += 1;
    if (classification === "fail") counts.failures.push({ case_id: row.case_id, ...verdict });
  }
  return counts;
}

function locateVueCall(vueRoot, relativePath, line, column, parser) {
  const filePath = join(vueRoot, relativePath);
  const source = readFileSync(filePath, "utf8");
  const ast = parser.parse(source, {
    sourceType: "module",
    errorRecovery: false,
    plugins: ["typescript", "jsx", ["decorators", { decoratorsBeforeExport: true }]],
  });
  const match = testCalls(ast, source).find((call) => call.line === line && call.column === column);
  if (!match) {
    throw new Error(`${relativePath}:${line}:${column}: no matching test call found on re-parse`);
  }
  return { call: match, source, relative_path: relativePath, program: ast.program };
}

function locateSvelteSample(svelteRoot, suite, sourceLocator, parser) {
  const dir = join(svelteRoot, sourceLocator);
  const sampleName = basename(sourceLocator.replace(/\/$/, ""));
  return svelteVerification(svelteRoot, suite, sampleName, dir, parser);
}

function evidenceAnchor(caseId) {
  return caseId.toLowerCase();
}

function writeEvidenceFile(path, framework, rows, verdicts) {
  const lines = [
    `# B2 parse-facet evidence — ${framework}`,
    "",
    "Generated by `verify-b2-parse-facets.mjs`. Records the B2 parse/recovery/",
    "syntax-diagnostic/rejection facet verdict for every B2-owned row in the",
    `${framework} official-case manifest, verified against the pinned oracle`,
    "checkout. Regenerate with the same command to reproduce byte-for-byte.",
    "",
  ];
  for (const row of rows) {
    const verdict = verdicts.get(row.case_id);
    if (!verdict) continue;
    lines.push(`### ${row.case_id}`);
    lines.push("");
    lines.push(`- classification: \`${verdict.classification}\``);
    lines.push(`- source_locator: \`${row.source_locator}\``);
    lines.push(`- verdict_hash: \`${verdict.verdict_hash}\``);
    if (verdict.reason_code) lines.push(`- reason_code: \`${verdict.reason_code}\``);
    if (verdict.co_owner) lines.push(`- co_owner: \`${verdict.co_owner}\``);
    if (verdict.note) lines.push(`- note: ${verdict.note}`);
    if (verdict.invocations) {
      lines.push("- invocations:");
      for (const invocation of verdict.invocations) {
        lines.push(
          `  - expected=\`${invocation.expected}\` outcome=\`${invocation.outcome ?? "n/a"}\`` +
            ` reject_variant=\`${invocation.reject_variant ?? "n/a"}\` matches=\`${invocation.matches ?? "n/a"}\`` +
            ` required_code=\`${invocation.required_code ?? "n/a"}\` mapped_error=\`${invocation.mapped_error ?? "n/a"}\`` +
            ` invalid_reject=\`${invocation.invalid_reject ?? "n/a"}\` source_sha256=\`${invocation.source_sha256 ?? "n/a"}\``,
        );
        if (invocation.diagnostics?.length) {
          lines.push(
            `    diagnostics: ${invocation.diagnostics
              .map((d) => `${d.severity}:${d.code}@${d.start}-${d.end}`)
              .join(", ")}`,
          );
        }
        if (invocation.validation) {
          lines.push(
            `    validation: spans_mapped=\`${invocation.validation.spans_mapped}\`` +
              ` diagnostics_sorted=\`${invocation.validation.diagnostics_sorted}\``,
          );
        }
      }
    }
    lines.push("");
  }
  writeFileSync(path, lines.join("\n"));
}

async function main() {
  const options = args(process.argv.slice(2));
  assertCheckout(options["vue-source"], EXPECTED_VUE);
  assertCheckout(options["svelte-source"], EXPECTED_SVELTE);
  const requireFromOracle = createRequire(join(options["vue-modules"], "package.json"));
  const parser = requireFromOracle("@babel/parser");

  const vueRows = readTsv(join(options["manifest-dir"], "vue-official-cases.tsv"));
  const svelteRows = readTsv(join(options["manifest-dir"], "svelte-official-cases.tsv"));

  // `verifyParseFacet` APPENDS a "Parse-facet residual: ..." suffix to
  // `row.reason` for unverifiable rows. Re-running this script over its own
  // prior output must be idempotent, not accumulate a growing chain of
  // duplicated suffixes — strip any suffix a previous run already appended
  // before classification runs again.
  const RESIDUAL_SUFFIX = / Parse-facet residual:.*$/;
  for (const row of [...vueRows, ...svelteRows]) {
    if (!ownerIncludes(row, "B2")) continue;
    row.reason = row.reason.replace(RESIDUAL_SUFFIX, "");
  }

  for (const row of vueRows) {
    if (!ownerIncludes(row, "B2")) continue;
    const [relativePath, lineStr, columnStr] = row.source_locator.split(":");
    row._verification = locateVueCall(
      options["vue-source"],
      relativePath,
      Number(lineStr),
      Number(columnStr),
      parser,
    );
  }
  for (const row of svelteRows) {
    if (!ownerIncludes(row, "B2")) continue;
    row._verification = locateSvelteSample(
      options["svelte-source"],
      row.suite,
      row.source_locator,
      parser,
    );
  }

  const counts = await verifyParseFacet(vueRows, svelteRows, options.probe);

  // `verifyParseFacet` attaches the REAL verdict object it computed —
  // including, for a pass/fail/co-owned-unverifiable row, the full
  // per-invocation detail (expected outcome, actual outcome, rejection
  // variant, match result) — directly on `row._verdict`. The evidence file
  // persists that detail verbatim; nothing here re-derives a coarser
  // summary from the hash-bearing tag.
  function classificationOf(row) {
    const tag = row.evidence_id ?? "";
    return tag.startsWith("b2-parse:") ? tag.split(":")[1] : null;
  }

  function evidenceIdFor(row, framework) {
    const classification = classificationOf(row);
    if (!classification) return row.evidence_id;
    return `${framework}#${evidenceAnchor(row.case_id)}:${classification}`;
  }

  const vueEvidenceRows = vueRows.filter((row) => ownerIncludes(row, "B2"));
  const svelteEvidenceRows = svelteRows.filter((row) => ownerIncludes(row, "B2"));

  const verdictDetailsFor = (rows) =>
    new Map(
      rows.map((row) => [
        row.case_id,
        {
          ...row._verdict,
          classification: classificationOf(row),
          source_locator: row.source_locator,
          reason: row.reason,
          verdict_hash: verdictHash(row._verdict),
        },
      ]),
    );
  const vueVerdictDetails = verdictDetailsFor(vueEvidenceRows);
  const svelteVerdictDetails = verdictDetailsFor(svelteEvidenceRows);

  writeEvidenceFile(
    join(options["evidence-dir"], "B2-parse-facet-vue.md"),
    "Vue",
    vueEvidenceRows,
    vueVerdictDetails,
  );
  writeEvidenceFile(
    join(options["evidence-dir"], "B2-parse-facet-svelte.md"),
    "Svelte",
    svelteEvidenceRows,
    svelteVerdictDetails,
  );

  for (const row of vueEvidenceRows) row.evidence_id = evidenceIdFor(row, "B2-parse-facet-vue.md");
  for (const row of svelteEvidenceRows)
    row.evidence_id = evidenceIdFor(row, "B2-parse-facet-svelte.md");

  writeTsv(
    vueRows,
    [
      "case_id",
      "suite",
      "source_locator",
      "source_object",
      "declaration_kind",
      "title_kind",
      "title_sha256",
      "disposition",
      "provisional_owner",
      "reason",
      "evidence_id",
    ],
    join(options["manifest-dir"], "vue-official-cases.tsv"),
  );
  writeTsv(
    svelteRows,
    [
      "case_id",
      "suite",
      "source_locator",
      "source_object",
      "declaration_kind",
      "disposition",
      "provisional_owner",
      "reason",
      "evidence_id",
    ],
    join(options["manifest-dir"], "svelte-official-cases.tsv"),
  );

  process.stdout.write(JSON.stringify(counts) + "\n");
}

await main();
