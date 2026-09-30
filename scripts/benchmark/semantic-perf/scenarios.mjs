// The scenario catalog of the equivalent-demand semantic benchmark.
//
// A scenario is ONE TypeScript module that both arms read byte for byte. It
// declares two type aliases the harness demands, in this order:
//
//   __BenchInit  a trivial alias (`0`); its request absorbs each tool's
//                one-time lazy initialisation and is reported separately;
//   __Probe      the demanded alias; its declared type is the answer.
//
// Each scenario demands exactly one probe: tsc's answers are order-dependent
// (an instantiation that failed with TS2589 poisons a later, shallower
// request for the same alias family), so two probes in one program would not
// be two independent demands.
//
// `beyond` is the answer the type system defines when tsc stops at one of its
// resource limits (TS2589 / TS2590 / TS2859) and answers `any` or a fallback
// instead: the answer an engine computing past tsc's limit must produce. It
// is derived from the construction, never from either tool's output.
//
// The perf-suite families this catalog does NOT cover, and why, are listed in
// UNCOVERED; each needs its own harness.

const range = (n) => Array.from({ length: n }, (_, i) => i);
const union = (items) => items.join(" | ");

/** Wrap a scenario body into the module both arms read. */
export function moduleText(body, probe) {
  return `type __BenchInit = 0;\n${body}type __Probe = ${probe};\nexport {};\n`;
}

function scenario(id, family, note, body, probe, extra = {}) {
  return { id, family, note, probe, source: moduleText(body, probe), ...extra };
}

/**
 * The whole-program (`tsc -p`) arms check the scenario plus a use of the
 * probe: tsc resolves an unused alias lazily, so without a use its full check
 * never computes the answer the probe arms demand.
 */
export const CLI_USE = "declare const __bench_use: __Probe;\n";

/** The module the whole-program arms check. */
export function cliSource(scenario) {
  return scenario.source + CLI_USE;
}

