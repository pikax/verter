import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { parseToml } from "./toml.mjs";
import { observationFiles, validateProbeAuthorities } from "./validate-probe-authorities.mjs";

const TOOL = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "validate-probe-authorities.mjs",
);

// A self-contained catalog over synthetic owner nodes, so every fixture is
// independent of the live catalog and ledger.
const CATALOG = `
schema = 1

[[authority]]
id = "compiler.public-request-route"
node = "ROUTE"
framework = "any"
dimensions = ["Route"]
atoms = { route-callable = { charter_atom = "Acceptance and evidence/1", dimensions = ["Route"], outcomes = ["pass"] }, typed-refusal = { charter_atom = "Acceptance and evidence/2", dimensions = ["Route"], outcomes = ["request_refused"] } }

[[authority]]
id = "vue.runtime-client-product"
node = "VUE"
framework = "vue"
dimensions = ["Compile", "Structural", "Runtime", "Map"]
comparator = { crate = "verter_vue_conformance", path = "src/compare.rs", function = "compare_modules", atom = "product-identity" }
atoms = { product-shape = { charter_atom = "VUE-AC2", dimensions = ["Compile"], outcomes = ["pass"] }, product-identity = { charter_atom = "VUE-AC2", dimensions = ["Structural"], outcomes = ["pass"] }, runtime-behavior = { charter_atom = "VUE-AC2", dimensions = ["Runtime"], outcomes = ["pass"] }, source-map = { charter_atom = "VUE-AC2", dimensions = ["Map"], outcomes = ["pass"] } }

[[authority]]
id = "svelte.runtime-client-product"
node = "SVELTE"
framework = "svelte"
dimensions = ["Compile", "Structural", "Runtime", "Map"]
atoms = { product-shape = { charter_atom = "SVELTE-AC2", dimensions = ["Compile"], outcomes = ["pass"] }, product-identity = { charter_atom = "SVELTE-AC2", dimensions = ["Structural"], outcomes = ["pass"] }, runtime-behavior = { charter_atom = "SVELTE-AC2", dimensions = ["Runtime"], outcomes = ["pass"] }, source-map = { charter_atom = "SVELTE-AC2", dimensions = ["Map"], outcomes = ["pass"] } }

[[authority]]
id = "compiler.equivalent-work-ledger"
node = "WORK"
framework = "any"
dimensions = ["Performance"]
atoms = { equivalent-work-ledger = { charter_atom = "WORK-AC2", dimensions = ["Performance"], outcomes = ["pass"] } }
`;

const CHARTERS = {
  ROUTE:
    "# Route\n\n## Acceptance and evidence\n\n- The route answers.\n- A refusal is typed.\n\n## Next\n\n- other\n",
  VUE: "# Vue\n\n- **VUE-AC1 — sole owner:** one owner.\n- **VUE-AC2 — positive contract:** exact identity.\n",
  SVELTE: "# Svelte\n\n- **SVELTE-AC2 — positive contract:** exact identity.\n",
  WORK: "# Work\n\n- **WORK-AC2 — positive contract:** equivalent work.\n",
};

const cell = (dimension, fields) => ({
  probe_id: "vue/fixtures/App.vue",
  framework: "vue",
  case: "fixtures/App.vue",
  dimension,
  ...fields,
});

function vueManifest() {
  return {
    framework: "vue",
    comparison: "structural",
    external_revision: "0123456789abcdef0123456789abcdef01234567",
    comparator: {
      crate: "verter_vue_conformance",
      path: "src/compare.rs",
      function: "compare_modules",
      atom: "product-identity",
    },
    applicability: { runtime: "inapplicable", map: "inapplicable" },
    inventory: [{ case_id: "vue/fixtures/App.vue" }],
    entries: [
      cell("Route", {
        expected_state: "gate",
        expected_class: "pass",
        authority: "compiler.public-request-route",
        atom: "route-callable",
      }),
      cell("Compile", {
        expected_state: "canary",
        expected_class: "pass",
        authority: "vue.runtime-client-product",
        atom: "product-shape",
      }),
      cell("Structural", {
        expected_state: "canary",
        expected_class: "semantic_mismatch",
        authority: "vue.runtime-client-product",
        atom: "product-identity",
      }),
      cell("Runtime", {
        expected_state: "skip",
        authority: "vue.runtime-client-product",
        atom: "runtime-behavior",
        reason: "runtime_executor_absent",
      }),
      cell("Map", {
        expected_state: "skip",
        authority: "vue.runtime-client-product",
        atom: "source-map",
        reason: "map_validator_absent",
      }),
      cell("Performance", {
        expected_state: "canary",
        expected_class: "pass",
        authority: "compiler.equivalent-work-ledger",
        atom: "equivalent-work-ledger",
      }),
    ],
  };
}

