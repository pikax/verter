/**
 * Vue runtime-constructor prop resolution — the COMPAT half of the matrix.
 *
 * `crates/verter_session/tests/cases/runtime_constructor_matrix.rs` proves the
 * same cross on the native surface. The fixtures here carry the SAME sources as
 * that file's cells (which are inline string literals, so the two are the same
 * source text, not the same bytes). This file proves the native and compat
 * surfaces agree across the imported-class route, the full five-kind ×
 * four-form cross, and every invocation mode — including checker batch under a
 * session-only overlay — with negative controls still off the primitive fold.
 *
 * Semantic decisions here read `prop.type` — never `prop.rawType`, which is a
 * display-only passthrough (`CLAUDE.md` → Typed-IR-Only Resolver Rule).
 *
 * Every mode asserts BOTH halves, for the same reason the native file does:
 * per-mode exactness (a mode-dependent answer is a defect even when each mode
 * is individually plausible) AND cross-mode agreement (two modes can agree on a
 * uniformly wrong answer).
 */

import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import type { PropertyMeta } from "../src/compat/types.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";
import { openComponentMetaSession } from "../src/project.js";

const fixtureDir = join(__dirname, "fixtures");
const checkerOptions = {
  compilerOptions: { strict: true },
  include: ["**/*.vue", "**/*.ts"],
};

afterAll(() => {
  shutdownMetaRuntime();
});

type Checker = Awaited<ReturnType<typeof createCheckerByJson>>;

let sharedChecker: Promise<Checker> | undefined;

/** ONE checker for the whole file, so a second read is a genuine WARM read of
 *  the same session rather than a fresh cold session that happens to agree. A
 *  per-call checker cannot observe a warm-vs-cold divergence at all, which is
 *  the defect this axis exists to catch. */
function checker(): Promise<Checker> {
  sharedChecker ??= createCheckerByJson(fixtureDir, checkerOptions);
  return sharedChecker;
}

// ─────────────────────────────────────────────────────────────────────────
// The matrix
// ─────────────────────────────────────────────────────────────────────────

/** What one cell requires of ONE published prop's rendered type. */
type Expect =
  /** A primitive-fold rendering: this exact string, AND provably fold-shaped. */
  | { fold: string }
  /**
   * NEGATIVE CONTROL. Three INDEPENDENT properties, and the second and third
   * are what make this a control rather than a tautology:
   *
   * 1. The position still publishes a type at all — "not a fold" is otherwise
   *    satisfied by publishing nothing.
   * 2. It is NOT fold-shaped. This is the fold-capture guard.
   * 3. It is EXACTLY this rendering. Without the pin, a control stays green
   *    while degrading into a DIFFERENT non-fold shape — `"unknown | undefined"`
   *    passes (1) and (2) while carrying no meaning at all.
   */
  | { notFolded: string };

interface PropExpect {
  name: string;
  expect: Expect;
  required: boolean;
  default?: string;
}

interface Cell {
  file: string;
  props: PropExpect[];
}

const fold = (name: string, type: string, required = false, def?: string): PropExpect => ({
  name,
  expect: { fold: type },
  required,
  ...(def === undefined ? {} : { default: def }),
});

const notFolded = (name: string, type: string, required = false, def?: string): PropExpect => ({
  name,
  expect: { notFolded: type },
  required,
  ...(def === undefined ? {} : { default: def }),
});

/**
 * The four primitive spellings the runtime-constructor fold can produce. A
 * rendering built only from them (alone or as a union) is fold-shaped — the
 * exact shape a negative control must never reach.
 */
const FOLDABLE = ["string", "number", "boolean", "null"];

function isPrimitiveFoldShape(type: string): boolean {
  const arms = type
    .split("|")
    .map((a) => a.trim())
    .filter((a) => a !== "" && a !== "undefined");
  return arms.length > 0 && arms.every((a) => FOLDABLE.includes(a));
}