/** Every scenario, in catalog order. */
export function allScenarios() {
  const out = [];

  out.push(
    scenario(
      "baseline-empty",
      "baseline",
      "fixed cost: a trivial module and a trivial probe (reported alone, never subtracted)",
      "",
      "1",
    ),
  );

  // Relations: object unions related position by position, and reversed.
  for (const m of [200, 600, 1800, 3200]) {
    out.push(
      scenario(
        `relation-aligned-${m}`,
        "relations",
        `[S] extends [T] over two ${m}-member object unions, target in source order`,
        `type S = ${union(range(m).map((i) => `{ p${i}: ${i} }`))};\n` +
          `type T = ${union(range(m).map((i) => `{ p${i}: number }`))};\n`,
        "[S] extends [T] ? 1 : 2",
      ),
    );
  }
  for (const m of [200, 600, 1800, 2100, 3200]) {
    out.push(
      scenario(
        `relation-reversed-${m}`,
        "relations",
        `[S] extends [T] over two ${m}-member object unions, target reversed`,
        `type S = ${union(range(m).map((i) => `{ p${i}: ${i} }`))};\n` +
          `type T = ${union(range(m).map((i) => `{ p${m - 1 - i}: number }`))};\n`,
        "[S] extends [T] ? 1 : 2",
        { beyond: "1" },
      ),
    );
  }

  // Products: spreads of object unions.
  for (const [a, b] of [
    [369, 271],
    [400, 250],
  ]) {
    out.push(
      scenario(
        `spread-${a}x${b}`,
        "products",
        `{ ...a, ...b } over a ${a}- and a ${b}-member object union (${a * b} members)`,
        `type A = ${union(range(a).map((i) => `{ a${i}: ${i} }`))};\n` +
          `type B = ${union(range(b).map((i) => `{ b${i}: ${i} }`))};\n` +
          `declare const a: A;\ndeclare const b: B;\nconst s = { ...a, ...b };\n`,
        "typeof s",
        {
          beyond: union(
            range(a).flatMap((i) => range(b).map((j) => `{ a${i}: ${i}; b${j}: ${j}; }`)),
          ),
        },
      ),
    );
  }
  for (const c of [39, 40]) {
    out.push(
      scenario(
        `spread3-50x50x${c}`,
        "products",
        `{ ...a, ...b, ...c } over 50 x 50 x ${c} object-union members (${50 * 50 * c})`,
        `type A = ${union(range(50).map((i) => `{ a${i}: ${i} }`))};\n` +
          `type B = ${union(range(50).map((i) => `{ b${i}: ${i} }`))};\n` +
          `type C = ${union(range(c).map((i) => `{ c${i}: ${i} }`))};\n` +
          `declare const a: A;\ndeclare const b: B;\ndeclare const c: C;\n` +
          `const s = { ...a, ...b, ...c };\n`,
        "typeof s",
        {
          beyond: union(
            range(50).flatMap((i) =>
              range(50).flatMap((j) =>
                range(c).map((k) => `{ a${i}: ${i}; b${j}: ${j}; c${k}: ${k}; }`),
              ),
            ),
          ),
        },
      ),
    );
  }

  // Products: template literal spans.
  const digits = `type D = ${union(range(10).map((i) => `"${i}"`))};\n`;
  const digitStrings = (spans) => {
    let acc = [""];
    for (let s = 0; s < spans; s++)
      acc = acc.flatMap((prefix) => range(10).map((d) => `${prefix}${d}`));
    return acc;
  };
  out.push(
    scenario(
      "template-4-spans",
      "products",
      "`${D}${D}${D}${D}` over ten digits (10,000 members)",
      digits,
      "`${D}${D}${D}${D}`",
    ),
  );
  out.push(
    scenario(
      "template-5-spans",
      "products",
      "`${D}${D}${D}${D}${D}` over ten digits (100,000 members)",
      digits,
      "`${D}${D}${D}${D}${D}`",
      { beyond: union(digitStrings(5).map((s) => `"${s}"`)) },
    ),
  );
  for (const [a, b] of [
    [369, 271],
    [400, 250],
  ]) {
    out.push(
      scenario(
        `template-${a}x${b}`,
        "products",
        `\`\${A}-\${B}\` over ${a} and ${b} string literals (${a * b} members)`,
        `type A = ${union(range(a).map((i) => `"a${i}"`))};\n` +
          `type B = ${union(range(b).map((i) => `"b${i}"`))};\n`,
        "`${A}-${B}`",
        { beyond: union(range(a).flatMap((i) => range(b).map((j) => `"a${i}-b${j}"`))) },
      ),
    );
  }
  out.push(
    scenario(
      "template-nested",
      "products",
      "a template nested in a template: `${`${D}${D}`}-${D}` (1,000 members)",
      digits,
      "`${`${D}${D}`}-${D}`",
    ),
  );
  out.push(
    scenario(
      "template-absorption-400x250",
      "products",
      "400 literals absorbed by `a${number}`, times 250 literals",
      `type A = ${union(range(400).map((i) => `"a${i}"`))} | \`a\${number}\`;\n` +
        `type B = ${union(range(250).map((i) => `"b${i}"`))};\n`,
      "`${A}-${B}`",
    ),
  );

  // Depth: alias chains and nested conditional chains.
  for (const n of [50, 200, 500, 1000]) {
    out.push(
      scenario(
        `alias-chain-${n}`,
        "depth",
        `alias chain A_i<T> = A_{i-1}<T>, ${n} links`,
        "type A0<T> = T | undefined;\n" +
          range(n)
            .map((i) => `type A${i + 1}<T> = A${i}<T>;`)
            .join("\n") +
          "\n",
        `A${n}<"ok">`,
      ),
    );
  }
  {
    // The 1,000-link chain in a 1 MiB module: unrelated interfaces fill the
    // rest, so setup parses and binds a megabyte both tools must read.
    const chain =
      "type A0<T> = T | undefined;\n" +
      range(1000)
        .map((i) => `type A${i + 1}<T> = A${i}<T>;`)
        .join("\n") +
      "\n";
    let filler = "";
    for (let i = 0; chain.length + filler.length < 1024 * 1024; i++) {
      filler += `interface Filler${i} { id: ${i}; name: "f${i}"; next?: Filler${i}; }\n`;
    }
    out.push(
      scenario(
        "alias-chain-1000-1mib",
        "depth",
        "the 1,000-link alias chain inside a 1 MiB module",
        chain + filler,
        'A1000<"ok">',
      ),
    );
  }
  for (const n of [97, 98, 200, 500]) {
    out.push(
      scenario(
        `conditional-chain-${n}`,
        "depth",
        `nested conditional chain E_i<X> = E_{i-1}<E0<X>>, ${n} links`,
        "type E0<X> = X extends string ? X : never;\n" +
          range(n)
            .map((i) => `type E${i + 1}<X> = E${i}<E0<X>>;`)
            .join("\n") +
          "\n",
        `E${n}<"ok">`,
        { beyond: '"ok"' },
      ),
    );
  }
  for (const n of [50, 500, 999, 1000]) {
    out.push(
      scenario(
        `tail-recursive-parse-${n}`,
        "depth",
        `tail-recursive digit parse with a constrained infer over ${n} characters`,
        "type Len<S extends string, N extends unknown[] = []> =\n" +
          '  S extends `${infer _ extends number}${infer R}` ? Len<R, [...N, 0]> : N["length"];\n' +
          `type S = "${"1".repeat(n)}";\n`,
        "Len<S>",
        { beyond: String(n) },
      ),
    );
  }

  // Inference.
  for (const n of [1025, 5000]) {
    out.push(
      scenario(
        `inference-deposits-${n}`,
        "inference",
        `one type parameter inferred from a ${n}-element tuple argument`,
        `declare function f<T>(xs: [${range(n)
          .map(() => "T")
          .join(", ")}]): T;\n` +
          `const r = f([${range(n)
            .map(() => "1")
            .join(", ")}]);\n`,
        "typeof r",
      ),
    );
  }
  out.push(
    scenario(
      "overloads-1025",
      "inference",
      "1,024 string-literal overloads then a number overload, called with a number",
      range(1024)
        .map((i) => `declare function f(x: "s${i}"): "s";`)
        .join("\n") + '\ndeclare function f(x: number): "n";\nconst r = f(1);\n',
      "typeof r",
    ),
  );
  for (const n of [10, 100, 500]) {
    out.push(
      scenario(
        `contravariant-callbacks-${n}`,
        "inference",
        `one T inferred from ${n} callback parameters`,
        `declare function co<T>(${range(n)
          .map((i) => `f${i}: (x: T) => void`)
          .join(", ")}): T;\n` +
          `const r = co(${range(n)
            .map(() => '(x: "a") => {}')
            .join(", ")});\n`,
        "typeof r",
      ),
    );
  }
  for (const n of [10, 100, 1000]) {
    out.push(
      scenario(
        `infer-pattern-repeat-${n}`,
        "inference",
        `a conditional whose pattern repeats an infer, distributed over ${n} object members`,
        range(n)
          .map((i) => `type M${i} = { a: ${i}; b: (x: ${i}) => void };`)
          .join("\n") +
          "\ntype Pat<T> = T extends { a: infer U; b: (x: infer U) => void } ? U : 0;\n",
        `Pat<${range(n)
          .map((i) => `M${i}`)
          .join(" | ")}>`,
      ),
    );
  }
  for (const d of [10, 100, 500]) {
    out.push(
      scenario(
        `reference-infer-depth-${d}`,
        "inference",
        `Box<…> extends Box<infer P> through ${d} nested aliases`,
        "interface Box<T> { v: T }\ntype N0<T> = Box<T>;\n" +
          range(d)
            .map((i) => `type N${i + 1}<T> = N${i}<T>;`)
            .join("\n") +
          "\n",
        `N${d}<"x"> extends Box<infer P> ? P : never`,
      ),
    );
  }
  for (const n of [100, 1000]) {
    out.push(
      scenario(
        `reference-infer-aliases-${n}`,
        "inference",
        `${n} distinct aliases of Box, each inferred through once`,
        "interface Box<T> { v: T }\n" +
          range(n)
            .map((i) => `type B${i}<T> = Box<T>;`)
            .join("\n") +
          "\n",
        `[${range(n)
          .map((i) => `B${i}<${i}>`)
          .join(", ")}] extends Box<infer P>[] ? P : never`,
      ),
    );
  }
  for (const n of [1, 10, 50]) {
    const params = range(n).map((i) => `T${i}`);
    out.push(
      scenario(
        `base-signature-${n}`,
        "inference",
        `(...a: infer A) => infer R over a generic signature with ${n} type parameters`,
        `declare function g<${params.join(", ")}>(${params.map((p, i) => `a${i}: ${p}`).join(", ")}): [${params.join(", ")}];\n`,
        "typeof g extends (...a: infer A) => infer R ? R : never",
      ),
    );
  }

  // Library shapes read through the benchmark library.
  out.push(
    scenario(
      "library-awaited",
      "library",
      "Awaited over nested promises",
      "",
      "Awaited<Promise<Promise<string>>>",
    ),
  );
  out.push(
    scenario(
      "library-array-map",
      "library",
      "the return of Array#map with a callback",
      "declare const xs: number[];\nconst ys = xs.map((x) => x > 0);\n",
      "typeof ys",
    ),
  );
  out.push(
    scenario(
      "library-promise-then",
      "library",
      "Promise#then chained twice",
      'declare const p: Promise<number>;\nconst q = p.then((n) => "" + n).then((s) => s.length > 0);\n',
      "typeof q",
    ),
  );
  out.push(
    scenario(
      "library-map-entries",
      "library",
      "Map's key and value read back by inference",
      "declare const m: Map<string, number[]>;\n",
      "typeof m extends Map<infer K, infer V> ? [K, V] : never",
    ),
  );
  out.push(
    scenario(
      "library-generic-call",
      "library",
      "a generic call inferring T and K from its arguments",
      "declare function pick<T, K extends keyof T>(o: T, k: K): T[K];\n" +
        "declare const o: { a: string; b: number };\n" +
        'const v = pick(o, "b");\n',
      "typeof v",
    ),
  );

  return out;
}

