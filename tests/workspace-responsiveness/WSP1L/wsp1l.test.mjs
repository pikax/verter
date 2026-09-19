// node --test tests/workspace-responsiveness/WSP1L/wsp1l.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const products = join(here, "products");

function load(name) {
  return JSON.parse(readFileSync(join(products, name), "utf8"));
}

test("WSP1L-AC-OWNER records one final owner and the launch-stamp retirement obligation", () => {
  const ownership = load("ownership-map.json");
  assert.equal(ownership.finalOwner, "expansion.workspace-responsiveness");
  assert.match(ownership.retirementObligation, /default-off instrumentation seam/);
  assert.equal(ownership.deletionPopulationThisNode.length, 0);
  assert.deepEqual(ownership.conflictDomains, [
    "lsp_publication",
    "performance_evidence",
    "scheduler_admission",
  ]);
  const driver = ownership.outcomes.find((row) => row.id === "LapceInteractionDriver");
  assert.equal(driver.surface, "packages/dx-harness/lapce/driver.ts");
});

test("WSP1L pins volt/server/provider versions against the shipped sources", () => {
  const manifest = load("version-manifest.v1.json");
  assert.equal(manifest.schema, "lapce-version-manifest.v1");
  const byItem = Object.fromEntries(manifest.items.map((row) => [row.item, row]));

  // The pinned plugin adapter is the volt crate version that actually ships.
  const voltToml = readFileSync(join(here, "../../../extensions/lapce/volt.toml"), "utf8");
  assert.match(voltToml, /^name = "verter-volt"$/m);
  const voltCargo = readFileSync(join(here, "../../../extensions/lapce/Cargo.toml"), "utf8");
  const voltVersion = /^version = "(.+)"$/m.exec(
    voltCargo.slice(voltCargo.indexOf("[package]")),
  )[1];
  assert.equal(byItem["verter-plugin-adapter"].version, voltVersion);
  assert.equal(byItem["verter-plugin-adapter"].status, "pinned");

  // The pinned server is the repo workspace version verter-lsp builds with.
  const workspaceCargo = readFileSync(join(here, "../../../Cargo.toml"), "utf8");
  const workspaceVersion = /^version = "(.+)"$/m.exec(
    workspaceCargo.slice(workspaceCargo.indexOf("[workspace.package]")),
  )[1];
  assert.equal(byItem["verter-lsp-server"].version, workspaceVersion);

  // The provider pin matches the volt's advertised default type provider.
  assert.match(voltToml, /\[config\."typeProvider"\]\ndefault = "tsgo"/);
  assert.equal(byItem["provider-engines-and-modes"].version, "tsgo");
});

test("WSP1L records lapce-client unavailable, never guessed", () => {
  const manifest = load("version-manifest.v1.json");
  const lapce = manifest.items.find((row) => row.item === "lapce-client");
  assert.equal(lapce.version, null);
  assert.equal(lapce.status, "unrecorded");
  assert.match(lapce.reason, /unavailable, not guessed/);
});

test("WSP1L-AC-EXPOSURE keeps promotion blocked until DX1", () => {
  const exposure = load("exposure-registration.v1.json");
  assert.equal(exposure.dx1SchemaOwner, "DX1");
  assert.equal(exposure.operations.length, 3);
  for (const op of exposure.operations) {
    assert.equal(op.playground.promotionBlocked, true);
    assert.equal(op.playground.status, "missing");
    assert.equal(op.exposure.state, "registered");
  }
});

test("WSP1L.3 records missing GUI instrumentation as unavailable, not simulated", () => {
  const availability = load("gui-instrumentation-availability.v1.json");
  assert.equal(availability.realClientCapture.state, "unavailable");
  assert.equal(availability.realClientCapture.notASimulation, true);
  assert.ok(availability.realClientCapture.reasons.length >= 3);
  assert.ok(availability.realClientCapture.whatIsProvenHermetically.length >= 4);
  assert.match(availability.realClientCapture.automationPathRecorded.fixture, /no GUI, no sleeps/);
});
