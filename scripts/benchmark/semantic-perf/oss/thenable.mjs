// The thenable catalog of the Biome comparison (`--biome`).
//
// Biome answers no type query: its type inference is reachable only through
// the decisions of its type-aware lint rules. The one decision that projects
// a declared type is `noFloatingPromises`: a call whose result type is
// Promise-like, left unhandled, is reported. So the Biome comparison asks
// every tool a BOOLEAN projection of the benchmark's demand — "is the
// declared type of `__Probe` Promise-like?" — on programs where the answer
// hinges on one semantic feature of the main catalog's families.
//
// Every feature comes as a PAIR of programs that differ in one place, one
// whose `__Probe` is Promise-like and one whose `__Probe` is not, so a tool
// that does not evaluate the feature answers both alike and cannot decide
// the pair: neither "always yes" nor "always no" scores. The expected answer
// of each program is tsc 7.0.2's, measured (thenable-expected.json), never
// the construction's.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { moduleText } from "../scenarios.mjs";

export const THENABLE_EXPECTED = join(
  dirname(fileURLToPath(import.meta.url)),
  "thenable-expected.json",
);

/** tsc 7.0.2's measured answers for the catalog (measure-thenable.mjs). */
export function loadThenableExpected() {
  return JSON.parse(readFileSync(THENABLE_EXPECTED, "utf8"));
}

const range = (n) => Array.from({ length: n }, (_, i) => i);
const union = (items) => items.join(" | ");

/** Each pair: its family, the feature, and its two programs (body + probe). */
function pairs() {
  const out = [];
  const pair = (id, family, feature, yes, no) => out.push({ id, family, feature, yes, no });

  const S = `type S = ${union(range(200).map((i) => `{ p${i}: ${i} }`))};\n`;
  const T = (t) => `type T = ${union(range(200).map((i) => `{ p${i}: ${t} }`))};\n`;
  pair(
    "relation-200",
    "relations",
    "[S] extends [T] over two 200-member object unions",
    [S + T("number"), "[S] extends [T] ? Promise<1> : 2"],
    [S + T("string"), "[S] extends [T] ? Promise<1> : 2"],
  );
  const D = `type D = ${union(range(10).map((i) => `"${i}"`))};\n`;
  pair(
    "template-membership",
    "products",
    "a literal against a template literal type over ten digits",
    [D, '"a5" extends `a${D}` ? Promise<1> : 2'],
    [D, '"b5" extends `a${D}` ? Promise<1> : 2'],
  );
  const chain =
    "type A0<T> = T;\n" +
    range(50)
      .map((i) => `type A${i + 1}<T> = A${i}<T>;`)
      .join("\n") +
    "\n";
  pair(
    "alias-chain-50",
    "depth",
    "a 50-link generic alias chain",
    [chain, "A50<Promise<1>>"],
    [chain, "A50<1>"],
  );
  const cond =
    "type E0<X> = X extends string ? X : never;\n" +
    range(20)
      .map((i) => `type E${i + 1}<X> = E${i}<E0<X>>;`)
      .join("\n") +
    "\n";
  pair(
    "conditional-chain-20",
    "depth",
    "a 20-link nested conditional chain",
    [cond, '[E20<"ok">] extends ["ok"] ? Promise<1> : 2'],
    [cond, '[E20<"no">] extends ["ok"] ? Promise<1> : 2'],
  );
  const len =
    "type Len<S extends string, N extends unknown[] = []> =\n" +
    '  S extends `${infer _ extends number}${infer R}` ? Len<R, [...N, 0]> : N["length"];\n';
  pair(
    "tail-recursive-parse-10",
    "depth",
    "a tail-recursive digit parse over 10 characters",
    [len, `Len<"${"1".repeat(10)}"> extends 10 ? Promise<1> : 2`],
    [len, `Len<"${"1".repeat(10)}"> extends 11 ? Promise<1> : 2`],
  );
  pair(
    "generic-call",
    "inference",
    "T inferred from a generic call's argument",
    [
      "declare function id<T>(x: T): T;\ndeclare const v: Promise<1>;\nconst r = id(v);\n",
      "typeof r",
    ],
    ["declare function id<T>(x: T): T;\ndeclare const v: 1;\nconst r = id(v);\n", "typeof r"],
  );
  const overloads = 'declare function f(x: "s"): Promise<1>;\ndeclare function f(x: number): 2;\n';
  pair(
    "overloads",
    "inference",
    "overload resolution by the argument",
    [overloads + 'const r = f("s");\n', "typeof r"],
    [overloads + "const r = f(1);\n", "typeof r"],
  );
  pair(
    "infer-reference",
    "inference",
    "Box<…> extends Box<infer P> through an interface",
    ["interface Box<T> { v: T }\n", "Box<Promise<1>> extends Box<infer P> ? P : never"],
    ["interface Box<T> { v: T }\n", "Box<1> extends Box<infer P> ? P : never"],
  );
  const co = "declare function co<T>(f: (x: T) => void): T;\n";
  pair(
    "contravariant-callback",
    "inference",
    "T inferred from a callback parameter",
    [co + "const r = co((x: Promise<1>) => {});\n", "typeof r"],
    [co + "const r = co((x: 1) => {});\n", "typeof r"],
  );
  pair(
    "library-then-awaited",
    "library",
    "Promise#then keeps a Promise; Awaited unwraps it",
    ["declare const p: Promise<number>;\nconst q = p.then((n) => n > 0);\n", "typeof q"],
    ["declare const p: Promise<number>;\n", "Awaited<typeof p>"],
  );
  pair(
    "library-map-infer",
    "library",
    "Map's value read back by inference",
    [
      "declare const m: Map<string, Promise<1>>;\n",
      "typeof m extends Map<infer K, infer V> ? V : never",
    ],
    ["declare const m: Map<string, 1>;\n", "typeof m extends Map<infer K, infer V> ? V : never"],
  );
  pair(
    "mapped-index",
    "mapped",
    "an indexed access into a mapped type",
    ["", '{ [K in "a" | "b"]: Promise<K> }["a"]'],
    ["", '{ [K in "a" | "b"]: K }["a"]'],
  );
  return out;
}

/** Every program of the thenable catalog, in catalog order. */
export function thenableCases() {
  return pairs().flatMap((p) =>
    ["yes", "no"].map((variant) => {
      const [body, probe] = p[variant];
      return {
        id: `${p.id}-${variant}`,
        pair: p.id,
        variant,
        family: p.family,
        note: p.feature,
        probe,
        source: moduleText(body, probe),
      };
    }),
  );
}

/**
 * The demand Biome answers: an unhandled call returning `__Probe`, which
 * `noFloatingPromises` reports exactly when it infers a Promise-like type.
 */
export const BIOME_DEMAND = "declare function __bench_probe(): __Probe;\n__bench_probe();\n";

/** The line of the demanded call in `source + BIOME_DEMAND` (1-based). */
export function demandLine(source) {
  return source.split("\n").length + 1;
}

/**
 * Whether a normalised type (canonical.mjs) is Promise-like the way the
 * floating-promise rules read it: a `Promise` / `PromiseLike` reference, or
 * a union with such a member.
 */
export function isThenable(node) {
  if (!node) return false;
  if (node.k === "ref") return node.name === "Promise" || node.name === "PromiseLike";
  if (node.k === "union") return node.members.some(isThenable);
  return false;
}
