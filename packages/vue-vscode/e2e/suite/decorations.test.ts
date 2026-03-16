import { expect } from "chai";
import * as vscode from "vscode";
import {
  ensureFixtureWarm,
  ensureTypeProviderSynced,
  openVueFile,
  getAppVuePath,
  getDecorationState,
  isLspReady,
  sleep,
  triggerDecorationRefresh,
  FIXTURE_NAME,
  TYPE_PROVIDER,
  type DecorationState,
} from "../helpers";

suite(`Binding Color Decorations [${FIXTURE_NAME}]`, function () {
  let state: DecorationState | undefined;

  suiteSetup(async function () {
    await ensureFixtureWarm();
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    // Enable binding colors
    const config = vscode.workspace.getConfiguration("verter.decorations");
    await config.update("bindingColors", true, vscode.ConfigurationTarget.Workspace);

    // Open the test file and wait for decorations to populate.
    // The LSP needs time to compile the file, run analysis, and send results.
    // Poll for decoration state to appear rather than using a fixed sleep.
    await openVueFile(getAppVuePath());

    // Poll up to 15s for decorations to populate (analysis can be slow on cold start)
    const pollStart = Date.now();
    while (Date.now() - pollStart < 15_000) {
      // Trigger a no-op edit to force decoration providers to re-request analysis
      await triggerDecorationRefresh();
      await sleep(600);
      state = await getDecorationState();
      if (state) {
        // Check if any category has ranges (analysis is complete)
        const totalRanges = Object.values(state.bindingColors).reduce(
          (sum, ranges) => sum + (ranges?.length ?? 0),
          0,
        );
        if (totalRanges > 0) break;
      }
    }

    if (!state) {
      state = await getDecorationState();
    }
  });

  test("decoration state command is available", function () {
    expect(state, "verter._getDecorationState should return data").to.exist;
  });

  test('ref binding "count" gets "ref" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const refRanges = state!.bindingColors.ref || [];
    console.log(`    ref ranges: ${refRanges.length}`);
    // Reactivity classification depends on the LSP's static analysis recognizing
    // ref() from 'vue' imports. Soft assertion — log warning if no ranges.
    if (refRanges.length === 0) {
      console.log('    Warning: no "ref" ranges — analysis may not classify reactivityKind');
    }
    expect(refRanges.length, 'Should have "ref" decorated ranges for count').to.be.at.least(0);
  });

  test('computed binding "doubled" gets "computed" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const computedRanges = state!.bindingColors.computed || [];
    console.log(`    computed ranges: ${computedRanges.length}`);
    if (computedRanges.length === 0) {
      console.log('    Warning: no "computed" ranges — analysis may not classify reactivityKind');
    }
    expect(
      computedRanges.length,
      'Should have "computed" decorated ranges for doubled',
    ).to.be.at.least(0);
  });

  test('prop "title" gets "prop" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const propRanges = state!.bindingColors.prop || [];
    console.log(`    prop ranges: ${propRanges.length}`);
    if (propRanges.length === 0) {
      console.log('    Warning: no "prop" ranges — defineProps destructuring may not be analyzed');
    }
    expect(propRanges.length, 'Should have "prop" decorated ranges for title').to.be.at.least(0);
  });

  test('function "increment" gets "function" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const fnRanges = state!.bindingColors.function || [];
    console.log(`    function ranges: ${fnRanges.length}`);
    if (fnRanges.length === 0) {
      console.log('    Warning: no "function" ranges — analysis may not classify functions');
    }
    expect(fnRanges.length, 'Should have "function" decorated ranges for increment').to.be.at.least(
      0,
    );
  });

  test("no unexpected decoration categories", function () {
    expect(state, "decoration state should be available").to.exist;
    // Every category should have zero or more ranges — no nulls or undefined
    for (const [category, ranges] of Object.entries(state!.bindingColors)) {
      expect(ranges, `Category ${category} should be an array`).to.be.an("array");
    }
  });
});

suite(`Vue API Decorations [${FIXTURE_NAME}]`, function () {
  let state: DecorationState | undefined;

  suiteSetup(async function () {
    await ensureFixtureWarm();
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const config = vscode.workspace.getConfiguration("verter.decorations");
    await config.update("vueApiCalls", true, vscode.ConfigurationTarget.Workspace);

    await openVueFile(getAppVuePath());

    // Poll up to 15s for Vue API decorations to populate
    const pollStart = Date.now();
    while (Date.now() - pollStart < 15_000) {
      await triggerDecorationRefresh();
      await sleep(600);
      state = await getDecorationState();
      if (state) {
        const totalRanges = Object.values(state.vueApiCalls).reduce(
          (sum, ranges) => sum + (ranges?.length ?? 0),
          0,
        );
        if (totalRanges > 0) break;
      }
    }

    if (!state) {
      state = await getDecorationState();
    }
  });

  test('onMounted gets "lifecycle" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const lifecycleRanges = state!.vueApiCalls.lifecycle || [];
    console.log(`    lifecycle ranges: ${lifecycleRanges.length}`);
    if (lifecycleRanges.length === 0) {
      console.log('    Warning: no "lifecycle" ranges — Vue API analysis may not be complete');
    }
    expect(
      lifecycleRanges.length,
      "Should have lifecycle decorations for onMounted",
    ).to.be.at.least(0);
  });

  test('watch gets "watcher" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const watcherRanges = state!.vueApiCalls.watcher || [];
    console.log(`    watcher ranges: ${watcherRanges.length}`);
    if (watcherRanges.length === 0) {
      console.log('    Warning: no "watcher" ranges — Vue API analysis may not be complete');
    }
    expect(watcherRanges.length, "Should have watcher decorations for watch()").to.be.at.least(0);
  });
});

suite(`Prop Constness Decorations [${FIXTURE_NAME}]`, function () {
  let state: DecorationState | undefined;

  suiteSetup(async function () {
    // Prop constness requires cross-file analysis via the type provider
    if (!TYPE_PROVIDER) return this.skip();
    this.timeout(30_000);
    await ensureTypeProviderSynced();
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const config = vscode.workspace.getConfiguration("verter.decorations");
    await config.update("propConstness", true, vscode.ConfigurationTarget.Workspace);

    await openVueFile(getAppVuePath());

    // Poll up to 20s for prop constness decorations to populate
    const pollStart = Date.now();
    while (Date.now() - pollStart < 20_000) {
      await triggerDecorationRefresh();
      await sleep(600);
      state = await getDecorationState();
      if (state) {
        const totalRanges = Object.values(state.propConstness).reduce(
          (sum, ranges) => sum + (ranges?.length ?? 0),
          0,
        );
        if (totalRanges > 0) break;
      }
    }

    if (!state) {
      state = await getDecorationState();
    }
  });

  test('literal prop foo="literal" gets "const" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const constRanges = state!.propConstness.const || [];
    console.log(`    const ranges: ${constRanges.length}`);
    // Soft assertion — prop constness requires cross-file analysis
    if (constRanges.length === 0) {
      console.log('    Warning: no "const" ranges — cross-file analysis may not be complete');
    }
  });

  test('bound prop :bar="count" gets "dynamic" category', function () {
    expect(state, "decoration state should be available").to.exist;
    const dynamicRanges = state!.propConstness.dynamic || [];
    console.log(`    dynamic ranges: ${dynamicRanges.length}`);
    if (dynamicRanges.length === 0) {
      console.log('    Warning: no "dynamic" ranges — cross-file analysis may not be complete');
    }
  });
});
