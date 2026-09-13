// Runtime directives keep following their values after mount, not just on
// the first render.
//
// `v-show` and the native choice-input `v-model` family (checkbox array,
// radio, select) apply their value from directive hooks the patcher runs
// only when a re-render revisits the element. The compiled output decides
// that: an otherwise-static element enters its block's dynamic children only
// through its patch flag (`512 /* NEED_PATCH */`). Without it the failure is
// silent — the mount is right, then the element stops following its value
// (a hidden element never reappears, a checkbox computes its next array from
// the mount-time value). These cases mount compiled output through the
// pinned official runtime and drive real updates: repeated transitions, a
// re-render that leaves the bound values unchanged, and model→DOM as well as
// DOM→model changes, reading visibility, form state, DOM node identity and
// writes per step.
//
// These cases execute OFFICIAL output: this hermetic package has no Verter
// binding. They pin the reference behaviour and prove, through the negative
// controls, that the executor discriminates the missing-flag class. Verter's
// own emission of the flag for these element kinds is pinned by the vdom
// codegen tests in `crates/verter_compiler/src/template/code_gen/vdom/`.

import { afterAll, describe, expect, it } from "vitest";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { cleanupScratch, executeVueClientInteractions } from "../src/execute-vue-runtime.mjs";

const V_SHOW_FIXTURE = `
<script setup>
import { ref } from "vue";

const visible = ref(true);
const other = ref(0);

function toggle() {
  visible.value = !visible.value;
}

function bump() {
  other.value++;
}
</script>

<template>
  <div class="v-show-toggle">
    <span v-show="visible" data-testid="shown">body</span>
    <button type="button" data-testid="toggle" @click="toggle">t</button>
    <button type="button" data-testid="bump" @click="bump">{{ other }}</button>
  </div>
</template>
`;

// Every model routes its writes through a counting setter, so a step that
// must not write (an already-selected control, a model-driven update) is
// observable in the rendered text rather than assumed.
const CHOICE_FIXTURE = `
<script setup>
import { computed, ref } from "vue";

const pickState = ref(["b"]);
const toneState = ref("low");
const cityState = ref("york");
const pickWrites = ref(0);
const toneWrites = ref(0);
const cityWrites = ref(0);

const picks = computed({
  get: () => pickState.value,
  set: (value) => {
    pickWrites.value++;
    pickState.value = value;
  },
});
const tone = computed({
  get: () => toneState.value,
  set: (value) => {
    toneWrites.value++;
    toneState.value = value;
  },
});
const city = computed({
  get: () => cityState.value,
  set: (value) => {
    cityWrites.value++;
    cityState.value = value;
  },
});

function reset() {
  pickState.value = ["a"];
  toneState.value = "high";
  cityState.value = "leeds";
}
</script>

<template>
  <div class="v-model-choice">
    <input type="checkbox" value="a" v-model="picks" data-testid="check-a" />
    <input type="checkbox" value="b" v-model="picks" data-testid="check-b" />
    <span data-testid="picks">{{ picks.slice().sort().join(",") }}|{{ pickWrites }}</span>

    <input type="radio" value="low" v-model="tone" data-testid="radio-low" />
    <input type="radio" value="high" v-model="tone" data-testid="radio-high" />
    <span data-testid="tone">{{ tone }}|{{ toneWrites }}</span>

    <select v-model="city" data-testid="select">
      <option value="york">york</option>
      <option value="leeds">leeds</option>
    </select>
    <span data-testid="city">{{ city }}|{{ cityWrites }}</span>

    <button type="button" data-testid="reset" @click="reset">reset</button>
  </div>
</template>
`;

const SHOWN = "[data-testid=shown]";
const V_SHOW_ACTIONS = [
  { kind: "click", target: "[data-testid=toggle]" }, // hide
  { kind: "click", target: "[data-testid=toggle]" }, // show again
  { kind: "click", target: "[data-testid=toggle]" }, // hide again
  { kind: "click", target: "[data-testid=bump]" }, // unrelated re-render while hidden
  { kind: "click", target: "[data-testid=toggle]" }, // show
  { kind: "click", target: "[data-testid=bump]" }, // unrelated re-render while shown
];

const CHECK_A = "[data-testid=check-a]";
const CHECK_B = "[data-testid=check-b]";
const RADIO_LOW = "[data-testid=radio-low]";
const RADIO_HIGH = "[data-testid=radio-high]";
const SELECT = "[data-testid=select]";
const CONTROLS = [CHECK_A, CHECK_B, RADIO_LOW, RADIO_HIGH, SELECT];
const READOUTS = ["[data-testid=picks]", "[data-testid=tone]", "[data-testid=city]"];
const CHOICE_ACTIONS = [
  { kind: "click", target: CHECK_A }, // 1: add a
  { kind: "click", target: CHECK_B }, // 2: remove b — computed from the CURRENT array
  { kind: "click", target: CHECK_A }, // 3: remove a
  { kind: "click", target: RADIO_HIGH }, // 4: low → high
  { kind: "click", target: RADIO_HIGH }, // 5: already selected — no change, no write
  { kind: "click", target: RADIO_LOW }, // 6: high → low
  { kind: "select", target: SELECT, value: "leeds" }, // 7
  { kind: "select", target: SELECT, value: "york" }, // 8
  { kind: "click", target: "[data-testid=reset]" }, // 9: model → DOM, no writes
  { kind: "click", target: CHECK_B }, // 10: add b to the model-driven array
];

