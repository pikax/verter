// The scenario catalog of the equivalent-demand semantic benchmark.
//
// `globalScope` names what a scenario adds to the global scope (a global
// interface, enum or library augmentation): two scenarios with the same one
// cannot share a program (the project workload keeps one of them).
//
// A scenario is ONE TypeScript module (`scenario.ts`) that both arms read
// byte for byte, and, for the program family, the companion files of its
// program (`files`: name to text), which every arm reads as root files of the
// same project. The module declares two type aliases the harness demands, in
// this order:
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

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { sha256Text } from "./provenance.mjs";

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

/** The scenario's companion file names, in root-file order (none for a one-module scenario). */
export function companionFiles(scenario) {
  return Object.keys(scenario?.files ?? {});
}

/**
 * The digest of everything a scenario contributes to its program: the
 * module's alone for a one-module scenario (so its recorded digests stay
 * valid), else the module and every companion file.
 */
export function scenarioSha256(scenario) {
  if (!companionFiles(scenario).length) return sha256Text(scenario.source);
  return sha256Text(JSON.stringify({ source: scenario.source, files: scenario.files }));
}

/** Write a scenario's companion files into `dir`; returns their digests by name. */
export function writeCompanions(dir, scenario) {
  const inputs = {};
  for (const [name, text] of Object.entries(scenario?.files ?? {})) {
    writeFileSync(join(dir, name), text);
    inputs[name] = sha256Text(text);
  }
  return inputs;
}

/** Problems with a cell's recorded companion inputs (prefix: the cell's subdirectory). */
export function companionInputProblems(inputs, scenario, prefix = "") {
  const problems = [];
  for (const [name, text] of Object.entries(scenario?.files ?? {}))
    if (inputs?.[`${prefix}${name}`] !== sha256Text(text))
      problems.push(`${prefix}${name} is not the catalog's companion file`);
  return problems;
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

  // Programs: answers that depend on files the module never imports (global
  // and module augmentation, global declaration merging), and a large type
  // spread across modules. Every companion file is a root file of the
  // project, as in a tsconfig `include`.
  out.push(
    scenario(
      "program-global-merge",
      "program",
      "keyof a global interface merged from two script files and a module's declare global, none imported",
      "",
      "keyof BenchConfig",
      {
        globalScope: "BenchConfig",
        files: {
          "config-base.ts": "interface BenchConfig { base: string; }\n",
          "config-extra.ts": "interface BenchConfig { extra: number; }\n",
          "config-late.ts":
            "export {};\ndeclare global { interface BenchConfig { late: boolean; } }\n",
        },
      },
    ),
  );
  out.push(
    scenario(
      "program-lib-augmentation",
      "program",
      "a library interface (Array) augmented by a module the scenario never imports",
      "",
      'string[]["benchFirst"]',
      {
        globalScope: "Array.benchFirst",
        files: {
          "augment.ts": "export {};\ndeclare global { interface Array<T> { benchFirst: T; } }\n",
        },
      },
    ),
  );
  out.push(
    scenario(
      "program-module-augmentation",
      "program",
      "keyof an imported interface augmented by declare module in a file the scenario never imports",
      'import type { Options } from "./dep";\n',
      "keyof Options",
      {
        files: {
          "dep.ts": "export interface Options { a: string; }\n",
          "plugin.ts": 'export {};\ndeclare module "./dep" { interface Options { b: number; } }\n',
        },
      },
    ),
  );
  out.push(
    scenario(
      "program-globals-mixed",
      "program",
      "an inferred return across modules reading globals declared in a .d.ts, a script and a module's declare global",
      'import { describe } from "./service";\n',
      "ReturnType<typeof describe>",
      {
        globalScope: "BenchUser",
        files: {
          "globals.d.ts":
            'declare const BENCH_ENV: "dev" | "prod";\n' +
            "declare const BENCH_ID: `u-${number}`;\n" +
            "interface BenchUser { id: number; name: string; }\n",
          "roles.ts":
            'export const ROLES = ["admin", "editor", "viewer"] as const;\n' +
            "declare global { interface BenchUser { role: (typeof ROLES)[number]; } }\n",
          "flags.ts": "interface BenchUser { flags: { beta: boolean; seats: number }; }\n",
          "service.ts":
            'import { ROLES } from "./roles";\n' +
            "export function describe(u: BenchUser) {\n" +
            "  return { id: BENCH_ID, env: BENCH_ENV, role: u.role, first: ROLES[0], name: u.name, flags: u.flags };\n" +
            "}\n",
        },
      },
    ),
  );
  for (const [n, depth] of [
    [20, 3],
    [100, 5],
    [300, 5],
  ]) {
    out.push(
      scenario(
        `program-paths-${n}x${depth}`,
        "program",
        `every dotted path to depth ${depth} through ${n} cross-referencing interfaces in four modules (a template-literal recursive Paths<T>)`,
        'import type { Api } from "./api";\nimport type { Paths } from "./utils";\n',
        "Paths<Api>",
        { files: pathsProgram(n, depth) },
      ),
    );
  }

  out.push(...adversarialScenarios());
  return out;
}

