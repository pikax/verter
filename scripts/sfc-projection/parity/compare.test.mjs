import assert from "node:assert/strict";
import test from "node:test";
import { compare } from "./compare.mjs";
import { normalize } from "./normalize.mjs";

const rows = (text) => normalize(text, "/repo");
const a = "src/A.vue(2,4): error TS2322: Wrong value.";
const b = "src/B.vue(3,7): error TS2339: Missing property.";

test("seeds left-only and right-only differences and preserves multiplicity", () => {
  const result = compare(rows(`${a}\n${a}`), rows(b));
  assert.deepEqual(
    result.differences.filter((d) => d.class !== "order").map((d) => [d.class, d.count]),
    [
      ["left-only", 2],
      ["right-only", 1],
    ],
  );
  const duplicate = compare(rows(`${a}\n${a}`), rows(a));
  assert.deepEqual(
    duplicate.differences.map((d) => [d.class, d.count]),
    [["left-only", 1]],
  );
  assert.equal(duplicate.gates.order, true);
});

test("a reordered project pair fails order with equal multisets", () => {
  const result = compare(rows(`${a}\n${b}\n${a}`), rows(`${b}\n${a}\n${a}`));
  assert.equal(result.gates.set, true);
  assert.equal(result.gates.order, false);
  assert.deepEqual(result.differences, [
    { class: "order", left: ["src/A.vue", "src/B.vue"], right: ["src/B.vue", "src/A.vue"] },
  ]);
});

test("stock ignores columns and messages in keys but reports both original rows", () => {
  const left = rows(a);
  const right = rows(a.replace("2,4", "2,9").replace("Wrong value.", "Different wording."));
  assert.equal(compare(left, right).gates.set, false);
  assert.equal(compare(left, right, { stock: true }).equal, true);
  const delta = compare(left, [...right, ...right], { stock: true }).differences[0];
  assert.equal(delta.class, "right-only");
  assert.equal(delta.count, 1);
  assert.deepEqual(delta.left, left);
  assert.deepEqual(delta.right, [...right, ...right]);
});

test("compares exits only when both sides are commands", () => {
  const same = rows(a);
  assert.equal(compare(same, same, { leftExit: 1, rightExit: 2 }).equal, false);
  assert.equal(compare(same, same, { leftExit: 1, rightExit: 2 }).gates.exit, false);
  assert.equal(compare(same, same, { leftExit: 1 }).gates.exit, null);
  assert.equal(compare(same, same, { leftExit: 1, rightExit: 1 }).equal, true);
});

test("a positionless row is its own key, and a global row has no place in file order", () => {
  const unpositioned = "src/A.vue: error TS2322: Wrong value.";
  const global = "error TS18003: No inputs were found.";
  assert.equal(
    compare(rows(`${unpositioned}\n${global}`), rows(`${unpositioned}\n${global}`)).equal,
    true,
  );
  const result = compare(rows(a), rows(`${global}\n${unpositioned}`), { stock: true });
  assert.deepEqual(
    result.differences.map((d) => [d.class, d.count]),
    [
      ["left-only", 1],
      ["right-only", 1],
      ["right-only", 1],
    ],
  );
  assert.equal(result.gates.order, true);
});