const CELLS: Cell[] = [
  {
    file: "RcShorthand.vue",
    props: [
      fold("label", "string | undefined"),
      fold("count", "number | undefined"),
      fold("flag", "boolean | undefined"),
    ],
  },
  {
    file: "RcExpanded.vue",
    props: [
      fold("label", "string | undefined"),
      fold("count", "number", true),
      fold("flag", "boolean | undefined"),
      fold("size", "string | undefined", false, '"md"'),
    ],
  },
  {
    file: "RcArrays.vue",
    props: [
      fold("value", "string | number | undefined"),
      fold("maybe", "string | null | undefined"),
      fold("nn", "number | null | undefined"),
      fold("three", "string | number | boolean | undefined"),
    ],
  },
  {
    file: "RcNonPrimitive.vue",
    props: [
      notFolded("list", "unknown | undefined"),
      notFolded("obj", "unknown | undefined"),
      notFolded("fn", "unknown | undefined"),
      notFolded("sym", "unknown | undefined"),
      notFolded("when", "unknown | undefined"),
      // Required, so no `| undefined` arm — the optionality axis crosses the
      // display-text-only route too.
      notFolded("re", "unknown", true),
      notFolded("p", "unknown | undefined"),
    ],
  },
  {
    file: "RcMixedArray.vue",
    props: [notFolded("mixed", "unknown | undefined"), fold("pure", "string | number | undefined")],
  },
  {
    file: "RcShadowModule.vue",
    props: [
      // The shadowing local's OWN resolved shape — not `unknown`. This is what
      // makes the shadow cell a real control: a gate regression publishes
      // `string`, and a resolution regression publishes `unknown`, and the pin
      // rejects both.
      notFolded("label", "{ from: number } | undefined"),
      fold("count", "number | undefined"),
    ],
  },
  {
    file: "RcImported.vue",
    props: [
      // Imported values resolve CROSS-FILE to their own shapes. `String` is a
      // recognised constructor spelling and `Shadow` is not; both must publish
      // the imported value, so the cell discriminates binding ORIGIN from
      // constructor-NAME classification.
      notFolded("label", "{ from: number } | undefined"),
      notFolded("thing", "{ via: number } | undefined"),
      fold("count", "number | undefined"),
    ],
  },
  {
    // Through a BARREL that only re-exports: the direct hop lands on a file
    // that declares nothing, so only walking the export route reaches the
    // real declaration.
    file: "RcImportedBarrel.vue",
    props: [
      notFolded("label", "{ from: number } | undefined"),
      notFolded("thing", "{ via: number } | undefined"),
      fold("count", "number | undefined"),
    ],
  },
  {
    file: "RcImportedOptions.vue",
    props: [
      notFolded("label", "{ from: number } | undefined"),
      notFolded("thing", "{ via: number } | undefined"),
      fold("count", "number | undefined"),
    ],
  },
  {
    file: "RcNegatives.vue",
    props: [
      // Deferred forms are captured in `runtime_constructor_matrix.rs` by
      // `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes` and
      // `script_setup_local_class_publishes_the_class_constructor_shape`.
      notFolded("thing", "{ new (): Thing } | undefined"),
      fold("label", "string | undefined"),
    ],
  },
  {
    // The same class control reached by IMPORT, which is the other route that
    // resolves the authored value to its real constructor shape.
    file: "RcImportedClass.vue",
    props: [
      notFolded("thing", "{ new (): Thing } | undefined"),
      fold("label", "string | undefined"),
    ],
  },
  // ─────────────────────────────────────────────────────────────────
  // The literal runtime-form × constructor-kind cross.
  //
  // FORMS (rows): shorthand `k: C`; expanded `{ type: C }`; required
  // `{ type: C, required: true }`; with-default `{ type: C, default: v }`.
  // KINDS (columns): `String`/`Number`/`Boolean`; `Date`, representing the
  // display-text-only route; and a module-declared custom class, representing
  // the authored-value route. Five kinds × four forms = twenty positions.
  // ─────────────────────────────────────────────────────────────────
  {
    file: "RcCrossShorthand.vue",
    props: [
      fold("s", "string | undefined"),
      fold("n", "number | undefined"),
      fold("b", "boolean | undefined"),
      notFolded("d", "unknown | undefined"),
      notFolded("k", "{ new (): Thing } | undefined"),
    ],
  },
  {
    file: "RcCrossExpanded.vue",
    props: [
      fold("s", "string | undefined"),
      fold("n", "number | undefined"),
      fold("b", "boolean | undefined"),
      notFolded("d", "unknown | undefined"),
      notFolded("k", "{ new (): Thing } | undefined"),
    ],
  },
  {
    file: "RcCrossRequired.vue",
    props: [
      fold("s", "string", true),
      fold("n", "number", true),
      fold("b", "boolean", true),
      notFolded("d", "unknown", true),
      notFolded("k", "{ new (): Thing }", true),
    ],
  },
  {
    file: "RcCrossDefault.vue",
    props: [
      fold("s", "string | undefined", false, '"md"'),
      fold("n", "number | undefined", false, "0"),
      fold("b", "boolean | undefined", false, "false"),
      notFolded("d", "unknown | undefined", false, "() => new Date()"),
      notFolded("k", "{ new (): Thing } | undefined", false, "() => new Thing()"),
    ],
  },
  {
    file: "RcOptions.vue",
    props: [
      fold("label", "string | undefined"),
      fold("count", "number", true),
      fold("flag", "boolean | undefined"),
      fold("value", "string | number | undefined"),
      fold("maybe", "string | null | undefined"),
      fold("size", "string | undefined", false, '"md"'),
      notFolded("when", "unknown | undefined"),
      notFolded("mixed", "unknown | undefined"),
      // Deferred forms are captured in `runtime_constructor_matrix.rs` by
      // `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes` and
      // `script_setup_local_class_publishes_the_class_constructor_shape`.
      notFolded("thing", "{ new (): Thing } | undefined"),
    ],
  },
  {
    file: "RcShadowOptions.vue",
    props: [
      notFolded("count", "{ parse: number } | undefined"),
      fold("label", "string | undefined"),
    ],
  },
];

