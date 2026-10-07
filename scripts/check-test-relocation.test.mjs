import assert from "node:assert/strict";
import test from "node:test";
import { compareTestRelocation } from "./check-test-relocation.mjs";

const inventory = (testcases) => ({ "rust-suites": { unit: { status: "listed", testcases } } });
const row = { kind: "test", ignored: false };
const mapping = [{ from: ["unit", "old"], to: ["unit", "suite::old"] }];

test("relocation preserves unchanged tests and ignored dispositions", () => {
  const ignored = { ...row, ignored: true };
  assert.equal(
    compareTestRelocation(
      inventory({ old: ignored, kept: row }),
      inventory({ "suite::old": ignored, kept: row }),
      mapping,
    ),
    2,
  );
  assert.throws(
    () =>
      compareTestRelocation(inventory({ old: ignored }), inventory({ "suite::old": row }), mapping),
    /disposition/,
  );
});

test("omissions, additions, stale mappings and collisions fail closed", () => {
  const before = inventory({ old: row, kept: row });
  assert.throws(
    () => compareTestRelocation(before, inventory({ "suite::old": row }), mapping),
    /population/,
  );
  assert.throws(
    () =>
      compareTestRelocation(
        before,
        inventory({ "suite::old": row, kept: row, extra: row }),
        mapping,
      ),
    /population/,
  );
  assert.throws(() => compareTestRelocation(before, inventory({ kept: row }), mapping), /Stale/);
  assert.throws(
    () =>
      compareTestRelocation(before, inventory({ "suite::old": row, kept: row }), [
        ...mapping,
        ...mapping,
      ]),
    /Duplicate/,
  );
  assert.throws(
    () =>
      compareTestRelocation(before, inventory({ kept: row }), [
        { from: ["unit", "old"], to: ["unit", "kept"] },
      ]),
    /collision/,
  );
});
