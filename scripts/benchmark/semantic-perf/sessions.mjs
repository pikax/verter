// The session workloads of the semantic benchmark: one live engine per
// invocation answering a script of edits and demands over a multi-file
// project, the way an editor drives it (see docs/contributing/semantic-benchmark.md).
//
// A session declares its project's files, the files tsc's program lists
// (a Vue SFC is Verter-only), and an ordered script:
//
//   { kind: "demand", concurrent?, requests: [{ file, alias, expect }] }
//   { kind: "edit", file, text }
//   { kind: "meta", file, expect: { props: [{ name, required }], events } }
//
// `expect` is the answer the type system defines for the demand at that
// point of the script, derived from the construction, never from either
// tool's output: tsc's answer must equal it (a run where it does not fails
// validation), Verter's is classified against it. A `meta` demand (a Vue
// component's props and events) has no tsc counterpart: Verter-only,
// classified, never compared.

import { SETTINGS } from "./scenarios.mjs";

const range = (n) => Array.from({ length: n }, (_, i) => i);
const union = (items) => items.join(" | ");

/**
 * tsc's incremental facility in a session: API program reuse — an edit is a
 * `fileChanges` notification to `updateSnapshot`, which derives the next
 * snapshot from the previous one.
 */
export const INCREMENTAL_FACILITY = "api-snapshot-reuse";

/** The alias every session's init file declares; its request absorbs lazy initialisation. */
export const INIT_ALIAS = "__BenchInit";

// ---------------------------------------------------------------- INCREMENTAL

const leaf = (v) => `export type Leaf = ${v};\n`;
const mid = (tag) =>
  `import type { Leaf } from "./leaf";\nexport type Mid<T> = [Leaf, T, "${tag}"];\n`;
const unrelated = (u) => `export type Unrelated = "${u}";\n`;

function incremental() {
  const probe = (expect) => ({
    kind: "demand",
    requests: [{ file: "main.ts", alias: "__Probe", expect }],
  });
  return {
    id: "incremental-edits",
    family: "incremental",
    note: "edit a leaf type, an intermediate generic and an unrelated file, re-requesting a dependent alias after each",
    files: {
      "leaf.ts": leaf(1),
      "mid.ts": mid("mid0"),
      "unrelated.ts": unrelated("u0"),
      "main.ts": `import type { Mid } from "./mid";\ntype ${INIT_ALIAS} = 0;\ntype __Probe = Mid<"m">[number];\nexport {};\n`,
    },
    initFile: "main.ts",
    steps: [
      probe('1 | "m" | "mid0"'),
      { kind: "edit", file: "leaf.ts", text: leaf(2) },
      probe('2 | "m" | "mid0"'),
      { kind: "edit", file: "mid.ts", text: mid("mid1") },
      probe('2 | "m" | "mid1"'),
      { kind: "edit", file: "unrelated.ts", text: unrelated("u1") },
      probe('2 | "m" | "mid1"'),
    ],
  };
}

// ---------------------------------------------------------------- EDITOR SESSION

// An InputMenu-equivalent component: props composed from a picked base and
// the component's own members (a userland Pick: the benchmark library has
// none), events from a tuple-map type, both imported from a types module.
const SIZES = ['"xs"', '"sm"', '"md"', '"lg"'];
function inputMenuTypes({ sizes = SIZES, clearable = false } = {}) {
  return (
    "export type PickOf<T, K extends keyof T> = { [P in K]: T[P] };\n" +
    "export interface InputMenuItem { label: string; value: string | number; disabled?: boolean; icon?: string }\n" +
    "export interface BaseInputProps { id?: string; name?: string; placeholder?: string; disabled?: boolean; required?: boolean; autofocus?: boolean }\n" +
    'export type InputMenuProps = PickOf<BaseInputProps, "id" | "name" | "placeholder" | "disabled"> & {\n' +
    "  items: InputMenuItem[];\n" +
    '  modelValue?: InputMenuItem["value"];\n' +
    "  multiple?: boolean;\n" +
    "  searchTerm?: string;\n" +
    `  size?: ${union(sizes)};\n` +
    (clearable ? "  clearable?: boolean;\n" : "") +
    "};\n" +
    'export type InputMenuEmits = { "update:modelValue": [value: InputMenuItem["value"]]; "update:searchTerm": [term: string]; focus: [] };\n'
  );
}
const INPUT_MENU_VUE =
  '<script setup lang="ts">\n' +
  'import type { InputMenuEmits, InputMenuProps } from "./types";\n' +
  "const props = defineProps<InputMenuProps>();\n" +
  "const emit = defineEmits<InputMenuEmits>();\n" +
  "</script>\n" +
  "<template><div /></template>\n";
