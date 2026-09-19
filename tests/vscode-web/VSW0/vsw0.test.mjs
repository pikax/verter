#!/usr/bin/env node
/**
 * VSW0 constitution harness: web extension feature contract /
 * main-browser entrypoint policy / host-status vocabulary checks.
 *
 * Grounds itself in the live repository (packages/vue-vscode manifest and
 * build config, the shipped VSC0 / BWH0 / DX0 machine products) only. It
 * never reads a roadmap/DAG store (those are database-owned) and mints no
 * parallel vocabulary. Each check has a named negative twin that mutates an
 * in-memory copy and must fail, so a clean pass is discrimination, not
 * vacuity.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));

const dxReceiptBasis = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/product-receipt-basis.v1.json"),
);
const dxHostClasses = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/host-execution-class.v1.json"),
);
const dxOwnership = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/exposure-ownership-map.json"),
);
const vscFeatureMatrix = readJson(
  path.join(REPO_ROOT, "tests/vscode-product/VSC0/products/feature-ownership-matrix.v1.json"),
);
const bwhPlatformServices = readJson(
  path.join(REPO_ROOT, "tests/browser-host/BWH0/products/platform-host-services.v1.json"),
);

const contract = readJson(path.join(HERE, "products/web-extension-feature-contract.v1.json"));
const entryPolicy = readJson(path.join(HERE, "products/entrypoint-contribution-policy.v1.json"));
const hostVocab = readJson(path.join(HERE, "products/host-status-vocabulary.v1.json"));

const livePkg = readJson(path.join(REPO_ROOT, "packages/vue-vscode/package.json"));
const liveEsbuildCfg = fs.readFileSync(
  path.join(REPO_ROOT, "packages/vue-vscode/esbuild.config.mjs"),
  "utf8",
);
const liveExtensionTs = fs.readFileSync(
  path.join(REPO_ROOT, "packages/vue-vscode/src/extension.ts"),
  "utf8",
);

const bwhServiceIds = new Set(bwhPlatformServices.services.map((s) => s.id));
const vscFeatures = Object.fromEntries(vscFeatureMatrix.features.map((f) => [f.id, f]));

/** Structural clone with a mutation applied; used for negative twins only. */
function perturb(obj, mutate) {
  const copy = structuredClone(obj);
  mutate(copy);
  return copy;
}

const filesExist = (rels) => {
  for (const rel of rels) {
    assert.ok(
      fs.existsSync(path.join(REPO_ROOT, rel)),
      `evidence path missing in live tree: ${rel}`,
    );
  }
};

// ---------------------------------------------------------------------------
// VSW0-AC1: a build with only a Node main entry cannot pass as a web
// extension. The admission predicate is factored so every twin re-runs the
// exact discriminator the live facts pass.
// ---------------------------------------------------------------------------

function webAdmissionFacts() {
  return {
    manifestMain: typeof livePkg.main === "string",
    manifestBrowser: typeof livePkg.browser === "string",
    buildPlatform: /platform:\s*"node"/.test(liveEsbuildCfg) ? "node" : "browser",
    browserEntryImportsNode:
      "browser" in livePkg &&
      Object.values(livePkg.browser).some((v) =>
        /node:|vscode-languageclient\/node/.test(String(v)),
      ),
  };
}

function admitWebExtension(facts) {
  if (!facts.manifestMain) return { admitted: false, reason: "main-required" };
  if (!facts.manifestBrowser) return { admitted: false, reason: "node-main-only" };
  if (facts.buildPlatform !== "browser") {
    return { admitted: false, reason: "browser-entry-node-platform" };
  }
  if (facts.browserEntryImportsNode) {
    return { admitted: false, reason: "node-import-in-browser-entry" };
  }
  return { admitted: true, reason: "admitted" };
}

function assertPolicyDeclaresAdmissionLaw(policy) {
  const rules = policy.entrypoint.webAdmissionRules.join("\n");
  assert.match(
    rules,
    /both main .* browser .* entries|browser .* entries .*main/,
    "an admission rule must require both main and browser entries",
  );
  assert.match(
    rules,
    /browser platform/i,
    "an admission rule must require a browser-platform build",
  );
  assert.match(
    rules,
    /no Node builtins|never spawns a child process/,
    "an admission rule must exclude Node imports and child processes from the web entry",
  );
  assert.match(
    rules,
    /re-derived from the live manifest/,
    "admission must be a re-derived predicate, not a packaging-time claim",
  );
}