/**
 * Demands aimed at a demand-driven engine's weak points, each answered by
 * tsc cleanly and quickly: answers that need a large fraction of the program
 * (star-export barrels, globals and augmentations merged from many files,
 * inference chained across files), full expansion of wide or deep types,
 * heavy generic and contextual inference, and control-flow narrowing. An
 * object answer is read through a declared value (`typeof __v`): an alias
 * whose own node is the object would be printed by its name.
 */
function adversarialScenarios() {
  const out = [];
  const add = (id, note, spec) =>
    out.push(
      scenario(`adv-${id}`, "adversarial", note, spec.body, spec.probe, {
        ...(spec.globalScope ? { globalScope: spec.globalScope } : {}),
        ...(spec.files ? { files: spec.files } : {}),
      }),
    );

  for (const n of [100, 500, 2000]) {
    const group = 20;
    const groups = Math.ceil(n / group);
    const files = {};
    for (let i = 0; i < n; i++)
      files[`leaf-${i}.ts`] =
        `export const v${i} = ${i} as const;\nexport interface T${i} { id: ${i} }\nexport type K${i} = "k${i}";\n`;
    for (let g = 0; g < groups; g++)
      files[`group-${g}.ts`] = range(group)
        .map((j) => g * group + j)
        .filter((i) => i < n)
        .map((i) => `export * from "./leaf-${i}";\n`)
        .join("");
    files["index.ts"] =
      range(groups)
        .map((g) => `export * from "./group-${g}";\n`)
        .join("") + 'export const v0 = "shadow" as const;\n';
    add(
      `barrel-star-${n}`,
      `the value union of a namespace import through a two-level export * barrel over ${n} modules, one star export shadowed locally`,
      { files, body: 'import * as ns from "./index";\n', probe: "(typeof ns)[keyof typeof ns]" },
    );
  }

  for (const n of [100, 500, 2000]) {
    const files = {};
    for (let i = 0; i < n; i++)
      files[`reg-${i}.ts`] =
        i % 2
          ? `export {};\ndeclare global { interface BenchRegistry { r${i}: { id: ${i}; tag: "t${i}" } } }\n`
          : `interface BenchRegistry { r${i}: { id: ${i}; tag: "t${i}" } }\n`;
    add(
      `global-registry-${n}`,
      `a global interface merged from ${n} files (scripts and declare global), indexed by its keys`,
      {
        globalScope: "BenchRegistry",
        files,
        body: "",
        probe: 'BenchRegistry[keyof BenchRegistry]["tag"]',
      },
    );
  }

  for (const n of [100, 500, 2000]) {
    const files = { "core.ts": "export interface Props { base: -1 }\n" };
    for (let i = 0; i < n; i++)
      files[`plugin-${i}.ts`] =
        `export {};\ndeclare module "./core" { interface Props { p${i}: ${i} } }\n`;
    add(
      `module-augment-${n}`,
      `an imported interface augmented by ${n} plugin modules the scenario never imports`,
      { files, body: 'import type { Props } from "./core";\n', probe: "Props[keyof Props]" },
    );
  }

  for (const n of [10, 100, 500]) {
    // Call resolution tries a later declaration's signatures first, so the
    // call resolves to the last file in program order; file names run in
    // the opposite order, so name order is not program order.
    const files = {};
    for (let i = 0; i < n; i++)
      files[`bus-${String(n - 1 - i).padStart(4, "0")}.ts`] =
        `interface BenchBus { emit(e: { k${i}: 1 }): ${i}; }\n`;
    add(
      `merged-overload-order-${n}`,
      `a call on an overload set merged from ${n} global interface declarations, every overload applicable`,
      {
        globalScope: "BenchBus",
        files,
        body:
          `declare const bus: BenchBus;\ndeclare const ev: { ${range(n)
            .map((i) => `k${i}: 1;`)
            .join(" ")} };\n` + "const r = bus.emit(ev);\n",
        probe: "typeof r",
      },
    );
  }

  for (const n of [20, 100, 300]) {
    const files = { "m-0.ts": "export function f0() { return { k0: 0 as const }; }\n" };
    for (let i = 1; i < n; i++)
      files[`m-${i}.ts`] =
        `import { f${i - 1} } from "./m-${i - 1}";\n` +
        `export function f${i}() { const p = f${i - 1}(); return { ...p, k${i}: ${i} as const }; }\n`;
    add(
      `return-chain-${n}`,
      `a return type inferred through ${n} modules, each spreading the previous module's result`,
      {
        files,
        body: `import { f${n - 1} } from "./m-${n - 1}";\n`,
        probe: `ReturnType<typeof f${n - 1}>`,
      },
    );
  }

  for (const n of [10, 25, 40]) {
    let body =
      "type Ctor<T = {}> = new (...args: any[]) => T;\nclass Base { base = -1 as const; }\n";
    for (let i = 0; i < n; i++)
      body += `function M${i}<TBase extends Ctor>(B: TBase) { return class extends B { p${i} = ${i} as const; }; }\n`;
    let expr = "Base";
    for (let i = 0; i < n; i++) expr = `M${i}(${expr})`;
    body += `class C extends ${expr} {}\ndeclare const __v: { [K in keyof C]: C[K] };\n`;
    add(`mixin-stack-${n}`, `the members of a class built from ${n} stacked generic mixins`, {
      body,
      probe: "typeof __v",
    });
  }

  for (const n of [50, 200, 500])
    add(
      `builder-chain-${n}`,
      `a fluent builder: ${n} chained generic calls growing an intersection, then flattened`,
      {
        body:
          "interface Builder<T> {\n" +
          "  add<K extends string, V extends string | number | boolean>(k: K, v: V): Builder<T & { [P in K]: V }>;\n" +
          "  build(): { [K in keyof T]: T[K] };\n}\n" +
          "declare function builder(): Builder<{}>;\n" +
          `const r = builder()${range(n)
            .map((i) => `.add("a${i}", ${i})`)
            .join("")}.build();\n`,
        probe: "typeof r",
      },
    );

  for (const k of [5, 12, 20]) {
    let body = "";
    for (let a = 1; a <= 20; a++) {
      const tp = range(a + 1).map((i) => `T${i}`);
      const params = range(a).map((i) => `f${i}: (x: T${i}) => T${i + 1}`);
      body += `declare function pipe<${tp.join(", ")}>(a: T0, ${params.join(", ")}): T${a};\n`;
    }
    body += `const r = pipe(0 as const, ${range(k)
      .map((i) => `(x) => ({ v${i}: x })`)
      .join(", ")});\n`;
    add(
      `pipe-contextual-${k}`,
      `pipe() over 20 arity overloads with ${k} callbacks, each contextually typed by the previous inference`,
      { body, probe: "typeof r" },
    );
  }

  for (const n of [20, 100, 400])
    add(
      `options-this-${n}`,
      `an options-API component: ${n} methods typed through ThisType reading ${n} data members`,
      {
        body:
          "declare global { interface ThisType<T> {} }\n" +
          "declare function defineComponent<D, M>(o: { data(): D; methods: M & ThisType<D & M> }): { [K in keyof M]: M[K] extends () => infer R ? R : never };\n" +
          "const comp = defineComponent({\n" +
          `  data() { return { ${range(n)
            .map((i) => `d${i}: ${i} as const`)
            .join(", ")} }; },\n` +
          `  methods: { ${range(n)
            .map((i) => `m${i}() { return this.d${i}; }`)
            .join(", ")} },\n` +
          "});\n",
        probe: "typeof comp",
      },
    );

  const routes = (n, which, k = 5) => ({
    body:
      `const routes = [\n${range(n)
        .map(
          (i) =>
            `  { name: "r${i}", path: "/r${i}", children: [${range(k)
              .map((j) => `{ name: "r${i}c${j}", path: "c${j}" }`)
              .join(", ")}] },`,
        )
        .join("\n")}\n] as const;\n` +
      "type Names<R> = R extends readonly (infer E)[]\n" +
      "  ? E extends { name: infer N } ? N | (E extends { children: infer C } ? Names<C> : never) : never\n" +
      "  : never;\n" +
      'type Paths<R, P extends string = ""> = R extends readonly (infer E)[]\n' +
      "  ? E extends { path: infer S extends string }\n" +
      "    ? `${P}${S}` | (E extends { children: infer C } ? Paths<C, `${P}${S}/`> : never)\n" +
      "    : never\n" +
      "  : never;\n",
    probe: `${which}<typeof routes>`,
  });
  for (const n of [20, 100, 300])
    add(
      `router-names-${n}`,
      `route names from a nested as-const config of ${n} routes with five children each`,
      routes(n, "Names"),
    );
  for (const n of [20, 100, 300])
    add(
      `router-paths-${n}`,
      `full route paths from a nested as-const config of ${n} routes with five children each`,
      routes(n, "Paths"),
    );

  for (const n of [50, 300, 1000])
    add(
      `switch-narrow-${n}`,
      `a return type inferred through a switch narrowing a ${n}-member discriminated union`,
      {
        body:
          `type U = ${range(n)
            .map((i) => `{ kind: "k${i}"; v${i}: ${i} }`)
            .join(" | ")};\n` +
          `function f(x: U) {\n  switch (x.kind) {\n${range(n)
            .map((i) => `    case "k${i}": return x.v${i};`)
            .join("\n")}\n  }\n}\n`,
        probe: "ReturnType<typeof f>",
      },
    );

  for (const n of [10, 25, 50]) {
    const files = { "enum-0.ts": "enum BenchFlag { F0 = 1 }\n" };
    for (let i = 1; i < n; i++)
      files[`enum-${i}.ts`] = `enum BenchFlag { F${i} = BenchFlag.F${i - 1} * 2 + ${i % 3} }\n`;
    add(
      `enum-merge-${n}`,
      `an enum merged across ${n} script files, each member computed from the previous file's`,
      { globalScope: "BenchFlag", files, body: "", probe: "`${BenchFlag}`" },
    );
  }

  for (const n of [1000, 3000, 10000])
    add(
      `mapped-collapse-${n}`,
      `a mapped conditional over a ${n}-member interface collapsing to three members`,
      {
        body: `interface Big { ${range(n)
          .map((i) => `k${i}: ${i % 3 === 0 ? i : i % 3 === 1 ? `"s${i}"` : "boolean"};`)
          .join(" ")} }\n`,
        probe:
          '{ [K in keyof Big]: Big[K] extends number ? "n" : Big[K] extends string ? "s" : "b" }[keyof Big]',
      },
    );

  for (const n of [100, 300, 600])
    add(
      `union-to-intersection-${n}`,
      `keyof the intersection inferred contravariantly from a ${n}-member object union`,
      {
        body:
          "type U2I<U> = (U extends unknown ? (x: U) => void : never) extends (x: infer I) => void ? I : never;\n" +
          `type Parts = ${range(n)
            .map((i) => `{ a${i}: ${i} }`)
            .join(" | ")};\n`,
        probe: "keyof U2I<Parts>",
      },
    );

  for (const n of [100, 1000, 3000])
    add(
      `key-remap-${n}`,
      `template-literal key remapping over a ${n}-member interface declared in another module`,
      {
        files: {
          "model.ts": `export interface Model { ${range(n)
            .map((i) => `field_${i}_name_x: ${i};`)
            .join(" ")} }\n`,
        },
        body:
          'import type { Model } from "./model";\n' +
          "type Kebab<S extends string> = S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S;\n" +
          "declare const __v: { [K in keyof Model as Kebab<K & string>]: Model[K] };\n",
        probe: "typeof __v",
      },
    );

  for (const n of [10, 40, 100]) {
    let body = "interface L0<T> { p0: T }\n";
    for (let i = 1; i < n; i++) body += `interface L${i}<T> extends L${i - 1}<[T]> { p${i}: T }\n`;
    body += `declare const __v: { [K in keyof L${n - 1}<0>]: L${n - 1}<0>[K] };\n`;
    add(
      `heritage-generic-${n}`,
      `the members of a ${n}-deep generic interface heritage chain, each base instantiated with a wrapped argument`,
      { body, probe: "typeof __v" },
    );
  }

  return out;
}