function hover(extra = "") {
  return (
    'import type { InputMenuItem, InputMenuProps, PickOf } from "./types";\n' +
    `type ${INIT_ALIAS} = 0;\n` +
    'type __HoverSize = InputMenuProps["size"];\n' +
    'type __HoverValue = InputMenuItem["value"];\n' +
    'type __HoverKeys = keyof PickOf<InputMenuProps, "items" | "multiple">;\n' +
    extra +
    "export {};\n"
  );
}
function menuMeta(clearable) {
  const optional = [
    "id",
    "name",
    "placeholder",
    "disabled",
    "modelValue",
    "multiple",
    "searchTerm",
  ];
  const props = [
    ...optional.map((name) => ({ name, required: false })),
    { name: "items", required: true },
    { name: "size", required: false },
    ...(clearable ? [{ name: "clearable", required: false }] : []),
  ];
  return { props, events: ["update:modelValue", "update:searchTerm", "focus"] };
}

function editorSession() {
  const sizes = (list) => union([...list, "undefined"]);
  const hoverDemands = (sizeList) => [
    { file: "hover.ts", alias: "__HoverSize", expect: sizes(sizeList) },
    { file: "hover.ts", alias: "__HoverValue", expect: "string | number" },
    { file: "hover.ts", alias: "__HoverKeys", expect: '"items" | "multiple"' },
  ];
  const label = 'type __HoverLabel = InputMenuItem["label"];\n';
  const grown = [...SIZES, '"xl"'];
  return {
    id: "editor-session",
    family: "editor",
    note: "an InputMenu-equivalent Vue component: hover-like demands and component metadata, cold, then across edits to its types and to the file being hovered",
    files: {
      "types.ts": inputMenuTypes(),
      "InputMenu.vue": INPUT_MENU_VUE,
      "hover.ts": hover(),
    },
    initFile: "hover.ts",
    steps: [
      { kind: "demand", requests: hoverDemands(SIZES) },
      { kind: "meta", file: "InputMenu.vue", expect: menuMeta(false) },
      { kind: "edit", file: "types.ts", text: inputMenuTypes({ sizes: grown, clearable: true }) },
      { kind: "demand", requests: hoverDemands(grown).slice(0, 1) },
      { kind: "meta", file: "InputMenu.vue", expect: menuMeta(true) },
      { kind: "edit", file: "hover.ts", text: hover(label) },
      {
        kind: "demand",
        requests: [
          { file: "hover.ts", alias: "__HoverLabel", expect: "string" },
          ...hoverDemands(grown),
        ],
      },
    ],
  };
}

// ---------------------------------------------------------------- CONCURRENT

const CONCURRENT_FILES = 8;
const CONCURRENT_MEMBERS = 100;
function concurrentFile(i) {
  const m = CONCURRENT_MEMBERS;
  return (
    (i === 0 ? `type ${INIT_ALIAS} = 0;\n` : "") +
    `type S = ${union(range(m).map((j) => `{ p${j}: ${j} }`))};\n` +
    `type T = ${union(range(m).map((j) => `{ p${j}: number }`))};\n` +
    `type __Probe = [S] extends [T] ? "c${i}-yes" : "c${i}-no";\n` +
    "export {};\n"
  );
}

function concurrent() {
  const files = Object.fromEntries(
    range(CONCURRENT_FILES).map((i) => [`c${i}.ts`, concurrentFile(i)]),
  );
  const requests = range(CONCURRENT_FILES).map((i) => ({
    file: `c${i}.ts`,
    alias: "__Probe",
    expect: `"c${i}-yes"`,
  }));
  return {
    id: "concurrent-demands",
    family: "concurrent",
    note: `${CONCURRENT_FILES} demands in ${CONCURRENT_FILES} files issued at once (a ${CONCURRENT_MEMBERS}-member union relation each), cold, then again`,
    files,
    initFile: "c0.ts",
    steps: [
      { kind: "demand", concurrent: true, requests },
      { kind: "demand", concurrent: true, requests },
    ],
  };
}