function assertLiveStateGrounded(policy, facts) {
  const live = policy.entrypoint.liveState;
  assert.equal(
    live.manifestBrowser,
    facts.manifestBrowser ? "<declared>" : "absent",
    "liveState.manifestBrowser must match the live manifest",
  );
  assert.equal(
    live.buildPlatform,
    facts.buildPlatform,
    "liveState.buildPlatform must match the live esbuild config",
  );
  assert.equal(
    live.verdict,
    "not-admitted: node-main-only",
    "the live verdict is node-main-only until a browser entry ships",
  );
  assert.match(
    liveEsbuildCfg,
    /entryPoints:\s*\[PRODUCTION_ENTRY_POINT\]/,
    "the shipped build has exactly the production entry point",
  );
  assert.match(
    liveExtensionTs,
    /from "vscode-languageclient\/node"/,
    "the shipped entry graph carries the desktop-owned Node transport",
  );
}

test("VSW0-AC1 clean pass: the live build is not-admitted node-main-only and the predicate can admit", () => {
  const facts = webAdmissionFacts();
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "node-main-only",
  });
  assertPolicyDeclaresAdmissionLaw(entryPolicy);
  assertLiveStateGrounded(entryPolicy, facts);

  // Non-vacuity: the same predicate admits a fully compliant manifest.
  assert.deepEqual(
    admitWebExtension({
      manifestMain: true,
      manifestBrowser: true,
      buildPlatform: "browser",
      browserEntryImportsNode: false,
    }),
    { admitted: true, reason: "admitted" },
  );
});

test("VSW0-AC1 twin: web-admission-without-policy (dropping the both-entries rule) fails", () => {
  const weakened = perturb(entryPolicy, (c) => {
    c.entrypoint.webAdmissionRules = c.entrypoint.webAdmissionRules.filter(
      (r) => !/both main/.test(r),
    );
  });
  assert.throws(() => assertPolicyDeclaresAdmissionLaw(weakened), /both main and browser/);
});

test("VSW0-AC1 twin: browser-only-entry is not admitted either", () => {
  const verdict = admitWebExtension({
    ...webAdmissionFacts(),
    manifestMain: false,
    manifestBrowser: true,
  });
  assert.equal(verdict.admitted, false);
  assert.equal(verdict.reason, "main-required");
});

test("VSW0-AC1 twin: browser-entry-node-platform (browser entry declared, node build) fails", () => {
  const verdict = admitWebExtension({
    ...webAdmissionFacts(),
    manifestBrowser: true,
  });
  assert.equal(verdict.admitted, false);
  assert.equal(verdict.reason, "browser-entry-node-platform");
});

test("VSW0-AC1 twin: node-import-in-browser-entry fails admission", () => {
  const verdict = admitWebExtension({
    manifestMain: true,
    manifestBrowser: true,
    buildPlatform: "browser",
    browserEntryImportsNode: true,
  });
  assert.equal(verdict.admitted, false);
  assert.equal(verdict.reason, "node-import-in-browser-entry");
});

test("VSW0-AC1 twin: live-state-drift (policy claiming a shipped browser entry) fails", () => {
  const drifted = perturb(entryPolicy, (c) => {
    c.entrypoint.liveState.manifestBrowser = "dist/web/extension.js";
  });
  assert.throws(
    () => assertLiveStateGrounded(drifted, webAdmissionFacts()),
    /manifestBrowser must match/,
  );
});

// ---------------------------------------------------------------------------
// VSW0-AC2: the active host/backend is visible and correct in a
// browser-connected remote workspace. The vocabulary owns the terms; the
// feature contract and every mode join them.
// ---------------------------------------------------------------------------

function assertHostStatusLaw(vocab) {
  for (const term of vocab.statusEnum) {
    assert.ok(vocab.terms[term], `status term ${term} has no definition`);
    assert.match(vocab.terms[term].definition, /\S/, `status term ${term} definition is empty`);
    assert.ok(vocab.terms[term].derivesFrom, `status term ${term} must state how it is derived`);
  }
  assert.match(
    vocab.terms["remote-native"].definition,
    /never claim browser-local receipts|does not make this browser-local/,
    "remote-native must explicitly deny browser-local parity",
  );
  assert.ok(
    vocab.claims.some((c) => /not browser-local parity/.test(c)),
    "a claim rule must state that a remote Node service is not browser-local parity",
  );
  assert.ok(
    vocab.visibility.binding.hostIdentity,
    "visibility must bind the DX0 hostIdentity receipt field",
  );
  assert.match(vocab.visibility.rule, /surfaced/, "the host status must be surfaced, not latent");
  assert.match(
    vocab.visibility.rule,
    /never guessed from the client/,
    "host status derives from executing entry and workspace topology, not the client kind",
  );
  assert.match(vocab.law, /visible and correct/, "the law restates the AC2 visibility obligation");
}