/** Perf-suite families this harness does not measure, each with its reason. */
export const UNCOVERED = [
  {
    family: "workspace concurrency",
    reason:
      "sibling batches of 12/50 components importing ./types measure resolution operations, fs reads, restarts and single-flight across a whole workspace; that is a multi-file, multi-request lifecycle workload with no single demanded probe and needs its own harness",
  },
  {
    family: "whole-project (InputMenu.vue)",
    reason:
      "an equivalent demand for a Vue SFC needs matched project dependencies, libraries and projection boundaries on both sides; Verter's SFC projection has no tsc counterpart request, so it needs its own harness",
  },
  {
    family: "frame-runtime overhead on shallow common paths",
    reason:
      "a Verter-against-Verter regression (baseline vs candidate commit); scripts/benchmark/signature-kernel-perf.mjs measures it",
  },
  {
    family: "recursion-detection time per shape",
    reason:
      "a Verter budget/divergence property with no equivalent tsc demand; measured by its own counting tests",
  },
];

/** The scenarios whose id starts with one of `prefixes` (all when empty). */
/**
 * Each scenario's tier:
 *
 * - `quick`: one representative normal size per scenario series (the
 *   default run; about five minutes);
 * - `standard`: the other normal sizes and the sizes at tsc's own limits
 *   (TS2589 / TS2590 / TS2859 onsets, elided prints) — the baseline;
 * - `stress`: sizes at Verter's limits and pathological sizes (tsc
 *   exhausting 8 GiB, multi-second Verter requests); opt-in, may take hours.
 *
 * Tiers nest: standard includes quick, stress includes both.
 */