function joinRecord() {
  return {
    probe: [
      {
        probe_id: "vue/fixtures/App.vue",
        dimension: "Route",
        state: "canary",
        transition: "canary -> gate",
        decided_by: { authority: "compiler.public-request-route", atom: "route-callable" },
        rationale: "the route owner is implemented",
      },
      {
        probe_id: "vue/fixtures/App.vue",
        dimension: "Compile",
        state: "canary",
        transition: "deferred",
        rationale: "owner pending",
      },
    ],
    observation: [
      {
        artifact_id: "a/b/1/1",
        row_id: "vue/fixtures/App.vue@cold",
        disposition: "adopted",
        rationale: "baseline",
        basis: { authority: "compiler.equivalent-work-ledger", atom: "equivalent-work-ledger" },
      },
    ],
  };
}

function observationArtifact() {
  return {
    rows: [
      {
        row_id: "vue/fixtures/App.vue@cold",
        case_id: "vue/fixtures/App.vue",
        comparison_eligible: false,
        semantic_basis: { authority: "vue.runtime-client-product", atom: "product-identity" },
        equivalent_work_basis: {
          authority: "compiler.equivalent-work-ledger",
          atom: "equivalent-work-ledger",
        },
      },
      {
        row_id: "vue/fixtures/App.vue@warm",
        case_id: "vue/fixtures/App.vue",
        comparison_eligible: false,
      },
    ],
  };
}

function run({
  catalog = parseToml(CATALOG),
  implemented = ["ROUTE", "WORK"],
  manifests,
  joins = [],
  observations = [],
} = {}) {
  return validateProbeAuthorities({
    catalog,
    charterFor: (nodeId) => CHARTERS[nodeId],
    implemented: new Set(implemented),
    manifests: (manifests ?? [vueManifest()]).map((doc) => ({
      file: `manifest/${doc.framework}.toml`,
      doc,
    })),
    joins: joins.map((doc, index) => ({ file: `join/${index}.toml`, doc })),
    observations: observations.map((doc, index) => ({
      file: `inventory/${index}/observations.json`,
      doc,
    })),
  });
}

function manifestWith(dimension, change) {
  const manifest = vueManifest();
  change(manifest.entries.find((entry) => entry.dimension === dimension));
  return manifest;
}

function refusedOnce(errors, pattern) {
  assert.equal(errors.length, 1, `exactly one refusal expected, got:\n${errors.join("\n")}`);
  assert.match(errors[0], pattern);
}

test("valid manifest, join record, and observations pass", () => {
  assert.deepEqual(run({ joins: [joinRecord()], observations: [observationArtifact()] }), []);
});

test("the live catalog, charters, ledger, and committed manifests validate", () => {
  const result = spawnSync(process.execPath, [TOOL], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /validate-probe-authorities: PASS authorities=6 /);
});

test("a catalog atom that does not exist in its owner's charter is refused", () => {
  const catalog = parseToml(
    CATALOG.replace("Acceptance and evidence/1", "Acceptance and evidence/9"),
  );
  refusedOnce(
    run({ catalog }),
    /route-callable: charter_atom "Acceptance and evidence\/9" does not exist in ROUTE's charter/,
  );
});

test("a cell without a citation is refused", () => {
  const manifest = manifestWith("Compile", (entry) => delete entry.authority);
  refusedOnce(
    run({ manifests: [manifest] }),
    /\[Compile\]: missing \{ authority, atom \} citation/,
  );
});

test("an unknown authority id is refused", () => {
  const manifest = manifestWith("Compile", (entry) => (entry.authority = "vue.compile-product"));
  refusedOnce(
    run({ manifests: [manifest] }),
    /\[Compile\]: unknown authority vue\.compile-product/,
  );
});

test("a wrong-framework authority is refused", () => {
  const manifest = manifestWith(
    "Compile",
    (entry) => (entry.authority = "svelte.runtime-client-product"),
  );
  refusedOnce(run({ manifests: [manifest] }), /covers framework svelte, not vue/);
});

test("a wrong-dimension authority is refused", () => {
  const manifest = manifestWith("Performance", (entry) => {
    entry.authority = "compiler.public-request-route";
    entry.atom = "route-callable";
  });
  const errors = run({ manifests: [manifest] });
  assert.ok(errors.length >= 1);
  assert.ok(
    errors.every((error) =>
      /\[Performance\]:.*(does not cover dimension|does not authorize dimension) Performance/.test(
        error,
      ),
    ),
    errors.join("\n"),
  );
});