/** Each adversarial series' tiers, smallest size first. */
const ADVERSARIAL_TIERS = {
  "barrel-star": [100, 500, 2000, ["quick", "standard", "standard"]],
  "global-registry": [100, 500, 2000, ["quick", "standard", "stress"]],
  "module-augment": [100, 500, 2000, ["quick", "standard", "standard"]],
  "merged-overload-order": [10, 100, 500, ["quick", "standard", "standard"]],
  "return-chain": [20, 100, 300, ["quick", "standard", "standard"]],
  "mixin-stack": [10, 25, 40, ["quick", "standard", "standard"]],
  "builder-chain": [50, 200, 500, ["quick", "standard", "stress"]],
  "pipe-contextual": [5, 12, 20, ["quick", "standard", "standard"]],
  "options-this": [20, 100, 400, ["quick", "standard", "standard"]],
  "router-names": [20, 100, 300, ["quick", "standard", "stress"]],
  "router-paths": [20, 100, 300, ["quick", "standard", "stress"]],
  "switch-narrow": [50, 300, 1000, ["quick", "standard", "stress"]],
  "enum-merge": [10, 25, 50, ["quick", "standard", "standard"]],
  "mapped-collapse": [1000, 3000, 10000, ["quick", "standard", "standard"]],
  "union-to-intersection": [100, 300, 600, ["quick", "standard", "standard"]],
  "key-remap": [100, 1000, 3000, ["quick", "standard", "stress"]],
  "heritage-generic": [10, 40, 100, ["quick", "standard", "standard"]],
};