test("VSW0-AC2 clean pass: vocabulary terms, visibility binding and non-parity law hold", () => {
  assertHostStatusLaw(hostVocab);
  // Every mode and vocabulary receiver joins real status terms.
  for (const mode of contract.hostModes) {
    assert.ok(
      hostVocab.statusEnum.includes(mode.hostStatus),
      `host mode ${mode.mode} uses unknown status ${mode.hostStatus}`,
    );
  }
  assert.equal(contract.hostModes.length, 3, "exactly the three charter execution placements");
  const browserLocalMode = contract.hostModes.find((m) => m.hostStatus === "browser-local");
  assert.equal(browserLocalMode.existsToday, false, "no browser-local mode exists today (AC1)");
});

test("VSW0-AC2 twin: remote-native-claimed-browser-local (dropping the non-parity claim) fails", () => {
  const weakened = perturb(hostVocab, (c) => {
    c.claims = c.claims.filter((x) => !/not browser-local parity/.test(x));
  });
  assert.throws(() => assertHostStatusLaw(weakened), /not browser-local parity/);
});

test("VSW0-AC2 twin: missing-hostidentity-binding fails", () => {
  const dropped = perturb(hostVocab, (c) => {
    delete c.visibility.binding.hostIdentity;
  });
  assert.throws(() => assertHostStatusLaw(dropped), /hostIdentity/);
});

test("VSW0-AC2 twin: hidden-host-status (weakening the surfaced rule) fails", () => {
  const hidden = perturb(hostVocab, (c) => {
    c.visibility.rule = c.visibility.rule
      .replace("and it is surfaced", "and it is recorded")
      .replace(", and it is recorded", "");
  });
  assert.throws(() => assertHostStatusLaw(hidden), /surfaced/);
});

test("VSW0-AC2 twin: status-term-drift (renamed term not joined by the modes) fails", () => {
  const drifted = perturb(hostVocab, (c) => {
    c.statusEnum = c.statusEnum.map((s) => (s === "browser-local" ? "web-local" : s));
  });
  const mode = contract.hostModes.find((m) => m.hostStatus === "browser-local");
  assert.ok(mode, "the contract still names the browser-local mode");
  assert.throws(
    () => assertHostStatusLaw(drifted) || assert.ok(drifted.statusEnum.includes(mode.hostStatus)),
    /unknown status browser-local|has no definition/,
  );
});

// ---------------------------------------------------------------------------
// Ratification: the feature contract joins VSC0 / BWH0 / DX0 exactly, the
// entrypoint policy joins the live manifest exactly, and ownership is
// single-owner docs-only.
// ---------------------------------------------------------------------------