const FILES = CELLS.map((c) => c.file);

// ─────────────────────────────────────────────────────────────────────────
// The single assertion authority
// ─────────────────────────────────────────────────────────────────────────

/** The facts this matrix compares across modes. */
function shape(props: PropertyMeta[]): [string, string, boolean, unknown][] {
  return props.map((p) => [p.name, p.type, p.required, p.default]);
}

/**
 * Assert one cell's published props satisfy its expectations EXACTLY.
 *
 * The ONLY place a compat expectation is checked. Every mode routes through it
 * so no mode is held to a weaker contract than another by accident.
 */
function assertCell(props: PropertyMeta[], cell: Cell, mode: string): void {
  expect(
    props.length,
    `${cell.file} [${mode}]: published prop count must match the authored one — ` +
      `an extra or dropped prop is a surface defect; published ${JSON.stringify(
        props.map((p) => p.name),
      )}`,
  ).toBe(cell.props.length);

  for (const want of cell.props) {
    const p = props.find((q) => q.name === want.name);
    expect(
      p,
      `${cell.file} [${mode}]: prop \`${want.name}\` must be published, never ` +
        `silently omitted; published ${JSON.stringify(props.map((q) => q.name))}`,
    ).toBeDefined();
    if (!p) continue;

    if ("fold" in want.expect) {
      expect(
        p.type,
        `${cell.file} [${mode}]: prop \`${want.name}\` must render exactly the ` +
          `primitive the runtime constructor folds to`,
      ).toBe(want.expect.fold);
      expect(
        isPrimitiveFoldShape(p.type),
        `${cell.file} [${mode}]: prop \`${want.name}\` is a POSITIVE cell and must ` +
          `be fold-shaped, got ${JSON.stringify(p.type)}`,
      ).toBe(true);
    } else {
      // (1) still publishes something
      expect(
        typeof p.type === "string" && p.type.length > 0,
        `${cell.file} [${mode}]: prop \`${want.name}\` is a NEGATIVE CONTROL and must ` +
          `still publish a type through its own route, got ${JSON.stringify(p.type)}`,
      ).toBe(true);
      // (2) not captured by the fold
      expect(
        isPrimitiveFoldShape(p.type),
        `${cell.file} [${mode}]: prop \`${want.name}\` is a NEGATIVE CONTROL — it ` +
          `carries no closed primitive fact and must stay on its own route. The ` +
          `runtime-constructor fold captured it and published a bare primitive ` +
          `shape: ${JSON.stringify(p.type)}`,
      ).toBe(false);
      // (3) exactly the pinned shape
      expect(
        p.type,
        `${cell.file} [${mode}]: prop \`${want.name}\` is a NEGATIVE CONTROL whose ` +
          `rendering is pinned. It is neither folded nor absent, but it is no longer ` +
          `the shape its route produces. If this change is intended, update the pin ` +
          `and say why`,
      ).toBe(want.expect.notFolded);
    }

    expect(
      p.required,
      `${cell.file} [${mode}]: prop \`${want.name}\` optionality must be exactly as ` +
        `authored — required is computed independently of the type fold and must ` +
        `not drift with it`,
    ).toBe(want.required);
    expect(
      p.default,
      `${cell.file} [${mode}]: prop \`${want.name}\` default presence must be exactly ` +
        `as authored`,
    ).toBe(want.default);
  }
}

