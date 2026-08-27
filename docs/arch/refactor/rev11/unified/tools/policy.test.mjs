/** @ai-generated - Class-wide graph policy and deterministic projection tests. */
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { loadAuthority, readToml, validateGraphModel, writeGenerated } from "./lib.mjs";

function cloneNodes(nodes) {
  return nodes.map((node) => structuredClone(node));
}

function ancestorSet(nodes, id) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const found = new Set();
  const visit = (current) => {
    for (const predecessor of byId.get(current)?.predecessors || []) {
      if (!found.has(predecessor)) { found.add(predecessor); visit(predecessor); }
    }
  };
  visit(id);
  return found;
}

function fileInventory(root) {
  const result = new Map();
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(absolute);
      else result.set(path.relative(root, absolute).split(path.sep).join("/"), crypto.createHash("sha256").update(fs.readFileSync(absolute)).digest("hex"));
    }
  };
  walk(root);
  return [...result].sort(([left], [right]) => left.localeCompare(right));
}

test("every mechanical recovery split has one explicit non-authoritative disposition", () => {
  const authority = loadAuthority();
  const model = readToml(path.join(authority.packageRoot, "provenance/collapsed-node-map.toml"));
  assert.equal(model.recovery_input_commit, "903f06b80e4416a19f4eeaf2f4ab7f02b09ec096");
  assert.equal(model.recovery_input_node_count, 523);
  assert.equal(model.current_node_count, 197);
  assert.equal(model.disposition.length, 326);
  assert.equal(new Set(model.disposition.map((row) => row.id)).size, 326);
  assert.deepEqual(authority.nodes.filter((node) => ["proposal-subblock", "split"].includes(node.class)), []);
  assert.ok(model.disposition.every((row) => row.disposition === "collapsed_into_atomic_source_node" || row.disposition === "deleted_unratified"));
});

test("source-canonical BR0 gates every product without globally joining independent products", () => {
  const authority = loadAuthority();
  const products = authority.nodes.filter((node) => node.release_gating === "product");
  assert.equal(products.length, 9);
  const br0 = authority.nodes.find((node) => node.id === "BR0");
  assert.deepEqual(br0.predecessors, []);
  assert.deepEqual(br0.external_requirements, ["maintainer_rev11_repair_freeze_lift", "maintainer_successor_genesis"]);
  for (const product of products) {
    assert.ok(ancestorSet(authority.nodes, product.id).has("BR0"), `${product.id} is downstream of BR0`);
    const mutated = cloneNodes(authority.nodes);
    const target = mutated.find((node) => node.id === product.id);
    target.predecessors = target.predecessors.filter((id) => !ancestorSet(authority.nodes, id).has("BR0") && id !== "BR0");
    assert.match(validateGraphModel(mutated, { skipCharters: true }).join("\n"), new RegExp(`${product.id}: product release gate is not downstream of BR0`));
  }
  const independent = products.filter((node) => node.id !== "CLI3" && node.id !== "CLI5");
  for (let left = 0; left < independent.length; left += 1) for (let right = left + 1; right < independent.length; right += 1) {
    assert.equal(ancestorSet(authority.nodes, independent[left].id).has(independent[right].id), false);
    assert.equal(ancestorSet(authority.nodes, independent[right].id).has(independent[left].id), false);
  }
});

test("two independent output roots are byte-for-byte deterministic", () => {
  const authority = loadAuthority();
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rev11 deterministic roots "));
  try {
    const first = path.join(scratch, "first output");
    const second = path.join(scratch, "second output");
    writeGenerated(authority, first);
    writeGenerated(authority, second);
    assert.deepEqual(fileInventory(first), fileInventory(second));
    assert.equal(fileInventory(first).length, 4);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});