function assertFeatureContractJoins(c) {
  const vscIds = vscFeatureMatrix.features.map((f) => f.id).sort();
  assert.deepEqual(
    c.featureRows.map((r) => r.id).sort(),
    vscIds,
    "featureRows must cover every VSC0 feature id exactly once (generated, not curated)",
  );
  for (const row of c.featureRows) {
    const vsc = vscFeatures[row.id];
    assert.ok(vsc, `unknown VSC0 feature id: ${row.id}`);
    assert.equal(
      row.vsc0Class,
      vsc.hostExecutionClass,
      `${row.id}: host-execution class drift against VSC0`,
    );
    assert.ok(
      dxHostClasses.enum.includes(row.vsc0Class),
      `${row.id}: class ${row.vsc0Class} is outside the DX0 enum`,
    );
    assert.ok(
      c.vocabulary.webDisposition.includes(row.webDisposition),
      `${row.id}: unknown webDisposition ${row.webDisposition}`,
    );
    if (row.status === "absent") {
      assert.equal(row.routeOwner, null, `${row.id}: absent row has no owner`);
    } else {
      assert.match(
        row.routeOwner ?? "",
        /^VSW[1-6]/,
        `${row.id}: routeOwner must be a VSW train node`,
      );
    }
    if (row.webDisposition === "browser-local-route") {
      assert.ok(row.webEngine, `${row.id}: browser-local-route rows name their web engine`);
    }
    if (row.webDisposition === "remote-native-only" || row.webDisposition === "companion-route") {
      assert.ok(
        row.hostIdentityOverride,
        `${row.id}: ${row.webDisposition} rows must override the browser-local hostIdentity`,
      );
      assert.match(
        row.hostIdentityOverride,
        /browser-local/,
        `${row.id}: the override must name what it may not claim`,
      );
    }
    for (const svc of row.bwhServices ?? []) {
      assert.ok(bwhServiceIds.has(svc), `${row.id}: unknown BWH platform service ${svc}`);
    }
  }
  assert.deepEqual(
    Object.keys(c.browserLocalReceipt).sort(),
    [...dxReceiptBasis.fields].sort(),
    "browserLocalReceipt must bind exactly the DX0 receipt-basis fields",
  );
  for (const mode of c.hostModes) {
    assert.equal(
      typeof mode.existsToday,
      "boolean",
      `host mode ${mode.mode} states whether it exists today`,
    );
    assert.match(
      mode.deliveryOwner,
      /^VSW[1-6]|^expansion\./,
      `host mode ${mode.mode} names its delivery owner`,
    );
  }
  for (const inv of c.startupAdaptationInventory) {
    assert.ok(inv.nodeAssumption && inv.webRoute, `${inv.id}: inventory row states both sides`);
  }
  assert.match(c.acBasisDownstreamTestOwner, /VSW1/, "AC-BASIS binds the browser-entrypoint owner");
  assert.match(c.acExposureRegistration, /DX1/, "AC-EXPOSURE registration stays DX1-owned");
  assert.match(
    c.acResourceRationale,
    /0 production LOC/,
    "AC-RESOURCE rationale records the contract-only basis",
  );
}

function liveContributionCounts() {
  const c = livePkg.contributes;
  return {
    languages: c.languages.length,
    grammars: c.grammars.length,
    commands: c.commands.length,
    menus: Object.keys(c.menus).length,
    viewsContainers: c.viewsContainers.activitybar.length,
    views: Object.values(c.views).flat().length,
    colors: c.colors.length,
    configuration: Object.keys(c.configuration.properties).length,
    configurationDefaults: Object.keys(c.configurationDefaults).length,
    typescriptServerPlugins: c.typescriptServerPlugins.length,
    mcpServerDefinitionProviders: c.mcpServerDefinitionProviders.length,
    activationEvents: livePkg.activationEvents.length,
  };
}

function assertEntryPolicyJoins(policy) {
  const live = liveContributionCounts();
  const categories = policy.contributions.map((x) => x.category).sort();
  // liveContributionCounts covers every live contributes key plus activationEvents.
  assert.deepEqual(
    categories,
    Object.keys(live).sort(),
    "contribution categories must cover exactly the live contributes keys plus activationEvents",
  );
  for (const row of policy.contributions) {
    assert.equal(
      row.liveCount,
      live[row.category],
      `${row.category}: liveCount drift against the live manifest (${row.liveCount} vs ${live[row.category]})`,
    );
    assert.ok(
      policy.webCompatibilityEnum.includes(row.webCompatibility),
      `${row.category}: unknown webCompatibility ${row.webCompatibility}`,
    );
    assert.ok(
      row.rule && row.rule.length > 20,
      `${row.category}: rule states the compatibility obligation`,
    );
  }
}

function assertOwnershipJoins(dxMap = dxOwnership, vscMatrix = vscFeatureMatrix) {
  const dx = dxMap.receivingAmendments.find((a) => a.receiver === "VSW0");
  assert.ok(dx, "DX0 exposure ownership map names VSW0 as a receiver");
  assert.equal(
    dx.productionCapable,
    false,
    "VSW0 is a contract-only receiver; VSW1+ own vscode-web production",
  );
  const vsc = vscMatrix.receivingAmendments.find((a) => a.receiver === "VSW0");
  assert.ok(vsc, "VSC0 feature matrix names VSW0 as a receiving amendment");
  assert.equal(vsc.productionCapable, false, "VSC0 keeps VSW0 docs-only");
  assert.deepEqual(vscMatrix.docsOnlyReceivers, ["VSW0"], "VSW0 stays the docs-only receiver list");
  for (const a of hostVocab.receivingAmendments) {
    assert.match(a.receiver, /^VSW[1-6][A-Z]?$/, `unknown vocabulary receiver ${a.receiver}`);
    assert.equal(typeof a.productionCapable, "boolean");
  }
}

test("VSW0 ratification clean pass: products join VSC0/BWH0/DX0 and the live manifest exactly", () => {
  assertFeatureContractJoins(contract);
  assertEntryPolicyJoins(entryPolicy);
  assertOwnershipJoins();
});