// ---------------------------------------------------------------- AUGMENTATION GROWTH

// The two augmentation reproductions (the augment-global and augment-module
// scenarios), each answered at FIRST contributors and then, in the same
// engine, at GROWN after edits fill in the remaining contributor files, as
// a project grows under an editor. Every contributor file exists from the
// start: a slot not yet contributing holds a declaration-free file of the
// same kind (a script, or a module), so only edits change the program.
// Each growth session has a baseline session answering GROWN alone; the
// retention proof (session-analyze.mjs) compares what the two engines retain
// after their last answer.
export const GROWTH_FIRST = 500;
export const GROWTH_GROWN = 2000;

const REPRODUCTIONS = {
  global: {
    probe: 'BenchRegistry[keyof BenchRegistry]["tag"]',
    prelude: "",
    fixed: {},
    file: (i) => `reg-${i}.ts`,
    contributor: (i) => `interface BenchRegistry { r${i}: { id: ${i}; tag: "t${i}" } }\n`,
    slot: "// an empty contributor slot\n",
    answer: (n) => union(range(n).map((i) => `"t${i}"`)),
    note: "a global interface merged from script contributors",
  },
  module: {
    probe: "Props[keyof Props]",
    prelude: 'import type { Props } from "./core";\n',
    fixed: { "core.ts": "export interface Props { base: -1 }\nexport {};\n" },
    file: (i) => `plugin-${i}.ts`,
    contributor: (i) => `declare module "./core" { interface Props { p${i}: ${i} } }\nexport {};\n`,
    slot: "export {};\n",
    answer: (n) => union(["-1", ...range(n).map(String)]),
    note: "a module interface augmented by modules the entry never imports",
  },
};

function augmentationSession(kind, first) {
  const r = REPRODUCTIONS[kind];
  const files = {
    "scenario.ts": `${r.prelude}type ${INIT_ALIAS} = 0;\ntype __Probe = ${r.probe};\nexport {};\n`,
    ...r.fixed,
  };
  for (const i of range(GROWTH_GROWN)) files[r.file(i)] = i < first ? r.contributor(i) : r.slot;
  const demand = (n) => ({
    kind: "demand",
    requests: [{ file: "scenario.ts", alias: "__Probe", expect: r.answer(n) }],
  });
  const baseline = `augment-${kind}-baseline`;
  if (first === GROWTH_GROWN)
    return {
      id: baseline,
      family: "augmentation",
      note: `${r.note}: ${GROWTH_GROWN} contributors answered once, the growth session's baseline`,
      files,
      initFile: "scenario.ts",
      steps: [demand(GROWTH_GROWN)],
    };
  return {
    id: `augment-${kind}-growth`,
    family: "augmentation",
    note: `${r.note}: answered at ${first} contributors, then at ${GROWTH_GROWN} after edits add the rest, one engine`,
    files,
    initFile: "scenario.ts",
    steps: [
      demand(first),
      ...range(GROWTH_GROWN)
        .slice(first)
        .map((i) => ({ kind: "edit", file: r.file(i), text: r.contributor(i) })),
      demand(GROWTH_GROWN),
    ],
    retentionProof: { baseline, first },
  };
}

function augmentationSessions() {
  return ["global", "module"].flatMap((kind) => [
    augmentationSession(kind, GROWTH_FIRST),
    augmentationSession(kind, GROWTH_GROWN),
  ]);
}

/** Every session, in catalog order. */
export function allSessions() {
  return [incremental(), editorSession(), concurrent(), ...augmentationSessions()];
}

/** Each session's tier (tiers nest as the scenarios' do). */
export const SESSION_TIERS = {
  "incremental-edits": "quick",
  "editor-session": "quick",
  "concurrent-demands": "quick",
  "augment-global-growth": "standard",
  "augment-global-baseline": "standard",
  "augment-module-growth": "standard",
  "augment-module-baseline": "standard",
};