function compileClient(fixture, id) {
  const result = compileVueFixture(fixture, id, {
    backend: "vdom",
    sourceMap: false,
    isProd: false,
  });
  expect(result.diagnostics).toEqual([]);
  expect(result.code).toBeTypeOf("string");
  return result.code;
}

/**
 * Removes every `512 /* NEED_PATCH *\/` argument from `code` after proving
 * the plant applies exactly `expected` times: a mutation that missed would
 * make a negative control pass while proving nothing.
 */
function stripNeedPatch(code, expected) {
  const flag = ", 512 /* NEED_PATCH */";
  expect(code.split(flag).length - 1).toBe(expected);
  const stripped = code.split(flag).join("");
  expect(stripped).not.toBe(code);
  expect(stripped.includes("NEED_PATCH")).toBe(false);
  return stripped;
}

async function run(code, observe, actions) {
  const result = await executeVueClientInteractions(code, { observe, actions });
  expect(result.error).toBeNull();
  expect(result.ok).toBe(true);
  expect(result.warnings).toEqual([]);
  expect(result.steps.length).toBe(actions.length + 1);
  return result.steps;
}

/** One row per step: `[checkA, checkB, radioLow, radioHigh, select, picks, tone, city]`. */
function choiceRows(steps) {
  return steps.map((step) => [
    ...CONTROLS.map((selector) =>
      selector === SELECT ? step.elements[selector].value : step.elements[selector].checked,
    ),
    ...READOUTS.map((selector) => step.elements[selector].text),
  ]);
}

// The scratch directory is shared by every case in this worker, so cleanup is
// file-scoped: a case that removed it on its way out could delete a module
// another case was still importing.
afterAll(() => {
  cleanupScratch();
});

describe("runtime directive updates through the pinned official runtime", () => {
  it("v-show follows every toggle and writes nothing when the value is unchanged", async () => {
    const code = compileClient(V_SHOW_FIXTURE, "vshow1");
    const steps = await run(code, [SHOWN], V_SHOW_ACTIONS);
    const shown = steps.map((step) => step.elements[SHOWN]);

    expect(shown.map((element) => element.display)).toEqual([
      "",
      "none",
      "",
      "none",
      "none",
      "",
      "",
    ]);
    // The element stays in the DOM and is never replaced.
    expect(new Set(shown.map((element) => element.node)).size).toBe(1);
    expect(shown.every((element) => element.text === "body")).toBe(true);
    // One style write per real transition; the re-renders that leave the
    // bound value unchanged (steps 4 and 6) write nothing.
    expect(shown.map((element) => element.attributeWrites)).toEqual([0, 1, 1, 1, 0, 1, 0]);
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention

  it("v-show stops following its value once its element loses the update flag (negative control)", async () => {
    const code = stripNeedPatch(compileClient(V_SHOW_FIXTURE, "vshow1"), 1);
    const steps = await run(code, [SHOWN], V_SHOW_ACTIONS);

    // The mount still hides nothing, and then no toggle ever reaches it.
    expect(steps.map((step) => step.elements[SHOWN].display)).toEqual(["", "", "", "", "", "", ""]);
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention

  it("checkbox, radio and select models follow repeated changes in both directions", async () => {
    const code = compileClient(CHOICE_FIXTURE, "vmodelchoice1");
    const steps = await run(code, [...CONTROLS, ...READOUTS], CHOICE_ACTIONS);

    expect(choiceRows(steps)).toEqual([
      [false, true, true, false, "york", "b|0", "low|0", "york|0"],
      [true, true, true, false, "york", "a,b|1", "low|0", "york|0"],
      [true, false, true, false, "york", "a|2", "low|0", "york|0"],
      [false, false, true, false, "york", "|3", "low|0", "york|0"],
      [false, false, false, true, "york", "|3", "high|1", "york|0"],
      [false, false, false, true, "york", "|3", "high|1", "york|0"],
      [false, false, true, false, "york", "|3", "low|2", "york|0"],
      [false, false, true, false, "leeds", "|3", "low|2", "leeds|1"],
      [false, false, true, false, "york", "|3", "low|2", "york|2"],
      [true, false, false, true, "leeds", "a|3", "high|2", "leeds|2"],
      [true, true, false, true, "leeds", "a,b|4", "high|2", "leeds|2"],
    ]);
    for (const selector of CONTROLS) {
      const observed = steps.map((step) => step.elements[selector]);
      // Form state is live element state: every control keeps its node and
      // no update rewrites its attributes.
      expect(new Set(observed.map((element) => element.node)).size).toBe(1);
      expect(observed.map((element) => element.attributeWrites)).toEqual(
        Array(steps.length).fill(0),
      );
    }
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention

  it("choice models go stale once their elements lose the update flag (negative control)", async () => {
    const code = stripNeedPatch(compileClient(CHOICE_FIXTURE, "vmodelchoice1"), 5);
    const steps = await run(code, [...CONTROLS, ...READOUTS], CHOICE_ACTIONS);
    const rows = choiceRows(steps);

    // The first change still lands (the mount-time state is current)...
    expect(rows[1]).toEqual([true, true, true, false, "york", "a,b|1", "low|0", "york|0"]);
    // ...but the second computes from the mount-time array and drops `a`,
    expect(rows[2][5]).toBe("|2");
    // a newly selected radio never unchecks its sibling,
    expect(rows[4].slice(2, 4)).toEqual([true, true]);
    // and a model-driven change never reaches the controls.
    expect(rows[9].slice(0, 5)).toEqual(rows[8].slice(0, 5));
  }, 120_000); // jsdom + pinned-runtime import + a real mount, under parallel-worker contention
});
