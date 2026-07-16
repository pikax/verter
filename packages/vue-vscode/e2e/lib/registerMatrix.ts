/**
 * Register a dense matrix of IDE cases as individual mocha tests.
 *
 * Failures are never converted to skips: a broken anchor, timeout, or product
 * gap all surface as failed required tests.
 */
import { FIXTURE_NAME } from "../helpers";
import {
  assertCleanErrors,
  assertCompletionsInclude,
  assertDefinitionTargetsFile,
  assertDefinitionTargetsToken,
  assertHoverNeedles,
  assertHoverRangeCoversToken,
  assertReferenceCountAtLeast,
  completionsAtOffset,
  definitionsAt,
  ensureParityReady,
  findOffset,
  openRelative,
} from "./parityHarness";
import type { MatrixCase } from "./matrixCases";

async function runCase(c: MatrixCase): Promise<void> {
  switch (c.kind) {
    case "clean":
      await assertCleanErrors(c.file);
      return;
    case "hover": {
      if (!c.anchor || !c.needles) {
        throw new Error(`TEST_DEFECT ${c.id}: hover needs anchor+needles`);
      }
      await assertHoverNeedles(c.anchor, c.needles, {
        forbidAny: true,
        forbidGenerated: true,
      });
      return;
    }
    case "definition": {
      if (!c.anchor || !c.target) {
        throw new Error(`TEST_DEFECT ${c.id}: definition needs anchor+target`);
      }
      await assertDefinitionTargetsToken(c.anchor, c.target);
      return;
    }
    case "definition-file": {
      if (!c.anchor || !c.targetFile) {
        throw new Error(`TEST_DEFECT ${c.id}: definition-file needs anchor+targetFile`);
      }
      await assertDefinitionTargetsFile(c.anchor, c.targetFile);
      return;
    }
    case "completion": {
      if (!c.completionOffsetNeedle || !c.completionLabels) {
        throw new Error(`TEST_DEFECT ${c.id}: completion needs offset needle + labels`);
      }
      const doc = await openRelative(c.file);
      const offset = findOffset(doc, c.completionOffsetNeedle) + (c.completionOffsetExtra ?? 0);
      const labels = await completionsAtOffset(c.file, offset);
      for (const want of c.completionLabels) {
        if (!labels.some((l) => l === want || l.startsWith(want))) {
          if (c.anchor) {
            await assertCompletionsInclude(c.anchor, [want]);
          } else {
            throw new Error(`missing completion ${want}; sample=${labels.slice(0, 30).join(", ")}`);
          }
        }
      }
      return;
    }
    case "references": {
      if (!c.anchor) throw new Error(`TEST_DEFECT ${c.id}: references need anchor`);
      await assertReferenceCountAtLeast(c.anchor, c.minRefs ?? 2);
      return;
    }
    case "hover-range": {
      if (!c.anchor) throw new Error(`TEST_DEFECT ${c.id}: hover-range needs anchor`);
      await assertHoverRangeCoversToken(c.anchor);
      return;
    }
    case "no-virtual-definition": {
      if (!c.anchor) throw new Error(`TEST_DEFECT ${c.id}: no-virtual needs anchor`);
      const locs = await definitionsAt(c.anchor);
      if (locs.length === 0) throw new Error("no definition locations");
      return;
    }
    default: {
      const _exhaustive: never = c.kind;
      throw new Error(`unknown kind ${String(_exhaustive)}`);
    }
  }
}

export function registerMatrixSuite(options: {
  title: string;
  fixture: string;
  entry: string;
  cases: readonly MatrixCase[];
}): void {
  // Wrong-fixture: do not register inapplicable framework tests.
  if (FIXTURE_NAME !== options.fixture) {
    return;
  }

  suite(options.title, function () {
    suiteSetup(async function () {
      this.timeout(60_000);
      await ensureParityReady(options.entry);
    });

    for (const c of options.cases) {
      test(c.id, async function () {
        this.timeout(30_000);
        // Hard fail — never failParityGap on unexpected errors.
        await runCase(c);
      });
    }
  });
}