export const SCENARIO_TIERS = {
  "baseline-empty": "quick",
  "relation-aligned-200": "quick",
  "relation-reversed-200": "quick",
  "template-4-spans": "quick",
  "template-nested": "quick",
  "template-absorption-400x250": "quick",
  "alias-chain-50": "quick",
  "conditional-chain-97": "quick",
  "tail-recursive-parse-50": "quick",
  "inference-deposits-1025": "quick",
  "overloads-1025": "quick",
  "contravariant-callbacks-10": "quick",
  "infer-pattern-repeat-10": "quick",
  "reference-infer-depth-10": "quick",
  "reference-infer-aliases-100": "quick",
  "base-signature-1": "quick",
  "library-awaited": "quick",
  "library-array-map": "quick",
  "library-promise-then": "quick",
  "library-map-entries": "quick",
  "library-generic-call": "quick",
  "relation-aligned-600": "standard",
  "relation-aligned-1800": "standard",
  "relation-aligned-3200": "standard",
  "relation-reversed-600": "standard",
  "relation-reversed-2100": "standard",
  "spread-400x250": "standard",
  "spread3-50x50x40": "standard",
  "template-5-spans": "standard",
  "template-400x250": "standard",
  "template-369x271": "standard",
  "alias-chain-200": "standard",
  "alias-chain-500": "standard",
  "alias-chain-1000": "standard",
  "conditional-chain-98": "standard",
  "conditional-chain-200": "standard",
  "tail-recursive-parse-500": "standard",
  "tail-recursive-parse-999": "standard",
  "tail-recursive-parse-1000": "standard",
  "inference-deposits-5000": "standard",
  "contravariant-callbacks-100": "standard",
  "contravariant-callbacks-500": "standard",
  "infer-pattern-repeat-100": "standard",
  "reference-infer-depth-100": "standard",
  "reference-infer-depth-500": "standard",
  "reference-infer-aliases-1000": "standard",
  "base-signature-10": "standard",
  "base-signature-50": "standard",
  "relation-reversed-1800": "stress",
  "relation-reversed-3200": "stress",
  "spread-369x271": "stress",
  "spread3-50x50x39": "stress",
  "alias-chain-1000-1mib": "stress",
  "conditional-chain-500": "stress",
  "infer-pattern-repeat-1000": "stress",
};

