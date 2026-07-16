/**
 * Vue extended surfaces: native v-model, dynamic binds, expose, options API, dual script.
 */
import { FIXTURE_NAME, waitForDiagnosticsSettled } from "../../../helpers";
import {
  assertCleanErrors,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  ensureParityReady,
  openRelative,
  failParityGap,
  verterUnknownPropDiags,
} from "../../../lib/parityHarness";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Vue extended features [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyVueParity(this);
    await ensureParityReady("src/App.vue");
  });

  test("vue.feature.native-v-model.definition", async function () {
    onlyVueParity(this);
    try {
      await assertDefinitionTargetsToken(
        { file: "src/features/NativeModel.vue", token: "inputVal", occurrence: 1 },
        { file: "src/features/NativeModel.vue", token: "inputVal", occurrence: 0 },
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.native-v-model.definition",
        "ISSUE-vue-native-vmodel-definition",
        `Native v-model binding definition failed: ${String(err)}`,
      );
    }
  });

  test("vue.feature.native-v-model.hover", async function () {
    onlyVueParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/features/NativeModel.vue", token: "inputVal", occurrence: 1 },
        ["inputVal"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.native-v-model.hover",
        "ISSUE-vue-native-vmodel-hover",
        `Native v-model hover failed: ${String(err)}`,
      );
    }
  });

  test("vue.feature.dynamic-bind.clean", async function () {
    onlyVueParity(this);
    try {
      await assertCleanErrors("src/features/DynamicBind.vue");
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.dynamic-bind.clean",
        "ISSUE-vue-dynamic-bind",
        `Dynamic :[attr] / @[event] surface not clean: ${String(err)}`,
      );
    }
  });

  test("vue.feature.define-expose.clean", async function () {
    onlyVueParity(this);
    try {
      await assertCleanErrors("src/features/ExposeParent.vue");
      await assertHoverNeedles(
        { file: "src/features/ExposeChild.vue", token: "exposedCount", occurrence: 0 },
        ["number", "Ref", "exposedCount"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.define-expose.clean",
        "ISSUE-vue-define-expose",
        `defineExpose surface incomplete: ${String(err)}`,
      );
    }
  });

  test("vue.feature.options-api.hover", async function () {
    onlyVueParity(this);
    try {
      await assertHoverNeedles(
        { file: "src/features/OptionsApi.vue", token: "clicks", occurrence: 2 },
        ["clicks"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.options-api.hover",
        "ISSUE-vue-options-api",
        `Options API template binding hover failed: ${String(err)}`,
      );
    }
  });

  test("vue.feature.dual-script.hover", async function () {
    onlyVueParity(this);
    try {
      await assertCleanErrors("src/features/DualScript.vue");
      await assertHoverNeedles(
        { file: "src/features/DualScript.vue", token: "dualConstant", occurrence: 2 },
        ["dualConstant"],
      );
    } catch (err) {
      failParityGap(
        this,
        "vue.feature.dual-script.hover",
        "ISSUE-vue-dual-script",
        `Dual <script> + <script setup> incomplete: ${String(err)}`,
      );
    }
  });

  test("vue.fallthrough.deep-native-listener-accepted", async function () {
    onlyVueParity(this);
    try {
      const doc = await openRelative("src/fallthrough/EventDeepConsumer.vue");
      await waitForDiagnosticsSettled(doc.uri, { timeoutMs: 12_000, stableMs: 700 });
      const diags = verterUnknownPropDiags(doc.uri);
      const clickFlagged = diags.some((d) => /click|onClick/i.test(d.message));
      if (clickFlagged) {
        throw new Error(`@click flagged on deep chain: ${diags.map((d) => d.message).join("; ")}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "vue.fallthrough.deep-native-listener-accepted",
        "ISSUE-vue-deep-fallthrough-listener",
        `Native listener fallthrough incomplete: ${String(err)}`,
      );
    }
  });
});