const adversarialTierEntries = () =>
  Object.entries(ADVERSARIAL_TIERS).flatMap(([series, [a, b, c, tiers]]) =>
    [a, b, c].map((size, i) => [`adv-${series}-${size}`, tiers[i]]),
  );

/**
 * A program of `n` interfaces spread over four entity modules, each pointing
 * at two others (often in another module) and at a generic from a utility
 * module, an `Api` module naming them all, and a recursive Paths<T> to
 * `depth` levels.
 */
function pathsProgram(n, depth, shards = 4) {
  const files = {};
  const shardOf = (i) => i % shards;
  for (let s = 0; s < shards; s++) {
    const refs = new Set();
    const decls = range(n)
      .filter((i) => shardOf(i) === s)
      .map((i) => {
        const next = (i + 1) % n;
        const owner = (i * 7 + 3) % n;
        for (const r of [next, owner]) if (shardOf(r) !== s) refs.add(r);
        return `export interface E${i} { id: ${i}; name: string; next: E${next}; owner: E${owner}; meta: Meta<${i}>; }`;
      });
    const imports = range(shards)
      .filter((t) => t !== s)
      .map((t) => {
        const names = [...refs].filter((r) => shardOf(r) === t).sort((a, b) => a - b);
        return names.length
          ? `import type { ${names.map((r) => `E${r}`).join(", ")} } from "./entities-${t}";\n`
          : "";
      })
      .join("");
    files[`entities-${s}.ts`] =
      `import type { Meta } from "./utils";\n${imports}${decls.join("\n")}\n`;
  }
  files["utils.ts"] =
    "export interface Meta<T> { tag: T; created: number; }\n" +
    `export type Paths<T, D extends unknown[] = []> = D["length"] extends ${depth}\n` +
    "  ? never\n" +
    "  : T extends object\n" +
    "    ? { [K in keyof T & string]: K | `${K}.${Paths<T[K], [...D, 0]>}` }[keyof T & string]\n" +
    "    : never;\n";
  files["api.ts"] =
    range(shards)
      .map((s) => {
        const names = range(n)
          .filter((i) => shardOf(i) === s)
          .map((i) => `E${i}`);
        return `import type { ${names.join(", ")} } from "./entities-${s}";\n`;
      })
      .join("") +
    `export interface Api { ${range(n)
      .map((i) => `e${i}: E${i};`)
      .join(" ")} }\n`;
  return files;
}