/** The files tsc's program lists: every TypeScript file (a Vue SFC is Verter-only). */
export const tscFiles = (session) => Object.keys(session.files).filter((f) => f.endsWith(".ts"));

/** The tsconfig both arms read for a session (the strict setting). */
export function sessionTsconfigText(session) {
  const strict = SETTINGS.find((s) => s.id === "strict");
  return (
    JSON.stringify(
      {
        compilerOptions: {
          strict: true,
          strictNullChecks: strict.strictNullChecks,
          noImplicitAny: strict.noImplicitAny,
          noLib: true,
          noEmit: true,
          target: "es2022",
          module: "esnext",
          moduleResolution: "bundler",
          skipLibCheck: false,
          noErrorTruncation: true,
        },
        files: ["lib.bench.d.ts", ...tscFiles(session)],
      },
      null,
      2,
    ) + "\n"
  );
}

/** The script both probes run: the session's steps without their expected answers. */
export function sessionScript(session) {
  return session.steps.map((step) => {
    if (step.kind === "demand")
      return {
        kind: "demand",
        concurrent: Boolean(step.concurrent),
        requests: step.requests.map(({ file, alias }) => ({ file, alias })),
      };
    if (step.kind === "edit") return { kind: "edit", file: step.file, text: step.text };
    return { kind: "meta", file: step.file };
  });
}

/** Problems with a session's construction (the catalog checks itself). */
export function sessionProblems(session) {
  const problems = [];
  const files = new Set(Object.keys(session.files));
  if (!files.has(session.initFile))
    problems.push(`${session.id}: no init file ${session.initFile}`);
  else if (!session.files[session.initFile].includes(`type ${INIT_ALIAS} = 0;`))
    problems.push(`${session.id}: the init file does not declare ${INIT_ALIAS}`);
  const text = { ...session.files };
  session.steps.forEach((step, index) => {
    const at = `${session.id} step ${index}`;
    if (step.kind === "edit") {
      if (!files.has(step.file)) problems.push(`${at}: edits an unknown file ${step.file}`);
      else if (text[step.file] === step.text) problems.push(`${at}: an edit that changes nothing`);
      text[step.file] = step.text;
    } else if (step.kind === "demand") {
      if (!step.requests.length) problems.push(`${at}: a demand with no request`);
      for (const r of step.requests) {
        if (!r.file.endsWith(".ts"))
          problems.push(`${at}: ${r.file} is not a module both tools read`);
        const declared = (text[r.file] ?? "").split(`type ${r.alias} `).length - 1;
        if (declared !== 1)
          problems.push(`${at}: ${r.file} declares ${r.alias} ${declared} times at this step`);
        if (typeof r.expect !== "string")
          problems.push(`${at}: ${r.alias} has no constructed answer`);
      }
    } else if (step.kind === "meta") {
      if (!step.file.endsWith(".vue")) problems.push(`${at}: meta of a non-component ${step.file}`);
      if (!Array.isArray(step.expect?.props) || !Array.isArray(step.expect?.events))
        problems.push(`${at}: no constructed surface`);
    } else problems.push(`${at}: unknown step kind ${step.kind}`);
  });
  if (!SESSION_TIERS[session.id]) problems.push(`${session.id} has no tier`);
  const proof = session.retentionProof;
  if (proof) {
    if (!SESSION_TIERS[proof.baseline])
      problems.push(`${session.id}: no baseline session ${proof.baseline}`);
    const first = session.steps.findIndex((step) => step.kind === "demand");
    const last = session.steps.length - 1;
    if (first < 0 || first === last || session.steps[last].kind !== "demand")
      problems.push(`${session.id}: a retention proof needs a first and a last demand`);
  }
  return problems;
}

/** The sessions a tier runs (or the ones `only` selects by id or id prefix). */
export function sessionsFor(tier, tiers, only = []) {
  const all = allSessions();
  if (only.length) return all.filter((s) => only.some((p) => s.id === p || s.id.startsWith(p)));
  const rank = tiers.indexOf(tier);
  return all.filter((s) => tiers.indexOf(SESSION_TIERS[s.id]) <= rank);
}
