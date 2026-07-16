/**
 * Vue macros, control-flow locals, and generic SFC surface.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Vue macros and control flow [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyVueParity(this);
    await ensureParityReady("src/App.vue");
  });

  test("vue.macro.defineProps.hover", async function () {
    onlyVueParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/components/PropChild.vue", token: "defineProps", occurrence: 0 },
        ["contractProp"],
      );
    } catch (err) {
      // Hover on the macro call itself is valuable but some providers only type the type arg.
      try {
        await assertHoverNeedles(
          { file: "src/components/PropChild.vue", token: "contractProp", occurrence: 0 },
          ["string"],
        );
      } catch (inner) {
        failParityGap(
          this,
          "vue.macro.defineProps.hover",
          "ISSUE-vue-defineProps-hover",
          `defineProps / prop field hover not typed: ${String(err)}; fallback: ${String(inner)}`,
        );
      }
    }
  });

  test("vue.macro.defineEmits.event-attr", async function () {
    onlyVueParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/macros/MacroSurface.vue", token: "pick", occurrence: 0 },
        ["string"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.macro.defineEmits.event-attr",
        "ISSUE-vue-defineEmits-event-hover",
        `Event attribute hover did not expose payload typing: ${String(err)}`,
      );
    }
  });

  test("vue.macro.defineModel.named", async function () {
    onlyVueParity(this);
    try {
      await assertDefinitionTargetsToken(
        { file: "src/macros/MacroSurface.vue", token: "open", occurrence: 0 },
        { file: "src/macros/ModelChild.vue", token: "open", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.macro.defineModel.named",
        "ISSUE-vue-defineModel-navigation",
        `Named v-model:open did not navigate to defineModel declaration: ${String(err)}`,
      );
    }
  });

  test("vue.macro.defineSlots.slot-local", async function () {
    onlyVueParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/macros/MacroSurface.vue", token: "slotItem", occurrence: 1 },
        ["name", "string"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.macro.defineSlots.slot-local",
        "ISSUE-vue-defineSlots-locals",
        `v-slot local hover not typed from defineSlots: ${String(err)}`,
      );
    }
  });

  test("vue.macro.withDefaults.optional-prop", async function () {
    onlyVueParity(this);
    try {
      await assertCleanErrors("src/macros/MacroSurface.vue");
      await assertHoverNeedles(
        { file: "src/macros/WithDefaultsChild.vue", token: "size", occurrence: 0 },
        ["number"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.macro.withDefaults.optional-prop",
        "ISSUE-vue-withDefaults",
        `withDefaults optional prop surface incomplete: ${String(err)}`,
      );
    }
  });

  test("vue.control.v-for-item-hover", async function () {
    onlyVueParity(this);
    try {
      // Hover may show `(parameter) user: User` or member access types.
      await assertHoverNeedles(
        { file: "src/control/ControlFlow.vue", token: "user", occurrence: 2 },
        ["User"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.control.v-for-item-hover",
        "ISSUE-vue-vfor-hover",
        `v-for item hover not typed: ${String(err)}`,
      );
    }
  });

  test("vue.control.v-if-narrowing", async function () {
    onlyVueParity(this);
    try {
      // Inside v-if="selected", hover on selected.name (member) is the strong case;
      // fall back to selected itself being present and not bare any.
      const text = await assertHoverNeedles(
        { file: "src/control/ControlFlow.vue", token: "selected", occurrence: 3 },
        ["name"],
      );
      if (/\bnull\b/.test(text) && !/User|string/.test(text)) {
        throw new Error(`narrowing may have failed: ${text}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.control.v-if-narrowing",
        "ISSUE-vue-vif-narrowing",
        `v-if narrowing hover incomplete: ${String(err)}`,
      );
    }
  });

  test("vue.generic.prop-type-param", async function () {
    onlyVueParity(this);
    try {
      await assertCleanErrors("src/generics/GenericConsumer.vue");
      await assertHoverNeedles(
        { file: "src/generics/GenericConsumer.vue", token: "items", occurrence: 0 },
        ["Row"],
        { forbidAny: true },
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.generic.prop-type-param",
        "ISSUE-vue-generic-sfc",
        `generic SFC prop inference incomplete: ${String(err)}`,
      );
    }
  });
});