async function propsFrom(c: Checker, file: string): Promise<PropertyMeta[]> {
  return (await c.getComponentMeta(join(fixtureDir, file))).props;
}

describe("runtime-constructor matrix — compat surface", () => {
  // Correction owner: the maintainer's post-plan type-correction work, per
  // `MAINTAINER-RULING-BUGS-AND-TYPES` rule 3.
  // Skip reason: captured authored runtime-prop `as` assertion defect: its
  // payload is discarded when the props normalizer selects the runtime object
  // member's closed `Unknown` leaf; deferred to that correction owner.
  // This is the same macro-route defect class as
  // `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes`, a
  // second spelling of one loss point, and not the declaration-site defect in
  // `script_setup_local_class_publishes_the_class_constructor_shape`.
  // Native capture: `runtime_prop_as_function_assertion_publishes_its_object_shape`.
  test.skip("runtime_prop_as_function_assertion_publishes_its_object_shape", async () => {
    const c = await checker();
    const props = await propsFrom(c, "RcMixedRuntimeTyped.vue");
    const controlProps = await propsFrom(c, "MemberValueProps.vue");
    const publishedType = (name: string): string | undefined =>
      props.find((prop) => prop.name === name)?.type;

    expect(
      publishedType("label"),
      "soundness anchor: `label: String` must resolve before the authored `as` defect",
    ).toBe("string | undefined");
    expect(
      publishedType("count"),
      "soundness anchor: `count: Number` must resolve before the authored `as` defect",
    ).toBe("number | undefined");
    expect(
      publishedType("flag"),
      "soundness anchor: `flag: Boolean` must resolve before the authored `as` defect",
    ).toBe("boolean | undefined");
    expect(
      controlProps.find((prop) => prop.name === "config")?.type,
      "soundness anchor: the type-declared control must publish an object rendering before the authored `as` defect",
    ).toBe("{ nested: number }");

    expect(
      publishedType("item"),
      "authored runtime-prop `as` assertion must publish the asserted object shape",
    ).toBe("{ id: number } | undefined");
  }, 300000);

  test("cold: every cell publishes its exact type in a session that has never seen it", async () => {
    for (const cell of CELLS) {
      const fresh = await createCheckerByJson(fixtureDir, checkerOptions);
      assertCell(await propsFrom(fresh, cell.file), cell, "cold");
    }
  }, 300000);

  test("warm: a second read of the SAME session is exact and identical to the first", async () => {
    const c = await checker();
    for (const cell of CELLS) {
      const first = await propsFrom(c, cell.file);
      const second = await propsFrom(c, cell.file);
      assertCell(second, cell, "warm");
      expect(shape(second), `${cell.file}: warm must equal the first read`).toEqual(shape(first));
    }
  }, 300000);

  test("concurrent: Promise.all resolution is exact and identical to sequential", async () => {
    // This is the CONCURRENCY axis, not batch. Batch has its own test below,
    // against the real batch surface.
    const c = await checker();
    const sequential: PropertyMeta[][] = [];
    for (const cell of CELLS) sequential.push(await propsFrom(c, cell.file));
    const concurrent = await Promise.all(CELLS.map((cell) => propsFrom(c, cell.file)));
    for (let i = 0; i < CELLS.length; i++) {
      assertCell(concurrent[i], CELLS[i], "concurrent");
      expect(
        shape(concurrent[i]),
        `${CELLS[i].file}: concurrent resolution must publish the identical surface`,
      ).toEqual(shape(sequential[i]));
    }
  }, 300000);

  test("batch/checker surface: every cell is exact and agrees with checker scalar", async () => {
    const c = await checker();
    const batched = await c.getComponentMetaBatch(FILES.map((f) => join(fixtureDir, f)));
    expect(batched.length, "one checker slot per input, in input order").toBe(CELLS.length);
    const scalar = await Promise.all(FILES.map((f) => c.getComponentMeta(join(fixtureDir, f))));
    for (let i = 0; i < CELLS.length; i++) {
      const batchProps = batched[i].props;
      const scalarProps = scalar[i].props;
      assertCell(batchProps, CELLS[i], "batch/checker");
      expect(
        shape(batchProps),
        `${CELLS[i].file}: checker batch must publish the identical surface as checker scalar`,
      ).toEqual(shape(scalarProps));
    }
  }, 300000);

  test("batch/session surface: every cell is exact and agrees with session scalar", async () => {
    // `ComponentMetaSession.getComponentMetaBatch` is the genuine batch entry —
    // one shared overlay view and a single native scheduler dispatch. This test
    // independently covers the public session projection surface.
    const session = await openComponentMetaSession(
      { root: fixtureDir, config: checkerOptions },
      checkerOptions,
    );
    try {
      const batched = await session.getComponentMetaBatch(FILES.map((f) => join(fixtureDir, f)));
      expect(batched.length, "one slot per input, in input order").toBe(CELLS.length);
      const scalar = await Promise.all(
        FILES.map((f) => session.getComponentMeta(join(fixtureDir, f))),
      );
      for (let i = 0; i < CELLS.length; i++) {
        // Both surfaces already return fully-projected `VolarComponentMeta`.
        const batchProps = batched[i].props;
        const scalarProps = scalar[i].props;
        assertCell(batchProps, CELLS[i], "batch/session");
        expect(
          shape(batchProps),
          `${CELLS[i].file}: the batch surface must publish the identical surface as scalar`,
        ).toEqual(shape(scalarProps));
      }
    } finally {
      session.close();
    }
  }, 300000);

  test("overlay: the overlay lane runs the full invocation cross", async () => {
    // The overlay lane is exercised cold, warm, concurrently AND in batch —
    // the same cross the base lane runs, because a result that is correct in
    // the base store but wrong (or mode-dependent) under an overlay request
    // view is invisible to a base-only cross. `ComponentMetaSession` carries
    // both halves: `updateFile` publishes a session-local overlay, and
    // `getComponentMetaBatch` is the real batch entry.
    const { readFileSync } = await import("fs");
    const session = await openComponentMetaSession(
      { root: fixtureDir, config: checkerOptions },
      checkerOptions,
    );
    try {
      // The base answer every overlay mode must reproduce.
      const base = await Promise.all(
        FILES.map(async (f) => (await session.getComponentMeta(join(fixtureDir, f))).props),
      );

      // Overlay paths the base project has never contained. They stay in the
      // fixture directory so each cell's relative imports resolve to the same
      // sibling modules the base lane reads.
      const overlayPaths = FILES.map((f) => join(fixtureDir, `RcOverlay-${f}`));
      for (let i = 0; i < FILES.length; i++) {
        session.updateFile(overlayPaths[i], readFileSync(join(fixtureDir, FILES[i]), "utf8"));
      }

      const cold: PropertyMeta[][] = [];
      for (const p of overlayPaths) cold.push((await session.getComponentMeta(p)).props);
      const warm: PropertyMeta[][] = [];
      for (const p of overlayPaths) warm.push((await session.getComponentMeta(p)).props);
      const concurrent = await Promise.all(
        overlayPaths.map(async (p) => (await session.getComponentMeta(p)).props),
      );
      const batched = await session.getComponentMetaBatch(overlayPaths);
      expect(batched.length, "one overlay slot per input, in input order").toBe(CELLS.length);

      for (let i = 0; i < CELLS.length; i++) {
        for (const [mode, observed] of [
          ["overlay/cold", cold[i]],
          ["overlay/warm", warm[i]],
          ["overlay/concurrent", concurrent[i]],
          ["overlay/batch", batched[i].props],
        ] as const) {
          // Per-mode exactness...
          assertCell(observed, CELLS[i], mode);
          // ...AND agreement with the base lane. Both, because either alone
          // can hold while the other fails.
          expect(
            shape(observed),
            `${CELLS[i].file}: the ${mode} surface must be IDENTICAL to the ` +
              `base-scoped surface — a request-view-dependent answer is a defect ` +
              `even when each view is individually plausible`,
          ).toEqual(shape(base[i]));
        }
      }
    } finally {
      session.close();
    }
  }, 300000);

  test("overlay/checker surface: checker batch shares the full overlay invocation cross", async () => {
    const { readFileSync } = await import("fs");
    // This checker is dedicated: checker overlays are session state, so using
    // the file-shared checker would leak these synthetic files into other tests.
    const c = await createCheckerByJson(fixtureDir, checkerOptions);
    try {
      const base = await Promise.all(
        FILES.map(async (f) => (await c.getComponentMeta(join(fixtureDir, f))).props),
      );
      const overlayPaths = FILES.map((f) => join(fixtureDir, `RcCheckerOverlay-${f}`));
      for (let i = 0; i < FILES.length; i++) {
        c.updateFile(overlayPaths[i], readFileSync(join(fixtureDir, FILES[i]), "utf8"));
      }

      const cold: PropertyMeta[][] = [];
      for (const p of overlayPaths) cold.push((await c.getComponentMeta(p)).props);
      const warm: PropertyMeta[][] = [];
      for (const p of overlayPaths) warm.push((await c.getComponentMeta(p)).props);
      const concurrent = await Promise.all(
        overlayPaths.map(async (p) => (await c.getComponentMeta(p)).props),
      );
      const batched = await c.getComponentMetaBatch(overlayPaths);
      expect(batched.length, "one checker-overlay slot per input, in input order").toBe(
        CELLS.length,
      );

      for (let i = 0; i < CELLS.length; i++) {
        for (const [mode, observed] of [
          ["overlay/checker/cold", cold[i]],
          ["overlay/checker/warm", warm[i]],
          ["overlay/checker/concurrent", concurrent[i]],
          ["overlay/checker/batch", batched[i].props],
        ] as const) {
          assertCell(observed, CELLS[i], mode);
          expect(
            shape(observed),
            `${CELLS[i].file}: the ${mode} surface must be IDENTICAL to the ` +
              `checker's own base-scoped answer`,
          ).toEqual(shape(base[i]));
        }
      }
    } finally {
      c.close();
    }
  }, 300000);
});