test("an atom not listed for the cited authority is refused", () => {
  const manifest = manifestWith("Compile", (entry) => (entry.atom = "route-callable"));
  refusedOnce(
    run({ manifests: [manifest] }),
    /atom route-callable is not listed for authority vue\.runtime-client-product/,
  );
});

test("a roadmap-shaped atom in an executable artifact is refused", () => {
  const manifest = manifestWith("Compile", (entry) => (entry.atom = "VUE-AC2"));
  refusedOnce(run({ manifests: [manifest] }), /atom "VUE-AC2" is roadmap-shaped/);
});

test("a gate whose owner is not implemented is refused", () => {
  const manifest = manifestWith("Compile", (entry) => (entry.expected_state = "gate"));
  refusedOnce(
    run({ manifests: [manifest] }),
    /\[Compile\]: authority vue\.runtime-client-product is owned by VUE, whose ledger row is not implemented/,
  );
});

test("a gate class its cited atom does not list is refused", () => {
  const manifest = manifestWith("Route", (entry) => (entry.expected_class = "request_refused"));
  refusedOnce(
    run({ manifests: [manifest] }),
    /atom route-callable of compiler\.public-request-route does not list gate class request_refused/,
  );
});

test("a promotion whose owner is not implemented is refused", () => {
  const record = joinRecord();
  record.probe[1] = {
    ...record.probe[1],
    transition: "canary -> gate",
    decided_by: { authority: "vue.runtime-client-product", atom: "product-shape" },
  };
  refusedOnce(
    run({ joins: [record] }),
    /probe vue\/fixtures\/App\.vue \[Compile\]: canary -> gate: .*owned by VUE, whose ledger row is not implemented/,
  );
});

test("a comparison-eligible basis whose owner is not implemented is refused", () => {
  const artifact = observationArtifact();
  artifact.rows[0].comparison_eligible = true;
  refusedOnce(
    run({ observations: [artifact] }),
    /@cold: semantic_basis: .*owned by VUE, whose ledger row is not implemented/,
  );
});

test("a structural manifest without its comparator is refused", () => {
  const manifest = vueManifest();
  delete manifest.comparator;
  refusedOnce(run({ manifests: [manifest] }), /comparison = structural requires a comparator/);
});

test("a structural manifest for a framework whose product authority has no comparator is refused", () => {
  const manifest = vueManifest();
  manifest.framework = "svelte";
  manifest.entries = [];
  refusedOnce(
    run({ manifests: [manifest] }),
    /product authority svelte\.runtime-client-product supplies no comparator/,
  );
});

test("a substituted comparator is refused", () => {
  const manifest = vueManifest();
  manifest.comparator.function = "compare_products";
  refusedOnce(
    run({ manifests: [manifest] }),
    /comparator differs from vue\.runtime-client-product's catalog comparator \(function\)/,
  );
});

test("the observations directory form validates each */observations.json separately", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "probe-observations-"));
  try {
    const write = (relative, value) => {
      fs.mkdirSync(path.dirname(path.join(root, relative)), { recursive: true });
      fs.writeFileSync(path.join(root, relative), JSON.stringify(value));
    };
    const ineligible = {
      rows: [
        {
          row_id: "vue/fixtures/App.vue@cold",
          case_id: "vue/fixtures/App.vue",
          comparison_eligible: false,
        },
      ],
    };
    const eligibleWithoutBases = {
      rows: [
        {
          row_id: "vue/fixtures/App.vue@warm",
          case_id: "vue/fixtures/App.vue",
          comparison_eligible: true,
        },
      ],
    };
    write("valid/observations.json", ineligible);
    write("invalid/observations.json", eligibleWithoutBases);
    write("observations.json", eligibleWithoutBases);
    write("valid/notes.json", eligibleWithoutBases);

    assert.deepEqual(
      observationFiles(root).map((file) => path.relative(root, file)),
      [path.join("invalid", "observations.json"), path.join("valid", "observations.json")],
    );

    const failing = spawnSync(process.execPath, [TOOL, "--observations", root], {
      encoding: "utf8",
    });
    assert.equal(failing.status, 1, failing.stdout);
    const refusals = failing.stderr.trim().split("\n");
    assert.ok(refusals.length >= 1, failing.stderr);
    assert.ok(
      refusals.every((line) => line.includes(path.join(root, "invalid", "observations.json"))),
      failing.stderr,
    );
    assert.match(failing.stderr, /comparison_eligible = true requires semantic_basis/);

    fs.rmSync(path.join(root, "invalid"), { recursive: true });
    const passing = spawnSync(process.execPath, [TOOL, "--observations", root], {
      encoding: "utf8",
    });
    assert.equal(passing.status, 0, passing.stderr);
    assert.match(passing.stdout, /observation_files=1 observation_rows=1/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