/** Perf-suite families this harness does not measure, each with its reason. */
export const UNCOVERED = [
  {
    family: "workspace concurrency at batch scale",
    reason:
      "sessions.mjs measures concurrent demands across files in one live engine; sibling batches of 12/50 components with fs-read, restart and single-flight counts are a workspace lifecycle workload that needs its own harness",
  },
  {
    family: "a real component library project",
    reason:
      "sessions.mjs measures an InputMenu-equivalent component built from local sources; a real library's dependency graph is an external corpus, outside the hermetic catalog",
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
  "program-global-merge": "quick",
  "program-lib-augmentation": "quick",
  "program-module-augmentation": "quick",
  "program-globals-mixed": "quick",
  "program-paths-20x3": "quick",
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
  "program-paths-100x5": "standard",
  "program-paths-300x5": "standard",
  "relation-reversed-1800": "stress",
  "relation-reversed-3200": "stress",
  "spread-369x271": "stress",
  "spread3-50x50x39": "stress",
  "alias-chain-1000-1mib": "stress",
  "conditional-chain-500": "stress",
  "infer-pattern-repeat-1000": "stress",
  ...Object.fromEntries(adversarialTierEntries()),
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

/**
 * The tsconfig both arms read for `setting`: the library, then the
 * scenario's companion files (the program family), then the module.
 */
export function tsconfigText(setting, scenario = null) {
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
        files: ["lib.bench.d.ts", ...companionFiles(scenario), "scenario.ts"],
      },
      null,
      2,
    ) + "\n"
  );
}