test("VSW0 ratification twin: feature-row-missing fails coverage", () => {
  const m = perturb(contract, (c) => {
    c.featureRows = c.featureRows.filter((r) => r.id !== "decorations");
  });
  assert.throws(() => assertFeatureContractJoins(m), /exactly once/);
});

test("VSW0 ratification twin: feature-disposition-drift (synonymous disposition) fails", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "css-intellisense").webDisposition = "portable-direct";
  });
  assert.throws(() => assertFeatureContractJoins(m), /unknown webDisposition/);
});

test("VSW0 ratification twin: receipt-field-missing fails the DX0 field loop", () => {
  const m = perturb(contract, (c) => {
    delete c.browserLocalReceipt.hostIdentity;
  });
  assert.throws(() => assertFeatureContractJoins(m), /exactly the DX0 receipt-basis fields/);
});

test("VSW0 ratification twin: remote-row-without-host-override fails", () => {
  const m = perturb(contract, (c) => {
    delete c.featureRows.find((r) => r.id === "engine-tiers").hostIdentityOverride;
  });
  assert.throws(
    () => assertFeatureContractJoins(m),
    /must override the browser-local hostIdentity/,
  );
});

test("VSW0 ratification twin: unknown-bwh-service fails the service join", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "virtual-files-view").bwhServices = ["filesystem"];
  });
  assert.throws(() => assertFeatureContractJoins(m), /unknown BWH platform service/);
});

test("VSW0 ratification twin: contribution-category-missing fails the live-manifest join", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions = c.contributions.filter((x) => x.category !== "colors");
  });
  assert.throws(() => assertEntryPolicyJoins(m), /exactly the live contributes keys/);
});

test("VSW0 ratification twin: contribution-compat-drift (synonymous compatibility) fails", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions.find((x) => x.category === "commands").webCompatibility = "web-safe";
  });
  assert.throws(() => assertEntryPolicyJoins(m), /unknown webCompatibility/);
});

test("VSW0 ratification twin: counts-drift against the live manifest fails", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions.find((x) => x.category === "commands").liveCount = 99;
  });
  assert.throws(() => assertEntryPolicyJoins(m), /liveCount drift/);
});

test("VSW0 ratification twin: dx0-receiver-row-missing fails ownership", () => {
  const m = perturb(dxOwnership, (c) => {
    c.receivingAmendments = c.receivingAmendments.filter((a) => a.receiver !== "VSW0");
  });
  assert.throws(() => assertOwnershipJoins(m), /names VSW0 as a receiver/);
});

test("VSW0 ratification twin: vsc0-receiver-drift (VSW0 flipped production-capable) fails", () => {
  const flipped = perturb(vscFeatureMatrix, (c) => {
    c.receivingAmendments.find((a) => a.receiver === "VSW0").productionCapable = true;
  });
  assert.throws(() => assertOwnershipJoins(dxOwnership, flipped), /VSC0 keeps VSW0 docs-only/);
});

test("VSW0 ratification twin: downstream-owner-unknown fails the train join", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "statistics").routeOwner = "VSC9";
  });
  assert.throws(() => assertFeatureContractJoins(m), /routeOwner must be a VSW train node/);
});

test("VSW0 grounding: cited evidence paths exist in the live repository", () => {
  filesExist(hostVocab.visibility.evidence);
  for (const term of Object.values(hostVocab.terms)) filesExist(term.evidence);
  filesExist(entryPolicy.entrypoint.liveState.evidence);
  for (const inv of contract.startupAdaptationInventory) filesExist(inv.evidence);
  // The charter's placement surfaces are characterized, not created, by VSW0.
  assert.equal(
    fs.existsSync(path.join(REPO_ROOT, "packages/vue-vscode/src/browser")),
    false,
    "VSW0 adds no production browser entry (contract-only node)",
  );
  assert.equal(
    fs.existsSync(path.join(REPO_ROOT, "packages/vue-vscode/src/shared")),
    false,
    "VSW0 adds no production shared directory (contract-only node)",
  );
  filesExist(["packages/vue-vscode/src/statusBar.ts", "packages/vue-vscode/src/activationGate.ts"]);
});

test("VSW0 grounding twin: missing-evidence-file fails", () => {
  assert.throws(
    () => filesExist(["packages/vue-vscode/src/does-not-exist.ts"]),
    /evidence path missing/,
  );
});