export const TIERS = ["quick", "standard", "stress"];

/** The scenarios a tier runs (its own and every lighter tier's). */
export function scenariosForTier(tier) {
  const rank = TIERS.indexOf(tier);
  if (rank < 0) throw new Error(`unknown tier ${tier}; tiers: ${TIERS.join(", ")}`);
  const all = allScenarios();
  for (const s of all) if (!SCENARIO_TIERS[s.id]) throw new Error(`scenario ${s.id} has no tier`);
  return all.filter((s) => TIERS.indexOf(SCENARIO_TIERS[s.id]) <= rank);
}

export function selectScenarios(prefixes) {
  const all = allScenarios();
  if (!prefixes.length) return all;
  const chosen = all.filter((s) => prefixes.some((p) => s.id === p || s.id.startsWith(p)));
  if (!chosen.length) throw new Error(`--only ${prefixes.join(",")} selects no scenario`);
  return chosen;
}

/** The four strictNullChecks x noImplicitAny settings. */
export const SETTINGS = [
  { id: "strict", strictNullChecks: true, noImplicitAny: true },
  { id: "snc-off", strictNullChecks: false, noImplicitAny: true },
  { id: "nia-off", strictNullChecks: true, noImplicitAny: false },
  { id: "both-off", strictNullChecks: false, noImplicitAny: false },
];

/** The tsconfig both arms read for `setting`. */
export function tsconfigText(setting) {
  return (
    JSON.stringify(
      {
        compilerOptions: {
          strict: true,
          strictNullChecks: setting.strictNullChecks,
          noImplicitAny: setting.noImplicitAny,
          noLib: true,
          noEmit: true,
          target: "es2022",
          module: "esnext",
          skipLibCheck: false,
          noErrorTruncation: true,
        },
        files: ["lib.bench.d.ts", "scenario.ts"],
      },
      null,
      2,
    ) + "\n"
  );
}
